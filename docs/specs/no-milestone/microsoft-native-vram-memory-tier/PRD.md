---
slug: microsoft-native-vram-memory-tier
title: "Microsoft-native VRAM memory tier — host-authoritative N3 RFC"
milestone: Microsoft-native N3 — Design
issues:
  - 196
---

# PRD — Microsoft-native VRAM memory tier: host-authoritative N3 RFC

## Summary

This PRD defines a bounded RFC and a pure Rust state model for a possible
Microsoft-native VRAM memory tier. `N3` is a RamShared design label for the
third, host-authoritative contract level; it is not a Microsoft commitment,
public API, or claim that WSL2 currently exposes device memory to the guest.

The host owns the authoritative facts: adapter identity, VRAM residency,
budget, eviction, reset/TDR, migration, and lifetime. A WSL2 guest may observe
bounded host-provided snapshots and request an intent, but it may not invent a
PFN range, claim residency, alter host accounting, or make guest NUMA/HMM
ownership true by configuration. If a host contract is approved, the only
authorization path is a host-initiated opaque `GRANT`/`REVOKE` lease protocol
with a monotonic generation. The local implementation target is a pure Rust
state machine that consumes observations and protocol events and emits
decisions; it has no Windows calls, kernel ABI, driver, or host mutation.

The RFC is therefore a boundary document, not a product transport change. The
existing NBD cascade remains the WSL2 product path. Native VRAM integration is
`REFUSED_HOST_CONTRACT` until Microsoft-owned semantics are documented and
validated on an approved host surface.

The pure model also consumes a bounded, serialized **host-authoritative restart
record**. It carries only opaque lease identities and their last accepted
generations, plus a host epoch and contract version. The model can encode,
decode, and validate that record, but it performs no filesystem, registry,
network, or device I/O: an eventual host adapter owns durable acquisition and
authentication. A fresh model that restores a valid record rejects an old
generation for a recorded lease. A missing, malformed, guest-authoritative, or
untrusted record is a fail-closed restart input, not evidence that an old lease
may resume.

## Technical context

WSL2 GPU-PV presents a guest-facing interface while Windows WDDM/VIDMM and the
host graphics stack retain ownership of physical GPU memory and residency.
Guest CUDA allocation, `/dev/dxg`, a custom kernel, or a successful Kconfig
build does not establish a guest-owned memory tier. HMM,
`MEMORY_DEVICE_PRIVATE`, fake PFNs, `add_memory()`, and Linux memory-tier
registration each require a real device-memory owner, migration/reclaim
semantics, reset recovery, and offline/lifetime guarantees.

The proposed RFC must use only current public Microsoft and Linux primary
documentation for facts. An undocumented host ioctl, private team protocol,
reverse-engineered counter, or guessed WDDM meaning is an unresolved boundary,
not an interface assumption.

## Recommended option

Produce an N3 RFC with two separately reviewable layers:

1. **Host contract:** Microsoft/Windows defines the authoritative observation
   and transition events, ownership, capacity, eviction, reset/TDR, migration,
   reclaim, offline, and versioning rules.
2. **Guest model:** RamShared implements a pure Rust model that accepts a
   validated `HostObservation` as preflight, then accepts only a
   host-initiated `GRANT(lease_id, generation, capacity)` and returns a
   fail-closed `GuestDecision`. It never calls Windows, writes `/dev/dxg`,
   registers memory, or asserts physical residency without that host grant.

The RFC must explicitly refuse integration when host authority is absent,
ambiguous, stale, revoked, or incompatible. It must not turn a design draft
into a WSL product feature or a Linux upstream contribution.

## Host-authoritative lease protocol

The protocol is an opaque host contract, not a guest allocation API. The host
initiates both `GRANT` and `REVOKE`; the guest cannot self-grant, advance a
generation, or treat a guest request as authorization. A future public host
contract must define the wire encoding and transport; this PRD defines the
semantic boundary that the pure Rust model must enforce.

