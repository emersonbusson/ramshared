# SPEC — Renewable broker lease lifecycle

> SSDV3 Step 2. The product result is **partial** until the isolated lifecycle
> drill in the required test matrix is complete.

## Closed scope

### In now

- Renewable, monotonic active leases in `ramshared-broker`.
- Explicit `lease_ttl` propagation through the pure and threaded broker config.
- Tick-owned expiry and delayed active-lease cleanup in `broker_srv`.
- Bounded named-pipe-driven expiry and delayed cleanup in `ramshared-winbroker`.
- Pure unit, loopback broker, and Windows cross-target compilation tests.

### Out now

- New protocol frames, on-disk lease persistence, Windows SCM lifecycle changes,
  NBD/swap actions, and ordinary disconnected swap-slice reclamation.

### Assumed-ready dependencies

- The existing PSI/ACK exchange supplies one live-session heartbeat.
- `core_loop` continues to emit `CoreEvent::Tick` despite a busy message stream.
- `SliceMap::unlease()` is the only legal `Leased → Free` transition.

## Traceability

| PRD | SPEC item |
| --- | --- |
| RF-1 | ITEM-2, ITEM-3 |
| RF-2 | ITEM-2, ITEM-3 |
| RF-3 | ITEM-2, ITEM-3 |
| RF-4 | ITEM-3 |
| RF-5 | ITEM-2, ITEM-3 |
| NFR-1..4 | ITEM-2, ITEM-3 |
| NFR-5 | ITEM-4 |

## Technical decisions

| ID | Decision | Why |
| --- | --- | --- |
| DT-1 | Use `Instant`, stored privately beside the active lease. | Wall-clock changes cannot subvert expiry. |
| DT-2 | Renew only on PSI from the active holder. | Reuses the existing liveness contract without widening the wire ABI. |
| DT-3 | Disconnect cancels pending work but retains the active lease. | A session loss is not proof that its native consumer is safe to reclaim immediately. |
| DT-4 | Expire at the beginning of `BrokerCore::on_tick()`. | One owner and ordering: expiry precedes new arbitration. |
| DT-5 | Use a shared 30-second production TTL, injected as `Duration` in tests. | Fifteen default WSL ticks and bounded Windows pipe operations tolerate transient delivery loss but bound orphaned capacity. |
| DT-6 | Leave normal disconnected swap slices frozen. | Lease-only cleanup must not bypass the established swapoff-first contract. |
| DT-7 | The Windows shell checks expiry after each bounded named-pipe operation and closes an expired live session. | An idle or dead client cannot indefinitely retain a logical budget, and no new protocol frame is required. |

## Atomicity and rollback

- **Atomicity frontier:** an active lease is removed from `LeaseBook` before
  its slices are unleased. If no lease is overdue, no slice mutates.
- **Userspace/daemon:** the WSL broker-core thread owns deadline, lease, and
  `SliceMap`; the Windows session core uses its existing mutex and bounded
  pipe loop. No lease timer thread, external command, or blocking action is
  added by expiry.
- **Kernel/module:** N/A — no kernel state or device operation is touched.
- **Host/persistent:** N/A — no persistent state or host configuration changes.
- **Forward-only:** no. A code revert restores immediate active-lease release,
  but that behavior is deliberately less safe for transient disconnects.

## Kahneman map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 | #13 — refusal + legitimate | Can only the holder refresh the deadline, while a valid holder can? | Named LeaseBook tests. | Foreign heartbeat changes deadline. |
| ITEM-3 | #16 — exhaustion | Does a dead holder eventually release capacity without a second owner racing it? | Named BrokerCore before/deadline/after tests. | Reallocation before deadline or retained lease after deadline. |
| ITEM-4 | #9 — numeric verification | Is the deadline exactly `now + ttl` across renewal? | Injected-clock tests plus coverage report. | Boundary tick differs from specified state. |

## Security checklist (pre-implementation)

