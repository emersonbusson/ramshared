# AUDIT-2.5 — gpu-reserve-floor-authority

> SSDV3 Step 2.5 · PRD: [PRD.md](PRD.md) · SPEC: [SPEC.md](SPEC.md)
> **Pass 1 — 2026-09-30.** Reviews the PRD/SPEC pair written this turn.
> **Pass 2 — 2026-09-30.** Re-audits the package after pass 1's same-turn fixes.
> **Pass 3 — 2026-09-30 (final).** Closing re-audit after pass 2's same-turn fixes.
> No code was implemented and no hardware was qualified by this audit.

## Direction audit — is this the best thing to do

**Yes, and the shape is right.** The finding is not "pick a bigger number"; it is that
RamShared has **three admission formulas and four constants**, and the only
integrity-protected value — the sealed host-origin manifest `gpu_reserve_min_mib` /
`gpu_reserve_percent` — is compared at seal verification and then **never read by any
admission path**. A seal that attests a policy no code enforces is a false guarantee, and the
most permissive path (`sparse_vram.rs`) drops both the percentage share and the runtime
buffer.

Binding the runtime to the sealed value is the only option that makes the existing
`configuration_sha256` proof meaningful **without a reseal**. The formula already exists
(`GpuBudgetSnapshot::required_free_bytes` / `safe_target_bytes` implement
`max(configured, ceil(min(total, budget)/5)) + runtime_headroom`), so this is a rewiring, not
a new allocator. Rejected alternatives (raising the env default, adopting 1536 MiB from the
parent PRD, deleting the seal fields, documenting three formulas) are correctly discarded.

**Priority is right.** This is the host-safety cushion (Principle 11) for every VRAM path.
It precedes cache compression and any capacity feature.

## Findings — pass 3 (final)

Pass 2's fabricated-provenance defect is closed. Pass 3 checked what remained against the tree
and against internal consistency. Four High findings, two Medium, one Low. One of them answers
an open question pass 1 left on the table.

