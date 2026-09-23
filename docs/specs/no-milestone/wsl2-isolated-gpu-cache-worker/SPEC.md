# SPEC — Process-isolated GPU cache worker for WSL2 origin swap

> SSDV3 Step 2 · PRD: docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/PRD.md

## Closed scope

### In now
- Process-isolated GPU cache worker execution model for `ramshared-wsl2d`.
- Framing protocol for non-blocking socket IPC between daemon and worker.
- LRU chunk management and physical VRAM allocation using `DxgProvider` / `/dev/dxg`.
- Dedicated control lane for synchronous revocation (`Disable`) and atomic telemetry querying.
- Fail-closed fallback: transparent origin serving upon worker timeout, crash, or disconnect.
- Real-time telemetry publication to `/run/ramshared/wsl2-cache-status.json`.
- Clean lifecycle supervision and child process reaping with `PR_SET_PDEATHSIG`.

### Out now
- Custom Linux kernel driver changes (operates over standard upstream WSL2 kernel and userspace `/dev/dxg`).
- Windows Host service modifications (host guardian contracts remain untouched).
- Persistent non-volatile VRAM caching across host power cycles.

### Assumed-ready dependencies
- `AuthoritativeOriginBackend` and `BoundedCacheClient` in `crates/ramshared-block/src/isolated_origin.rs`.
- `DxgProvider` in `crates/ramshared-vram/src/dxg.rs`.
- `/dev/dxg` device node present and accessible in WSL2 environment.

---

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 (Process Isolation) | ITEM-1, ITEM-3, DT-1 |
| RF-2 (Bounded IPC Protocol) | ITEM-2, DT-2 |
| RF-3 (Fail-Closed Fallback) | ITEM-2, ITEM-4, DT-3 |
| RF-4 (Telemetry Publication) | ITEM-4, ITEM-5, DT-4 |
| RF-5 (Supervised Teardown) | ITEM-3, ITEM-5, DT-5 |
| NFR-1 (Bounded Latency <= 50ms) | ITEM-2, DT-2 |
| NFR-2 (VRAM Reserve Floor) | ITEM-3, DT-3 |
| NFR-3 (Zero-Panic Guarantee) | ITEM-4, DT-3 |
| NFR-4 (Observability) | ITEM-5, DT-4 |

---

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Worker process dispatched via re-execution (`/proc/self/exe __gpu_worker`) using an anonymous `socketpair(AF_UNIX, SOCK_SEQPACKET, 0)` passed via inherited FD. | Eliminates the need for a separate binary packaging artifact while providing full address space and memory crash isolation. `SOCK_SEQPACKET` preserves frame boundaries without manual byte-stream reassembly. |
| DT-2 | 50ms hard timeout for cache read responses; non-blocking zero-wait sends for `Update` and `Promote`. | Origin swap must never stall waiting on GPU completion. Misses and timeouts fall back immediately to the authoritative SSD origin. |
| DT-3 | Strict host safety floor: worker queries adapter budget and caps maximum allocation at `total - max(1536 MiB, 20%)`. | Guarantees that Windows host graphics, desktop compositor, and external GPU workloads never suffer out-of-memory errors due to RamShared. |
| DT-4 | Atomic JSON telemetry publication via temporary file rename to `/run/ramshared/wsl2-cache-status.json`. | Prevents readers (`ramshared status`, `ramshared stress`) from seeing partial writes or corrupt JSON. |
| DT-5 | Worker containment uses `prctl(PR_SET_PDEATHSIG, SIGTERM)` upon child startup, plus a 5-second bounded join timeout in parent daemon before SIGKILL escalation. | Guarantees zero zombie processes or orphaned GPU allocations if the daemon crashes or is terminated abruptly. |

---

## Atomicity and rollback

### Atomicity frontier
- **Origin Backend (Authoritative):** Operates independently of the worker. Origin writes always complete and synchronize to disk prior to block layer acknowledgement.
- **Cache Worker (Ephemeral):** State is purely non-authoritative. Worker loss never invalidates data durability.
- **IPC Channel (Boundary):** Socket closure triggers an irrevocable transition to `CacheState::Unavailable` in the client, cleanly severing cache operations without touching block device integrity.

### Rollback
- **Daemon layer:** Revert to `DisabledCache` selection if worker process initialization fails.
- **Worker layer:** Child process dies; kernel OS reclaims `/dev/dxg` allocations automatically on process exit.
- **Host layer:** No persistent state modified; `/proc/swaps` and NBD device remain intact.

---