| Item | Host-authoritative rule |
| --- | --- |
| `lease_id` | Opaque host-issued identifier, bounded to the contract limit; the guest compares it but never derives meaning from it. |
| `generation` | Non-zero monotonic value scoped to `lease_id` and host contract; the host advances it, and every event/ack must match the active generation. |
| `event_id` | Opaque host event identity; an exact duplicate payload is idempotent, while a reused ID with different payload is a protocol failure. |
| `restart_record` | Bounded canonical serialization supplied by the host contract: schema version, host authority marker, host epoch, and unique `(lease_id, last_generation)` entries. The pure model validates and restores it atomically but does not persist it. |
| `capacity_bytes` | Host-defined logical capacity for this lease, with explicit units/alignment and a non-zero bounded value no greater than the host budget. Guest free memory never supplies this field. |
| `GRANT` | Host → guest event carrying contract version, `lease_id`, `generation`, capacity, and lease/liveness terms. It enters `NEGOTIATING`; it does not become active until the guest validates and acknowledges it. |
| `REVOKE` | Host → guest event for the active lease/generation. It stops new I/O and enters `QUIESCING`; absence of a drain acknowledgement is not success. |
| `ACK` | Guest → host `GRANT_ACK`, `DRAIN_ACK`, or `FAIL_ACK`, always echoing the opaque lease, generation, and event identity. An ACK never creates host authority. |
| `inflight_io` | Guest-owned bounded counter. `DRAINED` requires zero in-flight operations, completed callbacks, and completed privacy scrubbing. |

The decision-complete protocol state machine is:

`ABSENT → NEGOTIATING → GRANTED(generation) → QUIESCING → DRAINED → REVOKED|FAILED → ABSENT`

| State | Entry and owner | Required behavior and exit |
| --- | --- | --- |
| `ABSENT` | No active lease; guest start/restart. | Ignore guest-only authorization. A valid host `GRANT` enters `NEGOTIATING`; a stale or malformed grant enters `FAILED`. |
| `NEGOTIATING` | Host `GRANT` has been received and fully decoded. | Validate contract version, opaque identity, generation, capacity, freshness, and no conflicting lease. Guest sends `GRANT_ACK` only on success → `GRANTED(generation)`; otherwise `FAIL_ACK` → `FAILED`. |
| `GRANTED(generation)` | Host has accepted the guest's grant acknowledgement. | Permit only the bounded operations covered by the host grant. Host `REVOKE` for the exact lease/generation → `QUIESCING`; reset/channel loss/lease expiry → `FAILED` unless the host contract explicitly supplies the same safe revoke event. |
| `QUIESCING` | Host `REVOKE` or an equivalent host-owned safety event. | Stop new I/O, drain or cancel in-flight I/O, and scrub guest-visible buffers. Zero in-flight plus successful scrub → `DRAINED` and `DRAIN_ACK`; timeout, unknown I/O, or scrub failure → `FAILED` and `FAIL_ACK` (never `DRAIN_ACK`). |
| `DRAINED` | Guest has proved no in-flight I/O and completed scrubbing. | Wait for host revoke completion/acknowledgement. Matching host completion → `REVOKED`; lost/contradictory host event → `FAILED`. |
| `REVOKED` | Host has invalidated the lease after drain. | Keep the lease unusable, discard its generation, then return to `ABSENT`. No automatic reuse is allowed. |
| `FAILED` | Stale/conflicting event, timeout, crash, reset, privacy failure, or impossible counter. | Fail closed, stop new I/O, scrub when possible, emit `FAIL_ACK` only with the matching identity, discard the generation, and return to `ABSENT` after cleanup. |

Preflight observations may say that a host contract is unavailable or
constrained, but they cannot skip `NEGOTIATING` or create `GRANTED`. A generic
readiness label is intentionally not a protocol state.

## Functional requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-N3-1 | State that N3 is a design label, not a Microsoft API or adoption claim. | RFC and status text contain the boundary disclaimer. |
| RF-N3-2 | Make host/WDDM/VIDMM authority explicit for VRAM facts and lifetime. | Every residency/budget/reset transition has a host owner. |
| RF-N3-3 | Define a versioned, bounded observation contract. | Required fields, units, epoch, freshness, and unknown handling are closed. |
| RF-N3-4 | Keep the guest model pure Rust and side-effect free. | Unit tests run without Windows, GPU, kernel, or device handles. |
| RF-N3-5 | Refuse guest PFN/NUMA/HMM ownership without a host contract. | Legitimate/refusal pairs return `REFUSED_HOST_CONTRACT`. |
| RF-N3-6 | Define the host-authoritative lease state machine and GRANT/REVOKE/ACK/drain roles. | Only a host `GRANT` enters `GRANTED(generation)`; revoke, drain, failure, and return to `ABSENT` are explicit. |
| RF-N3-7 | Define version/epoch, lease-generation, event-id, and stale-observation behavior. | Replayed, reordered, stale, duplicate-conflict, or unknown records fail closed or are exact-payload idempotent. |
| RF-N3-8 | Keep N3 separate from NBD, ublk, upstream #41054, and Windows driver work. | No product transport or external patch is implied by the RFC. |
| RF-N3-9 | Require independent host and guest verification. | Guest acceptance cannot promote itself without host evidence, and host integration remains environment-bound. |
| RF-N3-10 | Preserve rollback and safe demotion semantics. | Host revocation/reset/offline returns the guest to a safe non-tier state. |
| RF-N3-11 | Reject generation reuse across a fresh guest process when the host supplies restart input. | A new pure model restores a bounded host-authoritative restart record and refuses the recorded old generation; only the next generation can negotiate. |

