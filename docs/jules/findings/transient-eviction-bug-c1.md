# Finding: Transient Eviction Bug C1 Rationale Analysis

## Overview
- **File**: `crates/ramshared-wsl2d/src/broker_srv.rs:803`
- **Context**: `// a transient eviction (1-2 DEMOTEs) and the eviction signal would never appear (bug C1).`

## Analysis
The comment at line 803 in `crates/ramshared-wsl2d/src/broker_srv.rs` documents the rationale for handling `ReconcileFlag::Eviction` as an immediate canary event bypass of hysteresis:

```rust
// Hysteresis (DT-12) for SUSTAINED flags (Unaccounted/StuckSlice/Partial): confirms after
// `recon_streak` consecutive identical ticks. `Eviction` is a canary EVENT (DT-6;
// `demotes_delta` is per-tick, lasts 1 tick) -> immediate confirmation; otherwise hysteresis would swallow
// a transient eviction (1-2 DEMOTEs) and the eviction signal would never appear (bug C1).
```

In the reconciliation loop:
1. `demotes_delta` is computed as the difference in DEMOTE calls between consecutive sampling ticks.
2. `ReconcileFlag::Eviction` represents a transient single-tick event triggered by non-zero `demotes_delta`.
3. If hysteresis filtering (`recon_streak` consecutive matching ticks) were applied to `ReconcileFlag::Eviction`, short/transient demote bursts would be swallowed because `demotes_delta` drops back to 0 on subsequent ticks before reaching `recon_streak`.
4. Therefore, `ReconcileFlag::Eviction` bypasses hysteresis (`match candidate { ReconcileFlag::Eviction => ReconcileFlag::Eviction, ... }`), ensuring transient evictions are immediately confirmed and processed.

## Conclusion
The comment correctly explains historical context and architectural reasoning for an existing, correctly implemented feature. Per ORTHOGONAL SCOPE / FINDING_ONLY principles, no code changes are required in `crates/ramshared-wsl2d/src/broker_srv.rs`.
