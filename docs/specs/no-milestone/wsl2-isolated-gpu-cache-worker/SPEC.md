# SPEC — Process-isolated GPU cache worker for WSL2 origin swap

> SSDV3 Step 2 · PRD: docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/PRD.md

## Closed scope

### In now
- Process-isolated GPU cache worker execution model for `ramshared-wsl2d`.
- Framing protocol for deadline-bounded socket IPC between daemon and worker.
- LRU chunk management and physical VRAM allocation using the selected CUDA/Vulkan provider, optionally constrained by matching `/dev/dxg` WDDM telemetry.
- Data, disable, and heartbeat messages over one ordered socket stream; no independently scheduled control lane is provided.
- Fail-closed fallback: transparent origin serving upon worker timeout, crash, or disconnect.
- Real-time telemetry publication to `/run/ramshared/wsl2-cache-status.json`.
- Bounded lifecycle supervision with `PR_SET_PDEATHSIG`; child exit and GPU memory release can remain unconfirmed after a blocked driver call.
- Optional WDDM headroom guard correlated by the exact LUID of the selected CUDA/Vulkan adapter.
- Conservative intersection of allocator and WDDM available bytes on startup and before every new allocation.
- Fail-closed admission if an established WDDM guard later returns stale, invalid, or mismatched data.
- Multi-adapter enumeration across CUDA and Vulkan, ranking by fresh safe target after the reserve and matching WDDM budget, with exact-adapter reopen and revalidation.

### Out now
- Custom Linux kernel driver changes (operates over standard upstream WSL2 kernel and userspace `/dev/dxg`).
- Windows Host service modifications (host guardian contracts remain untouched).
- Persistent non-volatile VRAM caching across host power cycles.

### Assumed-ready dependencies
- `AuthoritativeOriginBackend` and `BoundedCacheClient` in `crates/ramshared-block/src/isolated_origin.rs`.
- `DxgBudgetProvider` in `crates/ramshared-dxg/src/lib.rs` and adapter identity from the selected CUDA/Vulkan provider.
- `/dev/dxg` device node present and accessible in WSL2 environment.

---

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 (Process Isolation) | ITEM-1, ITEM-3, DT-1 |
| RF-2 (Bounded IPC Protocol) | ITEM-2, DT-2, DT-10, DT-11 |
| RF-3 (Fail-Closed Fallback) | ITEM-2, ITEM-4, DT-2, DT-11 |
| RF-4 (Telemetry Publication) | ITEM-4, ITEM-5, DT-4 |
| RF-5 (Supervised Teardown) | ITEM-3, ITEM-5, DT-5 |
| RF-6 (Cross-API Budget Correlation) | ITEM-7, DT-6, DT-7 |
| RF-7 (Secondary Budget Failure) | ITEM-7, DT-7 |
| RF-8 (Multi-adapter Selection) | ITEM-8, DT-8 |
| NFR-1 (Bounded Parent IPC <= 50ms) | ITEM-2, DT-2, DT-9 |
| NFR-2 (VRAM Reserve Floor) | ITEM-3, DT-3 |
| NFR-3 (Stability Evidence; no universal guarantee) | ITEM-3, ITEM-4, DT-5 |
| NFR-4 (Observability) | ITEM-5, DT-4 |
| NFR-5 (Budget Freshness) | ITEM-7, DT-7 |