## Non-functional requirements

| ID | Requirement |
| --- | --- |
| NFR-N3-1 | No private Microsoft endpoint, undocumented ioctl, token, host path, or raw host log is recorded. |
| NFR-N3-2 | Pure policy tests use deterministic timestamps, epochs, counters, and bounded inputs. |
| NFR-N3-3 | Missing host or hardware evidence is `PARTIAL`/`REFUSED`, never `DONE`. |
| NFR-N3-4 | A guest request is advisory until a host-authoritative acknowledgement arrives. |
| NFR-N3-5 | No reboot, WSL shutdown, driver install, kernel module, memory hotplug, or GPU pressure is part of the RFC slice. |
| NFR-N3-6 | The model has no allocation, I/O, FFI, thread, or global-state side effect. |
| NFR-N3-7 | A lease, generation, or host grant is never reused across guest crash, host reset/TDR, WSL restart, suspend/resume, or driver upgrade without a new host grant. |
| NFR-N3-8 | Restart serialization is pure, bounded, canonical, and does not itself claim durable storage, host authentication, or a live restart result. |

## Flows

### RFC and boundary review

1. Identify the public Microsoft-owned host surface and exact version/contract
   revision. If no public contract exists, record `REFUSED_HOST_CONTRACT`.
2. State which host facts are authoritative and which guest values are merely
   observations or intent.
3. Define the bounded observation schema, freshness, epoch, reset, revoke,
   migration, and offline semantics.
4. Review the guest model against the host contract without implementing a
   host call or Linux memory registration.

### Pure Rust state evaluation

1. Start at `HostUnavailable` or `ProductOff`; no guest claim is inferred.
2. Accept exactly one schema-valid, fresh observation with a monotonic epoch;
   this is preflight only and cannot authorize a lease.
3. Receive a host `GRANT`, enter `NEGOTIATING`, and validate its opaque
   lease/generation/capacity before sending `GRANT_ACK` and entering
   `GRANTED(generation)`.
4. On host `REVOKE`, stop new I/O, drain, scrub, send `DRAIN_ACK`, and wait for
   host completion before `REVOKED`; on timeout, reset, stale data, epoch
   discontinuity, crash, or privacy failure, fail closed through `FAILED`.
5. Discard the generation and return to `ABSENT`; a later grant must be a new
   host event. Guest intent alone never represents a resident tier.

### Fresh-process restart restoration

1. The host adapter supplies a bounded `restart_record` before any new grant.
2. The pure model decodes the entire record, validates its schema, authority,
   epoch, ordering, identity bounds, and generation values, then atomically
   seeds only its monotonic generation history.
3. A fresh process may observe a current host snapshot, but a grant at or below
   a restored generation is refused. The next exact generation remains the
   only local candidate; host acceptance is still required.
4. The pure model performs no persistence or host call. Missing host evidence
   keeps the host restart row environment-bound and `PARTIAL`.

### Refusal flow

If a proposal asks the guest to make VRAM a NUMA node, supply PFNs, register
`DEVICE_PRIVATE`, or bypass host accounting, return
`REFUSED_HOST_CONTRACT`. Preserve the existing NBD product boundary and create
no patch, module, host request, or external issue.

## Data/state model

### Host observation

The RFC schema is intentionally abstract until Microsoft defines the host
contract. A candidate observation contains:

