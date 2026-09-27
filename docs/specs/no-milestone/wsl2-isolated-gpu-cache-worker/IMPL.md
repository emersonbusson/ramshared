# IMPL — Process-isolated GPU cache worker for WSL2 origin swap

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/SPEC.md

## Status

**PARTIAL (hermetic source, fault-injection, lint, and Linux coverage gates passed)** · GPU worker runtime, Windows host installation, and physical qualification remain open. The September 23 audit reproduced and locally corrected partial-chunk cache hits, estimated allocation telemetry, and missing live free-VRAM admission. The source ranks CUDA/Vulkan adapters by fresh reserve-adjusted, exact-LUID-constrained safe target, then reopens and revalidates the selected adapter. Parent response paths use one absolute monotonic deadline across partial socket reads and writes; cache mutations use one nonblocking send capped at 64 KiB and revoke the cache if the frame is oversized, backpressured, or partially queued. Worker shutdown is bounded to a 5-second graceful window plus 500ms exit observation after SIGKILL; an unconfirmed child is handed to a background reaper. This keeps the daemon from waiting indefinitely but cannot prove that an uninterruptible driver call exits or frees GPU memory. Physical host evidence remains required before qualification.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | ITEM-1, ITEM-3 / RF-1, RF-2 | Implemented 32-byte framing IPC protocol, `GpuCacheWorker` with LRU eviction and headroom floor enforcement (`max(1536 MiB, 20%)`), allocation cleanup on worker disable, and the socket loop. Cleanup is not confirmed if a driver call prevents worker progress or exit. |
| `crates/ramshared-block/src/ipc_cache_client.rs` | ITEM-2 / RF-2, RF-3 | Implemented socket-backed `BestEffortCache` client with absolute monotonic deadlines across response reads and request/heartbeat writes. Cache mutations use one nonblocking frame send capped at 64 KiB; oversized, partial, or backpressured sends fail closed and shut down the socket. |
| `crates/ramshared-block/src/lib.rs` | ITEM-1, ITEM-2 | Exported `gpu_cache_worker` and `ipc_cache_client` modules and core types. |
| `crates/ramshared-wsl2d/src/main.rs` | ITEM-4, ITEM-5 / RF-1, RF-4, RF-5 | Integrated `__gpu_worker` re-exec via `socketpair(AF_UNIX, SOCK_STREAM, 0)` with manual 32-byte framing and `PR_SET_PDEATHSIG`; startup handshake failures shut down the child. Worker stop waits at most 5s gracefully and 500ms after SIGKILL before handing an unconfirmed child handle to a background reaper. The authoritative origin uses supervised `OriginCache::Ipc`; status publication is atomic at `/run/ramshared/wsl2-cache-status.json`. |
| `crates/ramshared-dxg/src/lib.rs`, `crates/ramshared-wsl2d/src/main.rs` | ITEM-7 / RF-6, RF-7 | Added canonical Windows LUID parsing and an optional WDDM budget guard for the selected CUDA/Vulkan allocator. When the same LUID is available, worker headroom is the minimum of allocator and WDDM headroom; stale samples, identity mismatch, and errors after guard activation block new allocation. Missing DXG/LUID keeps the selected provider's existing admission path. |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | ITEM-7 / RF-6, RF-7 | Isolated budget policy, exact-LUID provider correlation, and fail-closed WDDM wrapper from daemon entry-point wiring; named tests cover admission, startup fallback, adapter errors, and worker allocation prevention. Latest Linux slice coverage passes at 93.2%. |
| `crates/ramshared-vulkan/src/lib.rs` | ITEM-8 / RF-8 | Added exact Vulkan ordinal open and device enumeration so cross-backend ranking cannot silently resolve a requested ordinal to the first discrete device. |
| `crates/ramshared-wsl2d/src/main.rs`, `crates/ramshared-wsl2d/src/gpu_budget.rs` | ITEM-8 / RF-8, DT-8 | Enumerate CUDA and Vulkan candidates, apply the actual reserve/WDDM admission formula, choose largest safe target with stable tie-breaking, and fall back to a zero-target origin-only handshake if selected identity or budget revalidation fails. |

## Validation Results

1. **Unit & Protocol Tests (`ramshared-block`)**:
   - `gpu_cache_worker::tests::worker_handshake_and_read_hit_cycle` — **PASS**
   - `gpu_cache_worker::tests::worker_respects_headroom_floor` — **PASS**
   - `gpu_cache_worker::tests::worker_disable_frees_allocations` — **PASS**
   - `gpu_cache_worker::tests::worker_evicts_coldest_chunk_on_pressure` — **PASS**
   - `gpu_cache_worker::tests::worker_handles_promote_and_heartbeat_loop` — **PASS**
   - `ipc_cache_client::tests::read_timeout_falls_back_cleanly` — **PASS**
   - `ipc_cache_client::tests::invalid_timeout_configuration_disables_cache_before_io` — **PASS**
   - `ipc_cache_client::tests::socket_disconnect_marks_unavailable` — **PASS**
   - `ipc_cache_client::tests::small_update_and_promote_complete_within_the_deadline` — **PASS**
   - `ipc_cache_client::tests::trickled_response_cannot_extend_the_absolute_read_deadline` — **PASS** (reproduced a 251 ms wait under the old 30 ms per-call timeout; fixed to complete the parent call at the absolute deadline)
   - `gpu_cache_worker::tests::worker_teardown_is_idempotent_and_bounded` — **PASS**
   - Current package suite: 118 passed, 0 failed.

