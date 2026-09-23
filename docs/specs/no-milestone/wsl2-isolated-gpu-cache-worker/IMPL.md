# IMPL — Process-isolated GPU cache worker for WSL2 origin swap

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/SPEC.md

## Status

**PARTIAL (hermetic source and fault-injection gates passed)** · historical cover ✓ (86.7% / 88.7%) · E2E env-bound (requires host GPU validation) · BINARY_MATCH pending. The September 23 audit reproduced and locally corrected partial-chunk cache hits, estimated allocation telemetry, and missing live free-VRAM admission. The changed slice requires a fresh coverage gate and physical host evidence before qualification.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | ITEM-1, ITEM-3 / RF-1, RF-2 | Implemented 32-byte framing IPC protocol, `GpuCacheWorker` with LRU eviction and headroom floor enforcement (`max(1536 MiB, 20%)`), clean deallocation, and socket loop runner. |
| `crates/ramshared-block/src/ipc_cache_client.rs` | ITEM-2 / RF-2, RF-3 | Implemented socket-backed `BestEffortCache` client with bounded 50ms read timeout, non-blocking asynchronous mutation dispatch (`update`, `promote`), and fail-closed transition to `CacheState::Unavailable`. |
| `crates/ramshared-block/src/lib.rs` | ITEM-1, ITEM-2 | Exported `gpu_cache_worker` and `ipc_cache_client` modules and core types. |
| `crates/ramshared-wsl2d/src/main.rs` | ITEM-4, ITEM-5 / RF-1, RF-4, RF-5 | Integrated `__gpu_worker` re-exec via `socketpair(AF_UNIX, SOCK_STREAM, 0)` with manual 32-byte framing, `PR_SET_PDEATHSIG`, bounded 5s supervisor teardown, replaced `DisabledCache` in authoritative origin backend with supervised `OriginCache::Ipc`, and wired atomic status publication to `/run/ramshared/wsl2-cache-status.json`. |

## Validation Results

1. **Unit & Protocol Tests (`ramshared-block`)**:
   - `gpu_cache_worker::tests::worker_handshake_and_read_hit_cycle` — **PASS**
   - `gpu_cache_worker::tests::worker_respects_headroom_floor` — **PASS**
   - `gpu_cache_worker::tests::worker_disable_frees_allocations` — **PASS**
   - `gpu_cache_worker::tests::worker_evicts_coldest_chunk_on_pressure` — **PASS**
   - `gpu_cache_worker::tests::worker_handles_promote_and_heartbeat_loop` — **PASS**
   - `ipc_cache_client::tests::read_timeout_falls_back_cleanly` — **PASS**
   - `ipc_cache_client::tests::socket_disconnect_marks_unavailable` — **PASS**
   - `ipc_cache_client::tests::update_and_promote_are_non_blocking` — **PASS**
   - `gpu_cache_worker::tests::worker_teardown_is_idempotent_and_bounded` — **PASS**
   - Workspace suite: 104 passed, 0 failed.

2. **Crash Containment & Fault Injection (`ramshared-wsl2d`)**:
   - `tests::daemon_survives_abrupt_gpu_worker_kill` — **PASS**: Worker killed with `SIGKILL`; daemon continues serving origin reads without panic or hang; client transitions cleanly to `Unavailable`.
   - `tests::daemon_publishes_live_worker_telemetry` — **PASS**: Atomic telemetry published via temporary file rename to `/run/ramshared/wsl2-cache-status.json` with active status and cached kibibytes.
   - Package test suite: 256 passed, 0 failed.

3. **Rust Slice Coverage Gate**:
   - `crates/ramshared-block/src/gpu_cache_worker.rs`: **86.7%** (365 / 421 lines, >= 80% threshold) — **PASS**
   - `crates/ramshared-block/src/ipc_cache_client.rs`: **88.7%** (196 / 221 lines, >= 80% threshold) — **PASS**
   - Gate command: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-block --files crates/ramshared-block/src/gpu_cache_worker.rs,crates/ramshared-block/src/ipc_cache_client.rs --min 80` — **PASS**

4. **Code Quality & Lints**:
   - `cargo fmt --all --check` — **PASS**
   - `cargo clippy --workspace --all-targets -- -D warnings` — **PASS** (0 warnings, 0 errors)
   - `./scripts/docs-check.sh` — **PASS (`✓ docs-check OK`)**

5. **Kahneman Map Disciplines Addressed**:
   - **#13 (Worker Crash)**: Process isolation verified under abrupt SIGKILL. Zero panic, zero NBD stall.
   - **#16 (Read Latency Bounded)**: Hard 50ms read timeout in client prevents GPU driver hangs from stalling origin reads.
   - **#17 (Teardown Idempotency & Bounded Reap)**: Supervisor joins with 5s timeout, escalating to SIGKILL on unresponsive worker. Clean teardown guaranteed.

## Open Evidence (Live Host Qualification)

- **Physical Host Campaign**: Attended execution of 3-round host qualification on Windows 11 host with physical GPU (RTX 2060 or modern GeForce/Radeon) under `/dev/dxg` / GPU-PV.
- **Three-Tier Verification**: Full verification of ZRAM (Tier 1) + Physical VRAM Cache (Tier 2) + Authoritative SSD Origin (Tier 3) under swap load with `vram_cached_kib > 0` and zero host compositor crashes.
