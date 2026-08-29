# Audit Finding: Transient Eviction Bug Comment (bug C1)

## Summary
- **Target File**: `crates/ramshared-wsl2d/src/broker_srv.rs:803`
- **Context Line**: `// a transient eviction (1-2 DEMOTEs) and the eviction signal would never appear (bug C1).`
- **Classification**: `FINDING_ONLY` (Non-actionable item)

## Analysis & Evidence
The task item requests analysis and resolution regarding line 803 in `crates/ramshared-wsl2d/src/broker_srv.rs`.

Upon inspecting the code:
```rust
        let confirmed = match candidate {
            ReconcileFlag::Eviction => ReconcileFlag::Eviction, // evento: sem histerese
            ReconcileFlag::None => ReconcileFlag::None,
            sustained if self.recon_count >= self.recon_streak => sustained,
            _ => ReconcileFlag::None,
        };
```
1. The comment in question describes the rationale behind skipping hysteresis for `ReconcileFlag::Eviction`:
   "Hysteresis (DT-12) for SUSTAINED flags (Unaccounted/StuckSlice/Partial): confirms after `recon_streak` consecutive identical ticks. `Eviction` is a canary EVENT (DT-6; `demotes_delta` is per-tick, lasts 1 tick) -> immediate confirmation; otherwise hysteresis would swallow a transient eviction (1-2 DEMOTEs) and the eviction signal would never appear (bug C1)."
2. The code explicitly implements immediate confirmation for `ReconcileFlag::Eviction`, ensuring transient evictions are not swallowed by the hysteresis filter (`recon_streak`).
3. "bug C1" refers to a bug condition that was historically resolved by this design decision (bypassing hysteresis for transient eviction canary events).
4. No code defect, logic error, or regression exists. Modifying this logic or removing the explanatory comment would be improper code churn or documentation regression.

## Conclusion
This item refers to a historically resolved bug condition ('bug C1') and documents active, correct behavior. No code modifications are required.
