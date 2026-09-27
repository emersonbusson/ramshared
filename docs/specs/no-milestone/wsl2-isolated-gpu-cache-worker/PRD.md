---
slug: wsl2-isolated-gpu-cache-worker
title: Process-isolated GPU cache worker for WSL2 origin swap
milestone: —
issues: []
---

# PRD — Process-isolated GPU cache worker for WSL2 origin swap

## 1. Summary

Provide an optional process-isolated GPU cache worker for the RamShared WSL2 daemon (`ramshared-wsl2d`). The authoritative SSD origin remains the source of truth; GPU memory is a revocable cache. Parent-side IPC operations use absolute monotonic deadlines and cache transport failures disable the cache so origin I/O can continue. A GPU driver call can still leave the child process blocked in the kernel, so process isolation does not prove that the child exits, VRAM is released, the daemon never stalls for unrelated reasons, or Linux cannot hang.

## Current boundary — staged design

This PRD defines the architectural requirements, communication protocol, safety frontiers, and failure domains for the process-isolated GPU cache worker. It authorizes no unsupervised GPU memory allocation or uncontrolled memory pressure on the live WSL2 host. Physical 3-tier qualification requires an explicit attended validation session after code completion.

## 2. Technical context

- **Confirmed in codebase:** `ramshared-wsl2d` can select either an isolated IPC cache client or `DisabledCache` while serving an authoritative origin. Startup and worker failure paths are covered by hermetic tests; no live WSL2 GPU session is qualified here.
- **Confirmed in codebase:** `crates/ramshared-block/src/isolated_origin.rs` contains the origin/cache abstractions. `IpcCacheClient` and the worker use one ordered Unix stream; the protocol has message types for data, heartbeat, and disable operations, but does not provide an independently scheduled control lane.
- **Confirmed in codebase:** `ramshared-dxg::DxgBudgetProvider` queries WDDM budgets through `/dev/dxg`; the isolated worker allocates through the selected CUDA or Vulkan provider in the same process.
- **Confirmed in codebase:** `/run/ramshared/wsl2-cache-status.json` reports daemon/cache state and worker-reported allocated cache bytes. Stress admission also checks fresh daemon identity, active cache state, and nonzero target/cache observations.
- **Qualification limit:** A timeout can make the parent stop using the cache and shut down its socket. It cannot cancel an ioctl already blocked in the worker's kernel driver, prove VRAM release, or guarantee that all NBD/origin operations complete.

## 3. Recommended option

Run the GPU cache worker as a supervised re-exec child of `ramshared-wsl2d` during origin-backed startup.

The main daemon creates an anonymous Unix domain stream socket pair (`socketpair(AF_UNIX, SOCK_STREAM, 0)`) and passes the worker FD to the child process. The worker initializes its selected CUDA or Vulkan allocator, allocates physical VRAM chunks within its driver budget and reserve floor, and maintains a chunk-indexed cache. Where that adapter has a matching WDDM LUID, the worker also intersects the CUDA/Vulkan headroom with the WDDM budget.

### Discarded alternatives:
1. *In-process GPU worker thread:* Not selected because a blocking driver call would run in the daemon address space and could prevent the same process from serving other work. The separate child reduces this shared failure domain, but cannot cancel a driver call or establish a system-wide no-hang guarantee.
2. *Shared-memory ring buffer:* Not selected. Shared memory would need additional synchronization and recovery rules. The socket pair provides process-disconnect notification and bounded parent-side I/O; it does not provide independent scheduling for control frames.

## 4. Functional requirements (RF)

- **RF-1 (Process Isolation):** The GPU cache worker must run in an isolated process space separate from `ramshared-wsl2d`. A fatal signal, abort, or unhandled exception in the worker must not terminate or corrupt the parent daemon.
- **RF-2 (Bounded Parent IPC):** Data, heartbeat, and disable messages use one multiplexed stream. Cache reads and heartbeats use one absolute monotonic deadline across request and full response (50 ms); startup and teardown use explicit 5-second limits. `Update` and `Promote` use one nonblocking send, limited to 64 KiB of data. A partial/backpressured frame or oversized mutation disables the client and closes the socket, because an incomplete frame cannot safely remain on the stream. The worker may remain blocked in a driver call; parent time bounds do not cancel that call.
- **RF-3 (Fail-Closed Origin Fallback):** A channel disconnect, protocol error, invalid timeout setup, or expired cache-operation deadline transitions the client to `CacheState::Unavailable`, shuts down the IPC socket, and stops using the cache. Cache errors alone must not fail a read or write whose authoritative-origin I/O succeeds; origin storage errors can still reach the block layer.
- **RF-4 (Telemetry Publication):** The daemon must continuously observe worker health and publish verified atomic telemetry to `/run/ramshared/wsl2-cache-status.json`:
  - `origin_state`: `"READY"`
  - `cache_state`: `"ACTIVE"` (or `"UNAVAILABLE"` upon failure)
  - `vram_cached_kib`: worker-reported allocated cache bytes while the worker is reachable; zero/unavailable cache telemetry does not prove that a blocked worker released physical memory
  - `cache_target_kib`: configured capacity ceiling
  - `daemon_instance_id`: matching boot and PID identity
