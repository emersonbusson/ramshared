---
slug: wsl2-isolated-gpu-cache-worker
title: Process-isolated GPU cache worker for WSL2 origin swap
milestone: —
issues: []
---

# PRD — Process-isolated GPU cache worker for WSL2 origin swap

## 1. Summary

Implement a process-isolated GPU cache worker for the RamShared WSL2 daemon (`ramshared-wsl2d`). Replace the fail-closed `DisabledCache` in authoritative origin mode with an out-of-process worker that interfaces with `/dev/dxg` / WDDM, allocates clean, bounded physical VRAM buffers, and serves revocable cache requests over a bounded non-blocking IPC channel. If the GPU driver hangs, crashes, or resets, the failure is strictly contained within the worker process; the main NBD block daemon automatically flips to origin-only mode without blocking, dropping swap I/O, or inducing Linux kernel hangs.

## Current boundary — staged design

This PRD defines the architectural requirements, communication protocol, safety frontiers, and failure domains for the process-isolated GPU cache worker. It authorizes no unsupervised GPU memory allocation or uncontrolled memory pressure on the live WSL2 host. Physical 3-tier qualification requires an explicit attended validation session after code completion.

## 2. Technical context

- **Confirmed in codebase:** `ramshared-wsl2d` selects `DisabledCache` in authoritative origin mode (`crates/ramshared-wsl2d/src/main.rs:2930`), reporting `cache_state=UNAVAILABLE` and `vram_cached_kib=0`.
- **Confirmed in codebase:** `crates/ramshared-block/src/isolated_origin.rs` contains the core abstractions `BestEffortCache`, `BoundedCacheClient`, `IsolatedCacheRequest`, and `IsolatedCacheControl` with dedicated control/data lanes and non-blocking semantics.
- **Confirmed in codebase:** `DxgProvider` in `crates/ramshared-vram/src/dxg.rs` provides WDDM/DXG ioctl bindings (`/dev/dxg`) for allocating GPU memory under WSL2.
- **Confirmed in codebase:** `/run/ramshared/wsl2-cache-status.json` and the stress tool (`crates/ramshared-cli/src/stress.rs`) require fresh, matching daemon telemetry with `origin_state=READY`, `cache_state=ACTIVE`, and nonzero `target_mib` before permitting cascade pressure qualification.
- **Inference:** Crash isolation across a process boundary ensures that a GPU TDR (Timeout Detection and Recovery), unhandled ioctl stall, or driver crash terminates only the worker process, leaving the NBD swap daemon alive and servicing reads/writes directly from the durable SSD origin.

## 3. Recommended option

Run the GPU cache worker as a separate dedicated process (`ramshared-gpu-worker`) or supervised subprocess spawned by `ramshared-wsl2d` during cascade startup. 

The main daemon creates an anonymous Unix domain stream socket pair (`socketpair(AF_UNIX, SOCK_STREAM, 0)`) and passes the worker FD to the child process. The worker initializes `/dev/dxg`, allocates physical VRAM chunks respecting a mathematical host headroom floor, and maintains a chunk-indexed cache.

### Discarded alternatives:
1. *In-process GPU worker thread:* Rejected. If `/dev/dxg` hangs in kernel mode or crashes the driver, the entire daemon process hangs in D-state or dies with SIGSEGV/SIGBUS, immediately causing Linux kernel swap failure and whole-VM kernel panic.
2. *Shared-memory ring buffer without socket control lane:* Rejected. Shm requires complex cross-process lock recovery and robust mutexes, which can deadlock if the worker is killed while holding a lock. An IPC socket pair with bounded timeouts provides clean OS-level disconnect notifications on crash.

## 4. Functional requirements (RF)

- **RF-1 (Process Isolation):** The GPU cache worker must run in an isolated process space separate from `ramshared-wsl2d`. A fatal signal, abort, or unhandled exception in the worker must not terminate or corrupt the parent daemon.
- **RF-2 (Bounded IPC Protocol):** Communication between daemon and worker must use logical lanes over a single multiplexed socket:
  - *Data lane:* Non-blocking `Read`, `Update`, and `Promote` frames. Reads have a strict timeout (<= 50ms).
  - *Control lane:* Dedicated `Disable` and `Telemetry` requests that can never be starved by backlogged data frames.