---

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Worker process dispatched via re-execution (`/proc/self/exe __gpu_worker`) using an anonymous `socketpair(AF_UNIX, SOCK_STREAM, 0)` passed via inherited FD. Framing uses a fixed 32-byte header with `payload_len`-based reassembly (manual byte-stream framing). | Gives the worker a separate process address space, not a guarantee against kernel driver stalls or system-wide hangs. `SOCK_STREAM` keeps one ordered channel and needs explicit framing. |
| DT-2 | Cache reads and heartbeats share a 50ms absolute monotonic deadline across request writes, response headers, and payloads; startup handshake uses a 30s bound covering worker process init (CUDA/NVML load, context creation, WDDM revalidation) and teardown keeps an explicit 5s bound. `Update` and `Promote` are split by the caller into slices of at most 64 KiB (`mutation_frames`) and handed to the bounded nonblocking pending-frame queue (DT-10), which completes partial writes without stalling origin I/O; an oversized mutation still disables the cache and shuts down the socket. The parent paces round-trips so they are never issued into a mutation backlog (DT-9). | Per-syscall timeouts do not bound a trickled stream. Recomputing remaining time before each read prevents deadline extension, and mutation sends must never stall origin I/O. The parent cannot cancel a driver call already blocked in the child. Startup must cover NVML-backed device-wide occupancy (fail-closed without it), measured at 2.86s at load 12 and past 5s by load ~42; teardown stays tight so a wedged child cannot hang shutdown. Backpressure is not a broken peer: see DT-10 for why a partial mutation no longer revokes the cache and what still does. |
| DT-3 | Strict host safety floor: worker requires fresh driver-reported adapter telemetry and computes `min(request, capacity - reserve, live_available - reserve - 640 MiB)`, where `reserve = max(configured reserve, 20% of min(total, budget))`. Each allocation repeats the live check with the chunk size included. When exact-LUID WDDM telemetry is available, the lower correlated availability constrains admission. | The reserve must be subtracted from current free headroom as well as total capacity; otherwise existing Windows use can consume the intended display reserve. The independent runtime buffer remains available for driver/runtime allocations. |
| DT-4 | Atomic JSON telemetry publication via temporary file rename to `/run/ramshared/wsl2-cache-status.json`. | Prevents readers (`ramshared status`, `ramshared stress`) from seeing partial writes or corrupt JSON. |
| DT-5 | Worker uses `prctl(PR_SET_PDEATHSIG, SIGTERM)`. Shutdown waits at most 5s gracefully, then at most 500ms after SIGKILL; an unconfirmed child handle is handed to a background reaper. | A driver call can leave the child in uninterruptible sleep. The parent must not block in `Child::wait()` or claim that the child exited. Physical exit and GPU resource release remain unconfirmed until observed. |
| DT-6 | Open DXG with the LUID reported by the already-selected CUDA/Vulkan provider; do not enumerate-and-pick a separate “primary” GPU. If the backend lacks a LUID or DXG is unavailable, retain the allocator's own driver-reported budget. | Avoids combining budgets from different physical GPUs and preserves operation on native Linux or WSL configurations without DXG correlation support. |
| DT-7 | For a correlated adapter, admission headroom is `min(allocator.budget - allocator.used, WDDM.budget - WDDM.current_usage, WDDM.available_for_reservation)`; all subtractions saturate and both monotonic samples must be <=5 seconds old. Once attached, a DXG query/identity/freshness failure returns a provider error and prevents allocation. | The lower reported headroom is the conservative cross-API constraint; silently dropping an established guard after a driver error could over-allocate shared host VRAM. |
| DT-8 | Enumerate all CUDA and exact-index Vulkan devices, compute `min(request, capacity - reserve, live_available - reserve - 640 MiB)` against the allocator/WDDM intersection, choose the largest positive target with deterministic CUDA/ordinal/key tie-breaks, then reopen and revalidate identity and budget before serving. | Avoids hardcoded ordinal-zero selection and never ranks on advertised capacity that the reserve or current external use makes unavailable. A failed revalidation leaves the cache unavailable. |
| DT-9 | Parent-side pacing of round-trip requests on the shared ordered stream. A cache read is deferred (reported as a miss, origin fallback) for 50ms after a write-mirror (`Update`) frame. A heartbeat is deferred for 200ms after any mutation frame and spaced to at most one round-trip per second. Promotes from the read path never defer subsequent reads. Occupancy is no longer zeroed on a queued mutation: the last confirmed worker sample is retained until the next heartbeat replaces it. | DT-1 gives mutations and round-trips one FIFO socket with no control lane. A round-trip issued into a mutation backlog waits behind every queued 64 KiB frame and misses NFR-1's 50ms bound, which fail-closed the cache permanently — observed as `isolated GPU cache unavailable: heartbeat I/O failed` followed by worker `Broken pipe` under a 64 MiB write. Origin is authoritative, so a deferred read is the correct answer; telemetry may wait out a longer drain. Promotes must not gate reads or a cold read stream would never hit. |
| DT-10 | Parent-side bounded pending-frame queue for mutation sends. A mutation write stays nonblocking; a partial or would-block write stores the unwritten tail instead of abandoning it, so the stream never ends mid-frame. The queue is capped at 4 MiB. A `Promote` that cannot enter the queue is `Skipped` (the range is simply not cached and a later read misses). An `Update` that cannot enter the queue is converted to `MSG_INVALIDATE` for exactly its range (DT-11) so the worker drops any coverage there and a later read is a miss; the cache session stays `Active`. While the queue is non-empty a cache read returns `Miss` and a heartbeat is skipped, because a request written mid-frame would corrupt both frames. Deterministic write errors (peer gone) still revoke. | DT-2's original "one nonblocking send, partial kills the cache" conflated backpressure with a broken peer. Under a write burst the socket fills while the worker drains `handle_update`; the first partial write abandoned a mid-frame stream and revoked the cache permanently (`isolated GPU cache unavailable: nonblocking mutation frame write failed`, worker then exits on EOF and is left unreaped). Origin I/O must stay nonblocking, so completion happens from a bounded userspace queue rather than by blocking the NBD thread. Degrading to a read miss is sound only when nothing stale can be served: `Promote` adds nothing so dropping it is free, while `Update` must either be delivered or have its range dropped in the worker (DT-11). |
| DT-11 | `MSG_INVALIDATE` (type 11, no payload, `offset` = range start, `aux` = range length) tells the worker to drop every cached entry overlapping `[offset, offset+aux)`. The parent emits it when an `Update` cannot enter the pending queue: the write-mirror never reached the worker, so any coverage the worker still holds for that range is pre-write bytes. Invalidation is appended to the same pending queue so it is ordered after frames already in flight. If the invalidate itself cannot be queued, or the peer is gone, the cache revokes — at that point nothing can stop a stale Hit. A `Promote` never needs one. | A live 192 MiB O_DIRECT burst filled the 4 MiB pending queue and produced `isolated GPU cache unavailable: mutation backlog overflowed`, ending the cache session for a defect that is only "that range is not current". Revoking the whole cache for one dropped write-mirror throws away every unrelated hot range and leaves the tier dead until a daemon restart. The worker already invalidates overlapping extents before publishing (`handle_update`, DT-6); this message exposes that primitive to the parent so a dropped mutation degrades to a miss instead of killing the cache. |