| Sev | SPEC / PRD § | Issue | Required fix |
| :--- | :--- | :--- | :--- |
| High | DT-6; Assumed-ready | **One host-safety check, two named owners.** DT-6 opens with "provenance and freshness — validated by the **caller**, before the arithmetic", then says "`physical_target_bytes` refuses — returns `0` — on a stale sample…". The Assumed-ready section repeats "DT-6 therefore puts provenance and freshness on the origin caller", while the `origin_cache.rs` MODIFY entry puts the validation inside `physical_target_bytes`. An implementer can follow half the text and satisfy half the tests. This is the same two-owners defect class as pass 1's DT-4/DT-6. | Fix the ownership chain once: the **producer** carries provenance into `GpuSample`; **`physical_target_bytes` validates** and refuses; `observe_gpu` only forwards. Every reference (DT-6, Assumed-ready, MODIFY, Kahneman ITEM-3) states that chain and nothing else. *(Fixed this turn.)* |
| High | DT-2 vs `required_free_bytes` | **The `capacity` argument is unspecified per call site and is not the capacity the helper uses.** `required_free_bytes` / `safe_target_bytes` derive `capacity = total_bytes.unwrap_or(budget_bytes).min(budget_bytes)` **internally**. `configured_reserve_bytes(capacity)` takes `capacity` as a **parameter**. Nothing says which value each of the four callers passes. If a caller passes `GpuSample::total_vram_bytes` while the helper uses `min(total, budget)`, DT-2's three-term maximum mixes two definitions of capacity — and on WDDM, where `budget < total` is typical, the four surfaces diverge again. That is the exact defect this SPEC exists to remove. | Define `capacity` as **exactly** the helper's expression: `min(total_bytes.unwrap_or(budget_bytes), budget_bytes)`. Every caller derives it from the same snapshot it hands the helper. Add `configured_reserve_capacity_matches_helper_capacity` and extend `sparse_admission_uses_shared_reserve_floor` to assert both terms use it. *(Fixed this turn.)* |
| High | PRD RF-1; Files; **new surface** | **`scripts/safety/preflight.sh` is a fifth reserve constant and a startup go/no-go gate that this package never lists.** Line 19: `MIN_VRAM_FREE_MIB="${RAMSHARED_MIN_VRAM_FREE_MIB:-256}"`; lines 46–47 abort the start when free VRAM is below it. A **256 MiB** default is the operator's safety gate, and it will print `[ok] VRAM livre=300 MiB` on a host whose enforced floor is ~2688 MiB. RF-1's acceptance greps "production admission **paths**", so a gate that never allocates slips through. **This also answers pass 1's open question** — "does any operator script set a value below 2048 today?" — yes, `preflight.sh` defaults to 256 and gates startup on it. | Bring `preflight.sh` under the authority: its refusal threshold becomes the sealed floor (or the full three-term floor where it can compute it), never a silent 256 default. If the env override is set, preflight uses the same raise-only rule as the daemon so the two can never disagree. List the file in Files MODIFY, add `preflight_refuses_below_sealed_floor` to the matrix, and record the finding in the runbook. *(Fixed this turn.)* |
| High | DT-7(b); PRD RF-3 | **DT-7(b) asserts the policy's own definition; PRD RF-3 keeps the circular wording pass 2 removed from DT-7.** "Assert `configured_reserve_bytes(capacity)` equals `max(min_floor_bytes, floor(capacity * sealed_percent / 100))`" is true by construction of DT-2 and **cannot detect a lowered override**. The genuine RF-3 property is `resolved.min_floor_bytes >= verified sealed_min_bytes` **and** `resolved.sealed_percent == verified sealed_percent`. PRD RF-3 still reads "fail if the sealed reserve fields do not match the value the runtime will enforce" — the tautology reformulated. | Replace DT-7(b) with the two real inequalities and rename its test to `enforcement_binding_matches_verified_seal`. Reword PRD RF-3 to what check (a) actually proves: the seal rejects a manifest whose fields disagree with the verifier's sealed literals, and the resolved policy is never below those literals. *(Fixed this turn.)* |
| Medium | DT-6; Files MODIFY | **The specified `GpuSample` change breaks its `Copy` derive, and the clocks do not line up.** `GpuSample` is `#[derive(Clone, Copy, Debug, Eq, PartialEq)]` (`origin_cache.rs:124`). DT-6 adds `adapter: Option<GpuAdapterIdentity>`; `GpuAdapterIdentity` holds `String` fields (`lib.rs:52-57`) and is **not** `Copy`. The derive dies and every by-value `Option<GpuSample>` call site and inline test constructor ripples — unstated. Separately, three clocks are in play and DT-6 picks none: `observe_gpu` receives `now: Duration`, `GpuBudgetSnapshot.sampled_at` is `Instant`, `GpuBudgetTelemetry` uses `sampled_at_unix_ms`. "Passes the real sample age" is not implementable without choosing one. | State that `GpuSample` drops `Copy` (or keeps provenance in a non-owned form) and name the ripple; pick `Instant` for `sampled_at`, matching `GpuBudgetSnapshot::sampled_at` and `can_admit_at`, and have `observe_gpu` convert its `now: Duration` at the boundary rather than inventing a fourth clock. *(Fixed this turn.)* |
| Medium | DT-11; `ReserveFloorEnv` | **`ReserveFloorEnv::cascade_conf_mib` has no reader.** `MIN_VRAM_HEADROOM_MIB` is documented as a key of `/etc/ramshared/cascade.conf` (`wsl2-cascade-boot` PRD §7), but the only reader in the tree is `std::env::var("MIN_VRAM_HEADROOM_MIB")` (`sparse_vram.rs:459`). Nothing parses cascade.conf. The type is named `ReserveFloorEnv` and `resolve_with_env` implies environment reading, yet half its input is documented as a config-file key. Who fills `cascade_conf_mib` — the environment, a cascade.conf parser, or the cascade boot path exporting the key — is unspecified, and the name of the field asserts an origin the code does not have. | State the real source: both values are read from the **process environment** as they are today (the cascade boot path is what exports the key), rename `cascade_conf_mib` to `alias_mib` so the type does not claim to parse a file it never opens, and keep DT-11's "no name is deprecated" boundary. *(Fixed this turn.)* |
| Low | Files MODIFY; Implementation order | The `sparse_vram.rs` before→after line still says `max(sealed_min, sealed% of cap)` — the pre-rename name. ITEM-3's order line says only "delete the 2 GiB term" and omits the provenance work DT-6 requires, so an implementer following the order alone ships half of DT-6. | Align both. *(Fixed this turn.)* |