- **RF-3 (Fail-Closed Origin Fallback):** Any channel disconnect, protocol error, or read timeout must immediately transition the client to `CacheState::Unavailable`. All ongoing and subsequent read requests must transparently fall back to the authoritative SSD origin with zero I/O errors returned to the block layer.
- **RF-4 (Telemetry Publication):** The daemon must continuously observe worker health and publish verified atomic telemetry to `/run/ramshared/wsl2-cache-status.json`:
  - `origin_state`: `"READY"`
  - `cache_state`: `"ACTIVE"` (or `"UNAVAILABLE"` upon failure)
  - `vram_cached_kib`: physical resident bytes
  - `cache_target_kib`: configured capacity ceiling
  - `daemon_instance_id`: matching boot and PID identity
- **RF-5 (Supervised Lifecycle & Teardown):** 
  - On startup: daemon verifies worker health before announcing `cache_state=ACTIVE`.
  - On teardown: daemon sends `Disable` over the control lane, waits for worker ACK (up to 5s), and reaps the child process cleanly without leaving zombie processes.

## 5. Non-functional requirements (NFR)

- **NFR-1 (Bounded Latency):** Cache read operations must complete or timeout in <= 50ms. Timeout triggers immediate fallback to SSD origin without retries.
- **NFR-2 (Host Safety & VRAM Floor):** The worker must enforce a strict host display reserve floor: `reserve_floor = max(1536 MiB, 20% host VRAM)`. The worker must never allocate more than `total_vram - reserve_floor`.
- **NFR-3 (Zero-Panic Guarantee):** Under no circumstance (including sudden GPU unbind or driver reset) may the daemon enter kernel D-state or crash `/dev/nbd0`. Stability verdict must remain `PASS_ZERO_PANIC`.
- **NFR-4 (Observability):** Worker crashes or restarts must emit structured, single-line warning events to stderr/journald without polluting dmesg or leaking kernel memory pointers.

## 6. Execution flows

### 6.1 Happy Path — Startup, Active Caching, and Teardown
1. `ramshared-wsl2d` starts with `--origin <path>`.
2. Daemon creates an IPC socketpair and spawns `ramshared-gpu-worker` with child socket FD.
3. Worker opens `/dev/dxg`, validates adapter headroom, allocates initial VRAM buffer pool, and sends `READY` handshake with target capacity.
4. Daemon initializes `AuthoritativeOriginBackend` with active `BoundedCacheClient` and writes `cache_state=ACTIVE` to status file.
5. On block read: daemon checks cache via worker; if hit, returns VRAM data; if miss, reads origin and sends async `Promote` to worker.
6. On block write: daemon writes to origin first; on success, sends async `Update` to worker.
7. On teardown (`ramshared down` / `SIGTERM`): daemon sends `Disable` on control lane; worker frees VRAM buffers, acknowledges ACK, and exits 0; daemon reaps child.

### 6.2 Error Path — GPU Driver Hang or Crash
1. GPU driver crashes or resets due to host pressure (TDR).
2. The worker process crashes (exits with error or SIGKILL) or hangs in ioctl.
3. The IPC socket closes or read request exceeds 50ms deadline.
4. Daemon's `BoundedCacheClient` detects disconnect/timeout, transitions `state = CacheState::Unavailable`, and logs warning.
5. Pending read immediately reads from SSD origin. Subsequent reads and writes bypass cache completely.
6. Daemon updates telemetry to `cache_state=UNAVAILABLE` and `vram_cached_kib=0`.
7. Linux kernel swap continues without interruption.

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
  - `vram_cached_kib`: `u64`
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
  - Worker process zombie leak. *Mitigation:* explicit parent-death tracking (`PR_SET_PDEATHSIG` with `SIGTERM`) and bounded daemon join timeout.
- **Rollback trigger:** Any worker crash or disconnect that causes `ramshared-wsl2d` to fail an origin read/write or stall swap for > 100ms.

## 10. Implementation strategy

1. **Slice 1:** Define wire protocol and IPC transport in `crates/ramshared-block/src/isolated_origin.rs` with mock socketpair integration tests.
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
- Injected worker kill (`kill -9 <worker_pid>`) during active I/O must transition cache to `UNAVAILABLE` within 50ms and complete all reads from origin with 0 errors.
- `/run/ramshared/wsl2-cache-status.json` reports `cache_state=ACTIVE` and nonzero `vram_cached_kib` during normal operation.
- Daemon teardown cleanly reaps worker with 0 residual processes.

## 14. Validation plan

- Hermetic unit tests with mock workers, socket fault injection, and timeout simulation.
- End-to-end drill: daemon startup -> worker attach -> verified cache hits -> worker kill -> verified seamless origin fallback -> teardown.
