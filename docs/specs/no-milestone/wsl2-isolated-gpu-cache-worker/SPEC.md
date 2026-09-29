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
| RF-2 (Bounded IPC Protocol) | ITEM-2, DT-2 |
| RF-3 (Fail-Closed Fallback) | ITEM-2, ITEM-4, DT-2 |
| RF-4 (Telemetry Publication) | ITEM-4, ITEM-5, DT-4 |
| RF-5 (Supervised Teardown) | ITEM-3, ITEM-5, DT-5 |
| RF-6 (Cross-API Budget Correlation) | ITEM-7, DT-6, DT-7 |
| RF-7 (Secondary Budget Failure) | ITEM-7, DT-7 |
| RF-8 (Multi-adapter Selection) | ITEM-8, DT-8 |
| NFR-1 (Bounded Parent IPC <= 50ms) | ITEM-2, DT-2 |
| NFR-2 (VRAM Reserve Floor) | ITEM-3, DT-3 |
| NFR-3 (Stability Evidence; no universal guarantee) | ITEM-3, ITEM-4, DT-5 |
| NFR-4 (Observability) | ITEM-5, DT-4 |
| NFR-5 (Budget Freshness) | ITEM-7, DT-7 |

---

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Worker process dispatched via re-execution (`/proc/self/exe __gpu_worker`) using an anonymous `socketpair(AF_UNIX, SOCK_STREAM, 0)` passed via inherited FD. Framing uses a fixed 32-byte header with `payload_len`-based reassembly (manual byte-stream framing). | Gives the worker a separate process address space, not a guarantee against kernel driver stalls or system-wide hangs. `SOCK_STREAM` keeps one ordered channel and needs explicit framing. |
| DT-2 | Cache reads and heartbeats share a 50ms absolute monotonic deadline across request writes, response headers, and payloads; startup and teardown use explicit 5s limits. `Update` and `Promote` use one nonblocking frame send capped at 64 KiB. A partial/backpressured or oversized mutation disables the cache and shuts down the socket. | Per-syscall timeouts do not bound a trickled stream. Recomputing remaining time before each read prevents deadline extension, and one nonblocking mutation write cannot stall origin I/O. The parent cannot cancel a driver call already blocked in the child. |
| DT-3 | Strict host safety floor: worker requires fresh driver-reported adapter telemetry and computes `min(request, capacity - reserve, live_available - reserve - 640 MiB)`, where `reserve = max(configured reserve, 20% of min(total, budget))`. Each allocation repeats the live check with the chunk size included. When exact-LUID WDDM telemetry is available, the lower correlated availability constrains admission. | The reserve must be subtracted from current free headroom as well as total capacity; otherwise existing Windows use can consume the intended display reserve. The independent runtime buffer remains available for driver/runtime allocations. |
| DT-4 | Atomic JSON telemetry publication via temporary file rename to `/run/ramshared/wsl2-cache-status.json`. | Prevents readers (`ramshared status`, `ramshared stress`) from seeing partial writes or corrupt JSON. |
| DT-5 | Worker uses `prctl(PR_SET_PDEATHSIG, SIGTERM)`. Shutdown waits at most 5s gracefully, then at most 500ms after SIGKILL; an unconfirmed child handle is handed to a background reaper. | A driver call can leave the child in uninterruptible sleep. The parent must not block in `Child::wait()` or claim that the child exited. Physical exit and GPU resource release remain unconfirmed until observed. |
| DT-6 | Open DXG with the LUID reported by the already-selected CUDA/Vulkan provider; do not enumerate-and-pick a separate “primary” GPU. If the backend lacks a LUID or DXG is unavailable, retain the allocator's own driver-reported budget. | Avoids combining budgets from different physical GPUs and preserves operation on native Linux or WSL configurations without DXG correlation support. |
| DT-7 | For a correlated adapter, admission headroom is `min(allocator.budget - allocator.used, WDDM.budget - WDDM.current_usage, WDDM.available_for_reservation)`; all subtractions saturate and both monotonic samples must be <=5 seconds old. Once attached, a DXG query/identity/freshness failure returns a provider error and prevents allocation. | The lower reported headroom is the conservative cross-API constraint; silently dropping an established guard after a driver error could over-allocate shared host VRAM. |
| DT-8 | Enumerate all CUDA and exact-index Vulkan devices, compute `min(request, capacity - reserve, live_available - reserve - 640 MiB)` against the allocator/WDDM intersection, choose the largest positive target with deterministic CUDA/ordinal/key tie-breaks, then reopen and revalidate identity and budget before serving. | Avoids hardcoded ordinal-zero selection and never ranks on advertised capacity that the reserve or current external use makes unavailable. A failed revalidation leaves the cache unavailable. |

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
| ITEM-2 (Parent IPC Deadline) | #16 | Can a stalled/trickled response or saturated mutation extend parent work beyond its bound? | `cargo test -p ramshared-block ipc_cache_client::tests::read_timeout_falls_back_cleanly`; `...::trickled_response_cannot_extend_the_absolute_read_deadline`; `...::saturated_mutation_socket_does_not_block_origin_thread`; `...::oversize_mutation_disables_cache_without_touching_ipc` | Parent does not fall back/disable at the bound; physical driver cancellation is not established by these source tests |
| ITEM-3 (Worker Crash) | #13 | In the source harness, does worker exit leave the origin backend usable? | `cargo test -p ramshared-wsl2d daemon_survives_abrupt_gpu_worker_kill` | Test harness loses origin service; this does not qualify live NBD or driver behavior |
| ITEM-4 (Teardown) | #17 | Does repeated teardown preserve origin service and avoid an unbounded parent wait when worker exit is unconfirmed? | `cargo test -p ramshared-wsl2d isolated_worker_shutdown_stays_bounded_when_kill_is_not_observed` and `cargo test -p ramshared-block worker_teardown_is_idempotent_and_bounded` | Origin path blocks after the 5s graceful window plus 500ms exit observation, or child ownership is dropped without reaper handoff |