2. **Crash Containment & Fault Injection (`ramshared-wsl2d`)**:
   - `tests::daemon_survives_abrupt_gpu_worker_kill` — **PASS**: Worker killed with `SIGKILL`; origin reads continue and the client transitions to `Unavailable`.
   - `tests::isolated_worker_shutdown_stays_bounded_when_kill_is_not_observed` — **PASS**: a fake child whose exit remains unobserved is handed to a background reaper without an unbounded supervisor wait.
   - `tests::daemon_publishes_live_worker_telemetry` — **PASS**: Atomic telemetry published via temporary file rename to `/run/ramshared/wsl2-cache-status.json` with active status and cached kibibytes.
   - Current workspace run passed the `ramshared-wsl2d` library and daemon suites plus applicable integrations; hardware/root-only cases remained ignored.

3. **Rust Slice Coverage Gate**:
   - Latest gate: `gpu_cache_worker.rs` **93.4%** (764 / 818 lines), `ipc_cache_client.rs` **84.2%** (368 / 437 lines), and `gpu_budget.rs` **93.2%** (591 / 634 lines) — **PASS**.
   - Gate command: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-block,ramshared-wsl2d --files crates/ramshared-block/src/gpu_cache_worker.rs,crates/ramshared-block/src/ipc_cache_client.rs,crates/ramshared-wsl2d/src/gpu_budget.rs --min 80` — **PASS**

4. **Code Quality & Lints**:
   - `cargo fmt --all --check` — **PASS**
   - `cargo clippy --workspace --all-targets -- -D warnings` — **PASS** (0 warnings, 0 errors)
   - `./scripts/docs-check.sh` — **PASS (`✓ docs-check OK`)**

5. **Kahneman Map Disciplines Addressed**:
   - **#13 (Worker Crash)**: Process isolation and origin service are verified after abrupt SIGKILL. A stuck driver call remains a separate physical case.
   - **#16 (Parent IPC Deadline)**: The client bounds total IPC work across partial reads and writes and closes the socket on failure; this does not cancel a driver call inside the child.
   - **#17 (Teardown Idempotency & Bounded Reap)**: Graceful wait is capped at 5s, post-SIGKILL observation at 500ms, then an asynchronous reaper retains the child handle. The source test simulates an exit that remains unobserved; physical child release is not guaranteed.

6. **WDDM and allocator budget composition (2026-09-25)**:
   - `cargo test -p ramshared-dxg -p ramshared-wsl2d -- --quiet` — **PASS**: DXG 13, WSL library 142, daemon 104, and applicable integration tests passed; hardware/root-dependent tests remain ignored.
   - `cargo clippy -p ramshared-dxg -p ramshared-wsl2d --all-targets -- -D warnings` and `cargo fmt --all -- --check` — **PASS**.
   - Named guard tests cover same-LUID minimum headroom, mismatched LUID, stale/future snapshots, invalid used-vs-budget arithmetic, and a WDDM query error preventing allocation.
   - Initial full-file coverage of the large daemon entry point was 78.8% (6708/8516). The budget policy was separated into its own business-logic module and the required gate now targets that SPEC slice: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-wsl2d --files crates/ramshared-wsl2d/src/gpu_budget.rs --min 80` — **PASS (93.0%, 359/386)**.
   - No live `/dev/dxg` query, CUDA/Vulkan allocation, multi-adapter exercise, physical GPU stress, or host installation was performed.

7. **Cross-backend adapter selection (2026-09-25)**:
   - `cargo test -p ramshared-vulkan -p ramshared-wsl2d -- --quiet` — **PASS**: daemon library 151, binary 101, applicable integration tests passed; hardware/root tests remain ignored.
   - `cargo clippy -p ramshared-vulkan -p ramshared-wsl2d --all-targets -- -D warnings`, format, and whitespace checks — **PASS**.
   - `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-wsl2d --files crates/ramshared-wsl2d/src/gpu_budget.rs --min 80` — **PASS (93.9%, 447/476)**. New tests verify request cap, reserve, freshness/future rejection, largest safe target, and deterministic tie-break.
   - Host preflight found `/dev/dxg`, an RTX 2060 with 4,976 MiB free and 6% utilization at observation, but only 988 MiB WSL memory available and 4,193,160/4,194,304 KiB fallback swap used. Installed RamShared status was `Off` with `guardian_state=BLOCKED` due to stale telemetry. No installation or GPU/memory stress was started.
   - Windows WMI reported LG ULTRAWIDE and DP2HDMI monitors active; no matching Display/NVIDIA/DXG events appeared in the preceding 45 minutes. This source/test session did not install or run a GPU workload, so it supplies no evidence linking the user's dark monitor to RamShared.

