# Finding 27: Transient Eviction Signal Verification (Bug C1)

- **Source PR:** Jules PR
- **Crate:** `ramshared-wsl2d`
- **Module:** `broker_srv.rs`
- **Classification:** `FINDING_ONLY`

## Observation

Analysis of `crates/ramshared-wsl2d/src/broker_srv.rs` at line 803 confirms that the comment referring to 'bug C1' describes a historically resolved condition where hysteresis logic in `reconcile()` could swallow transient eviction signals (1-2 DEMOTEs).

The current implementation in `broker_srv.rs` correctly bypasses hysteresis for `ReconcileFlag::Eviction` events:
```rust
let confirmed = match candidate {
    ReconcileFlag::Eviction => ReconcileFlag::Eviction, // evento: sem histerese
    ReconcileFlag::None => ReconcileFlag::None,
    sustained if self.recon_count >= self.recon_streak => sustained,
    _ => ReconcileFlag::None,
};
```
This ensures transient eviction signals are confirmed immediately without waiting for hysteresis consecutive ticks, preventing bug C1. No code fix is needed as the condition is resolved and verified by existing unit tests in `broker_srv.rs`.

## Verdict

Accepted as documented architectural verification (`FINDING_ONLY`).
