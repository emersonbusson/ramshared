# Finding Report: sanity check lease TTL against clock skew and monotonic timer

**Target Scope:** `crates/ramshared-broker/src/arbiter.rs` / `crates/ramshared-broker/src/lease.rs`

**Constraint / Prompt Directive:**
The objective was to "sanity check lease TTL against clock skew and monotonic timer" by enforcing minimum/maximum TTL bounds and timer resolution.

**Finding:**
This task constitutes an architectural mismatch / scope trap. The `Lease` system in RamShared broker does not use, store, or rely on a "Time to Live" (TTL). Leases in the `ramshared-broker` logic are indefinitely held physical allocations ("reservations") over physical bytes and slices.

Concrete Evidence:
- In `crates/ramshared-broker/src/lease.rs`, the structures `PendingLease` and `LogicalLease` contain only `holder`, `requested_bytes` / `bytes`, and `id`. There is no `duration`, `ttl`, or `expiration` timestamp.
- In `crates/ramshared-broker/src/arbiter.rs`, the `pending_lease` payload is `Option<(TenantId, u64)>` (a `holder` and a requested `bytes` size).
- Leases are released explicitly via `LeaseBook::disconnect(holder)`, not through a TTL timeout or ticking clock mechanism.
- Attempting to add a TTL check to the codebase would require inventing and architecting an entirely new timeout lease concept that is not currently present in the system, violating the principle of the prompt to avoid arbitrary creation of capabilities.

Therefore, no safe orthogonal slice can be implemented as the target logic does not exist.
