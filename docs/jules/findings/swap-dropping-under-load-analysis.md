# Analysis Finding: Swap Dropping Under Load (e2e civm bug)

## Context & Rationale Analysis
- **Target Location**: `crates/ramshared-wsl2d/src/residency.rs`
- **Description**: "Not inferrable"
- **Task Context Comment**: `// With 8x this triggered and dropped the swap under load (e2e civm bug); with 64x, it remains Ok.`
- **Assessment**: As stated in the task rationale, this code comment documents a historical bug (DT-31) where an 8x latency multiplier triggered false positives under load (~17x baseline) and demoted the swap. The default `ResidencyConfig` latency multiplier was previously adjusted to 64x, and regression test `load_spike_below_threshold_stays_ok` verifies that load spikes (~17x) remain `Verdict::Ok`.

## Conclusion
The codebase is fully compliant with the requirements and already includes the necessary protection (64x threshold) and regression testing. In accordance with the project's ORTHOGONAL SCOPE principles, no code modifications are required in production or test files.
