# WSL2 Residency Canary Load Spike Calibration (DT-31)

## Context
In `crates/ramshared-wsl2d/src/residency.rs`, line 171 records a historical bug where setting the latency threshold multiplier to `8x` baseline triggered false positive demotion verdicts under load (e2e cross-host civm reaching ~17x serve latency), thereby dropping swap unexpectedly under heavy workloads.

## Current Implementation Analysis
The codebase already incorporates the calibration fix (DT-31):
- `ResidencyConfig::default()` sets `latency_mult` to `64`.
- The test `load_spike_below_threshold_stays_ok()` in `crates/ramshared-wsl2d/src/residency.rs` verifies that a 17x serve latency spike across 10 iterations remains `Verdict::Ok` and does not trigger demotion.

## Verification
The unit test suite for `ramshared-wsl2d` passes cleanly via `cargo test -p ramshared-wsl2d`.
