# AUDIT-2.5 — Microsoft-native VRAM memory tier

This audit reviews [`SPEC.md`](SPEC.md) against the proposed host-authoritative
N3 RFC boundary, the pure Rust state-model target, SSDV3 security rules, and
Kahneman #2/#3/#9/#13/#15–#18. The bounded pure-model Step 3 source slice is
implemented, including fresh-model restart-record restoration. It does not
claim Microsoft adoption, host access, live restart evidence, or native VRAM
residency.

## Findings

| Sev | SPEC § | Finding | Disposition |
| --- | --- | --- | --- |
| CRITICAL | DT-N3-1/2; Interfaces | Calling N3 a Microsoft feature or letting the guest own residency would create a false native-memory claim. | Fixed by the explicit design-label disclaimer, host-owner table, and `N3_HOST_CONTRACT_REFUSAL`. |
| HIGH | DT-N3-3; ITEM-2 | A stale or replayed budget/pressure observation could authorize an unsafe transition. | Fixed by bounded schema, monotonic epoch, freshness, all-record validation, and refusal tests. |
| HIGH | DT-N3-5/6/7; ITEM-3 | A generic readiness state or guest-originated intent could be mistaken for host authorization. | Fixed by removing that ambiguity: only host `GRANT` enters `GRANTED(generation)` in the exact `ABSENT → NEGOTIATING → GRANTED → QUIESCING → DRAINED → REVOKED\|FAILED → ABSENT` protocol; guest ACKs never self-authorize. |
| HIGH | DT-N3-6/7; ITEM-3 | Reused lease generations, stale events, or duplicate event IDs could mutate the wrong host lease. | Fixed by opaque host-owned lease/generation/event identity, monotonic generation checks, exact-payload duplicate idempotence, and `N3_RUST_STALE_GENERATION_REFUSAL` / `N3_RUST_DUPLICATE_EVENT_IDEMPOTENCE`. |
| HIGH | DT-N3-11; ITEM-5a | A guest crash/restart could erase in-memory generation history and accept an old lease generation in a fresh process. | Fixed locally by bounded canonical host restart-record serialization/validation and `N3_RUST_DURABLE_RESTART_GENERATION_REFUSAL`; host acquisition/authentication and live restart evidence remain outside the pure model. |
| HIGH | DT-N3-8/9; ITEM-3 | Revoke, reset/TDR, crash, restart, suspend, or driver upgrade could leave in-flight I/O or private data live. | Fixed by `QUIESCING`/`DRAINED`/`FAILED`, no `DRAIN_ACK` until zero callbacks and scrub, new generation after every lifecycle boundary, and `N3_RUST_REVOKE_WITH_INFLIGHT_REFUSAL` / `N3_RUST_GUEST_CRASH_FAILSAFE`. |
| HIGH | Files; ITEM-2 | A Windows/`/dev/dxg` call hidden in the pure model would make independent tests meaningless. | Fixed by an explicit no-FFI/no-I/O model and separate future host-adapter SPEC requirement. |
| MEDIUM | DT-N3-9; ITEM-4 | N3 could silently become an NBD, ublk, or #41054 product dependency. | Fixed by product-scope refusal and separate owner links. |
| MEDIUM | DT-N3-10; ITEM-1/5 | Private or unversioned Microsoft behavior could be presented as RFC evidence. | Fixed by public primary-source/version requirement and independent review gate. |
| LOW | Observability | Wall-clock timestamps alone would not prove freshness or ordering. | Fixed by monotonic age plus host epoch; wall clock is informational only. |

## Open questions

The host contract is intentionally unresolved. No question blocks the pure
model/RFC boundary slice, but host integration is blocked until an owner
provides all of the following in a public, versioned contract:

- host-authoritative budget/residency semantics and adapter identity;
- pressure, eviction, migration, reset/TDR, revoke, and offline ordering;
- opaque lease, generation, event-id, capacity, liveness, acknowledgement,
  and lifetime rules for any host grant or guest intent;
- in-flight I/O drain/cancellation, guest-crash failsafe, WSL restart,
  suspend/resume, driver-upgrade, and zeroing/privacy guarantees; and
- an independently testable host surface with no private API dependency.

If any proposed implementation cannot satisfy these conditions, it must remain
`PARTIAL` or `REFUSED_HOST_CONTRACT`; it may not fill the gap with a guessed
ioctl, PFN, NUMA node, CUDA ordinal, or guest-side retry.

## Re-audit checks

| Gate | Result |
| --- | --- |
| N3 is labeled as a RamShared proposal, not Microsoft adoption | PASS |
| Host/WDDM/VIDMM owns native-memory facts | PASS |
| Observation schema is bounded/versioned/freshness-checked | PASS |
| Host-only `GRANT`/`REVOKE` protocol and exact lease state sequence are explicit | PASS |
| Capacity, generation, duplicate/replay, ACK, drain, and failure semantics are explicit | PASS |
| Named pure-Rust protocol tests map to implemented functions | PASS locally — `N3_RUST_GRANT_REVOKE_STATE_MACHINE`, `N3_RUST_STALE_GENERATION_REFUSAL`, `N3_RUST_DUPLICATE_EVENT_IDEMPOTENCE`, `N3_RUST_REVOKE_WITH_INFLIGHT_REFUSAL`, `N3_RUST_GUEST_CRASH_FAILSAFE`, `N3_RUST_DURABLE_RESTART_GENERATION_REFUSAL` |
| `N3_HOST_ENV_BOUND_MATRIX` covers budget/revoke, restart, TDR, suspend, zeroing, owner, and public-contract gates | PASS — rows are specified; live host evidence not run |
| Pure Rust model has no host/kernel/FFI side effect | PASS |
| Legitimate/refusal boundary pairs exist | PASS |
| Reset/revoke/offline transitions are safe and forward-only | PASS |
| NBD, ublk, and #41054 remain separate | PASS |
| Independent host/guest verification is required | PASS |
| Public source and privacy boundary is explicit | PASS |
| Microsoft host integration and hardware evidence | NOT RUN — blocked by missing host contract |

## Verdict

**`go` for the bounded pure-Rust state model and host-authoritative N3 RFC
source-partial implementation in ITEM order.**

**`no-go` for Microsoft host integration, native VRAM residency, guest NUMA/
HMM ownership, or any product claim** until the open host contract and its
independent platform evidence exist. Any implementation that lets a guest
initiate `GRANT`, reuses a stale generation, treats a duplicate conflict as
success, sends `DRAIN_ACK` with in-flight I/O, resumes after crash/reset, skips
zeroing/privacy, uses a private host API, asserts guest PFNs/residency, or
routes N3 through NBD/ublk changes this verdict to `no-go` and requires an
in-place SPEC correction plus re-audit.
