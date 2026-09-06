# Finding Report: Historical Bug C1 Transient Eviction Comment

## Summary
The prompt asks to address line 803 in `crates/ramshared-wsl2d/src/broker_srv.rs`:
`// a transient eviction (1-2 DEMOTEs) and the eviction signal would never appear (bug C1).`

## Analysis
1. Code Context: In `BrokerServer::reconcile_tick()`, `ReconcileFlag::Eviction` is classified as a canary EVENT (DT-6; `demotes_delta` is per-tick, lasts 1 tick) which requires immediate confirmation rather than sustained hysteresis confirmation.
2. The code explicitly implements this logic:
```rust
let confirmed = match candidate {
    ReconcileFlag::Eviction => ReconcileFlag::Eviction, // evento: sem histerese
    ReconcileFlag::None => ReconcileFlag::None,
    sustained if self.recon_count >= self.recon_streak => sustained,
    _ => ReconcileFlag::None,
};
```
3. The comment `(bug C1)` documents a historically resolved bug condition where transient evictions (1-2 DEMOTEs) would be swallowed if hysteresis were applied to canary eviction events.
4. The current codebase already correctly handles canary eviction events without applying hysteresis. Therefore, no code modification is needed in `crates/ramshared-wsl2d/src/broker_srv.rs`.

## Conclusion
This item is a historical bug explanation comment rather than an actionable code defect. A FINDING_ONLY report is cataloged to document code safety and architectural completeness.
