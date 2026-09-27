# IMPL — WSL2 VMBus Anti-Fragmentation Governor and Dedicated Ring Pool Resilience

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-vmbus-anti-fragmentation-governor/SPEC.md`

## Status

Partial · buddyinfo governor requirements remain implemented; RF-6 Tier 3-only stress and dynamic cache target pass source regression and coverage gates. Live pressure qualification was not run.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-cli/src/stress.rs` | ITEM-1, ITEM-2, ITEM-5, ITEM-6 / RF-1..4, RF-6 | Buddyinfo order-7 parser, elevated headroom floor (1024 MB), anti-fragmentation interlock, GPU-independent Tier 3-only mode, active-cache-derived full-profile target, and no cross-adapter NVIDIA admission probe. |
| `crates/ramshared-vram/src/lib.rs` | GPU budget contract | Added adapter identity, normalized Windows LUID matching across APIs, budget source, freshness, and fail-closed admission policy. |
| `crates/ramshared-vulkan/src/lib.rs` | GPU budget contract | Queries `VK_EXT_memory_budget`, reports physical-device UUID and valid Windows LUID when exposed, and marks fallback estimates ineligible for automatic cache admission. |
| `crates/ramshared-cuda/src/driver.rs`, `crates/ramshared-cuda/src/vram_impl.rs` | GPU budget contract | Loads optional CUDA UUID and LUID queries; reports driver free/total with adapter-bound identity when available. |
| `crates/ramshared-dxg/src/lib.rs` | GPU budget contract | Converts the WDDM budget and adapter LUID into the shared representation. |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | GPU budget contract | Requires a fresh, adapter-bound, driver-reported snapshot at worker setup and before each allocation. |
| `crates/ramshared-block/src/ipc_cache_client.rs`, `crates/ramshared-wsl2d/src/main.rs` | GPU budget telemetry | Returns a bounded budget snapshot with worker heartbeats and publishes it in the daemon cache-status file. |
| `crates/ramshared-cli/src/cascade/mod.rs`, `crates/ramshared-cli/src/cascade/lifecycle.rs` | GPU budget telemetry | Validates status freshness and driver budget arithmetic, reports adapter identity and capacity in `status --json`, and leaves headroom unknown when telemetry is stale or malformed. |
| `crates/ramshared-cli/src/monitor.rs` | GPU dashboard telemetry | Reads only the fresh adapter-bound budget published by the active cache worker; omits the GPU sample when unavailable and reports budget usage without vendor-specific probes, guessed PCIe values, idle speedups, or default latencies. |
| `crates/ramshared-cli/src/main.rs` | ITEM-5 / RF-6 | CLI help documents the separate Tier 3-only mode. |
| `docs/upstream/patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch` | ITEM-3 / RF-5 | Upstream patch formulation for VMBus ring virtual allocation fallback. |

## Validation

- Targeted tests: `cargo test -p ramshared-cli tier3_only` and `cargo test -p ramshared-cli full_profile` passed.
- Full CLI suite: `cargo test -p ramshared-cli` passed (331 unit tests + 10 dispatch tests).
- Strict Clippy passed: `cargo clippy -p ramshared-cli --all-targets -- -D warnings`.
- Monitor tests cover active adapter budget rendering and reject stale, local-only, malformed, or unidentified telemetry; an absent GPU sample does not change the host status. Unmeasured tier throughput, latency, and link data render as unavailable instead of hardware estimates.
- Slice coverage passed at 80.9%: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/stress.rs --min 80`.
- `cargo fmt --check`, `git diff --check`, and `./scripts/docs-check.sh` passed.
- No live Tier 3 stress, GPU allocation, or host qualification was performed; existing host external-swap pressure makes a live campaign unsafe to start now.

## Gaps

- Cross-vendor product qualification remains open: stress no longer compares its active cache worker to a separate NVIDIA-only probe, and the worker gates allocation using its active provider. The daemon publishes that worker's budget and identity; `status --json` and the interactive dashboard accept only fresh, well-formed, driver-reported telemetry. The dashboard no longer uses NVIDIA-specific observation or reports unmeasured PCIe, throughput, or latency values. The isolated worker optionally intersects its selected CUDA/Vulkan budget with WDDM for the exact matching Windows LUID and fails closed on stale or failed WDDM observations after guard activation. The policy module passes unit tests and 93.0% slice coverage, but the source path is not live-qualified; multi-GPU selection remains unqualified and no AMD/Intel physical cache campaign has run. Do not claim all-vendor cascade support.
- Tier 3-only source behavior is unit-tested but not qualified by a live saturation run. Its verdict is explicitly separate from full-cascade qualification.

## Rollback trigger

Revert changes if buddyinfo parsing causes panics on non-standard kernel zone layouts or if false-positive halts occur when order-7 is abundant.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1 | ITEM-1, ITEM-2 | `6b4a6901`, `3a1b75eb` |
| RF-2 | ITEM-2 | `6b4a6901`, `3a1b75eb` |
| RF-3 | ITEM-2 | `6b4a6901`, `3a1b75eb` |
| RF-4 | ITEM-2 | `6b4a6901`, `3a1b75eb` |
| RF-5 | ITEM-3 | `3a1b75eb` |
| RF-6 | ITEM-5, ITEM-6 | working tree (uncommitted) |