- [x] Privilege: no new privilege boundary or host command.
- [x] User/host copy: N/A — no buffer or uAPI path.
- [x] Flags/IOCTL codes: N/A — no wire/protocol change.
- [x] Info-leak: no new public data or address logging.
- [x] IRQ/atomic or IRQL: N/A — userspace single-thread owner; no new lock.
- [x] Lifetime: lease state is released before slice reuse; no `Drop` cleanup.
- [x] Hot-unplug / device-gone: N/A — no device operation.
- [x] Host safety: no WSL2 pressure, swap, GPU, VM, or host action.
- [x] Shared-hardware cushion: N/A — lease lifetime only; allocation size is unchanged.
- [x] Bounded foreign calls: none added.
- [x] Cooperative cascade spillover: N/A — ordinary swap leases remain unchanged.
- [x] Replayable ops: expiry and release are idempotent (#17).

## Files to CREATE / MODIFY / DELETE

### CREATE

**`docs/specs/no-milestone/broker-lease-lifecycle/{PRD,SPEC,IMPL,AUDIT-2.5}.md`**

- Purpose: record lifecycle ownership, test matrix, and environment-bound
  validation honestly.
- RF / DT: RF-1..5; DT-1..6.
- Required tests: named matrix below.
- Cover target: N/A — documentation.

### MODIFY

**`crates/ramshared-broker/src/lease.rs`**

- RF / DT: RF-1..3, RF-5; DT-1..3.
- Symbols: `LeaseBook`, `LeaseDisconnect`, `grant_pending`, `disconnect`.
- Before → after: store active deadline privately; accept injected `Instant`
  and TTL for grant/renew/expire; retain active lease on disconnect.
- Required tests: `active_lease_renews_only_for_holder`,
  `active_lease_expires_at_its_monotonic_deadline`, and
  `disconnect_cancels_pending_but_retains_active_lease`.
- Cover target: ≥80%.
- Kahneman: #13, #16.

**`crates/ramshared-wsl2d/src/broker_srv.rs`**

- RF / DT: RF-2..5; DT-2..6.
- Symbols: `BrokerCoreConfig`, `BrokerConfig`, `BrokerCore::on_psi`,
  `BrokerCore::on_disconnect`, `BrokerCore::on_tick`, configuration builders.
- Before → after: pass injected time to PSI processing, renew only holder
  lease, retain it on disconnect, and expire it before arbitration.
- Required tests: `disconnect_holds_active_lease_until_deadline` and
  `holder_heartbeat_renews_active_lease`.
- Cover target: ≥80%.
- Kahneman: #13, #16, #9.

**`crates/ramshared-wsl2d/tests/broker_e2e.rs`**

- RF / DT: RF-2..4; DT-2..5.
- Required test: `e2e_disconnected_lease_expires_without_reuse_before_deadline`.
- Cover target: E2E-only; source rows above own coverage.
- Kahneman: #16.

**`crates/ramshared-winbroker/src/lib.rs`**

- RF / DT: RF-1..5; DT-1, DT-2, DT-3, DT-5, DT-7.
- Symbols: `BrokerSessionCore::{on_authenticated_msg_at,on_disconnect,on_tick}`.
- Before → after: holder PSI renews the shared logical lease; disconnect
  retains it; an injected monotonic tick releases it exactly once and closes a
  stale live session.
- Required tests: `disconnect_retains_server_lease_until_deadline` and
  `holder_heartbeat_renews_windows_lease_deadline`.
- Cover target: ≥80% for `src/lib.rs`.
- Kahneman: #13, #16, #17.

**`crates/ramshared-winbroker/src/{pipe,service}.rs`**

- RF / DT: RF-4; DT-7.
- Before → after: the existing bounded pipe operation interval invokes the
  core tick; expiry effects close a stale session through the existing control
  path.
- Required validation: `cargo check -p ramshared-winbroker --target
  x86_64-pc-windows-gnu`.
- Cover target: environment-bound Windows runtime.
- Kahneman: #16, #17.

### DELETE

None.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| Lease retained after disconnect | broker log | Holder ID and lease ID only |
| Lease renewal | named unit/core tests | Deliberately not logged per heartbeat, preventing control-plane log flooding |
| Lease expired | broker log | Lease ID and bounded TTL |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | N/A — existing broker ownership remains |
| `docs/reliability/JULES-PR-AUDIT-20260914.md` | Alter |
| `docs/specs/no-milestone/windows-autonomous-broker-service/SPEC.md` | Alter |
| `validation.md` | Append only after isolated lifecycle drill |
| `docs/BENCHMARKS.md` + results | N/A — no performance claim |
| `.claude/rules/*`, `CLAUDE.md`, `AGENTS.md` | N/A — no convention change |

## Implementation order

1. `ITEM-1`: add named RED unit and core tests.
2. `ITEM-2`: implement private deadline, renewal, disconnect retention, and
   pure expiry in `LeaseBook`.
3. `ITEM-3`: wire TTL/time into both broker shells; expire before WSL
   arbitration and from bounded Windows pipe operations.
4. `ITEM-4`: execute cover/local and Windows cross-target checks, then record
   the environment-bound drill.

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `lease.rs` grant/renew/expiry | same :: `active_lease_renews_only_for_holder` | unit | #13 | ≥80% |
| `lease.rs` deadline boundary | same :: `active_lease_expires_at_its_monotonic_deadline` | unit | #16, #9 | ≥80% |
| `lease.rs` disconnect distinction | same :: `disconnect_cancels_pending_but_retains_active_lease` | unit | #16 | ≥80% |
| `broker_srv.rs` core lifecycle | same :: `disconnect_holds_active_lease_until_deadline` | integration | #16 | ≥80% |
| `broker_srv.rs` core renewal | same :: `holder_heartbeat_renews_active_lease` | integration | #13, #9 | ≥80% |
| live broker socket | `tests/broker_e2e.rs` :: `e2e_disconnected_lease_expires_without_reuse_before_deadline` | E2E | #16 | E2E-only |
| `winbroker/src/lib.rs` lifecycle | same :: `disconnect_retains_server_lease_until_deadline` | unit | #16, #17 | ≥80% |
| `winbroker/src/lib.rs` renewal | same :: `holder_heartbeat_renews_windows_lease_deadline` | unit | #13, #9 | ≥80% |
| Windows pipe integration | `cargo check -p ramshared-winbroker --target x86_64-pc-windows-gnu` | cross-target static | #16 | environment-bound runtime |
| isolated broker/agent lifecycle | before→action→after disconnect/reconnect/expiry drill | environment | #9, #13, #16 | env-bound |

## Validation checklist

- [x] RED tests executed before production code.
- [x] Named unit and core tests GREEN.
- [x] Socket-level test GREEN.
- [x] All three pure lifecycle files pass the 80% slice coverage gate.
- [x] The Windows pipe service compiles for `x86_64-pc-windows-gnu`.
- [x] Rust fmt and targeted Clippy pass.
- [x] `./scripts/docs-check.sh` passes.
- [ ] Isolated lifecycle drill passes (environment-bound).
- [x] No WSL2 pressure, swap, GPU, VM, or host action succeeded.
