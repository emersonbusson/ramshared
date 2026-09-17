---
slug: broker-lease-lifecycle
title: Renewable broker lease lifecycle
milestone: —
issues: []
---

# PRD — Renewable broker lease lifecycle

## 1. Summary

Make active broker leases renewable and time-bounded across both broker shells.
A session disconnect cancels an ungranted request but does not immediately
return an active lease. The registered holder's existing PSI heartbeat renews
the lease. The WSL broker tick or the bounded Windows named-pipe loop expires
a lease only after its monotonic deadline and then returns its capacity.

This is a successor to the lifecycle intent behind #1753 and #1804. It does
not import either proposal's unconnected state or redefine the agent↔broker
wire format.

## 2. Technical context

- **Prior gap:** `crates/ramshared-broker/src/lease.rs` owned one pending and
  one active logical lease, but active leases had no deadline.
- **Prior gap:** both `crates/ramshared-wsl2d/src/broker_srv.rs` and
  `crates/ramshared-winbroker/src/lib.rs` treated session loss as immediate
  active-lease release.
- **Confirmed in codebase:** `BrokerCore::handle()` receives an injected
  `Instant` and `CoreEvent::Tick` already runs independently of message rate.
- **Confirmed in codebase:** a registered tenant sends `Msg::Psi` and receives
  `Msg::Ack`; this is the existing liveness signal.
- **Confirmed in codebase:** the lease has no per-slice protocol representation;
  `SliceMap` is the sole owner of `Leased → Free` transitions.

## 3. Recommended option

Use the existing PSI heartbeat to renew the active lease of its holder. Keep
the deadline private to `LeaseBook`, supplied by a named shared policy and an
explicit WSL broker configuration duration. On disconnect, retain only an
active lease until it expires; cancel an ungranted request immediately. At
each WSL broker tick or bounded Windows pipe interval, expire the active lease
if its deadline has passed before capacity can be reused.

Discarded alternatives:

- Immediate reclaim on disconnect: the peer may reconnect or still be
  unwinding its native consumer; reuse is too early.
- A new `LeaseRenew` wire frame: redundant while a registered PSI heartbeat is
  already mandatory and authenticated by the live session table.
- Timer thread per lease: creates concurrent ownership and races with the
  single-threaded `SliceMap` owner.
- Infinite lease: leaks capacity after a lost client.

## 4. Functional requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | Every granted lease has a non-zero monotonic TTL. | A zero TTL refuses the grant before slice ownership changes. |
| RF-2 | A PSI heartbeat from the active holder renews its deadline. | The lease survives its former deadline and expires only after the renewed one. |
| RF-3 | Disconnect cancels a pending request but retains an active lease until expiry. | The leased slice remains `Leased` immediately after disconnect. |
| RF-4 | The broker tick expires only overdue leases and returns their slices once. | Before-deadline ticks retain slices; deadline tick returns them to `Free`. |
| RF-5 | A foreign tenant cannot renew or release the holder's lease. | Existing holder checks continue to reject the foreign session. |

## 5. Non-functional requirements

| ID | Requirement |
| --- | --- |
| NFR-1 | The deadline uses `Instant`; wall-clock changes cannot extend or shorten a lease. |
| NFR-2 | The WSL core remains single-thread owned; the Windows session core remains serialized by its existing mutex and no lease timer thread is added. |
| NFR-3 | Expiry and release are idempotent: no later tick re-releases the same lease. |
| NFR-4 | The shared 30-second TTL permits fifteen missed 2-second WSL ticks; the Windows named-pipe loop checks the same deadline at its bounded 10-second operation boundary. |
| NFR-5 | This source-only slice is partial until an isolated broker/agent lifecycle drill supplies before→action→after evidence. |

## 6. Flows

### Healthy holder

1. A registered tenant requests and receives a logical lease.
2. The broker records `deadline = now + ttl`.
3. Each later PSI from that holder extends the deadline by the same TTL.
4. The lease remains `Leased` while heartbeats remain fresh.

### Disconnect and expiry

1. A holder session disconnects.
2. A pending request is cancelled; an active lease remains reserved.
3. Before its deadline, ticks preserve the reservation.
4. At or after the deadline, the tick removes the active lease and unleases
   its slices exactly once.
