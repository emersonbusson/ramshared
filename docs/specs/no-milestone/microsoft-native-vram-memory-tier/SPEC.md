# SPEC — Microsoft-native VRAM memory tier: host-authoritative N3 RFC

> SSDV3 Step 3 is **source-partial**. [`PRD.md`](PRD.md) is implemented by a
> bounded pure Rust state model, including host-authoritative restart-record
> serialization and fresh-model restore validation. `N3` is a RamShared design
> label, not a Microsoft API. This slice has no host adapter, driver, kernel
> memory tier, or live host before → action → after evidence; its product
> verdict remains `PARTIAL`.

## Closed scope

### In now

- Host-authoritative ownership and Microsoft boundary contract.
- Versioned observation, epoch, freshness, event, and refusal semantics.
- Pure Rust state model with no I/O, FFI, Windows handle, kernel ABI, or GPU.
- Bounded host-authoritative restart record decode/encode and fresh-model
  restore validation for remembered lease generations.
- N3 RFC review contract and independent verification matrix.

### Out now

- Microsoft host/WDDM/VIDMM code, private API, driver, or host protocol.
- Linux HMM/NUMA/`MEMORY_DEVICE_PRIVATE`, PFN, `add_memory()`, LKM, uAPI,
  memory hotplug, or native guest residency.
- NBD/ublk transport changes, product readiness, custom-kernel boot, reboot,
  WSL shutdown, GPU pressure, external RFC/issue/PR, validation, release docs,
  or `MEMORY.md` edits.

## Traceability

| PRD | SPEC item |
| --- | --- |
| RF-N3-1..3 | ITEM-1 — N3 RFC boundary and observation contract |
| RF-N3-3..7 | ITEM-2 — pure Rust preflight, freshness, epochs, and lease protocol |
| RF-N3-5, RF-N3-6, RF-N3-10 | ITEM-3 — refusal, GRANT/REVOKE, drain, reset, and offline states |
| RF-N3-8 | ITEM-4 — separation from product/upstream work |
| RF-N3-9, RF-N3-10 | ITEM-5 — independent verification and rollback |
| RF-N3-11 | ITEM-5a — durable host-authoritative restart input and fresh-model stale-generation refusal |
| NFR-N3-1..7 | ITEM-6 — security, observability, and environment boundary |
| NFR-N3-8 | ITEM-6a — bounded restart-record serialization and restore fail-closed behavior |

## Technical decisions

| ID | Decision | Reason |
| --- | --- | --- |
| DT-N3-1 | N3 is a design label only. | Avoids implying Microsoft adoption or a public API. |
| DT-N3-2 | Host authority is the source of residency, budget, reset, and lifetime facts. | WSL2 GPU-PV does not grant the guest physical VRAM ownership. |
| DT-N3-3 | Observation input is bounded, versioned, epoch-tagged, and freshness-checked. | Prevents stale/replayed/reordered state from becoming memory policy. |
| DT-N3-4 | The guest model is pure Rust and side-effect free. | It can be independently tested without a host or kernel. |
| DT-N3-5 | Observation preflight has no authorization state; only host `GRANT` enters `GRANTED(generation)`. | A generic readiness label cannot be confused with host authorization or residency. |
| DT-N3-6 | The host owns opaque `lease_id`, monotonic `generation`, `event_id`, and `capacity_bytes`; the host initiates `GRANT` and `REVOKE`. | The guest cannot self-grant, advance a generation, or invent capacity. |
| DT-N3-7 | Guest `GRANT_ACK`, `DRAIN_ACK`, and `FAIL_ACK` are identity-bound; exact duplicate events are idempotent and conflicting duplicates fail. | ACKs make protocol progress but never create host authority. |
| DT-N3-8 | `REVOKE` blocks new I/O; zero in-flight operations and privacy scrub are required before `DRAIN_ACK`. | Unknown or unfinished work fails closed through `FAILED`. |
| DT-N3-9 | Reset/TDR, crash, WSL restart, suspend/resume, and driver upgrade invalidate the old lease/generation. | No stale lease can be resumed or silently retain memory. |
| DT-N3-10 | NBD and #41054 remain independent owners; public host facts require exact source/version identity. | N3 is not a transport, upstream, or private-API gate. |
| DT-N3-11 | A restart record is a bounded, canonical, host-authoritative input; the pure model serializes, validates, and restores it without I/O. | A fresh process must reject a previously accepted generation instead of relying on lost in-memory history. Host acquisition, durability, authenticity, and epoch issuance remain outside this slice. |