---

## Atomicity and rollback

### Atomicity frontier
- **Origin Backend (Authoritative):** Operates independently of the worker. Origin writes always complete and synchronize to disk prior to block layer acknowledgement.
- **Cache Worker (Ephemeral):** State is purely non-authoritative. Worker loss never invalidates data durability.
- **IPC Channel (Boundary):** Socket failure or deadline expiry triggers `CacheState::Unavailable` and shuts down the client socket. Cache errors do not bypass the authoritative origin; origin I/O errors remain possible.

### Rollback
- **Daemon layer:** Revert to `DisabledCache` selection if worker process initialization fails.
- **Worker layer:** The daemon requests bounded graceful/forced shutdown. If exit remains unconfirmed, a background reaper retains the child handle; GPU resource release remains unconfirmed until the process exits.
- **Host layer:** No persistent state modified; `/proc/swaps` and NBD device remain intact.

---

## Kahneman map (critical only)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 (Parent IPC Deadline) | #16 | Can a stalled/trickled response or saturated mutation extend parent work beyond its bound? | `cargo test -p ramshared-block ipc_cache_client::tests::read_timeout_falls_back_cleanly`; `...::trickled_response_cannot_extend_the_absolute_read_deadline`; `...::saturated_mutation_socket_does_not_block_origin_thread`; `...::partial_mutation_write_completes_the_frame_and_keeps_the_cache_active`; `...::backpressured_update_is_queued_never_dropped`; `...::backpressured_promote_degrades_to_not_cached`; `...::cache_read_is_a_miss_while_a_mutation_frame_drains`; `...::pending_mutation_backlog_is_bounded`; `...::backpressured_update_invalidates_its_range_and_stays_active`; `...::invalidate_that_cannot_queue_revokes_the_cache`; `...::oversize_mutation_disables_cache_without_touching_ipc` | Parent does not fall back/disable at the bound; physical driver cancellation is not established by these source tests |
| ITEM-3 (Worker Crash) | #13 | In the source harness, does worker exit leave the origin backend usable? | `cargo test -p ramshared-wsl2d daemon_survives_abrupt_gpu_worker_kill` | Test harness loses origin service; this does not qualify live NBD or driver behavior |
| ITEM-4 (Teardown) | #17 | Does repeated teardown preserve origin service and avoid an unbounded parent wait when worker exit is unconfirmed? | `cargo test -p ramshared-wsl2d isolated_worker_shutdown_stays_bounded_when_kill_is_not_observed` and `cargo test -p ramshared-block worker_teardown_is_idempotent_and_bounded` | Origin path blocks after the 5s graceful window plus 500ms exit observation, or child ownership is dropped without reaper handoff |