- **RF-5 (Supervised Lifecycle & Teardown):**
  - On startup: daemon completes a handshake and only advertises an active cache when the worker reports a nonzero safe target.
  - On teardown: daemon requests disable and waits within a 5-second graceful window, then sends SIGKILL and observes exit for at most 500 ms. If exit remains unconfirmed, a background reaper retains the child handle; worker exit and physical memory release remain unconfirmed until observed.
- **RF-6 (Cross-API Budget Correlation):** When the selected CUDA or Vulkan adapter exposes a normalized Windows LUID and `/dev/dxg` exposes the same adapter, the worker must query WDDM for that exact LUID and use the lower of the allocator-reported and WDDM-reported available headroom for every admission decision. It must never select an unrelated DXG adapter by enumeration order.
- **RF-7 (Secondary Budget Failure):** After the worker has established an exact-LUID WDDM guard, a stale, malformed, failed, or mismatched WDDM sample must prevent new cache allocations. Existing clean cache entries remain revocable, and origin I/O remains available.
- **RF-8 (Multi-adapter Selection):** Enumerate usable CUDA and Vulkan adapters, calculate each adapter's fresh safe cache target after the reserve and exact-LUID WDDM intersection, and select the candidate with the largest target. Reopen and revalidate that exact adapter before starting the worker; if no candidate remains safe, expose origin-only operation.

## 5. Non-functional requirements (NFR)

- **NFR-1 (Bounded Parent IPC):** Cache reads/heartbeats use a 50 ms absolute monotonic deadline, including partial frame reads and request writes. `Update` and `Promote` do not wait for socket capacity; their single frame send either queues the whole frame or disables the cache. Expiry/failure allows the backend to use the authoritative origin. This bounds the parent-side cache wait, not origin-device latency, scheduler delays, or a driver call in the child.
- **NFR-2 (Host Safety & VRAM Floor):** The worker must enforce `reserve = max(configured reserve, 20% of min(total, budget))` against both total capacity and live available headroom. Its advertised target is at most `min(request, capacity - reserve, available - reserve - 640 MiB)`, and every allocation rechecks the chunk plus reserve and runtime buffer against a fresh snapshot.
- **NFR-3 (Stability Evidence):** Make no universal no-panic, no-D-state, or no-freeze guarantee from process isolation or hermetic tests. A `PASS_ZERO_PANIC` verdict requires a separately defined, supervised physical run with complete host and guest logs; it cannot be inferred from this source contract.
- **NFR-4 (Observability):** Worker crashes, transport failures, and unconfirmed teardown must emit concise single-line warnings to stderr/journald without exposing kernel memory pointers. These messages are userspace logs; they do not prove physical driver recovery.
- **NFR-5 (Budget Freshness):** Each available budget source must be sampled within 5 seconds of an allocation decision. Unknown identity or capacity is never treated as additional headroom.

## 6. Execution flows

### 6.1 Happy Path — Startup, Active Caching, and Teardown
1. `ramshared-wsl2d` starts with `--origin <path>`.
2. Daemon creates an IPC socketpair and spawns the re-exec GPU worker with the child socket FD.
3. Worker enumerates CUDA and Vulkan adapters, ranks them by their fresh safe target after reserve and exact-LUID WDDM intersection, then reopens and revalidates the exact selected adapter before allocation. If no candidate passes, the worker reports unavailable cache and leaves reads on the authoritative origin.
4. Daemon initializes `AuthoritativeOriginBackend` with the handshaken cache client. `cache_state=ACTIVE` means the cache is usable according to current worker telemetry; it is not a physical-hardware qualification.
5. On block read: daemon checks cache via worker; if hit, returns VRAM data; if miss, reads origin and sends async `Promote` to worker.
6. On block write: daemon writes to origin first; on success, sends async `Update` to worker.
7. On teardown (`ramshared down` / `SIGTERM`): daemon sends `Disable` on the shared stream; the worker frees its tracked buffers if it can process the request, acknowledges, and exits; the daemon observes exit or hands an unconfirmed handle to the background reaper.

### 6.2 Error Path — GPU Driver Hang or Crash
1. GPU driver crashes or resets due to host pressure (TDR).
2. The worker process crashes (exits with error or SIGKILL) or hangs in ioctl.
3. The IPC socket closes or read request exceeds 50ms deadline.
4. Daemon's `IpcCacheClient` detects disconnect/deadline, shuts down the socket, transitions to `CacheState::Unavailable`, and logs a warning.
5. The pending cache probe returns after the bounded deadline and the backend attempts the authoritative SSD origin. Subsequent reads and writes bypass the cache.
6. Daemon updates telemetry to `cache_state=UNAVAILABLE` and reports no usable cached bytes. A zero value is not evidence that a blocked worker released its allocations.
7. The daemon continues origin I/O when the storage path and daemon remain responsive. A physical driver hang, origin error, or unrelated kernel fault can still interrupt service and must be captured in live qualification.