## Host-authoritative lease protocol

Observation freshness is preflight only. It may keep the model in
`HostUnavailable`, `ProductOff`, `Observing`, `Constrained`, or `Refused`; it
cannot authorize a native tier. The only authorization state is the exact
host-authoritative sequence:

`ABSENT → NEGOTIATING → GRANTED(generation) → QUIESCING → DRAINED → REVOKED|FAILED → ABSENT`

The host initiates `GRANT` and `REVOKE`. A guest request or intent is never a
grant, and the guest never allocates, increments, or interprets host identity.

### Opaque contract fields and message semantics

| Field/message | Contract |
| --- | --- |
| `lease_id` | Bounded opaque host-issued bytes. Equality is checked; contents have no guest meaning. |
| `generation` | Non-zero host-monotonic value scoped to the lease and host contract. Less than the active value is stale; a gap or unexpected greater value fails closed; the guest never advances it. |
| `event_id` | Bounded opaque host event identity. An exact duplicate event payload is idempotent; the same ID with any different field is a protocol failure. |
| `capacity_bytes` | Host-authoritative logical lease capacity, with explicit unit/alignment and a bounded non-zero value. The model rejects overflow, impossible alignment, or a value above the host-observed budget; guest free memory cannot fill this field. |
| `restart_record` | Versioned bounded bytes containing the host authority marker, non-zero host epoch, and canonical lease/generation checkpoints. The pure model rejects unknown schema, guest/unknown authority, malformed length, duplicate/out-of-order IDs, zero generation, overflow, or more than `MAX_GENERATION_HISTORY` checkpoints before replacing any history. |
| `GRANT` (host → guest) | Includes contract version, `lease_id`, `generation`, `event_id`, capacity, host epoch, and liveness/deadline terms. A valid event enters `NEGOTIATING`, not `GRANTED`. |
| `GRANT_ACK` (guest → host) | Echoes the exact grant identity and generation after complete validation. It permits `GRANTED(generation)` only when the host contract accepts the acknowledgement. |
| `REVOKE` (host → guest) | Includes the active lease/generation, event identity, reason, and bounded drain deadline. It blocks new I/O and enters `QUIESCING`. |
| `DRAIN_ACK` (guest → host) | Sent only after all in-flight operations/callbacks reach zero and guest-visible buffers/metadata are scrubbed. It never asserts physical host zeroing. |
| `FAIL_ACK` (guest → host) | Reports a matching identity when validation, drain, timeout, reset, privacy, or lifecycle safety fails. It never claims the lease was drained. |
| revoke completion (host → guest) | Host confirmation after `DRAIN_ACK` enters `REVOKED`; missing or contradictory confirmation enters `FAILED`. |

### Protocol transition matrix

| Prior state | Event/guard | Result and required action |
| --- | --- | --- |
| `ABSENT` | Valid host `GRANT` | Enter `NEGOTIATING`; retain no prior lease state. |
| `ABSENT` | Guest-only request, stale grant, malformed grant, or old generation | Ignore guest authorization; enter `FAILED` for a received protocol event and discard it. |
| `NEGOTIATING` | Complete valid grant, fresh host epoch, capacity within host budget, no conflict | Send `GRANT_ACK`; enter `GRANTED(generation)` only after host acceptance. |
| `NEGOTIATING` | Unknown schema, stale/gap generation, duplicate conflict, impossible capacity, or expired grant | Send `FAIL_ACK` when identity is safe; enter `FAILED`; never partially install the lease. |
| `GRANTED(generation)` | Matching host `REVOKE` | Stop new I/O and enter `QUIESCING`; preserve the generation only for drain identity. |
| `GRANTED(generation)` | Host reset/TDR, channel loss, lease expiry, WSL restart, suspend, or driver upgrade | Invalidate the lease; use `QUIESCING` only if a safe revoke is delivered, otherwise enter `FAILED`. |
| `QUIESCING` | In-flight count and callbacks are zero; scrub succeeds before deadline | Enter `DRAINED` and send `DRAIN_ACK` for the exact lease/generation/event. |
| `QUIESCING` | Any unknown/non-zero in-flight work, timeout, cancellation failure, or scrub failure | Send `FAIL_ACK` if possible; enter `FAILED`; never send `DRAIN_ACK`. |
| `DRAINED` | Matching host revoke completion | Enter `REVOKED`, discard generation, then cleanly return to `ABSENT`. |
| `DRAINED` | Missing, stale, or contradictory host completion | Enter `FAILED`; do not assume revocation or reuse the lease. |
| `REVOKED` or `FAILED` | Cleanup and privacy scrub complete | Return to `ABSENT`; a future grant must carry a new host generation. |