| Field | Constraint |
| --- | --- |
| `schema_version` | Bounded known version; unknown versions refuse. |
| `host_epoch` | Monotonic opaque epoch; replay or regression refuses. |
| `adapter_id` | Host-issued stable identity; guest does not derive it from CUDA ordinal. |
| `authority` | Explicit host-authoritative marker; false/unknown refuses. |
| `budget_bytes` | Non-negative bounded integer with units. |
| `resident_bytes` | Non-negative and not greater than host budget unless contract says otherwise. |
| `available_bytes` | Host-defined semantics; never inferred from guest free memory. |
| `events` | Versioned pressure/revoke/reset/migrate/offline events. |
| `observed_at` / `max_age` | Monotonic freshness contract; wall-clock alone is insufficient. |

The exact wire representation is not frozen by this PRD. A future SPEC may
select a public host interface only after its ownership and lifetime rules are
verified.

### Observation preflight state

| State | Entry condition | Allowed next preflight states |
| --- | --- | --- |
| `HostUnavailable` | No valid host authority or host disconnected. | `Observing`, `Refused`. |
| `ProductOff` | Guest deliberately has no native tier. | `Observing`, `Refused`. |
| `Observing` | Fresh, schema-valid observation received. | `Constrained`, `HostUnavailable`, `Refused`. |
| `Constrained` | Host budget/pressure crosses the declared floor. | `DemotionRequested`, `HostUnavailable`, `Refused`. |
| `DemotionRequested` | Guest asks host to reclaim/demote; no residency claim. | `HostUnavailable`, `Observing`, `Refused`. |
| `Refused` | Contract, epoch, freshness, or boundary is invalid. | `Observing` only after a new reviewed contract/epoch. |

The preflight state is parallel to, and never a replacement for, the lease
protocol above. `Observing` or `Constrained` may gate a host grant, but only
`GRANTED(generation)` is the protocol state that records host authorization.

### Lease lifecycle and safety matrix

| Event | Required host/guest result |
| --- | --- |
| Guest crash or missing heartbeat | Host stops honoring the lease; the next guest starts `ABSENT`. An old lease/generation cannot be resumed. |
| Fresh guest process with restart record | The host-supplied record seeds last accepted generations before any grant. A recorded old generation fails; the record does not itself authorize a grant or assert durable host evidence. |
| Host reset/TDR or channel loss | Host invalidates the generation and sends `REVOKE` when possible; otherwise the guest enters `FAILED`. Regrant requires a new generation. |
| WSL restart | Both sides invalidate the lease; restart begins `ABSENT` and waits for a fresh host `GRANT`. |
| Suspend/resume | Revoke and drain before suspend. Resume never auto-reuses a lease; host must issue a new grant after fresh observations. |
| Driver upgrade | Host revokes and drains before replacement; no grant during the upgrade; a new driver must issue a new generation. |
| In-flight I/O | `REVOKE` blocks new I/O. A non-zero or unknown counter, callback, or cancellation result prevents `DRAIN_ACK` and enters `FAILED`. |
| Zeroing/privacy | Guest scrubs visible buffers and metadata before `DRAINED`; host owns physical zeroing before reassigning memory. Scrub failure is `FAILED`, not drained. |

## Interfaces

| Boundary | Allowed contract | Forbidden claim/action |
| --- | --- | --- |
| Windows host/WDDM/VIDMM | Authoritative versioned observations and events. | Guest infers ownership or changes host residency. |
| Windows host/WDDM/VIDMM ↔ WSL2 guest | Host `GRANT`/`REVOKE`; guest `GRANT_ACK`/`DRAIN_ACK`/`FAIL_ACK` with opaque lease and generation. | Guest-originated grant, generation advance, or host-accounting mutation. |
| WSL2 guest | Pure Rust observation/preflight, lease state, drain, scrub, and intent model. | Direct private host API, PFNs, NUMA hotplug, or host accounting. |
| Linux kernel | None in this slice. | New uAPI, HMM registration, `add_memory()`, or LKM. |
| RamShared product | Existing NBD cascade remains independent. | N3 silently switches transport or readiness. |
| External contribution | Future human RFC only, if host owner accepts. | Automatic issue, patch, PR, or Microsoft route. |

## Dependencies/risks