---

## Security checklist (pre-impl)

- [x] Privilege: CUDA/Vulkan access is required for allocation. `/dev/dxg` access is required only to enable the optional WDDM cross-budget guard; neither path requires a new root capability in worker logic.
- [x] User/host copy: worker frames have a 32-byte header and payloads are capped at 16 MiB; cache mutation payloads are capped at 64 KiB before the parent's nonblocking send.
- [x] Flags/IOCTL codes: worker rejects unknown IPC frame types fail-closed.
- [x] Info-leak: no kernel pointers or physical host memory addresses transmitted across IPC frames.
- [x] IRQ/atomic: all GPU operations occur in user-space worker context. Driver calls themselves do not have a hard cancellation deadline.
- [x] Lifetime: socket closure revokes the cache; shutdown is bounded and transfers an unconfirmed child to a background reaper. Physical allocation release is not claimed until exit.
- [x] Hot-unplug: if a device error returns, the client marks `Unavailable` and origin continues; a call stuck in the driver remains isolated but may not exit promptly.
- [ ] Host safety: the worker enforces `max(configured floor, 20%)` plus the separate 640 MiB runtime buffer, but the production origin-cache caller currently defaults the configured floor to 512 MiB while the PRD mitigation requires 1536 MiB. Reconcile the contract and production default, then rerun capacity-boundary and live-adapter qualification.
- [ ] Bounded DMA: only parent IPC waits are bounded; a driver ioctl that has entered an uninterruptible wait cannot be cancelled in userspace.
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
- Cover target: >= 80%

**`crates/ramshared-block/src/ipc_cache_client.rs`**
- Purpose: Socket-based implementation of `BestEffortCache` backed by the isolated worker process.
- RF / DT: RF-2, RF-3, DT-2.
- Types / fns:
  ```rust
  pub struct IpcCacheClient {
      socket: UnixStream,
      read_timeout: Duration,
      state: CacheState,
      cached_bytes: u64,
      target_bytes: u64,
  }
  impl BestEffortCache for IpcCacheClient { ... }
  ```
- Required tests:
  - `ipc_cache_client::tests::read_timeout_falls_back_cleanly`
  - `ipc_cache_client::tests::socket_disconnect_marks_unavailable`
  - `ipc_cache_client::tests::small_update_and_promote_complete_within_the_deadline`
  - `ipc_cache_client::tests::trickled_response_cannot_extend_the_absolute_read_deadline`
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
2. **ITEM-2:** Implement `IpcCacheClient` with an absolute read/heartbeat deadline and one nonblocking mutation frame send; revoke cache after timeout or incomplete frame.
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
| `crates/ramshared-vulkan/src/lib.rs` | `tests::exact_device_open_rejects_out_of_range_ordinal_without_clamping` | ignored software-ICD integration | #13/#16 | >= 80% |

---

## Validation checklist

- [x] `cargo fmt --all -- --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test -p ramshared-block -p ramshared-wsl2d` (also covered by the passing workspace suite)
- [x] Slice coverage: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-block,ramshared-wsl2d --files crates/ramshared-block/src/gpu_cache_worker.rs,crates/ramshared-block/src/ipc_cache_client.rs,crates/ramshared-wsl2d/src/gpu_budget.rs --min 80`
- [ ] Vulkan provider coverage with the hosted Mesa software ICD: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-vulkan --files crates/ramshared-vulkan/src/lib.rs --min 80 --include-ignored`. This runs the Vulkan integration cases marked ignored on machines without an ICD; the CI runner must set `VK_ICD_FILENAMES` to Mesa lavapipe before the gate.
- [ ] Live path verification: `sudo ramshared check --json` confirms `cache_state=ACTIVE` with valid worker instance.
- [ ] Live fault drill: interrupt a worker during cache I/O, verify origin responses within the IPC deadline, capture worker state until reaped, and confirm clean teardown. A software kill test does not establish zero kernel D-state on physical drivers.