Duplicate semantics are strict: replaying the same event ID with the same
lease, generation, payload, and prior state has no second effect; replaying it
with a changed payload, changed generation, or changed prior-state expectation
is `FAILED`. A stale generation is never upgraded by retry.

### Lifecycle and privacy matrix

| Host/guest condition | Required result |
| --- | --- |
| Guest crash or missing heartbeat | Host stops honoring the lease at the bounded liveness deadline. A restarted guest begins `ABSENT`; it cannot resume the old lease. |
| Host reset/TDR or channel loss | Host invalidates the generation and sends `REVOKE` when possible. If delivery is impossible, the guest fails closed; regrant requires a new generation. |
| WSL restart | Both ends invalidate the lease. Startup begins `ABSENT` and waits for a fresh host grant. |
| Fresh process after restart | A host-provided restart record may seed bounded generation history before observation/grant processing. An old generation for a restored lease fails closed; no record acquisition, filesystem write, or host-authenticity assertion occurs in the pure model. |
| Suspend/resume | Revoke and drain before suspend. Resume does not auto-reuse a lease; the host must issue a new grant after fresh preflight. |
| Driver upgrade | Revoke and drain before replacement. No grant is valid during the upgrade; the replacement must issue a new generation. |
| In-flight I/O | Revoke blocks new I/O. A non-zero, unknown, or uncancellable operation prevents `DRAIN_ACK` and yields `FAILED`. |
| Zeroing/privacy | Scrub guest-visible buffers and metadata before `DRAINED`; host owns physical zeroing before reassignment. Scrub failure is `FAILED`, never drained. |

## Atomicity/rollback

| Layer | Atomic operation | Rollback frontier |
| --- | --- | --- |
| Observation decode | Validate the complete bounded record before state transition. | Reject the whole record; retain the prior safe state. |
| Epoch | Accept only monotonic, contract-valid epochs. | Regression/replay → `Refused`; no partial field update. |
| Freshness | Compare monotonic observation age to the declared contract. | Stale/missing clock → `HostUnavailable`; no guest allocation. |
| Grant install | Validate the entire host `GRANT` before installing lease, generation, capacity, or liveness terms. | Any invalid field → `FAILED`; no partial lease. |
| Generation/event identity | Match the active opaque lease/generation and exact event payload. | Stale, gap, or conflicting duplicate → `FAILED`; exact replay has no second effect. |
| State transition | Apply one event to one prior state and emit one decision. | Invalid transition → `Refused` in preflight or `FAILED` in protocol; no side effect. |
| Revoke/drain | Block new I/O, account for every in-flight callback, and scrub before `DRAIN_ACK`. | Non-zero/unknown work or scrub failure → `FAILED`; never infer drained. |
| Host acknowledgement | Treat `GRANT_ACK`, `DRAIN_ACK`, and host revoke completion as separate inputs. | Missing/revoked acknowledgement → `FAILED`/`HostUnavailable`; never infer success. |
| Lease retirement | Discard generation only after `REVOKED`/`FAILED` cleanup. | Crash/reset/restart/suspend/upgrade cannot resume the old lease. |
| Restart-record restore | Decode and validate the complete bounded canonical host record before replacing generation history. | Malformed, non-host, stale-format, duplicate, unordered, over-bound, or partial input → `FAILED`; retain the prior history. |
| External boundary | No host/kernel/external write in this slice. | Any attempted write is a hard stop and SPEC violation. |

Rollback trigger: a state model accepts an unknown schema, stale or replayed
epoch/generation, impossible capacity or in-flight counter, unacknowledged
residency, reset/revoke without safe drain, guest-only ownership, lease reuse,
or a side effect in the pure layer. Return to preflight `HostUnavailable`/
`Refused` or protocol `FAILED`/`ABSENT` and do not retry deterministically.

## Kahneman map