5. Normal arbitration may allocate the now-free slices after that boundary.

### Errors

| Trigger | Result | State |
| --- | --- | --- |
| Zero TTL at grant | `InvalidTtl` | Pending request remains; no slice is leased. |
| Foreign PSI | No renewal | Active lease and deadline unchanged. |
| Foreign release | Existing wrong-holder refusal | Active lease unchanged. |
| Repeated expiry tick | No action | Slices remain free. |

## 7. Data / state model

`LogicalLease` remains the wire-neutral lease identity. `LeaseBook` gains a
private active-deadline record:

```text
Pending --grant(now, ttl)--> Active(deadline)
Active --holder PSI(now, ttl)--> Active(now + ttl)
Pending --disconnect(holder)--> None
Active --disconnect(holder)--> Active(deadline)
Active --tick(now >= deadline)--> None
```

The corresponding slice state is `Free → Leased → Free`; expiry never touches
an `Active` or `Draining` swap slice.

## 8. Interfaces

No wire frame, CLI flag, or serialized lease field changes. `BrokerConfig` and
`BrokerCoreConfig` receive an explicit `lease_ttl: Duration`; the WSL
production builder and the Windows session core use the named shared 30-second
policy. Tests inject short durations. `LeaseBook` exposes internal Rust methods
for grant, renew, and expiry with injected `Instant` values.

## 9. Dependencies and risks

| Risk | Mitigation |
| --- | --- |
| A disconnected consumer still needs its reservation briefly. | Keep the active lease until its monotonic deadline. |
| A dead consumer leaks capacity. | Tick expiry releases it without a per-lease thread. |
| A busy message stream starves expiry. | Existing deadline-based core loop emits ticks independently. |
| A foreign tenant attempts to retain another holder's lease. | Renewal checks the active holder identity. |
| TTL is too short for a legitimate reconnect. | 30 seconds / 15 default ticks; controlled isolated drill is required before changing the policy. |

Rollback trigger: revert if an in-process or isolated lifecycle drill shows an
active lease being reallocated before its deadline, surviving after the
deadline without a current holder heartbeat, or preventing a valid
post-expiry allocation.

## 10. Implementation strategy

1. Add pure `LeaseBook` and `BrokerCore` RED tests for deadline, renewal, and
   disconnect retention.
2. Add the lease deadline model and inject `Instant`/TTL through both broker shells.
3. Expire before normal arbitration on every WSL tick and at Windows bounded I/O intervals.
4. Run slice coverage, local broker integration tests, cross-target compilation, and workspace checks.
5. Leave target deployment and isolated live lifecycle drill as partial.

## 11. Documents to update

- This SSDV3 folder.
- `docs/reliability/JULES-PR-AUDIT-20260914.md` to record the successor result.
- Generated documentation index, inventory, and capability observations.

## 12. Out of scope

- NBD framing/authentication changes, Windows process handling, swap/device
  mutation, persistent lease storage, and any host/VM action.
- A new lease wire frame or lease protocol version.
- Reclamation of ordinary disconnected swap slices; their swapoff-first
  lifecycle remains frozen under the existing contract.

## 13. Acceptance criteria

- [x] RED test proves that active lease release on disconnect is no longer acceptable.
- [x] Active holder heartbeats renew a monotonic deadline.
- [x] Pending requests cancel on disconnect; active leases do not.
- [x] Expiry executes only at the broker tick and only once.
- [x] The three pure Rust lifecycle files meet the 80% slice coverage gate.
- [x] Status is `partial` until isolated live lifecycle evidence exists.

## 14. Validation plan

- `cargo test -p ramshared-broker lease::tests`
- `cargo test -p ramshared-wsl2d broker_srv::tests`
- `cargo test -p ramshared-wsl2d --test broker_e2e`
- `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-broker,ramshared-winbroker,ramshared-wsl2d --files crates/ramshared-broker/src/lease.rs,crates/ramshared-winbroker/src/lib.rs,crates/ramshared-wsl2d/src/broker_srv.rs --min 80`
- `cargo fmt --all -- --check`, targeted Clippy, `./scripts/docs-check.sh`, and `git diff --check`.
- Environment-bound: isolated WSL and Windows agent
  disconnect/reconnect/expiry drills with no NBD swap activation or shared-host pressure.
