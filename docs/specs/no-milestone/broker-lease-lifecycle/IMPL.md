# IMPL — Renewable broker lease lifecycle

> SSDV3 Step 3 · SPEC:
> `docs/specs/no-milestone/broker-lease-lifecycle/SPEC.md`

## Status

**partial** · source, loopback, and Windows cross-target checks green ·
isolated lifecycle drills environment-bound. The broker lifecycle change is
complete in source, but it does not claim a live agent reconnect or allocation
campaign outside the repository test harness.

## Delivered contract

- An active `LeaseBook` record stores a private `Instant` deadline alongside
  the public logical lease.
- Granting rejects a zero or unrepresentable TTL before state changes.
- Only the active holder's existing PSI heartbeat refreshes the deadline.
- Disconnect cancels a pending request but retains an active lease.
- `BrokerCore::on_tick()` expires an overdue lease before arbitration and
  returns its `Leased` slices once.
- The Windows session core follows the same deadline, ticks at bounded
  named-pipe operation boundaries, and closes a silent expired session.
- The 30-second production policy is a named constant shared by broker shells;
  tests supply short TTLs without changing production policy.

## Files

| Path | ITEM / RF | Change |
| --- | --- | --- |
| `crates/ramshared-broker/src/lease.rs` | ITEM-2 / RF-1..5 | Private active deadline, holder-only renewal, delayed disconnect handling, and one-shot expiry. |
| `crates/ramshared-wsl2d/src/broker_srv.rs` | ITEM-3 / RF-2..5 | TTL configuration, PSI renewal, tick-first expiry, and defensive lease/slice transition rollback. |
| `crates/ramshared-wsl2d/src/main.rs` | ITEM-3 / DT-5 | Supplies the shared named 30-second production TTL. |
| `crates/ramshared-wsl2d/tests/broker_e2e.rs` | ITEM-3 / RF-3..4 | Loopback disconnect-before-deadline and expiry-after-deadline contract. |
| `crates/ramshared-winbroker/src/lib.rs` | ITEM-3 / RF-1..5 | Holder-only renewal, delayed disconnect handling, deterministic expiry, and stale-session closure. |
| `crates/ramshared-winbroker/src/{pipe,service}.rs` | ITEM-3 / DT-7 | Named bounded pipe deadline reused as the Windows expiry scheduler. |

## Validation

- RED: `cargo test -p ramshared-broker lease::tests --lib` failed before the
  implementation because TTL grant arguments, `renew_active`, `expire`, and
  retained-disconnect state did not yet exist.
- GREEN: `cargo test -p ramshared-broker lease::tests --lib` passed 12/12;
  `cargo test -p ramshared-wsl2d broker_srv::tests --lib` passed 32/32; and
  `cargo test -p ramshared-wsl2d --test broker_e2e` passed 5/5. The Windows
  logical core passed `cargo test -p ramshared-winbroker --lib` (18/18), and
  `cargo check -p ramshared-winbroker --target x86_64-pc-windows-gnu` passed.
- Slice coverage: `node tools/ci/check-rust-slice-coverage.mjs -p
  ramshared-broker,ramshared-winbroker,ramshared-wsl2d --files
  crates/ramshared-broker/src/lease.rs,crates/ramshared-winbroker/src/lib.rs,
  crates/ramshared-wsl2d/src/broker_srv.rs --min 80` passed: `lease.rs` 97.9%
  (237/242), `winbroker/lib.rs` 92.4% (376/407), and `broker_srv.rs` 87.8%
  (1357/1546).
- The coverage harness exercised repository-owned refusal fixtures only; no
  swap, GPU, VM, or host action succeeded.

## Gaps

- **Environment-bound:** isolated WSL and Windows broker/agent
  before→action→after drills for disconnect, reconnect-within-TTL, expiry, and
  post-expiry reallocation.
- **Not a claim:** no NBD swap activation, GPU allocation, WSL2 pressure,
  cross-host recovery, or live Windows named-pipe service result is asserted.

## Rollback trigger

Revert if a valid holder cannot renew before its configured deadline, a lease
is reused before expiry, an expired lease retains any `Leased` slice, or a
holder heartbeat can renew a foreign lease.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1..5 | ITEM-1 | `139fc363` |
| RF-1..5 | ITEM-2..3 | `bd665627` |
| RF-1..5 | ITEM-3 | `9e039749`, `5a40bed9` |