---

## Security checklist (pre-impl)

- [x] Privilege: CUDA/Vulkan access is required for allocation. `/dev/dxg` access is required only to enable the optional WDDM cross-budget guard; neither path requires a new root capability in worker logic.
- [x] User/host copy: worker frames have a 32-byte header and payloads are capped at 16 MiB; cache mutation payloads are capped at 64 KiB before the parent's nonblocking send (DT-10 rejects anything larger fail-closed and never fragments it across the queue).
- [x] Flags/IOCTL codes: worker rejects unknown IPC frame types fail-closed.
- [x] Info-leak: no kernel pointers or physical host memory addresses transmitted across IPC frames.
- [x] IRQ/atomic: all GPU operations occur in user-space worker context. Driver calls themselves do not have a hard cancellation deadline.
- [x] Lifetime: socket closure revokes the cache; shutdown is bounded and transfers an unconfirmed child to a background reaper. Physical allocation release is not claimed until exit.
- [x] Hot-unplug: if a device error returns, the client marks `Unavailable` and origin continues; a call stuck in the driver remains isolated but may not exit promptly.
- [x] Host safety: the worker enforces `max(configured floor, ceil(capacity/5))` plus the separate 640 MiB runtime buffer, with the configured floor supplied by the sealed policy. (2026-10-01: the row's "production origin-cache caller currently defaults the configured floor to 512 MiB" claim is a **stale incorrect conclusion** — that was the pre-`76a90c55` sparse-tier env-reader default, deleted together with the per-surface constants. Current production: `ReserveFloorPolicy::min_floor_bytes` (sealed `gpu_reserve_min_mib = 2048`, `gpu_reserve_percent = 20`) is passed as `--reserve-floor`, which is now **required** — a missing flag is `missing --reserve-floor for isolated gpu worker` (`isolated_gpu_worker_arguments_require_reserve_floor`); `origin_cache.rs` `configured_reserve_bytes: 0` is DT-9 case 3 ("no configured floor"; the `capacity.div_ceil(5)` share still binds inside `safe_target_bytes`); `GpuWorkerConfig::default().reserve_floor_bytes` is `0`, not the superseded `1536 MiB` literal. The PRD `max(1536 MiB, 20%)` mitigation is superseded by the three-term maximum `max(min_floor_bytes, floor(capacity * sealed_percent / 100), ceil(capacity/5)) + runtime_headroom` and the PRD now says so. Contract and production default are reconciled; **capacity-boundary and live-adapter requalification remain environment-bound** (ITEM-6 of `gpu-reserve-floor-authority`) and are not claimed here.)
- [x] Bounded DMA: honest documented boundary, not a closable gap. Only parent IPC waits are bounded (50 ms client deadline, bounded stop plus background reaper); a driver ioctl already inside an uninterruptible wait cannot be cancelled from userspace — that is a driver-side property. PRD §9 records the matching risk ("Worker blocked in a driver call, with exit or VRAM release unconfirmed after bounded stop") and the IRQ/atomic row above already states that driver calls have no hard cancellation deadline. Live-driver stuck-ioctl and zero-D-state evidence is environment-bound (physical driver lab) and stays on the live fault drill row below.
- [x] Origin fallback: cache miss or failure proceeds through the authoritative origin backend; origin storage errors can still fail I/O. Simultaneous physical tier saturation is not qualified by this unit contract.
- [x] Replayable ops: `Disable` and cleanup are fully idempotent (#17).

---

## Files to CREATE / MODIFY / DELETE

### CREATE

**`crates/ramshared-block/src/gpu_cache_worker.rs`**
- Purpose: Core worker loop, protocol frame serialization, chunk cache management, and adapter allocation.
- RF / DT: RF-1, RF-2, DT-1, DT-2, DT-3.
- Types / fns:
  ```rust
  pub struct GpuWorkerConfig {
      pub target_bytes: u64,
      pub chunk_bytes: usize,
      pub reserve_floor_bytes: u64,
  }
  pub fn run_gpu_worker_loop<P: GpuProvider>(
      socket: UnixStream,
      provider: P,
      config: GpuWorkerConfig,
  ) -> Result<(), String>;
  ```
- Required tests:
  - `gpu_cache_worker::tests::worker_handshake_and_read_hit_cycle`
  - `gpu_cache_worker::tests::worker_respects_headroom_floor`
  - `gpu_cache_worker::tests::worker_disable_frees_allocations`
  - `gpu_cache_worker::tests::invalidate_drops_overlapping_coverage`
- Cover target: >= 80%

**`crates/ramshared-block/src/ipc_cache_client.rs`**
- Purpose: Socket-based implementation of `BestEffortCache` backed by the isolated worker process.
- RF / DT: RF-2, RF-3, DT-2, DT-9, DT-10, DT-11.
- Types / fns:
  ```rust
  pub struct IpcCacheClient {
      socket: UnixStream,
      read_timeout: Duration,
      state: CacheState,
      cached_bytes: u64,
      target_bytes: u64,
      pending_frame: Vec<u8>,
  }
  pub struct PacingPolicy { /* read/heartbeat drain graces + heartbeat spacing */ }
  impl BestEffortCache for IpcCacheClient { ... }
  ```
- Required tests:
  - `ipc_cache_client::tests::read_timeout_falls_back_cleanly`
  - `ipc_cache_client::tests::socket_disconnect_marks_unavailable`
  - `ipc_cache_client::tests::small_update_and_promote_complete_within_the_deadline`
  - `ipc_cache_client::tests::trickled_response_cannot_extend_the_absolute_read_deadline`
  - `ipc_cache_client::tests::saturated_mutation_socket_does_not_block_origin_thread`
  - `ipc_cache_client::tests::partial_mutation_write_completes_the_frame_and_keeps_the_cache_active`
  - `ipc_cache_client::tests::backpressured_update_is_queued_never_dropped`
  - `ipc_cache_client::tests::backpressured_promote_degrades_to_not_cached`
  - `ipc_cache_client::tests::cache_read_is_a_miss_while_a_mutation_frame_drains`
  - `ipc_cache_client::tests::pending_mutation_backlog_is_bounded`
  - `ipc_cache_client::tests::backpressured_update_invalidates_its_range_and_stays_active`
  - `ipc_cache_client::tests::invalidate_that_cannot_queue_revokes_the_cache`
  - `ipc_cache_client::tests::oversize_mutation_disables_cache_without_touching_ipc`
  - `ipc_cache_client::tests::mutation_preserves_last_confirmed_occupancy`
  - `ipc_cache_client::tests::heartbeat_is_deferred_while_write_mirrors_drain`
  - `ipc_cache_client::tests::heartbeat_is_spaced_to_one_per_interval`
  - `ipc_cache_client::tests::cache_read_is_deferred_while_write_mirrors_drain`
  - `ipc_cache_client::tests::promote_does_not_defer_subsequent_reads`
- Cover target: >= 80%

### MODIFY

**`crates/ramshared-block/src/lib.rs`**
- Export `gpu_cache_worker` and `ipc_cache_client` modules and types.

**`crates/ramshared-wsl2d/src/main.rs`**
- In `run_nbd_origin_loop`: Replace `DisabledCache` with spawned worker child and `IpcCacheClient`.
- Implement `spawn_isolated_gpu_worker` with `socketpair` and `PR_SET_PDEATHSIG`.
- Update telemetry loop to record worker-reported cache bytes and `cache_state=ACTIVE` only from fresh successful worker communication.
- In `run_isolated_gpu_worker_entry`, wrap the selected provider only after exact-LUID DXG selection; keep origin-only operation when DXG correlation is unavailable and refuse invalid correlation.
- Enumerate CUDA and Vulkan candidates, rank using `gpu_budget::safe_cache_target`, reopen the selected exact ordinal, and verify its identity and fresh safe target before entering the worker loop.

---

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| `cache_state` | `/run/ramshared/wsl2-cache-status.json` | String (`ACTIVE`, `UNAVAILABLE`, `OFF`) |
| `vram_cached_kib` | `/run/ramshared/wsl2-cache-status.json` | Worker-reported allocated cache bytes (`u64`); zero after communication loss is not proof that physical allocations were released |
| `worker_pid` | Daemon debug log / stdout | Integer (`pid_t`) |
| `worker_crash` | Daemon stderr / systemd journal | Warning / Event |

---

## Living docs

| Document | Action |
| --- | --- |
| `docs/reliability/GAP-REGISTER.md` | Update WSL2 control-plane & Build #5 stress gates upon qualification |
| `validation.md` | Append validation record upon successful qualification drill |
| `trovaldo.md` | Update WSL2 driver and origin status |

---

## Implementation order

1. **ITEM-1:** Implement IPC protocol frames and serialization in `crates/ramshared-block/src/gpu_cache_worker.rs`.
2. **ITEM-2:** Implement `IpcCacheClient` with an absolute read/heartbeat deadline and a bounded nonblocking pending-frame queue for mutation sends; revoke cache after timeout or a deterministic peer error, degrade a backpressured `Promote` to not-cached, convert a backpressured `Update` into `MSG_INVALIDATE` for its range (DT-11) and a read issued while the queue drains to a miss (DT-10); pace round-trips so none is issued into a mutation backlog (DT-9).
3. **ITEM-3:** Implement `GpuCacheWorker` memory manager with chunk LRU and host reserve floor.
4. **ITEM-4:** Implement child process spawning and supervision in `ramshared-wsl2d`.
5. **ITEM-5:** Wire real-time telemetry output to `/run/ramshared/wsl2-cache-status.json`.
6. **ITEM-6:** Add hermetic fault-injection tests (process kill, timeout, socket tear).
7. **ITEM-7:** Add exact-LUID selection and a conservative WDDM budget guard with tests for intersection, mismatch, stale samples, and guard failure.
8. **ITEM-8:** Enumerate CUDA/Vulkan adapters, rank by reserve-adjusted and WDDM-constrained target, open the exact selected ordinal, and revalidate identity/budget before serving. If revalidation fails, complete the zero-target handshake so the client stays on origin immediately.

---

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::read_timeout_falls_back_cleanly` | unit | #16 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::socket_disconnect_marks_unavailable` | unit | #13 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::small_update_and_promote_complete_within_the_deadline` | unit | #9 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::trickled_response_cannot_extend_the_absolute_read_deadline` | unit | #16 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::saturated_mutation_socket_does_not_block_origin_thread` | unit | #16 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::partial_mutation_write_completes_the_frame_and_keeps_the_cache_active` | unit | #16 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::backpressured_update_is_queued_never_dropped` | unit | #13 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::backpressured_promote_degrades_to_not_cached` | unit | #13 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::cache_read_is_a_miss_while_a_mutation_frame_drains` | unit | #16 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::pending_mutation_backlog_is_bounded` | unit | #16 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::backpressured_update_invalidates_its_range_and_stays_active` | unit | #13 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::invalidate_that_cannot_queue_revokes_the_cache` | unit | #13 | >= 80% |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `tests::invalidate_drops_overlapping_coverage` | unit | #13 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::oversize_mutation_disables_cache_without_touching_ipc` | unit | #13 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::oversize_read_is_a_cache_miss_without_waiting_for_ipc` | unit | #16 | >= 80% |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `tests::oversized_mutation_disables_cache_before_worker_frame_is_sent` | unit | #13 | origin remains authoritative |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `tests::worker_handshake_and_read_hit_cycle` | unit | #9 | >= 80% |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `tests::worker_respects_headroom_floor` | unit | #16 | >= 80% |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `tests::worker_disable_frees_allocations` | unit | #17 | >= 80% |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `tests::worker_evicts_coldest_chunk_on_pressure` | unit | #9 | >= 80% |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `tests::worker_teardown_is_idempotent_and_bounded` | unit | #17 | >= 80% |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::daemon_survives_abrupt_gpu_worker_kill` | integration | #13 | runtime evidence; daemon host qualification remains partial |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::daemon_publishes_live_worker_telemetry` | integration | #9 | runtime evidence; daemon host qualification remains partial |
| `crates/ramshared-dxg/src/lib.rs` | `tests::adapter_luid_parser_rejects_noncanonical_or_overflow_values` | unit | #13 | parser behavior |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | `tests::same_adapter_budget_uses_lower_allocator_and_wddm_headroom` | unit | #9 | >= 80% |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | `tests::mismatched_stale_future_and_malformed_budgets_are_rejected` | unit | #13/#16 | >= 80% |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | `tests::established_wddm_query_failure_blocks_allocations` | unit | #16 | >= 80% |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | `tests::provider_open_uses_exact_luid_and_rejects_other_adapter` | unit | #13 | >= 80% |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | `tests::missing_luid_and_unavailable_dxg_allow_allocator_only_startup` | unit | #13 | >= 80% |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | `tests::candidate_target_applies_reserve_freshness_and_request_cap` | unit | #13/#16 | >= 80% |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | `tests::candidate_selection_prefers_largest_safe_target_then_stable_ties` | unit | #9 | >= 80% |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::isolated_gpu_worker_arguments_require_reserve_floor` | unit | #13/#15 | >= 80% |
| `crates/ramshared-vulkan/src/lib.rs` | `tests::exact_device_open_rejects_out_of_range_ordinal_without_clamping` | ignored software-ICD integration | #13/#16 | >= 80% |

---

## Validation checklist

- [x] `cargo fmt --all -- --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test -p ramshared-block -p ramshared-wsl2d` (also covered by the passing workspace suite)
- [x] Slice coverage: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-block,ramshared-wsl2d --files crates/ramshared-block/src/gpu_cache_worker.rs,crates/ramshared-block/src/ipc_cache_client.rs,crates/ramshared-wsl2d/src/gpu_budget.rs --min 80`
- [x] Vulkan provider coverage with the hosted Mesa software ICD: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-vulkan --files crates/ramshared-vulkan/src/lib.rs --min 80 --include-ignored`. This runs the Vulkan integration cases marked ignored on machines without an ICD; the CI runner must set `VK_ICD_FILENAMES` to Mesa lavapipe before the gate. (2026-10-01: `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json` set; gate → 80.1% (444/554) lines, PASSED at the named 80% minimum.)
- [ ] Live path verification: `sudo ramshared check --json` confirms `cache_state=ACTIVE` with valid worker instance.
- [ ] Live fault drill: interrupt a worker during cache I/O, verify origin responses within the IPC deadline, capture worker state until reaped, and confirm clean teardown. A software kill test does not establish zero kernel D-state on physical drivers.
