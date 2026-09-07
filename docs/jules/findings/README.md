# FINDING_ONLY: Cascading Demotion Failure Isolation

## Finding
The request to implement cascading demotion failure isolation (Tier 1 -> Tier 2) with reconnection, retry loops, and state journal restoration in `crates/ramshared-tier/src/cascade.rs` is an architectural scope trap.

## Evidence
`crates/ramshared-tier/src/cascade.rs` is a pure state model and configuration logic for tier sizes and limits (e.g., `vram_safety_net`, `validate_migration_speed`, `validate_tier_resize`). It contains no data-plane concurrency primitives or migration queues. Any implementation of data-plane isolation or retry loops here would be a domain violation.