## Findings — pass 2

Pass 1 closed six defects. Pass 2 checked the result against the code it claims to rewire and
found one false safety guarantee, three unimplementable or incomplete contracts, and two
wording defects. Every claim below was verified in the tree.

| Sev | SPEC / PRD § | Issue | Required fix |
| :--- | :--- | :--- | :--- |
| **Hard no-go** | DT-6; Security checklist | **DT-6 fabricates the three fields the staleness/consistency preconditions actually check, and attributes those preconditions to a helper that does not implement them.** `GpuSample` (`crates/ramshared-block/src/origin_cache.rs:125-129`) has exactly three fields — `budget_bytes`, `external_usage_bytes`, `total_vram_bytes`. It has **no** `source`, **no** `adapter`, **no** timestamp. DT-6 says the normalization adds "`GpuBudgetSource::DriverReported`, the adapter identity, and a current `sampled_at`" and that the origin path "inherits the same staleness and consistency checks the broker path already uses." Those three invented fields are the entire content of the checks being claimed, so they would always pass. Worse, `safe_target_bytes` is **arithmetic only** — its own doc comment says "Callers must first validate source, identity, freshness, and budget consistency." The checks live in `GpuBudgetSnapshot::can_admit_at` / `GpuBudgetTelemetry::trusted_available_at` and in the broker caller, never in the helper. The security checklist's shared-hardware cushion box is already `[x]` on this basis. This is the same false-guarantee class as the sealed-but-unread reserve this SPEC exists to remove. | Restate DT-6 as two separate obligations: (a) `safe_target_bytes` supplies **arithmetic only** and must not be described as carrying preconditions; (b) the origin path validates provenance and freshness **before** calling, and refuses what it cannot prove. `GpuSample` must gain the provenance the checks need (`sampled_at`/`source`/`adapter`), or `physical_target_bytes` must take a `&GpuBudgetSnapshot` the caller already validated. `observe_gpu` already receives `now: Duration` (`origin_cache.rs:331`) — that is the real time signal; never stamp `Instant::now()` inside the normalization. `origin_unnormalizable_sample_returns_zero` must cover a stale and an untrusted sample, not only field mismatch. Uncheck the shared-hardware box until it is proven. *(Fixed this turn.)* |
| High | PRD NFR-1 vs DT-2 | **The host-safety bound omits the sealed-percent term.** NFR-1 states the floor as `max(sealed_min_bytes, ceil(min(total, budget)/5)) + runtime_headroom_bytes`. DT-2 makes the configured component `max(sealed_min_bytes, floor(capacity * sealed_percent / 100))`, so the enforced floor is `max(sealed_min, floor(cap * sealed_percent / 100), ceil(cap/5)) + headroom`. On a reseal to 30% over a large adapter, NFR-1 understates the bound by the whole percentage-share delta. NFR-1 is the acceptance artifact for the host-safety cushion, so an understated bound is an untestable gate. PRD §3 also still says the percentage is realized "through the existing `ceil(capacity/5)` term, **or** explicitly through the sealed percent — see DT-2", leaving in the PRD an ambiguity DT-2 already closed. | Fix NFR-1 to the three-term maximum and remove the "or" from PRD §3. *(Fixed this turn.)* |
| High | DT-1; Files CREATE | **`ReserveFloorPolicy.runtime_headroom_bytes` has no constructor input and duplicates a constant in another crate.** The struct declares three fields; `from_manifest(gpu_reserve_min_mib, gpu_reserve_percent)` takes two arguments and never assigns the third. The 640 MiB value lives at `crates/ramshared-block/src/gpu_cache_worker.rs:30` as `RUNTIME_FREE_BUFFER_BYTES`, while the policy is specified in `crates/ramshared-vram/src/reserve_policy.rs` — reading it there would be a new cross-crate dependency or a silent copy, which is the exact duplication class this SPEC kills. DT-5 passes `RUNTIME_FREE_BUFFER_BYTES` while the `gpu_cache_worker` MODIFY entry passes `policy.runtime_headroom_bytes`: two named sources again. | Drop `runtime_headroom_bytes` from `ReserveFloorPolicy`. The policy owns the **configured** component only; callers pass `RUNTIME_FREE_BUFFER_BYTES` as the second helper argument, exactly as the existing code already does. DT-5 and the MODIFY entry become one statement. *(Fixed this turn.)* |
| High | DT-7; RF-3 | **The seal comparison is a tautology and its test has no mismatch it can detect.** `ReserveFloorPolicy::from_manifest` is built from `host.gpu_reserve_min_mib` / `host.gpu_reserve_percent`, and DT-7 then requires "the sealed reserve fields must equal the values `ReserveFloorPolicy` will enforce." That is true by construction. `manifest_seal_rejects_reserve_mismatch` cannot fail through this path, so RF-3's acceptance ("a named test feeds a manifest whose reserve fields disagree with the configured runtime authority and asserts the seal rejects") is unsatisfiable as specified. The real integrity proof is already in the tree: the verifier's literals (`!= 2048 \|\| != 20`) reject a disagreeing manifest before any policy is built. | Restate DT-7 as the two real checks: (a) the existing literal seal rejection remains the integrity proof and is covered by `manifest_seal_rejects_reserve_mismatch` with an injected disagreeing manifest; (b) a startup invariant asserts the resolved policy's `configured_reserve_bytes(capacity)` equals `max(sealed_min, floor(capacity * sealed_percent / 100))` for the **verified** manifest terms, so a raise-only override can never lower the percentage share. Delete the circular "sealed fields equal what the policy enforces" phrasing. *(Fixed this turn.)* |
| Medium | DT-9 | **Three different zeros are collapsed into one refusal, and one of them is not implementable.** DT-9 states that "`0` … are refusals, not clamped-to-zero successes." There are three distinct zeros: `gpu_reserve_min_mib = 0` (a manifest error — `from_manifest` should refuse), `capacity = 0` (should return the configured floor, not error), and `configured_reserve_bytes = 0` (safe, because `required_free_bytes` still applies `capacity.div_ceil(5)` — this is Kahneman #16 *evidence to assert*, not a value to refuse). `zero_and_overflow_refuse` cannot be written against an undifferentiated "0". | Split DT-9 into the three cases with the correct behavior for each. *(Fixed this turn.)* |
| Medium | Kahneman map | **ITEM-4 has no row.** The map covers ITEM-1, 2, 3, 5, 6. ITEM-4 (`gpu_cache_worker.rs` + `gpu_budget.rs` rewire) carries two named matrix tests but no critical-evidence row, so "critical only" either omits a critical item or silently declares it non-critical. The worker is the production cache path. | Add an ITEM-4 row (#9) with its two named tests and an abort condition. *(Fixed this turn.)* |
| Low | PRD §7 vs SPEC | PRD §7 shows `pub fn resolve(override_mib: Option<u64>)`, a single-argument API that contradicts the two documented configuration names (`ReserveFloorEnv` in SPEC). An implementer reading the PRD as the contract builds the wrong surface. PRD §11 also omits `ARCHITECTURE.md`, which the SPEC Living-docs table lists. | Align PRD §7 to the SPEC shape (`from_manifest` / `resolve_with_env` / `ReserveFloorEnv`) and add `ARCHITECTURE.md` to §11. *(Fixed this turn.)* |
| Low | DT-1; DT-10 | After `resolve_with_env` writes a raise override into `sealed_min_bytes`, the field name is no longer true — it is no longer the sealed value — and DT-10 logs `lab-override-raise` even when the override is **subsumed** by the percentage share (override 3000 MiB against a 6144 MiB share changes nothing). The operator is told a raise happened when the enforced floor is the sealed one. | Rename the field to `min_floor_bytes`, keep the sealed terms for the log, and report the source honestly: `lab-override-raise` only when the override actually raises the enforced result above the sealed-only result; otherwise `sealed-manifest` with a note that the override was subsumed. *(Fixed this turn.)* |

## Findings — pass 1

| Sev | SPEC § | Issue | Required fix |
| :--- | :--- | :--- | :--- |
| **Hard no-go** | DT-11; PRD §8 | **Day-0 exception without a real removal deadline, and wrong scope.** DT-11 gives `MIN_VRAM_HEADROOM_MIB` a removal date of "the release after the runbook is updated" — not a deadline. Worse, that name is not a stray env var: it is a documented key of `/etc/ramshared/cascade.conf` (`wsl2-cascade-boot` PRD §7 Data model). Deprecating it is a cascade-config change, and this SPEC's Out-of-scope explicitly excludes cascade tiering policy. The exception was trying to solve a problem this slice does not own. | Drop the deprecation and delete the Day-0 exception entirely. Both documented names are accepted inputs to **one** raise-only resolver; conflicting values are a refusal; **neither name is deprecated in this slice**. Renaming or removing a cascade.conf key is a separate cascade-config decision. *(Fixed this turn.)* |
| High | DT-4 vs DT-6 | **Internal contradiction re-creates the defect this SPEC exists to remove.** DT-4 forbids inline reserve math and requires every surface to obtain its threshold from `required_free_bytes` / `safe_target_bytes`. DT-6 then keeps a bespoke expression in `origin_cache.rs :: physical_target_bytes` — `budget − external − required_free_bytes(configured, 0)` — which is still inline capacity math with a shared reserve term bolted on. Two formulas again. | Route `physical_target_bytes` through `safe_target_bytes` by normalizing its `GpuSample` into a `GpuBudgetSnapshot` (`external_usage_bytes` → `used_bytes`, plus source/adapter/sample age for the existing staleness preconditions). One formula, one helper. *(Fixed this turn.)* |
| High | DT-2 vs Required tests matrix | **A closed decision has no named proof.** DT-2 states `configured_reserve_bytes` honors `sealed_percent` above 20%, but the matrix only tests the ≥20% floor (`configured_reserve_never_below_twenty_percent`) and the seal refusal below 20. A resealed 30% silently reverting to 20% would pass every listed test. | Add `reserve_policy::tests::configured_reserve_honors_sealed_percent_above_twenty`. *(Fixed this turn.)* |
| Medium | DT-8; PRD §6 Errors | **New exit-code taxonomy without justification.** SPEC and PRD specify process exit `2` for a low override. `crates/ramshared-wsl2d/src/main.rs` returns `ExitCode::from(1)` for every error and reserves `COMMAND_FATAL_EXIT_CODE = 125` for a separate fatal class. Exit `2` would be a third, undocumented class. | Use the existing `ExitCode::from(1)` path with a distinct message (`reserve floor override below sealed authority: <env> < <sealed>`). Tests assert a nonzero exit **and** the message. No new taxonomy in this slice. *(Fixed this turn.)* |
| Medium | DT-1; Files CREATE | **`ReserveFloorEnv` is used and never defined.** DT-1 and the `resolve_with_env(base, env)` signature reference it; the CREATE block does not declare its fields. An implementer must invent them. | Define `ReserveFloorEnv { primary_mib: Option<u64>, alias_mib: Option<u64> }` in the CREATE block. *(Fixed this turn.)* |
| Low | PRD §8 vs DT-11 | PRD states the alias is "retained as a deprecated alias for one release", which contradicts the corrected DT-11. | Align PRD to "both documented names accepted, neither deprecated in this slice". *(Fixed this turn.)* |

## Open questions — pass 1

- With `sealed_min_mib = 2048` on a 6 GiB adapter, the enforced free floor is
  `max(2048, ceil(6144/5)=1229) + 640 = 2688 MiB` (~45% of the card). **Is the remaining
  cache useful?** Unanswered until ITEM-6 measures it. If it is not, the lever is an explicit
  reseal of the number with the measurement attached (NFR-5) — never a relaxed formula.
- Does `external_usage_bytes` in `GpuSample` mean the same thing as `used_bytes` in
  `GpuBudgetSnapshot` at the moment the origin path samples? The DT-6 normalization assumes
  they do. If they diverge, the normalization must map them explicitly rather than structurally.
- Is `MIN_VRAM_HEADROOM_MIB` (cascade.conf) intended to be the *same* policy knob as
  `RAMSHARED_MIN_VRAM_FREE_MIB`, or a separate cascade-specific headroom? This slice treats
  both names as one raise-only resolver because that is what the code does today; if the
  cascade key is semantically distinct, that is a cascade-config question, not this one.
- Which adapter is the requalification target? The local RTX 2060 (`sm_75`, 6 GiB) is the
  obvious candidate, but ITEM-6 is env-bound and must not be closed on a host without a live
  adapter session.
- Does any operator runbook or script already set `RAMSHARED_MIN_VRAM_FREE_MIB` **below**
  2048 today? The raise-only change will make that invocation exit nonzero. Affected scripts
  must be identified before merge, not after.

## Open questions — pass 3 (final)

- **Closed this pass:** pass 1 asked whether any operator surface sets a reserve below 2048
  today. Yes — `scripts/safety/preflight.sh` defaults to 256 MiB and gates daemon startup on
  it. That is no longer an open question; it is finding 3 above and a required fix.
- **Still open, and now load-bearing:** pass 2 recorded that `safe_target_bytes` subtracts
  `runtime_headroom_bytes` from live headroom but **not** from `within_capacity`, while
  `required_free_bytes` **adds** it to the reserve. DT-2's three-term maximum states the floor
  as a single quantity, so a reader may assume the two helpers bound the same thing. They do
  not. This SPEC claims not to change the helpers' math — so either the asymmetry is
  documented as intentional in DT-2 and the ITEM-6 campaign reports both quantities, or the
  helper math is reconciled in a separate slice. **ITEM-6 must not be closed without stating
  which quantity it measured.**
- `GpuBudgetSource::ProviderLocalEstimate` is still unpicked for the origin path. DT-6 now
  refuses anything but `DriverReported`, which is the safer default and matches
  `can_admit_at`. If a real adapter session ever reports only `ProviderLocalEstimate`, that
  becomes a measured decision, not a silent widening.
- The four callers each need a `capacity` for `configured_reserve_bytes`. Pass 3 fixed the
  definition to the helper's expression, but there is still no named test asserting **all
  four** derive it the same way from their own snapshot types. `configured_reserve_capacity_
  matches_helper_capacity` covers the policy side; a cross-surface equivalence assertion
  belongs in ITEM-2/ITEM-3/ITEM-4 and is listed there.

## Verdict — pass 3 (final)

**`no-go`** on the package as reviewed. No hard-gate failure this pass — pass 2 closed the
fabricated-provenance defect — but four High findings remain, one of which names a **fifth
reserve surface** (`scripts/safety/preflight.sh`) that the package never listed and that gates
daemon startup on a 256 MiB floor.

All seven fixes were applied in the same turn. **This is the closing pass.** After those edits
the verdict is **`go`** for Step 3, with these standing conditions:

1. **ITEM-6 is env-bound.** Until a live adapter session completes the capacity-boundary and
   live-adapter runs, the work is recorded **partial**, never DONE.
2. **The sealed numeric value does not change.** `2048` MiB / `20` stay as sealed. Any
   revision is a separate, measured, resealed decision with the measurement attached.
3. **No operator surface may disagree with the authority.** `preflight.sh` and the daemon must
   refuse on the same floor; the runbook and any affected scripts are updated in the same
   change.
4. **Cascade configuration is not touched.** `MIN_VRAM_HEADROOM_MIB` remains a live
   `/etc/ramshared/cascade.conf` key. Renaming or removing it is a separate slice.
5. **No precondition may be claimed that no code path executes.** Every line of the security
   checklist stays unchecked until the implementation names the code that enforces it.
6. **Provenance is carried, never stamped.** Any sample type feeding `required_free_bytes` /
   `safe_target_bytes` carries its own age and source; a normalization that writes
   `DriverReported` or `Instant::now()` into a snapshot is a defect.
7. **The two helpers do not bound the same quantity.** ITEM-6 reports which of the two it
   measured before any gate is closed.

## Open questions — pass 2

- Who produces `GpuSample` today, and does that producer already validate freshness? If it
  does, the fix is a narrow one (carry the age it already has). If it does not, the origin
  path has been accepting stale capacity data all along and that is a separate defect worth
  its own record.
- `safe_target_bytes` subtracts `runtime_headroom_bytes` from live headroom but **not** from
  `within_capacity`, while `required_free_bytes` **adds** it to the reserve. The two helpers
  therefore do not bound the same quantity. That asymmetry is pre-existing and this SPEC
  claims not to change the helpers' math — but DT-5/DT-6 now route *both* surfaces through
  them, so the asymmetry becomes load-bearing. Is it intentional?
- `GpuBudgetSource` also has `ProviderLocalEstimate`. Should the origin path accept it (it
  may be the only source available before an allocation) or refuse it the way
  `can_admit_at` does? DT-6 must pick one; "DriverReported only" is the safer default and
  matches the broker path.
- The `manifest_seal_rejects_reserve_mismatch` test lives under `crates/ramshared-wsl2d/src/main.rs`.
  `main.rs` is not a natural unit-test host. Confirm the test target is an extracted
  pure function, not a `#[cfg(test)]` block that cannot be invoked without starting the daemon.

## Verdict — pass 2

**`no-go`** on the package as reviewed: one hard-gate failure — DT-6's normalization invents
the provenance the staleness preconditions check, and attributes those preconditions to
`safe_target_bytes`, which is arithmetic-only. The security checklist's shared-hardware
cushion was checked on that basis.

That fix and the six alignment fixes above were applied in the same turn. After those edits
the verdict is **`go`** for Step 3, carrying forward pass 1's four standing conditions plus:

5. **No precondition may be claimed that no code path executes.** Every line of the security
   checklist stays unchecked until the implementation names the code that enforces it.
6. **Provenance is carried, never stamped.** Any future sample type that feeds
   `required_free_bytes` / `safe_target_bytes` must carry its own age and source; a
   normalization that writes `DriverReported` or `Instant::now()` into a snapshot is a defect.

## Verdict — pass 1

**`no-go`** on the package as reviewed: one hard-gate failure — DT-11's Day-0 exception has
no removal deadline and attempts to deprecate a cascade.conf key that this SPEC declares out
of scope.

That fix and the five alignment fixes above were applied in the same turn. After those edits
the verdict is **`go`** for Step 3, with these standing conditions:

1. **ITEM-6 is env-bound.** Until a live adapter session completes the capacity-boundary and
   live-adapter runs, the work is recorded **partial**, never DONE (PRD NFR-5, SPEC validation
   checklist).
2. **The sealed numeric value does not change.** `2048` MiB / `20` stay as sealed. Any
   revision is a separate, measured, resealed decision with the measurement attached.
3. **No operator script may break silently.** The open question above must be answered before
   merge; if scripts set a lower override today, the runbook and those scripts are updated in
   the same change.
4. **Cascade configuration is not touched.** `MIN_VRAM_HEADROOM_MIB` remains a live
   `/etc/ramshared/cascade.conf` key. Renaming or removing it is a separate slice.
