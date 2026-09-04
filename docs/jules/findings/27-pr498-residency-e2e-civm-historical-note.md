# Finding 27: Residency Canary Cross-Host CIVM Swap Drop Historical Note Verification

- **Source PR:** Jules PR
- **Crate:** `ramshared-wsl2d`
- **Module:** `residency.rs`
- **Classification:** `FINDING_ONLY`

## Observation

The comment in `crates/ramshared-wsl2d/src/residency.rs:253` documents a historical bug (DT-31 / e2e cross-host civm) where a latency multiplier threshold of `8×` triggered false positive demotions under heavy server load (~17× baseline load spike), erroneously dropping the VRAM swap device.

The issue was resolved by recalibrating `ResidencyConfig::latency_mult` to `64×` (default), which provides ample margin between heavy load spikes (17×) and actual WDDM eviction latency spikes (330×).

This historical behavior and current fix are fully guarded by unit test `load_spike_below_threshold_stays_ok`.

## Verdict

Accepted as documented architectural verification (`FINDING_ONLY`).