## Open Evidence (Live Host Qualification)

- **Physical Host Campaign**: Attended execution of 3-round host qualification on Windows 11 host with physical GPU (RTX 2060 or modern GeForce/Radeon) under `/dev/dxg` / GPU-PV.
- **Three-Tier Verification (open)**: Qualify ZRAM (Tier 1), physically allocated VRAM cache (Tier 2), and authoritative SSD origin (Tier 3) together under supervised swap load. Require worker identity/allocation evidence, cache fallback and teardown records, plus host and guest logs; no compositor-crash or no-panic guarantee follows from source tests.

## 2026-09-26 — Startup readiness publication

- The host campaign reached the selected Vulkan worker and NBD socket, then `ramshared up` failed with `daemon did not publish a valid current cache identity`. It stopped before NBD attach, `BINARY_MATCH`, or any stress tier; the run has no `stress.json` or tier evidence. The exact sealed origin VHDX was detached after the failed attempt.
- A named daemon test reproduced the readiness gap: origin mode could exit before publishing any current cache status. The daemon now publishes the first status immediately after listener and shutdown-bridge setup, using the same status path as later polling. The CLI readiness bound is 15 seconds to allow bounded cold GPU initialization.
- Regression test was RED before the change and GREEN afterward. `cargo test -p ramshared-wsl2d` passed 151 library tests, 101 daemon tests, and all applicable integrations; 19 hardware/root-only tests remained ignored. `cargo test -p ramshared-cli` passed 330 unit tests and 10 CLI integration tests. Clippy (`-p ramshared-wsl2d -p ramshared-cli --all-targets -- -D warnings`), formatting, and `git diff --check` passed.
- The revised source has not been release-built or installed. At the failed campaign the guardian evidence was stale; a later plan-only preflight reports fresh `HEALTHY` evidence and 27,802 MiB host commit headroom against 20,480 MiB required. RamShared remains Off; no live cache activation, physical VRAM allocation, or three-tier stress qualification is claimed. Repeat only after installing a binary built from this source.

## 2026-09-26 — Host swap and installed-release recheck

- Windows `.wslconfig` currently sets a 4 GiB fallback swap at `C:/wsl/swap.vhdx`; that file exists at 4,300,210,176 bytes. The guest reports an active 4 GiB default-priority swap device. The non-elevated `Get-VHD` query was denied, so this records the configured path and active guest swap, not an independently verified VHD attachment. The distro root VHDX remains on `I:`.
- The historical storage comparison in `docs/BENCHMARKS.md` measured synchronous writes at 85.4 MB/s on C: and 38.0 MB/s on I: for the tested drives and workload. That supports the current C: swap-file placement for that profile; it is not a fresh benchmark of current host hardware.
- RamShared currently reports `Off`, with no daemon process; only the 4 GiB fallback swap is active. The installed release metadata is timestamped 10:20, while the readiness-fix source files were modified at 11:02 and 11:05. The startup-readiness fix is therefore not in the installed release, and the host campaign has not been repeated against it.

## 2026-09-27 — Parent IPC deadline and source gates refreshed

- `CARGO_BUILD_JOBS=2 cargo test --workspace -- --quiet` passed on the current
  worktree. Hardware, root-only, and Windows-only tests remained ignored or
  unavailable on this Linux host.
- `CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, `git diff --check`, and
  `./scripts/docs-check.sh` passed.
- The SPEC coverage gate passed at 93.4% for `gpu_cache_worker.rs`, 84.2% for
  `ipc_cache_client.rs`, and 93.2% for `gpu_budget.rs`. Separate gates passed at
  94.8% for `ramshared-vram/src/lib.rs`, 90.0% for `ramshared-ipc/src/lib.rs`,
  and 85.7% for `ramshared-ipc/src/vsock.rs`.
- The host-gate slice passed at 95.0%. The first native-vsock coverage command
  revealed that its matrix pointed VHDX lease tests at a `cfg(windows)` file
  that does not contain those tests. Source review located them in
  `control_plane.rs`; the SPEC matrix and command were corrected, and the
  combined gate passed there at 87.0%. The separate Windows workflow tests the
  product composition; its live listener remains unqualified.
- The updated shell pressure fixtures and syntax checks passed. PowerShell is
  not installed in this environment, so changed `.ps1` tests were not executed
  here. No GPU worker, live WSL pressure, stress campaign, host install, or
  hardware qualification ran.

**Verdict:** 🟡 `PARTIAL` — current source and Linux gates pass; Windows-specific
coverage, live worker parity, GPU hardware, and three-tier qualification remain
open.