| Risk | Mitigation and rollback trigger |
| --- | --- |
| No public host contract | Set `REFUSED_HOST_CONTRACT`; keep NBD; do not implement guest ownership. |
| Host event ordering is ambiguous | Require epoch/version and monotonic checks; stale/replayed event → `Refused`. |
| Lease generation is reused or event IDs conflict | Require host-monotonic generation and exact-payload duplicate semantics; conflict → `FAILED`, never replay. |
| Guest budget differs from host truth | Host value wins; mismatch → `Constrained` or `HostUnavailable`, never extra allocation. |
| Reset/TDR/revocation loses device state | Host event forces demotion/offline path; no page is silently retained. |
| In-flight I/O or privacy scrub cannot complete | Do not send `DRAIN_ACK`; enter `FAILED`, keep the lease unusable, and require a new grant. |
| Guest crash, WSL restart, suspend, or driver upgrade | Invalidate the old lease/generation; only a fresh host grant may re-enter `NEGOTIATING`. A fresh process must restore a valid host record before it can reject prior generations locally. |
| Private API is proposed for convenience | Stop RFC and record the boundary as unresolved. |
| Pure model grows I/O/FFI/global state | Reject the change; split a new host adapter SPEC. |
| Native work is confused with NBD product readiness | `N3_PRODUCT_SCOPE_REFUSAL` preserves the NBD PRD as owner. |

Rollback trigger: any state transition that claims guest-owned residency without
a host acknowledgement, accepts stale/replayed authority, bypasses reset/offline
semantics, or mutates a host/kernel surface. Return to `HostUnavailable` or
`Refused` and retain only public-safe evidence.

## Implementation strategy

This Step 3 source-partial slice implements only the pure model, including
bounded restart-record encoding/decoding and restoration validation. It may:

1. implement the pure Rust model and deterministic tests;
2. write the N3 RFC section from public, version-pinned host facts;
3. add host/guest contract fixtures without a host call;
4. run independent verification of the state model and boundary refusals; and
5. stop with `PARTIAL`/`REFUSED_HOST_CONTRACT` if Microsoft-owned evidence is
   unavailable.

The host adapter, Windows driver, kernel memory tier, NBD transport, ublk
transport, and external RFC submission require separate approved specs.

## Documents

| Document | Action |
| --- | --- |
| `PRD.md` | Create the host-authoritative N3 decision. |
| `SPEC.md` | Create the pure Rust/RFC implementation contract. |
| `AUDIT-2.5.md` | Create the risk and boundary audit. |
| `wsl2-nbd-product-readiness/` | Remains the independent WSL2 product transport owner. |
| `IMPL.md` | Create as an explicit source-partial record after local tests and coverage; it cannot claim host E2E. |
| `validation.md`, release docs | Not changed in this task; only live evidence can enter the validation log. |
| `docs/INDEX.md` | Regenerate as a generated index after adding this pack. |

## Out of scope

- Any claim that Microsoft has accepted N3 or exposes a native VRAM tier;
- Windows driver, WDDM/VIDMM implementation, `/dev/dxg` private protocol, or
  undocumented host API;
- Linux HMM/NUMA/`MEMORY_DEVICE_PRIVATE`/PFN/`add_memory()` implementation;
- NBD/ublk product transport, release readiness, or custom-kernel activation;
- host reboot, WSL shutdown, GPU pressure, driver installation, or external
  issue/RFC submission;
- CI/workflows, release docs, validation records, or `MEMORY.md` edits in this
  task.

## Acceptance

| ID | Criterion |
| --- | --- |
| A-N3-1 | N3 is explicitly a RamShared design label, not a Microsoft commitment. |
| A-N3-2 | Host authority and Microsoft boundaries are explicit for every native-memory claim. |
| A-N3-3 | The pure Rust state model has bounded inputs, refusal states, and reset/offline transitions. |
| A-N3-4 | The exact `ABSENT → NEGOTIATING → GRANTED(generation) → QUIESCING → DRAINED → REVOKED\|FAILED → ABSENT` protocol is explicit; host initiates `GRANT`/`REVOKE`, and the guest drains/acks or fails closed. |
| A-N3-5 | NBD, ublk, upstream #41054, and external RFC actions remain separate. |
| A-N3-6 | Independent verification, rollback, Kahneman, and environment-bound partial rules are present. |
| A-N3-7 | Capacity, generation, duplicate/replay, in-flight I/O, crash/reset, lifecycle, zeroing/privacy, and fresh-process restart-record semantics are decision-complete. |

## Validation

This source-partial slice is validated by pure-Rust unit/coverage plus the
repository docs/index, hygiene, and diff checks. Host integration remains
`PARTIAL` or `REFUSED_HOST_CONTRACT` until an owner supplies public, versioned,
independently verified evidence. No live host or GPU result is claimed here.