## 7. Data and state model

```text
       ┌────────────────────────┐
       │   CacheState Machine   │
       └───────────┬────────────┘
                   │
         [Worker Connected]
                   ▼
               ┌────────┐
               │ ACTIVE │
               └───┬────┘
                   │
   [Timeout / Disconnect / GPU Crash]
                   ▼
            ┌─────────────┐
            │ UNAVAILABLE │ (Fail-closed to SSD Origin)
            └──────┬──────┘
                   │
             [Teardown]
                   ▼
               ┌─────┐
               │ OFF │
               └─────┘
```

- Worker IPC Frame Header:
  - `msg_type`: `u8` (1 = ReadReq, 2 = ReadResp, 3 = Update, 4 = Promote, 5 = DisableReq, 6 = DisableResp, 7 = HeartbeatReq, 8 = HeartbeatResp, 9 = HandshakeReq, 10 = HandshakeResp)
  - `correlation_id`: `u64`
  - `offset`: `u64`
  - `payload_len`: `u32`
- Telemetry Schema:
  - `origin_state`: String (`READY`, `DEGRADED`, `FAILED`)
  - `cache_state`: String (`ACTIVE`, `UNAVAILABLE`, `STUCK`, `OFF`)
  - `vram_cached_kib`: `u64` worker-reported allocated cache bytes; not proof of release after worker loss
  - `cache_target_kib`: `u64`
  - `written_at_unix_ms`: `u64`
  - `daemon_instance_id`: String

## 8. Interfaces

- Binary: re-exec via `/proc/self/exe __gpu_worker` (no separate binary artifact).
- Arguments: `--fd <socket_fd> --target-bytes <bytes> --chunk-bytes <bytes> --reserve-floor <bytes>`
- Telemetry: `/run/ramshared/wsl2-cache-status.json` (atomic write via tempfile rename).

## 9. Dependencies and risks

- **Prerequisites:** `/dev/dxg` device accessible in WSL2; NVIDIA DirectX user-mode driver (`/usr/lib/wsl/lib/libdxcore.so`).
- **Risks:** 
  - Host GPU contention with Windows applications. *Mitigation:* strict reserve floor enforcement (`max(1536 MiB, 20%)`).
  - Worker blocked in a driver call, with exit or VRAM release unconfirmed after bounded stop. *Mitigation:* parent-death signal, bounded stop, socket shutdown, and asynchronous reaper; live driver and reboot evidence remains an open gate.
- **Rollback trigger:** Any worker crash or disconnect that causes `ramshared-wsl2d` to fail an origin read/write or stall swap for > 100ms.

## 10. Implementation strategy

1. **Slice 1:** Define the wire protocol and IPC client in `crates/ramshared-block/src/gpu_cache_worker.rs` and `ipc_cache_client.rs`, with mock socketpair tests including partial-frame deadlines.
2. **Slice 2:** Implement `GpuCacheWorker` loop handling memory chunks, LRU eviction, and `/dev/dxg` allocation with headroom floor.
3. **Slice 3:** Implement child process spawning, supervision, and `PDEATHSIG` containment in `crates/ramshared-wsl2d/src/main.rs`.
4. **Slice 4:** Wire real telemetry publication to `/run/ramshared/wsl2-cache-status.json` and verify end-to-end crash recovery.

## 11. Documents to update

- `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/SPEC.md`
- `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/AUDIT-2.5.md`
- `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/IMPL.md`
- `docs/reliability/GAP-REGISTER.md`
- `validation.md`
- `trovaldo.md`

## 12. Out of scope

- Direct PCIe P2P DMA bypass (requires dedicated custom kernel driver, out of WSL2 day-1 scope).
- Persistent VRAM cache across host reboots (VRAM is volatile and treated strictly as a clean cache).

## 13. Acceptance criteria

- Unit test coverage >= 80% on business-logic files.
- Hermetic stalled/trickled IPC and socket-saturation tests prove bounded parent behavior, socket shutdown on ambiguous frames, and origin fallback. Oversized reads miss the cache; oversized mutations disable it before sending a frame.
- `/run/ramshared/wsl2-cache-status.json` reports `cache_state=ACTIVE` and nonzero `vram_cached_kib` during normal operation.
- Source tests prove bounded parent teardown and reaper handoff when child exit remains unconfirmed. A clean physical teardown with zero residual worker processes requires live evidence.

## 14. Validation plan

- Hermetic unit tests with mock workers, socket fault injection, and timeout simulation.
- Live drill (still open): daemon startup -> identified GPU allocation -> cache hits -> worker fault -> origin fallback -> worker exit/reap observation -> clean teardown, with guest kernel and host GPU logs. A software kill test does not establish physical driver behavior.
