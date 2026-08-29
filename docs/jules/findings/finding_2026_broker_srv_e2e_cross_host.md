# Finding Report: Historical Bug Note in `broker_srv.rs:986`

## Overview
- **File:** `crates/ramshared-wsl2d/src/broker_srv.rs`
- **Line:** 986
- **Context:**
```rust
    // Wall-clock deadline for the next Tick. CRITICAL: the Arbiter's Tick MUST NOT be starved
    // by messages. Pure `recv_timeout(tick)` never expires under normal `Psi` flow
    // (~1/s per tenant) → the arbiter would never run `AssignFree`/rebalance. Here the wait shrinks
    // as messages arrive, and the Tick fires when the deadline passes, regardless of
    // message rate. (Bug caught in e2e cross-host civm; the QEMU drill passed by luck of timing.)
```

## Analysis
The task description points to line 986 in `crates/ramshared-wsl2d/src/broker_srv.rs`. Upon inspecting the file, the referenced comment is a historical inline documentation note explaining why wall-clock deadline tracking (`next_tick`) is implemented in `core_loop()`.

The comment details a bug that was previously caught in cross-host testing (where message floods starved the Arbiter's tick when using standard `recv_timeout(tick)` without tracking shrinking wait intervals) and explains how the existing implementation solves it:
1. `let wait = next_tick.saturating_duration_since(Instant::now());` calculates the remaining duration until the next tick deadline.
2. `io_rx.recv_timeout(wait)` waits up to `wait`.
3. If `Instant::now() >= next_tick`, `CoreEvent::Tick` is dispatched and `next_tick = Instant::now() + tick;` advances the deadline.

This logic is fully operational, correct, and thoroughly tested in `crates/ramshared-wsl2d/tests/broker_e2e.rs` via `e2e_psi_flood_does_not_starve_arbiter_tick`.

## Conclusion
This item is an inline historical code comment describing a bug that has already been resolved and verified. No code modifications are required in `crates/ramshared-wsl2d/src/broker_srv.rs`.

CLASSIFICATION: `FINDING_ONLY`
