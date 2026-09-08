# Finding: Swap dropping under load (e2e civm bug)

## Context & Analysis
The comment in `crates/ramshared-wsl2d/src/residency.rs` at line 171 refers to a historical bug and calibration context:
```rust
// With 8× this triggered and dropped the swap under load (e2e civm bug); with 64×, it remains Ok.
```

## Finding Summary
1. **Historical Context**: In earlier tuning (DT-31), a 8× baseline multiplier for canary latency detection caused false positive triggers when serving latency reached ~17× baseline under heavy end-to-end civm load. This caused the residency canary to incorrectly signal `DemoteReason::Latency` and drop swap under load.
2. **Current Implementation**: The parameter `latency_mult` in `ResidencyConfig` default was updated to 64× (well above 17× load latency spikes and well below 330× WDDM eviction spikes).
3. **Verification**: The unit test `load_spike_below_threshold_stays_ok` in `crates/ramshared-wsl2d/src/residency.rs` explicitly tests and validates this behavior, ensuring that 17× load spikes remain `Verdict::Ok` and do not drop swap.
4. **Action**: The issue describes historical context rather than an active bug requiring code changes. Aborting unsafe modifications per orthogonal slice rules, generating this finding artifact.