| # | Question | Required evidence | Abort |
| --- | --- | --- | --- |
| #2 | Does `/dev/dxg`/CUDA prove host-owned native memory? | `N3_HOST_CONTRACT_REFUSAL` | Any guest PFN/NUMA claim from capability. |
| #3 | Are budget, epoch, age, and units explicit? | `N3_OBSERVATION_NUMBERS` | Text-only or wall-clock-only evidence. |
| #9 | Are transitions, generations, and counters reproducible? | `N3_STATE_TRANSITION_MATRIX`, `N3_RUST_GRANT_REVOKE_STATE_MACHINE` | Unbounded input or unspecified event. |
| #13 | Are legitimate and refusal cases paired? | `valid_host_observation`, `STALE_GENERATION_REFUSAL`, `guest_residency_claim_refused` | Fail-open unknown data. |
| #13 | Can a fresh process reject an old generation rather than trusting volatile history? | `N3_RUST_DURABLE_RESTART_GENERATION_REFUSAL` | Restored input is accepted without host authority or old generation is granted. |
| #15 | Is a mismatch transient? | `N3_DETERMINISTIC_REFUSAL_NO_RETRY` | Blind resubmission of invalid contract. |
| #16 | Can reset/revoke exhaust the safe path? | `N3_RESET_REVOKE_OFFLINE_SAFE`, `REVOKE_WITH_INFLIGHT_REFUSAL`, `GUEST_CRASH_FAILSAFE` | Silent page retention or unsafe offline. |
| #17 | Is observation/protocol replay idempotent? | `N3_EPOCH_REPLAY_IDEMPOTENCY`, `DUPLICATE_EVENT_IDEMPOTENCE` | Duplicate event changes state twice. |
| #18 | Who owns the boundary? | `N3_HOST_OWNER_BOUNDARY`, `N3_PRODUCT_SCOPE_REFUSAL` | Guest or NBD layer makes host decision. |

## Security checklist

- [x] No private Microsoft endpoint, token, host path, raw log, or guessed
  contract is admitted as an interface.
- [x] Input counts, byte fields, event lists, and strings are bounded before
  allocation/transition.
- [x] Unknown schema/version/authority/epoch/freshness fails closed.
- [x] Host identity is opaque and host-issued; no CUDA ordinal or guest PFN is
  used as an owner key.
- [x] Lease IDs, generations, and event IDs are opaque, bounded, identity-bound,
  and never guest-generated; conflicting duplicate events fail closed.
- [x] Capacity is host-authoritative and bounded; it is never inferred from
  guest free memory or used to claim physical residency.
- [x] Revoke blocks new I/O; in-flight work and privacy scrub must complete
  before `DRAIN_ACK`, and a crash/reset cannot resume the old lease.
- [x] Pure model has no FFI, filesystem, process, network, device, thread, or
  global mutable state; restart serialization is caller-supplied bytes only.
- [x] Reset/TDR/revoke/offline paths cannot bypass the safe state.
- [x] Public evidence is sanitized and never includes host or account identity.
- [ ] Kernel/uAPI/IRQL/DMA/MMIO: N/A for this pure-Rust source-only slice;
  any future host/kernel surface requires a new boundary review.

## Files create/modify/delete

The source paths below are the implemented local Step 3 slice. They do not
claim a host adapter, durable host store, or live Microsoft/WSL evidence.

| Path | Action | Contract / test owner |
| --- | --- | --- |
| `crates/ramshared-tier/src/n3_state.rs` | Implemented | Pure observation preflight, opaque lease protocol, drain, scrub, restart-record serialization/restore, and decision model. |
| `crates/ramshared-tier/src/lib.rs` | Implemented shared glue | Export only the pure model; no host adapter. |
| `crates/ramshared-tier/tests/n3_state.rs` | Implemented | Legitimate/refusal/epoch/grant/revoke/drain/reset/restart lifecycle matrix. |
| `docs/specs/no-milestone/microsoft-native-vram-memory-tier/SPEC.md` | Maintain | RFC contract and named tests. |
| `docs/specs/no-milestone/microsoft-native-vram-memory-tier/IMPL.md` | Create as `partial` | Exact local implementation/test/coverage evidence; no live host claim. |
| `validation.md` | Append later | Only after independently approved host evidence. |

No Windows source, kernel tree, NBD/ublk source, CI/workflow, release file,
validation record, or `MEMORY.md` entry is created, modified, or deleted.

## Observability

Pure model evidence is a sanitized record with:

| Signal | Required fields | Pass condition |
| --- | --- | --- |
| Contract | schema, source/version, authority marker | Exact public contract revision or explicit refusal. |
| Observation | adapter ID, budget, resident, available, units | Bounded and internally valid. |
| Time | monotonic age and freshness limit | Freshness decision is deterministic. |
| Ordering | prior/current epoch and event | Replays/regressions are refused/idempotent. |
| Lease | opaque lease ID, generation, event ID, capacity | Exact identity match; stale/conflicting records fail. |
| Decision | prior state, event, next state, refusal code | One explicit preflight or protocol transition. |
| Drain | in-flight count, callback completion, scrub result | `DRAIN_ACK` only at zero and after privacy scrub. |
| Host boundary | host grant/revoke and ACK status | Guest intent never becomes authorization or residency alone. |

## Living docs

| Document | Action |
| --- | --- |
| `docs/specs/no-milestone/wsl2-native-vram-autotier/` | Existing WDDM budget policy remains separate; no edits here. |
| `docs/specs/no-milestone/wsl2-nbd-product-readiness/` | Existing NBD product owner remains separate. |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/` | #41054 config-only lane remains separate. |
| `ARCHITECTURE.md` | Update only if a later approved RFC changes architecture. |
| `docs/INDEX.md` | Regenerate as a generated index for this new pack. |
| `AGENTS.md`, `CLAUDE.md`, `.claude/rules/*` | N/A — no convention change. |
| `IMPL.md` | Create an explicit `partial` local-evidence record after tests and coverage. |
| `validation.md`, release docs | N/A without live host evidence. |

## Implementation order

1. **ITEM-1:** freeze public host-authority and N3 RFC boundary language.
2. **ITEM-2:** implement bounded decode, freshness, epoch, and observation
   preflight types; no preflight state authorizes a lease.
3. **ITEM-3:** implement the opaque `GRANT`/`REVOKE`/ACK state machine,
   generation semantics, drain, scrub, reset, and lifecycle failures.
4. **ITEM-4:** add explicit refusal tests separating N3 from NBD, ublk, and #41054.
5. **ITEM-5:** run independent Rust tests and record exact local evidence in
   `IMPL.md`; no host integration.
6. **ITEM-6:** retain `PARTIAL`/`REFUSED_HOST_CONTRACT` until public host
   evidence exists; do not append root validation without a live run.

## Required tests matrix

These names are contractual for the implemented pure-Rust slice. Exact local
results belong in `IMPL.md`; host rows remain environment-bound.

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover / pass condition |
| --- | --- | --- | --- | --- |
| `crates/ramshared-tier/src/n3_state.rs` | `valid_host_observation_enters_observing` | unit | #3 | Bounded valid snapshot enters the model. |
| `crates/ramshared-tier/src/n3_state.rs` | `unknown_schema_is_refused` | unit/refusal | #13 | Unknown schema has no partial effect. |
| `crates/ramshared-tier/src/n3_state.rs` | `stale_observation_is_unavailable` | unit/refusal | #13/#16 | Age beyond declared max cannot authorize a lease. |
| `crates/ramshared-tier/src/n3_state.rs` | `epoch_regression_is_refused` | unit/refusal | #13/#17 | Reordered/replayed authority is safe. |
| `crates/ramshared-tier/src/n3_state.rs` | `impossible_budget_counters_are_refused` | unit/refusal | #3/#13 | Resident > budget or overflow refuses. |
| `crates/ramshared-tier/src/n3_state.rs` | `preflight_never_claims_grant` | unit/refusal | #2/#18 | Observation preflight never enters `GRANTED(generation)`. |
| `crates/ramshared-tier/src/n3_state.rs` | `host_pressure_requests_demote` | unit | #16 | Host event leads to bounded intent. |
| `crates/ramshared-tier/src/n3_state.rs` | `reset_revoke_and_offline_are_safe` | unit/refusal | #16 | No unsafe retained residency after owner event. |
| `crates/ramshared-tier/src/n3_state.rs` | `guest_pfn_or_numa_claim_is_refused` | unit/refusal | #2/#13 | Guest cannot create native memory ownership. |
| `crates/ramshared-tier/src/n3_state.rs` | `deterministic_contract_failure_is_not_retried` | unit/refusal | #15 | One refusal, no blind retry. |
| `crates/ramshared-tier/src/n3_state.rs` | `replayed_observation_is_idempotent` | unit | #17 | Same epoch/event produces no duplicate effect. |
| `crates/ramshared-tier/src/n3_state.rs` | **TestName: `N3_RUST_GRANT_REVOKE_STATE_MACHINE`**; function: `n3_rust_grant_revoke_state_machine` | unit/state | #9/#16/#18 | Exact host-led `ABSENT → NEGOTIATING → GRANTED → QUIESCING → DRAINED → REVOKED → ABSENT` path and failure branch. |
| `crates/ramshared-tier/src/n3_state.rs` | **TestName: `N3_RUST_STALE_GENERATION_REFUSAL`**; function: `n3_rust_stale_generation_refusal` | unit/refusal | #13/#17 | Lower, skipped, or old lease generation cannot be accepted or retried. |
| `crates/ramshared-tier/src/n3_state.rs` | **TestName: `N3_RUST_DUPLICATE_EVENT_IDEMPOTENCE`**; function: `n3_rust_duplicate_event_idempotence` | unit/idempotence | #17 | Exact duplicate event has one effect; reused ID with changed payload fails. |
| `crates/ramshared-tier/src/n3_state.rs` | **TestName: `N3_RUST_REVOKE_WITH_INFLIGHT_REFUSAL`**; function: `n3_rust_revoke_with_inflight_refusal` | unit/refusal | #16 | Non-zero/unknown in-flight I/O prevents `DRAIN_ACK` and enters `FAILED`. |
| `crates/ramshared-tier/src/n3_state.rs` | **TestName: `N3_RUST_GUEST_CRASH_FAILSAFE`**; function: `n3_rust_guest_crash_failsafe` | unit/lifecycle | #16/#18 | Crash/liveness expiry invalidates the old lease; restart begins `ABSENT`. |
| `crates/ramshared-tier/tests/n3_state.rs` | **TestName: `N3_RUST_DURABLE_RESTART_GENERATION_REFUSAL`**; function: `n3_rust_durable_restart_generation_refusal` | integration/refusal | #13/#17 | A fresh pure model restores a bounded host restart record and rejects its old generation; a strictly newer generation remains eligible. |
| `crates/ramshared-tier/tests/n3_state.rs` | `n3_product_transport_scope_refusal` | integration/refusal | #18 | N3 never selects NBD/ublk or changes product state. |
| `crates/ramshared-tier/tests/n3_state.rs` | `host_contract_owner_is_explicit` | integration | #18 | Every native fact has a host owner. |
| RFC review packet | `N3_PUBLIC_PRIMARY_SOURCE_REVIEW` | static/manual | #3/#18 | Public source/version and no private API. |
| RFC review packet | `N3_INDEPENDENT_BOUNDARY_REVIEW` | independent review | #13/#18 | Guest and host reviewers agree on refusal boundaries. |

### N3_HOST_ENV_BOUND_MATRIX

The following rows are environment-bound and cannot pass in the pure Rust
slice. They require an owner-approved, public host contract plus an isolated
host/GPU harness; no shared daily host pressure is implied.

| TestName ID | Environment-bound path | Required host evidence | Local result without evidence |
| --- | --- | --- | --- |
| `N3_HOST_BUDGET_REVOKE_DRAIN` | Host grant capacity, pressure, revoke, and guest drain | Host budget semantics, matching generation, bounded revoke deadline, and drain acknowledgement | `PARTIAL` / `REFUSED_HOST_CONTRACT` |
| `N3_HOST_WSL_RESTART_RECOVERY` | WSL restart with an active lease | Host and guest invalidate the old lease and issue a new generation only after restart | `PARTIAL`; local restart-record test is not live restart evidence |
| `N3_HOST_GPU_RESET_TDR_STOP` | GPU reset/TDR or channel loss | Host reset event, lease invalidation, no retained residency, and safe stop | `REFUSED_HOST_CONTRACT` |
| `N3_HOST_SUSPEND_RESUME` | Suspend/resume around revoke and drain | Host revoke before suspend and a fresh grant after resume | `PARTIAL` |
| `N3_HOST_DATA_INTEGRITY_AND_ZEROING` | Scrub/zeroing before lease reuse | Guest scrub evidence and host physical-zeroing/reassignment contract | `REFUSED_HOST_CONTRACT` |
| `N3_OWNER_MATRIX_REVIEW` | Host/guest ownership review | Named Microsoft/Windows host owner, RamShared guest owner, and boundary approver | `PARTIAL` |
| `N3_PUBLIC_CONTRACT_REVIEW` | Public RFC/interface review | Version-pinned public host contract with no private endpoint or guessed semantics | `REFUSED_HOST_CONTRACT` |

### Host-boundary claim matrix

| Claim | Guest evidence allowed | Host evidence required | Local verdict without host evidence |
| --- | --- | --- | --- |
| Adapter identity | Opaque ID is well-formed | Host-issued ownership and lifetime | `PARTIAL` |
| Budget/availability | Bounded observation arithmetic | Host semantic definition and freshness | `PARTIAL` |
| Lease authorization/residency | Exact `GRANT` identity and generation are recorded | Host grant, capacity, lifetime, and acknowledgement | `REFUSED_HOST_CONTRACT` |
| Pressure/eviction | Pure transition test | Host event source and ordering | `PARTIAL` |
| Reset/TDR | Safe fallback transition | Host reset/recovery contract | `REFUSED_HOST_CONTRACT` |
| Offline/unbind | Refusal transition | Host drain/offline guarantee | `REFUSED_HOST_CONTRACT` |

## Validation checklist

Local Step 3 checks only; no completed row substitutes for host evidence:

- [ ] `cargo fmt --all -- --check`, clippy, and the exact pure-Rust package tests.
- [ ] `node tools/ci/check-rust-slice-coverage.mjs` with `--files` limited to
  the pure model and minimum 80% per business-logic file.

The Step 3 coverage owner for the pure state model is
`microsoft-native-vram-memory-tier-n3-state`; its planner owner test is
`tools/ci/plan-rust-slice-coverage.test.mjs` ::
`microsoft_native_vram_n3_state_has_exact_coverage_owner`. Its exact per-file
gate and report location are:

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-tier --files crates/ramshared-tier/src/n3_state.rs --min 80 --report-json tmp/microsoft-native-vram-memory-tier-n3-cov.json
```

The `crates/ramshared-tier/src/lib.rs` change is a shared N/A module-export
glue owner, `microsoft-native-vram-memory-tier-n3-module-export-glue`, for the
concurrent N3 and NBD pure models. The current base-to-worktree projection is
permitted only when removing exactly these two LF-terminated lines in their
declared order, `pub mod n3_state;` and `pub mod nbd_readiness;`, produces a
byte-for-byte match with the immutable base. This file has no business logic;
any function, body, static, re-export, unsafe item, extra line, changed
existing line, whitespace/line-ending change, duplicate declaration, or use of
this owner for another path fails closed. The owner must also run:

```bash
cargo test -p ramshared-tier --all-targets
```

<!-- rust-slice-module-export-glue-differential-v1
{"schema_version":1,"id":"microsoft-native-vram-memory-tier-n3-module-export-glue","kind":"rust-module-export-glue-differential","files":["crates/ramshared-tier/src/lib.rs"],"package":"ramshared-tier","declaration":"pub mod n3_state;\npub mod nbd_readiness;","cargo_test":["cargo","test","-p","ramshared-tier","--all-targets"]}
-->

- [ ] Every legitimate boundary has a paired refusal/ambiguity test.
- [ ] Deterministic timestamp/epoch fixtures prove replay and stale handling.
- [ ] The six named Rust protocol tests cover grant/revoke, stale generation,
  duplicate idempotence, in-flight refusal, guest-crash failsafe, and durable
  fresh-model restart refusal.
- [ ] `N3_HOST_ENV_BOUND_MATRIX` is independently reviewed; host rows remain
  `PARTIAL`/`REFUSED_HOST_CONTRACT` until owner-approved evidence exists.
- [ ] No test opens `/dev/dxg`, calls Windows, loads a module, or changes host state.
- [ ] Independent review checks the RFC against public Microsoft/Linux sources.
- [ ] Host integration remains blocked unless the owner supplies a versioned,
  public contract and separate platform SPEC.
- [ ] `IMPL.md` is `partial`/`REFUSED_HOST_CONTRACT` for missing host evidence;
  no root validation entry is created without a live before → action → after run.

## Out of SPEC

- Native VRAM implementation, host adapter, kernel ABI, driver, Windows code,
  memory hotplug, HMM, NUMA, PFNs, or device-private migration.
- NBD/ublk product routing, #41054 config adoption, release readiness, reboot,
  WSL shutdown, GPU pressure, or external RFC submission.
- Any inferred Microsoft position or claim that N3 is already available.