## Kahneman map (critical only)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 (Read Timeout) | #16 | Can a hung GPU ioctl delay an NBD block read past acceptable swap latency? | `cargo test -p ramshared-block isolated_worker_read_timeout_falls_back_to_origin` | Read takes > 50ms or returns EIO |
| ITEM-3 (Worker Crash) | #13 | If the worker process receives SIGKILL, does the daemon stay alive and continue serving swap? | `cargo test -p ramshared-wsl2d daemon_survives_abrupt_gpu_worker_kill` | Daemon crashes or NBD client disconnects |
| ITEM-4 (Teardown) | #17 | Does repeated teardown (`Disable` + reap) cleanly release all GPU memory without hanging? | `cargo test -p ramshared-block worker_teardown_is_idempotent_and_bounded` | Child remains zombie or cleanup > 5s |

---

## Security checklist (pre-impl)

- [x] Privilege: worker requires access to `/dev/dxg` (standard video group permissions in WSL2). No root capability needed for worker logic.
- [x] User/host copy: frames use bounded sizes (max 64 KiB payload + 32-byte header).
- [x] Flags/IOCTL codes: worker rejects unknown IPC frame types fail-closed.
- [x] Info-leak: no kernel pointers or physical host memory addresses transmitted across IPC frames.
- [x] IRQ/atomic: all GPU operations occur in user-space worker context with bounded timeouts.
- [x] Lifetime: socket closure triggers automatic worker buffer deallocation and clean process exit.
- [x] Hot-unplug: if GPU device disappears, worker ioctl fails, worker exits, client marks `Unavailable`, origin continues.
- [x] Host safety: mathematical reserve floor enforced (`max(1536 MiB, 20%)`).
- [x] Bounded DMA: worker ioctls wrapped with bounded context timeouts.
- [x] Cooperative cascade spillover: cache miss or failure immediately spills to SSD origin.
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
  - `ipc_cache_client::tests::update_and_promote_are_non_blocking`
- Cover target: >= 80%

### MODIFY

**`crates/ramshared-block/src/lib.rs`**
- Export `gpu_cache_worker` and `ipc_cache_client` modules and types.

**`crates/ramshared-wsl2d/src/main.rs`**
- In `run_nbd_origin_loop`: Replace `DisabledCache` with spawned worker child and `IpcCacheClient`.
- Implement `spawn_isolated_gpu_worker` with `socketpair` and `PR_SET_PDEATHSIG`.
- Update telemetry loop to record live `vram_cached_kib` and `cache_state=ACTIVE`.

---

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| `cache_state` | `/run/ramshared/wsl2-cache-status.json` | String (`ACTIVE`, `UNAVAILABLE`, `OFF`) |
| `vram_cached_kib` | `/run/ramshared/wsl2-cache-status.json` | Counter (`u64`) |
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
2. **ITEM-2:** Implement `IpcCacheClient` adhering to `BestEffortCache` with hard read timeouts.
3. **ITEM-3:** Implement `GpuCacheWorker` memory manager with chunk LRU and host reserve floor.
4. **ITEM-4:** Implement child process spawning and supervision in `ramshared-wsl2d`.
5. **ITEM-5:** Wire real-time telemetry output to `/run/ramshared/wsl2-cache-status.json`.
6. **ITEM-6:** Add hermetic fault-injection tests (process kill, timeout, socket tear).

---

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::read_timeout_falls_back_cleanly` | unit | #16 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::socket_disconnect_marks_unavailable` | unit | #13 | >= 80% |
| `crates/ramshared-block/src/ipc_cache_client.rs` | `tests::update_and_promote_are_non_blocking` | unit | #9 | >= 80% |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `tests::worker_handshake_and_read_hit_cycle` | unit | #9 | >= 80% |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `tests::worker_respects_headroom_floor` | unit | #16 | >= 80% |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `tests::worker_disable_frees_allocations` | unit | #17 | >= 80% |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::daemon_survives_abrupt_gpu_worker_kill` | integration | #13 | >= 80% |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::daemon_publishes_live_worker_telemetry` | integration | #9 | >= 80% |

---

## Validation checklist

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test -p ramshared-block -p ramshared-wsl2d`
- [ ] Slice coverage: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-block,ramshared-wsl2d --files crates/ramshared-block/src/gpu_cache_worker.rs,crates/ramshared-block/src/ipc_cache_client.rs --min 80`
- [ ] Live path verification: `sudo ramshared check --json` confirms `cache_state=ACTIVE` with valid worker instance.
- [ ] Fault injection drill: `kill -9 <worker_pid>` during active I/O verifies zero kernel D-state and clean fallback to SSD origin.
