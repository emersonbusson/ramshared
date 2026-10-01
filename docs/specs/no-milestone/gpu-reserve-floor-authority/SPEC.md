# SPEC — Single authoritative GPU reserve floor for shared-host VRAM admission

> SSDV3 Step 2 · PRD: docs/specs/no-milestone/gpu-reserve-floor-authority/PRD.md
>
> Closes the reserve-floor contract drift recorded in `wsl2-isolated-gpu-cache-worker`
> (validation checklist) and left open by `cuda-rust-native-tiering` Step 2.5 pass 3.
>
> Step 2.5 pass 1 (2026-09-30): dropped the DT-11 Day-0 deprecation of
> `MIN_VRAM_HEADROOM_MIB` (it is a live `cascade.conf` key, out of scope here) and routed
> both documented names through one raise-only resolver; routed `physical_target_bytes`
> through `safe_target_bytes` so DT-4 and DT-6 no longer disagree; added the named proof for
> sealed percent above 20%; replaced the invented exit `2` with the existing failure path;
> defined `ReserveFloorEnv`. See AUDIT-2.5.md.
>
> Step 2.5 pass 2 (2026-09-30): DT-6 no longer fabricates provenance — `GpuSample` has no
> source, adapter, or timestamp, and `safe_target_bytes` is arithmetic-only, so the origin
> path must validate freshness and provenance before calling and refuse what it cannot
> prove; dropped `runtime_headroom_bytes` from `ReserveFloorPolicy` (no constructor input,
> and it duplicated `RUNTIME_FREE_BUFFER_BYTES` across crates); restated DT-7 as two real
> checks instead of the tautology "sealed fields equal what the policy enforces"; split
> DT-9's three distinct zeros; added the ITEM-4 Kahneman row; renamed `sealed_min_bytes` to
> `min_floor_bytes` and made DT-10 report an override as `lab-override-raise` only when it
> actually raises the enforced result.
>
> Step 2.5 pass 3 (2026-09-30, **final**): fixed DT-6's two-owners contradiction — the
> producer carries provenance, `physical_target_bytes` validates, `observe_gpu` only
> forwards; defined `capacity` as the helper's own `min(total.unwrap_or(budget), budget)` so
> the percentage share and the 20% share use one capacity; brought
> `scripts/safety/preflight.sh` (a fifth reserve constant, 256 MiB, gating daemon startup)
> under the authority; replaced DT-7(b)'s self-referential assertion with the real
> inequalities `min_floor_bytes >= sealed_min` and `sealed_percent == verified percent`; stated
> that `GpuSample` drops `Copy` and pinned the sample clock to `Instant`; renamed
> `cascade_conf_mib` to `alias_mib` because nothing parses cascade.conf; aligned the
> `sparse_vram` before→after and ITEM-3's order line.

## Closed scope

### In now

- One configured-reserve authority: the sealed host-origin manifest fields
  `gpu_reserve_min_mib` and `gpu_reserve_percent`.
- One admission formula: `GpuBudgetSnapshot::required_free_bytes` /
  `safe_target_bytes` in `crates/ramshared-vram/src/lib.rs`, reached through a new
  `ReserveFloorPolicy` resolver.
- Rewiring the four VRAM admission surfaces to that formula and deleting their inline
  reserve math.
- A raise-only lab override, failing closed below the sealed authority.
- Binding manifest seal verification to the value the runtime enforces.
- Requalification of the capacity boundary and live adapter, and closing the parent gap.

### Out now

- Any change to the sealed numeric values (`2048` MiB, `20`).
- Any kernel, LKM, HMM, DRM, CXL, uAPI, sysfs, ioctl, or Windows driver surface.
- Swap-format, origin-format, pagefile, or block-ABI changes.
- Removal or redefinition of `RUNTIME_FREE_BUFFER_BYTES` (stays 640 MiB).
- Cache compression, cascade tiering, reclaim policy, and capacity autotuning.
- Runtime re-reading of the environment after startup; the resolved policy is immutable.

### Assumed-ready dependencies

- `GpuBudgetSnapshot::required_free_bytes(configured_reserve_bytes, runtime_headroom_bytes)`
  and `safe_target_bytes(...)` in `crates/ramshared-vram/src/lib.rs` (lines 155–189). These
  already implement `reserve = max(configured, ceil(min(total, budget)/5))` and subtract it
  from both capacity and live headroom. This SPEC does **not** change their math.
  **Both are arithmetic only.** `safe_target_bytes`'s own doc comment states "Callers must
  first validate source, identity, freshness, and budget consistency" — the helpers implement
  no precondition. DT-6's ownership chain is: the producer **carries** provenance,
  `physical_target_bytes` **validates** and refuses, `observe_gpu` only **forwards**.
  Both helpers derive `capacity = total_bytes.unwrap_or(budget_bytes).min(budget_bytes)`; DT-2
  pins `configured_reserve_bytes`'s argument to that same expression.
- `GpuBudgetSnapshot` staleness/consistency preconditions already used by
  `crates/ramshared-wsl2d/src/gpu_budget.rs :: safe_cache_target_with_runtime`
  (`GpuBudgetSource::DriverReported`, `used <= budget`, `budget <= total`, adapter present,
  sample age `<= WDDM_BUDGET_MAX_AGE`).
- Host-origin manifest load and seal verification in `crates/ramshared-wsl2d/src/main.rs`
  (`host_configuration_sha256`, `host and guest origin manifests disagree on sealed policy or
  identity`).
- `RUNTIME_FREE_BUFFER_BYTES = 640 MiB` in `crates/ramshared-block/src/gpu_cache_worker.rs:30`.
- Parent qualification procedures from `wsl2-isolated-gpu-cache-worker` (capacity boundary,
  live adapter).

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 single authority | DT-1, DT-2, DT-3, ITEM-1, ITEM-4, ITEM-5 |
| RF-2 single formula | DT-4, DT-5, DT-6, ITEM-2, ITEM-3, ITEM-4 |
| RF-3 seal binds enforcement | DT-7, ITEM-5 |
| RF-4 raise-only lab override | DT-8, ITEM-5 |
| RF-5 no silent weakening | DT-9, ITEM-1 |
| NFR-1 host safety bound | DT-2, DT-4 |
| NFR-2 numbers before adjectives | ITEM-6 |
| NFR-3 observability | DT-10, ITEM-5 |
| NFR-4 no latency shim | DT-4 |
| NFR-5 capacity measured, not promised | ITEM-6 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | The configured reserve component is resolved once at startup by a new `ReserveFloorPolicy` in `crates/ramshared-vram/src/reserve_policy.rs`, then passed by value to every admission surface. The policy owns **only the configured component**: `min_floor_bytes` (the resolved minimum, which a raise-only override may raise and never lower) and `sealed_percent`. It carries **no** runtime headroom — that is `RUNTIME_FREE_BUFFER_BYTES` (`crates/ramshared-block/src/gpu_cache_worker.rs:30`) and stays the caller's second argument to `required_free_bytes` / `safe_target_bytes`, exactly as the existing call sites already pass it. | One resolution point means one place to test, audit, and log. Passing by value makes the policy immutable and removes any dual reader. A `runtime_headroom_bytes` field would have no constructor input and would duplicate a constant that lives in another crate — the duplication class this SPEC exists to remove. |
| DT-2 | `ReserveFloorPolicy::configured_reserve_bytes(capacity)` returns `max(min_floor_bytes, floor(capacity * sealed_percent / 100))`, and that value is passed unchanged as the `configured_reserve_bytes` argument to `required_free_bytes` / `safe_target_bytes`. **`capacity` is exactly the expression the helpers already use:** `total_bytes.unwrap_or(budget_bytes).min(budget_bytes)` derived from the **same snapshot** handed to the helper — never `total_vram_bytes` alone, never a second definition. The helper's internal `ceil(capacity/5)` term is left in place as a **non-negotiable 20% safety floor**, so a resealed percent below 20 can only fail the seal, never lower the reserve. Seal verification rejects `gpu_reserve_percent < 20`. The enforced free floor is therefore the three-term maximum `max(min_floor_bytes, floor(capacity * sealed_percent / 100), ceil(capacity/5)) + runtime_headroom_bytes` over that **one** capacity — the first two terms come from the policy, the third is the helper's safety floor. | Expresses the sealed percentage explicitly without changing a shared helper's signature, and fails safe: the existing 20% term can never be undercut. Pinning `capacity` to the helper's expression is what keeps the three terms comparable; two definitions of capacity would diverge the four surfaces again, especially on WDDM where `budget < total` is typical. Stating the three-term maximum here is what makes the PRD NFR-1 bound testable. |
| DT-3 | The policy is built from the already-verified host-origin manifest: `min_floor_bytes` starts as `gpu_reserve_min_mib * 1 MiB` (and is the only field a lab override may raise, DT-8), and `sealed_percent = gpu_reserve_percent` (never overridable). No new source of truth is introduced. | The manifest is the only integrity-protected, host/guest-agreed value. Honoring it requires no reseal and makes the existing `configuration_sha256` proof meaningful. |
| DT-4 | No new inline reserve math remains. `sparse_vram.rs`, `origin_cache.rs`, `gpu_cache_worker.rs`, and `gpu_budget.rs` all obtain their reserve threshold from `required_free_bytes` / `safe_target_bytes`. The hardcoded `2 * GIB` in `origin_cache.rs :: physical_target_bytes` is deleted. | Three formulas will diverge again. `required_free_bytes` is already unit-tested and already subtracts from live headroom as well as capacity; reusing it is the Day-0 single path. |
| DT-5 | `sparse_vram.rs` admission changes from `free < reserve_floor_bytes + chunk` to `free < required_free_bytes(policy.configured_reserve_bytes(capacity), RUNTIME_FREE_BUFFER_BYTES) + chunk`, and the demotion probe floor uses the same value. One named source for the headroom: `RUNTIME_FREE_BUFFER_BYTES`, passed by the caller — the policy does not carry it (DT-1). | This path currently drops both the percentage share and the runtime buffer — the most permissive production surface. Closing it is the highest-safety change in this slice. |
| DT-6 | `origin_cache.rs :: physical_target_bytes` no longer computes its own capacity expression. **One ownership chain, no second owner:** the **producer** carries provenance into `GpuSample` (`sampled_at: Instant`, `source: GpuBudgetSource`, `adapter: Option<GpuAdapterIdentity>`) — it does not validate; `observe_gpu` **forwards** the sample and its `now: Duration`, converted to `Instant` at that boundary (`origin_cache.rs:331`), and does not validate; **`physical_target_bytes` validates** and refuses — returns `0` — on a stale sample, a `source` other than `DriverReported`, a missing adapter, or an inconsistent sample (`external_usage_bytes > budget_bytes`, `budget_bytes > total_vram_bytes` when the total is known). Only after that does it return `safe_target_bytes(logical_bytes, configured, runtime_headroom)` over the numeric mapping `budget_bytes` → `budget_bytes`, `external_usage_bytes` → `used_bytes`, `total_vram_bytes` → `total_bytes`, with `capacity` per DT-2. `safe_target_bytes` is arithmetic only (its own doc comment: "Callers must first validate source, identity, freshness, and budget consistency") and supplies the arithmetic **after** the checks — this SPEC does not claim it implements them. **No field is ever invented**: never `Instant::now()` inside the path, never `GpuBudgetSource::DriverReported` written by the normalizer. The `.max(2 * GIB)` term is deleted. `GpuSample` **drops `Copy`** (see DT-12). | Satisfies DT-4: one formula, one helper, one set of preconditions, each enforced where it actually lives and each named exactly once. Naming two owners for one host-safety check is how half-implementations happen. Inventing the three provenance fields would make every staleness check pass by construction — the same false-guarantee class as the sealed-but-unread reserve this SPEC exists to remove. Mapping `external_usage_bytes` to `used_bytes` is correct at the origin path because it samples before allocating cache, so external use is the whole of used VRAM; the mapping is asserted by a named test. |
| DT-7 | Two checks, neither of them a tautology. **(a) Seal integrity (existing, kept):** manifest verification continues to reject a manifest whose `gpu_reserve_min_mib` / `gpu_reserve_percent` disagree with the verifier's sealed literals; that rejection is the integrity proof and is what `manifest_seal_rejects_reserve_mismatch` exercises, with an injected disagreeing manifest. **(b) Enforcement binding (new, startup invariant):** after resolution, assert the two inequalities that actually bind the runtime to the seal — `resolved.min_floor_bytes >= verified_sealed_min_bytes` **and** `resolved.sealed_percent == verified_sealed_percent`. These can fail: a resolver that wrote a lower minimum, or that dropped or altered the percentage, trips them. Disagreement is a startup failure on the existing error path. **What is explicitly not checked:** `configured_reserve_bytes(capacity) == max(min_floor_bytes, floor(capacity * sealed_percent / 100))` — that is DT-2's own definition and is true by construction; it is a unit test of the formula (`configured_reserve_capacity_matches_helper_capacity`), not a seal binding. Nor is "the sealed fields equal what the policy will enforce" checked — the policy is built *from* those fields. | A seal that attests a policy no code path enforces is a false guarantee; this is the defect that made the drift invisible. Check (a) proves the manifest was not altered; check (b) proves the runtime did not silently undercut what the manifest sealed. The two together are RF-3. A test that cannot fail is not evidence — and asserting a function against its own definition is exactly that. |
| DT-8 | `RAMSHARED_MIN_VRAM_FREE_MIB` and `MIN_VRAM_HEADROOM_MIB` become raise-only inputs to the same resolver, **both read from the process environment** exactly as `reserve_floor_bytes_from_env()` reads them today (DT-11; `MIN_VRAM_HEADROOM_MIB` is documented as a cascade.conf key that the cascade boot path exports — this type does not parse that file). Resolution accepts an override only when `override_mib >= sealed_min_mib`; otherwise startup prints `reserve floor override below sealed authority: <env> < <sealed>` and exits through the existing failure path (`ExitCode::from(1)` in `main()`), not a new exit-code class. Clamp-to-128 behavior is removed. The override writes **`min_floor_bytes` only** — `sealed_percent` is always taken from the verified manifest and is never overridable, so `configured_reserve_bytes` keeps applying `floor(capacity * sealed_percent / 100)` after any override and the percentage share cannot be dropped through the seam (DT-7 check (b)). | Today a value of 128 MiB is silently accepted on a shared host. An operator may be more conservative than the seal, never less. Reusing the existing failure exit keeps one error taxonomy (`COMMAND_FATAL_EXIT_CODE = 125` stays reserved for its own class); the message is the observable distinction. Letting the override touch only the minimum term is what makes the seam raise-only in *effect*, not merely in the comparison. |
| DT-9 | Checked/saturating arithmetic at the policy and helper boundary. **Three distinct zeros, three behaviors** — they are not one refusal: (1) `gpu_reserve_min_mib = 0` or `gpu_reserve_percent = 0` is a **manifest error** and `from_manifest` refuses it; (2) `capacity = 0` is a **degenerate query** and `configured_reserve_bytes(0)` returns `min_floor_bytes` (never an error); (3) `configured_reserve_bytes = 0` is **safe**, because `required_free_bytes` still applies `capacity.div_ceil(5)` — this is Kahneman #16 evidence to *assert* (a zero configured value never yields a larger allocation than a well-formed one), not a value to refuse. Beyond the zeros: `checked` overflow refuses, and a stale or inconsistent sample (`used > budget`, `budget > total` when known, `source != DriverReported`, sample older than `WDDM_BUDGET_MAX_AGE`) refuses per DT-6. | Prevents a malformed value or telemetry from silently producing a larger allocation (PRD RF-5, Kahneman #16). Collapsing the three zeros into one refusal is unimplementable: a zero capacity cannot be a refusal when the caller is simply asking for a floor. |
| DT-10 | Startup logs one line with the enforced floor in MiB, its source, the sealed minimum, the override value when one was supplied, the percentage share used, and the runtime buffer. The source label is **honest**: `lab-override-raise` only when the override actually raises the enforced result above the sealed-only result; when the override is **subsumed** by the percentage share (override ≥ `sealed_min_mib` but `floor(capacity * sealed_percent / 100)` already exceeds it) the source is `sealed-manifest` and the log notes the override was subsumed. The same fields are added to the existing status/monitor output with an explicit VRAM-reserve label. | NFR-3: an operator must see what is enforced, not what is configured. Logging `lab-override-raise` for an override that changed nothing tells the operator the floor moved when it did not. |
| DT-11 | **No name is deprecated in this slice.** `RAMSHARED_MIN_VRAM_FREE_MIB` and `MIN_VRAM_HEADROOM_MIB` are both documented configuration names: the first is an environment override, the second is a live key of `/etc/ramshared/cascade.conf` (`wsl2-cascade-boot` PRD §7). Both are accepted inputs to the **one** raise-only resolver defined in DT-8; setting both with different values is a refusal. Renaming, merging, or removing either name is a separate cascade-config decision and is out of scope here. **No Day-0 exception is required**, because this creates no shim, no dual path, and no deprecated surface — one resolver, two documented inputs, one policy. | Keeps a single policy path without asserting authority over a cascade.conf key this SPEC does not own. Removing a configuration name that the cascade boot surface documents would be a silent contract break. |
| DT-12 | `GpuSample` gains `sampled_at: Instant`, `source: GpuBudgetSource`, `adapter: Option<GpuAdapterIdentity>` and **drops `Copy`** — `GpuAdapterIdentity` owns `String` fields (`crates/ramshared-vram/src/lib.rs:52-57`) and cannot be `Copy`, so the derive goes and call sites take `&GpuSample` or clone explicitly. The sample clock is **`Instant`**, matching `GpuBudgetSnapshot::sampled_at` and `can_admit_at`; `observe_gpu`'s `now: Duration` is converted to `Instant` once at that boundary, and no fourth clock is introduced (`sampled_at_unix_ms` stays a serialization concern of `GpuBudgetTelemetry`). | Three clocks were in play and the spec picked none; a `Copy` derive on a type that would own `String` is a compile error, not an implementation detail. Pinning `Instant` keeps the staleness comparison in the one unit the existing precondition already uses. |

## Atomicity and rollback

### Atomicity frontier

- **Startup:** the policy is resolved once before any allocation. A resolution failure exits
  the process; no VRAM has been touched.
- **Admission:** each allocation re-evaluates `required_free_bytes` against a fresh snapshot
  and proceeds only if `available_bytes >= required_free_bytes + request`. There is no cached
  "already admitted" state.
- **Seal:** manifest verification completes before the daemon serves. A seal failure aborts
  startup; no partial policy is active.

### Rollback

- **Userspace/daemon:** revert the caller to the previous constant and redeploy. Because the
  resolved policy is passed by value and computed once, reverting is a build change, not a
  data migration.
- **Kernel/module:** N/A — no kernel code or ABI is changed.
- **Host/persistent:** N/A — the sealed manifest values are unchanged, so no host
  reconfiguration or reseal is required to roll back. `/proc/swaps`, leases, and the VHDX
  origin are untouched.
- **Forward-only?** No. This change is fully reversible without host action.

### Numeric rollback trigger

Any of the following reverts the admission caller and opens a defect: one allocation observed
below the enforced floor in a capacity-boundary run; one Windows display or foreground GPU
starvation event attributable to the new floor; one seal verification that accepts a manifest
whose reserve fields disagree with the runtime authority; one production path found still
computing a reserve inline.

## Kahneman map (critical only)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-1 / policy resolution | #16 — from exhaustion | Can a malformed, zero, or overflowing reserve value ever produce a *larger* allocation than a well-formed one? | `cargo test -p ramshared-vram reserve_policy` — `override_below_sealed_is_refused`, `zero_sealed_field_is_refused`, `zero_configured_never_raises_allocation`, `overflow_is_refused`, `percent_below_twenty_is_rejected`, `override_writes_min_floor_only` | Any case where a bad input yields a lower threshold: stop and fix the resolver before rewiring callers. |
| ITEM-2 / sparse rewire | #13 — refusal + legitimate | Does the sparse tier refuse below the shared floor **and** still admit a legitimate request above it? | `cargo test -p ramshared-block sparse_admission_uses_shared_reserve_floor` | A refusal that also blocks a legitimate request, or an admission below the floor: revert this caller. |
| ITEM-3 / origin rewire | #9 — number, not adjective | Does the origin physical target equal `safe_target_bytes(logical, configured, runtime_headroom)` for the same normalized snapshot, with no residual 2 GiB term — and does the path **refuse** stale, untrusted, or inconsistent samples rather than stamping provenance? | `cargo test -p ramshared-block origin_physical_target_uses_shared_reserve_floor`, `origin_sample_maps_external_usage_to_used_bytes`, `origin_stale_sample_refuses`, `origin_untrusted_sample_refuses`, `origin_path_never_stamps_provenance` | Any snapshot where the origin target exceeds what `safe_target_bytes` returns; any unnormalizable, stale, or untrusted sample yielding a nonzero target; any `Instant::now()` or `GpuBudgetSource::DriverReported` written inside the path. |
| ITEM-4 / worker and broker rewire | #9 — number, not adjective | Does each caller admit at the same free-bytes threshold the shared helper computes for an identical snapshot, with the headroom argument taken from the single named constant? | `cargo test -p ramshared-block worker_admission_uses_resolved_policy`, `worker_admission_refuses_on_stale_budget`; `cargo test -p ramshared-wsl2d broker_admission_uses_resolved_policy`, `candidate_target_applies_reserve_freshness_and_request_cap` | Any caller admitting below the helper threshold, any second source of the 640 MiB headroom, or any stale budget accepted. |
| ITEM-5 / seal bind | #13 — refusal + legitimate | Does the seal reject a disagreeing manifest **and** accept an agreeing one — and does the runtime refuse to run below the seal it verified? | `cargo test -p ramshared-wsl2d manifest_seal_rejects_reserve_mismatch`, `enforcement_binding_matches_verified_seal`; drill: `preflight_refuses_below_sealed_floor` | A disagreeing manifest that passes, an agreeing one that fails, a resolved policy below the verified seal, or a preflight that starts the daemon under the sealed floor: do not proceed to requalification. |
| ITEM-5 / lab override | #15/#16 | Can an operator lower the floor below the seal through any configuration name or combination? | `cargo test -p ramshared-vram override_below_sealed_is_refused`, `conflicting_overrides_refuse`, `both_documented_names_resolve_raise_only`; live: `low_reserve_override_fails_startup` | Any path that accepts a lower value: treat as a safety defect and block merge. |
| ITEM-6 / requalification | #5 — worst case / real load | Under a predeclared foreground GPU workload, does the new floor hold without display or deadline starvation, and what usable cache remains? | Capacity-boundary run + live adapter session, four-category table with Tier 3 origin metrics and `PASS_ZERO_PANIC` | Any starvation event, any allocation below the floor, or any verdict other than `PASS_ZERO_PANIC`. |

## Security checklist (pre-impl)

- [x] Privilege: **N/A** — no new privileged surface; existing daemon startup path only (2026-10-01: confirmed — pure userspace policy resolution.)
- [x] User/host copy: **N/A** — no user buffer is copied; env and manifest are read once at startup into owned values (2026-10-01: confirmed.)
- [x] Flags/IOCTL codes: **N/A** — no ioctl or uAPI (2026-10-01: confirmed.)
- [x] Info-leak: enforced floor and source may be logged; no kernel addresses, KASLR material, or host paths beyond those already logged (2026-10-01: confirmed — the logged `source_label` is an enum string, not a pointer or path.)
- [x] IRQ/atomic or IRQL: **N/A** — pure userspace, no atomic context (2026-10-01: confirmed.)
- [x] Lifetime: policy is owned and immutable for the process lifetime; no get/put surface (2026-10-01: confirmed.)
- [x] Hot-unplug / device-gone: an adapter that disappears leaves the last snapshot stale; stale snapshots refuse (DT-9), they do not allocate (2026-10-01: `origin_stale_sample_refuses`, `origin_untrusted_sample_refuses`, and `physical_target_bytes_refuses_inconsistent_sample` are green — stale and untrusted samples produce a refusal, never an allocation.)
- [x] Host safety: no unsupervised live WSL2 pressure. The requalification pressure component runs only through the approved watchdog harness or an isolated lab VM.
- [x] Shared-hardware cushion: `max(min_floor_bytes, floor(capacity * sealed_percent / 100), ceil(capacity/5)) + runtime_headroom` survives admission. Restated precisely, because pass 2 cleared an earlier false `[x]`: `safe_target_bytes` subtracts `runtime_headroom` from its **live-headroom** term only, and `required_free_bytes` adds it to the reserve. The two expressions differ on paper, but every production caller refuses `budget_bytes > total_bytes` **before** the arithmetic, so `capacity == budget_bytes`, `within_live_headroom <= within_capacity`, and the live-headroom term always binds. Proven by `safe_target_keeps_the_floor_for_trusted_snapshots` (`capacity - used - target >= required_free_bytes`, six trusted snapshots including the live host figure). The counter-case `safe_target_can_violate_the_floor_without_budget_consistency` proves the consistency check is load-bearing. DT-6's caller-side checks are enforced by `origin_stale_sample_refuses`, `origin_untrusted_sample_refuses`, and `physical_target_bytes_refuses_inconsistent_sample`.
- [x] Bounded DMA / foreign driver calls: **N/A** — no DMA and no new driver call. Driver-query staleness is **not** "handled by the assumed-ready preconditions" in the helper: DT-6 puts those checks on the origin caller, and the broker path keeps its own (`can_admit_at` / `trusted_available_at`). Both must be green before this box is left N/A-by-design. (2026-10-01: both gates confirmed — `can_admit_at` (`gpu_budget.rs`) and `trusted_available_at` (`ramshared-vram/src/lib.rs`) are exercised by green tests; `cargo test -p ramshared-vram --lib` 42 passed / 0 failed, `cargo test -p ramshared-wsl2d --lib` 179 passed / 0 failed.)
- [x] Cooperative cascade spillover: out of scope here; the SSD origin remains authoritative and unchanged (2026-10-01: confirmed — no spillover path is implemented or claimed in this slice.)
- [x] Replayable ops: startup resolution is idempotent — the same env and manifest always produce the same policy; re-running startup after a failed run changes nothing persistent.

## Files to CREATE / MODIFY / DELETE

### CREATE

**`crates/ramshared-vram/src/reserve_policy.rs`**
- Purpose: the single configured-reserve authority. Resolve once from the sealed manifest, apply the raise-only env override, and expose `configured_reserve_bytes(capacity)` for the existing helpers.
- RF / DT: RF-1, RF-4, RF-5; DT-1, DT-2, DT-3, DT-8, DT-9, DT-11.
- Types / fns:
  ```rust
  /// The two documented configuration names, both read from the **process environment**
  /// exactly as `reserve_floor_bytes_from_env()` reads them today (DT-11). Neither is
  /// deprecated in this slice:
  /// `env_mib` from RAMSHARED_MIN_VRAM_FREE_MIB;
  /// `alias_mib` from MIN_VRAM_HEADROOM_MIB — a name documented as a
  /// `/etc/ramshared/cascade.conf` key (`wsl2-cascade-boot` PRD §7) that the cascade boot
  /// path exports into the environment. The field is `alias_mib`, not `cascade_conf_mib`:
  /// this type does not parse that file and must not claim to.
  pub struct ReserveFloorEnv {
      pub env_mib: Option<u64>,
      pub alias_mib: Option<u64>,
  }
  /// The configured-reserve authority only (DT-1). No runtime headroom field: callers pass
  /// `RUNTIME_FREE_BUFFER_BYTES` as the second helper argument.
  pub struct ReserveFloorPolicy {
      /// Resolved minimum floor. Starts as the sealed manifest minimum; a raise-only
      /// override may raise it and never lower it. Not the sealed value after an override.
      pub min_floor_bytes: u64,
      /// Always the verified manifest percentage. Never overridable (DT-8).
      pub sealed_percent: u64,
  }
  pub enum ReserveFloorError {
      OverrideBelowSealed { override_mib: u64, sealed_min_mib: u64 },
      ConflictingOverrides { env_mib: u64, alias_mib: u64 },
      PercentBelowSafetyFloor { sealed_percent: u64 },
      ZeroSealedField { field: &'static str },
      Overflow,
  }
  impl ReserveFloorPolicy {
      /// Refuses `gpu_reserve_min_mib == 0` or `gpu_reserve_percent == 0` (DT-9 case 1),
      /// and `gpu_reserve_percent < 20` (DT-2).
      pub fn from_manifest(gpu_reserve_min_mib: u64, gpu_reserve_percent: u64) -> Result<Self, ReserveFloorError>;
      /// Raise-only. Writes `min_floor_bytes` only; `sealed_percent` is unchanged (DT-8).
      pub fn resolve_with_env(base: &Self, env: &ReserveFloorEnv) -> Result<Self, ReserveFloorError>;
      /// `max(min_floor_bytes, floor(capacity * sealed_percent / 100))`. `capacity == 0`
      /// returns `min_floor_bytes` and is not an error (DT-9 case 2).
      pub fn configured_reserve_bytes(&self, capacity: u64) -> u64;
  }
  ```
- Reference pattern: `GpuBudgetSnapshot` in `crates/ramshared-vram/src/lib.rs`.
- Required tests: `reserve_policy::tests::override_below_sealed_is_refused`; `reserve_policy::tests::override_above_sealed_is_raise_only`; `reserve_policy::tests::override_writes_min_floor_only`; `reserve_policy::tests::conflicting_overrides_refuse`; `reserve_policy::tests::percent_below_twenty_is_rejected`; `reserve_policy::tests::configured_reserve_never_below_twenty_percent`; `reserve_policy::tests::configured_reserve_honors_sealed_percent_above_twenty`; `reserve_policy::tests::configured_reserve_capacity_matches_helper_capacity`; `reserve_policy::tests::zero_sealed_field_is_refused`; `reserve_policy::tests::zero_capacity_returns_min_floor`; `reserve_policy::tests::zero_configured_never_raises_allocation`; `reserve_policy::tests::overflow_is_refused`; `reserve_policy::tests::both_documented_names_resolve_raise_only`.
- Cover target: ≥80% on business logic.
- Kahneman: #16, #13.

**`scripts/p0/measure-gpu-reserve-floor.sh`**
- Purpose: capacity-boundary campaign harness. Runs the fixed three-run comparison of the old sparse floor versus the sealed shared floor on an isolated canary origin with a predeclared foreground GPU workload, captures the automatic context (timestamp, branch+commit, kernel, `nvidia-smi`, RAM/swap, disk util/latency, what was open), aggregates median + p99 + deviation, and writes both `docs/benchmarks/results.jsonl` and `docs/BENCHMARKS.md` per [`.claude/rules/benchmarks.md`](../../../../.claude/rules/benchmarks.md).
- RF / DT: NFR-2, NFR-5; DT-4.
- Reference pattern: existing `scripts/p0/measure-*.sh` harnesses and `scripts/p0/bench.sh`.
- Owning command (the `--drill` value is the exact test name below, so the matrix row is executable as written):
  ```bash
  scripts/p0/measure-gpu-reserve-floor.sh --drill measure_gpu_reserve_floor::capacity_boundary_campaign \
    --rounds 3 --condition loaded --allocating --workload-cmd '<predeclared foreground GPU workload>'
  scripts/p0/measure-gpu-reserve-floor.sh --drill measure_gpu_reserve_floor::live_adapter_before_action_after \
    --allocating --workload-cmd '<predeclared foreground GPU workload>'
  ```
  Both invocations require `RAMSHARED_ALLOW_PRESSURE=1`; without it the harness exits 77 and refuses. A bare `--drill snapshot` is read-only and is **not** either campaign.
- Required tests: `measure_gpu_reserve_floor::capacity_boundary_campaign`; `measure_gpu_reserve_floor::live_adapter_before_action_after` (drills, not unit tests). Enforced floor, usable cache bytes, and the four-category table with Tier 3 origin metrics and `PASS_ZERO_PANIC`; live before → action → after on an isolated canary origin with `BINARY_MATCH` when `ramsharedd` is exercised.
- Cover target: N/A — shell harness; E2E evidence is the campaign record.
- Kahneman: #5.

### MODIFY

**`crates/ramshared-vram/src/lib.rs`**
- What/how/why: `pub mod reserve_policy;` re-export. **No change** to `required_free_bytes` or `safe_target_bytes` math (DT-2 leaves the `div_ceil(5)` term in place as the 20% safety floor).
- RF / DT: RF-2; DT-2, DT-4.
- Required tests: existing `budget_target_preserves_reserve_after_existing_use` and `required_free_bytes` assertions remain and still pass unchanged.
- Cover target: existing business logic stays ≥80%.

**`crates/ramshared-block/src/sparse_vram.rs`**
- What/how/why: `reserve_floor_bytes: reserve_floor_bytes_from_env()` at line 71 is replaced by a `ReserveFloorPolicy` supplied by the caller. The admission check at line 273 becomes `need = required_free_bytes(configured, RUNTIME_FREE_BUFFER_BYTES) + chunk`. The demotion probe floor at line 226 uses the same value. `reserve_floor_bytes_from_env()` moves its parse into `ReserveFloorEnv` / `ReserveFloorPolicy::resolve_with_env` and is deleted here (DT-8, DT-11).
- Before → after: `free < env_floor + chunk` → `free < required_free_bytes(policy.configured_reserve_bytes(helper_capacity), RUNTIME_FREE_BUFFER_BYTES) + chunk`.
- RF / DT: RF-1, RF-2; DT-4, DT-5, DT-11.
- Required tests: `sparse_admission_uses_shared_reserve_floor`; `sparse_probe_floor_matches_shared_reserve`.
- Cover target: ≥80%.

**`crates/ramshared-block/src/origin_cache.rs`**
- What/how/why: `GpuSample` (line 125) gains the provenance the preconditions need — `sampled_at`, `source: GpuBudgetSource`, `adapter: Option<GpuAdapterIdentity>` — carried from the producer; `observe_gpu` already receives `now: Duration` (line 331) and passes the real sample age, never a fresh stamp. `physical_target_bytes` (line 135) validates provenance and freshness **first** (refuse on stale, `source != DriverReported`, missing adapter, or `external_usage_bytes > budget_bytes` / `budget_bytes > total_vram_bytes` when known), then returns `safe_target_bytes(logical_bytes, configured, runtime_headroom)` over the numeric mapping `budget_bytes`→`budget_bytes`, `external_usage_bytes`→`used_bytes`, `total_vram_bytes`→`total_bytes`. The `.max(2 * GIB)` term is deleted. `safe_target_bytes` supplies **arithmetic only** — this file owns the checks (DT-6).
- Before → after: `budget − external − max(ceil(total/5), 2 GiB)` → validate provenance, then `safe_target_bytes(logical, configured, runtime_headroom)`.
- RF / DT: RF-1, RF-2; DT-4, DT-6, DT-9.
- Required tests: `origin_physical_target_uses_shared_reserve_floor`; `origin_physical_target_has_no_hardcoded_two_gib_term`; `origin_sample_maps_external_usage_to_used_bytes`; `origin_unnormalizable_sample_returns_zero`; `origin_stale_sample_refuses`; `origin_untrusted_sample_refuses`; `origin_path_never_stamps_provenance`.
- Cover target: ≥80%.

**`crates/ramshared-block/src/gpu_cache_worker.rs`**
- What/how/why: `GpuWorkerConfig::reserve_floor_bytes: 1536 * 1024 * 1024` default is removed; the field becomes a `ReserveFloorPolicy`. The call at line 274 becomes `required_free_bytes(policy.configured_reserve_bytes(capacity), RUNTIME_FREE_BUFFER_BYTES)` — the headroom argument is the existing local constant (DT-1, DT-5), not a policy field. Line 304 uses the same form.
- RF / DT: RF-1, RF-2; DT-1, DT-4, DT-5.
- Required tests: `worker_admission_uses_resolved_policy`; `worker_admission_refuses_on_stale_budget`.
- Cover target: ≥80%.

**`crates/ramshared-wsl2d/src/gpu_budget.rs`**
- What/how/why: `safe_cache_target`, `safe_broker_slice_bytes`, and `ReserveCheckedProvider::new` take the resolved `ReserveFloorPolicy` instead of a bare `reserve_floor_bytes: u64`. The existing `required_free_bytes` / `safe_target_bytes` calls are unchanged apart from their argument source.
- RF / DT: RF-1, RF-2; DT-1, DT-4.
- Required tests: `broker_admission_uses_resolved_policy`; `candidate_target_applies_reserve_freshness_and_request_cap` (existing, must still pass).
- Cover target: ≥80%.

**`crates/ramshared-wsl2d/src/main.rs`**
- What/how/why: after manifest verification, build `ReserveFloorPolicy::from_manifest(host.gpu_reserve_min_mib, host.gpu_reserve_percent)` and `resolve_with_env` with both documented names read from the process environment (`ReserveFloorEnv`). Replace `reserve_floor_bytes_from_env()` at line 3247. Add the DT-7 enforcement binding: `resolved.min_floor_bytes >= verified_sealed_min_bytes` and `resolved.sealed_percent == verified_sealed_percent` — not a comparison against the policy's own formula. Log the DT-10 startup line. On `ReserveFloorError`, return `Err` from `run()` so `main()` exits via its existing `ExitCode::from(1)` path with a distinct message — no new exit-code class.
- RF / DT: RF-1, RF-3, RF-4; DT-3, DT-7, DT-8, DT-10, DT-11.
- Required tests: `manifest_seal_rejects_reserve_mismatch`; `enforcement_binding_matches_verified_seal`; `low_reserve_override_fails_startup`; `high_reserve_override_logs_raise_only`.
- Cover target: ≥80% on extracted business logic only; do not widen `main.rs` coverage by unrelated lines.

**`crates/ramshared-cli/src/monitor.rs`**
- What/how/why: surface the enforced reserve floor and its source with an explicit VRAM-reserve label; never merge with guest/host RAM counters.
- RF / DT: NFR-3; DT-10.
- Required tests: `monitor_labels_enforced_reserve_floor_as_vram`.
- Cover target: ≥80% on touched business logic.

**`scripts/safety/preflight.sh`**
- What/how/why: this is a **fifth reserve surface** and the operator's startup go/no-go gate. Line 19 currently sets `MIN_VRAM_FREE_MIB="${RAMSHARED_MIN_VRAM_FREE_MIB:-256}"` and lines 46–47 abort the start below that value — a 256 MiB floor that will print `[ok] VRAM libre=300 MiB` on a host whose enforced floor is ~2688 MiB. The refusal threshold becomes the **same authority the daemon enforces**: at minimum the sealed `gpu_reserve_min_mib` (2048), and where the script can observe capacity, the full three-term floor of DT-2. If `RAMSHARED_MIN_VRAM_FREE_MIB` is set, preflight applies the **same raise-only rule** (DT-8): below the sealed minimum is a refusal, above it is honored. The silent 256 default is removed.
- Before → after: `free < env_or_256` → `free < sealed_min_mib` (or the computed three-term floor), raise-only on the env override.
- RF / DT: RF-1, RF-4; DT-2, DT-8.
- Required tests: `preflight_refuses_below_sealed_floor`; `preflight_honors_raise_only_override`.
- Cover target: N/A — shell gate; evidence is the drill record plus the two named cases in the campaign harness.

**`docs/decisions/ADR-NNN-gpu-reserve-floor-authority.md`**
- What/how/why: record the authority decision, the discarded alternatives, and the numeric rollback trigger.
- RF / DT: RF-1; DT-1, DT-3.
- Required tests: `docs-check` and generated-index check.
- Cover target: N/A — documentation.

**`docs/reliability/GAP-REGISTER.md`; `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/{SPEC,IMPL}.md`; `docs/specs/no-milestone/cuda-rust-native-tiering/{AUDIT-2.5,IMPL}.md`; `validation.md`**
- What/how/why: close or update the reserve-drift gap and the child-spec open dependency; append the requalification result.
- RF / DT: NFR-2, NFR-5.
- Required tests: `docs-check`.
- Cover target: N/A — documentation.

### DELETE

- `reserve_floor_bytes_from_env()` in `crates/ramshared-block/src/sparse_vram.rs` — its parse moves into `ReserveFloorEnv`, and its clamp-to-128 behavior is the safety hole DT-8 closes. **No configuration name is deleted:** `RAMSHARED_MIN_VRAM_FREE_MIB` and `MIN_VRAM_HEADROOM_MIB` both remain live (DT-11).
- The `2 * GIB` reserve term in `crates/ramshared-block/src/origin_cache.rs` is deleted by DT-6. The `GIB` constant is removed only if no other use remains.
- No files are deleted.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| Enforced reserve floor (MiB) | daemon startup log; status JSON; monitor | info / gauge |
| Reserve floor source (`sealed-manifest` \| `lab-override-raise`) | daemon startup log; status JSON; monitor | info / enum |
| Sealed percent share used | daemon startup log | info |
| Runtime headroom (MiB) | daemon startup log | info |
| `ReserveFloorError` on startup | stderr via existing `run()` error path, nonzero exit | error |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Alter — record the single reserve authority |
| `docs/decisions/ADR-NNN-gpu-reserve-floor-authority.md` | Create |
| `docs/reliability/DEGRADATION-MATRIX.md` | N/A — no new degradation mode; the previous overcommit mode is removed |
| `docs/reliability/GAP-REGISTER.md` | Close the reserve-floor contract gap on requalification |
| `validation.md` | Append on close |
| `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl` | Register the capacity-boundary campaign (≥3 runs, median + p99 + deviation, four-category table) |
| `.claude/rules/*` · `CLAUDE.md` · `AGENTS.md` | N/A — no convention change |

## Implementation order

- **ITEM-1 — `ReserveFloorPolicy` and its unit tests.** Types, `from_manifest`,
  `resolve_with_env`, `configured_reserve_bytes`, all refusals. Nothing else changes until the
  resolver is complete and its Kahneman #16 row is green.
- **ITEM-2 — sparse tier rewire.** `sparse_vram.rs` to the shared helper (DT-5). Highest safety
  value; do this before the lower-risk callers so the most permissive path closes first.
- **ITEM-3 — origin physical target rewire.** `origin_cache.rs` to the shared helper (DT-6):
  add provenance fields to `GpuSample` and drop `Copy` (DT-12), have `physical_target_bytes`
  validate and refuse before calling, delete the 2 GiB term, and prove the path never stamps
  provenance. DT-6 is **not** satisfied by deleting the constant alone.
- **ITEM-4 — worker and broker rewire.** `gpu_cache_worker.rs` and `gpu_budget.rs` take the
  resolved policy (DT-1, DT-4).
- **ITEM-5 — seal bind, raise-only env, observability.** `main.rs` seal comparison (DT-7),
  env semantics (DT-8), startup log and monitor label (DT-10).
- **ITEM-6 — requalification and closure.** Capacity-boundary and live-adapter runs, four-
  category table with Tier 3 metrics and `PASS_ZERO_PANIC`, then close the parent checklist
  item, the GAP-REGISTER entry, and the `cuda-rust-native-tiering` open dependency.

ITEM-1 through ITEM-5 are unit-testable without hardware. ITEM-6 is env-bound and produces
**partial** if no adapter session is available.

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::override_below_sealed_is_refused` | unit | #13/#16 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::override_above_sealed_is_raise_only` | unit | #13 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::override_writes_min_floor_only` | unit | #13/#16 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::conflicting_overrides_refuse` | unit | #13 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::percent_below_twenty_is_rejected` | unit | #13 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::configured_reserve_never_below_twenty_percent` | unit | #9 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::configured_reserve_honors_sealed_percent_above_twenty` | unit | #9 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::zero_sealed_field_is_refused` | unit | #16 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::zero_capacity_returns_min_floor` | unit | #9 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::zero_configured_never_raises_allocation` | unit | #16 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::overflow_is_refused` | unit | #16 | ≥80% |
| Policy resolution | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::both_documented_names_resolve_raise_only` | unit | #13/#17 | ≥80% |
| Policy capacity identity | `crates/ramshared-vram/src/reserve_policy.rs :: reserve_policy::tests::configured_reserve_capacity_matches_helper_capacity` | unit | #9 | ≥80% |
| Enforcement binding | `crates/ramshared-wsl2d/src/main.rs :: enforcement_binding_matches_verified_seal` | unit | #13 | ≥80% on extracted logic |
| Sparse tier admission | `crates/ramshared-block/src/sparse_vram.rs :: sparse_admission_uses_shared_reserve_floor` | unit | #13 | ≥80% |
| Sparse tier probe | `crates/ramshared-block/src/sparse_vram.rs :: sparse_probe_floor_matches_shared_reserve` | unit | #9 | ≥80% |
| Origin physical target | `crates/ramshared-block/src/origin_cache.rs :: origin_physical_target_uses_shared_reserve_floor` | unit | #9 | ≥80% |
| Origin physical target | `crates/ramshared-block/src/origin_cache.rs :: origin_physical_target_has_no_hardcoded_two_gib_term` | unit | #13 | ≥80% |
| Origin sample mapping | `crates/ramshared-block/src/origin_cache.rs :: origin_sample_maps_external_usage_to_used_bytes` | unit | #9 | ≥80% |
| Origin sample mapping | `crates/ramshared-block/src/origin_cache.rs :: origin_unnormalizable_sample_returns_zero` | unit | #16 | ≥80% |
| Origin provenance | `crates/ramshared-block/src/origin_cache.rs :: origin_stale_sample_refuses` | unit | #16 | ≥80% |
| Origin provenance | `crates/ramshared-block/src/origin_cache.rs :: origin_untrusted_sample_refuses` | unit | #13 | ≥80% |
| Origin provenance | `crates/ramshared-block/src/origin_cache.rs :: origin_path_never_stamps_provenance` | unit | #16 | ≥80% |
| Cache worker admission | `crates/ramshared-block/src/gpu_cache_worker.rs :: worker_admission_uses_resolved_policy` | unit | #9 | ≥80% |
| Cache worker admission | `crates/ramshared-block/src/gpu_cache_worker.rs :: worker_admission_refuses_on_stale_budget` | unit | #16 | ≥80% |
| Broker slice admission | `crates/ramshared-wsl2d/src/gpu_budget.rs :: broker_admission_uses_resolved_policy` | unit | #9 | ≥80% |
| Broker candidate cap (regression) | `crates/ramshared-wsl2d/src/gpu_budget.rs :: candidate_target_applies_reserve_freshness_and_request_cap` | unit (existing) | #9 | ≥80% |
| Parent helper regression | `crates/ramshared-vram/src/lib.rs :: budget_target_preserves_reserve_after_existing_use` | unit (existing) | #9 | ≥80% |
| Manifest seal | `crates/ramshared-wsl2d/src/main.rs :: manifest_seal_rejects_reserve_mismatch` | unit | #13 | ≥80% on extracted logic |
| Lab override refusal | `crates/ramshared-wsl2d/src/main.rs :: low_reserve_override_fails_startup` | integration | #13 | ≥80% on extracted logic |
| Lab override raise | `crates/ramshared-wsl2d/src/main.rs :: high_reserve_override_logs_raise_only` | integration | #13 | ≥80% on extracted logic |
| Monitor label | `crates/ramshared-cli/src/monitor.rs :: monitor_labels_enforced_reserve_floor_as_vram` | unit | #9 | ≥80% |
| Preflight gate | `scripts/safety/preflight.sh :: preflight_refuses_below_sealed_floor` | drill/E2E | #13 | hardware evidence |
| Preflight gate | `scripts/safety/preflight.sh :: preflight_honors_raise_only_override` | drill/E2E | #13 | hardware evidence |
| Docs | `docs/` :: `docs-check` and `node tools/generate-docs-index.mjs --check` | check | — | N/A |
| Capacity boundary | `scripts/p0/measure-gpu-reserve-floor.sh :: measure_gpu_reserve_floor::capacity_boundary_campaign` | drill/E2E | #5 | hardware evidence |
| Live adapter | `scripts/p0/measure-gpu-reserve-floor.sh :: measure_gpu_reserve_floor::live_adapter_before_action_after` | E2E | #5 | hardware evidence |

Kinds: unit · integration · drill/E2E. Every row names a real test; none is "add unit tests".

## Validation checklist

- [x] `cargo fmt --all -- --check` (2026-10-01: exit 0.)
- [x] `cargo clippy -p ramshared-vram -p ramshared-block -p ramshared-wsl2d -p ramshared-cli --all-targets -- -D warnings` (2026-10-01: exit 0, zero warnings.)
- [x] `cargo test -p ramshared-vram -p ramshared-block -p ramshared-wsl2d -p ramshared-cli` (2026-10-01: 1122 passed, 0 failed, 19 hardware-gated ignored.)
- [x] Coverage for `crates/ramshared-vram/src/reserve_policy.rs`
  (2026-09-30: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-vram --files crates/ramshared-vram/src/reserve_policy.rs --min 80 --report-json tmp/gpu-reserve-floor-policy-cov.json`
  — 93.7% lines (118/126), gate PASSED. All thirteen named
  `reserve_policy::tests` are present and green, including
  `override_below_sealed_is_refused`, `override_writes_min_floor_only`,
  `percent_below_twenty_is_rejected`, `zero_configured_never_raises_allocation`,
  and `overflow_is_refused`.)
- [x] Every other touched Rust business-logic file is already owned by a
  `docs/governance/rust-slice-coverage.json` line-coverage entry:
  `crates/ramshared-vram/src/lib.rs` (`vram-provider-abstraction`),
  `crates/ramshared-block/src/sparse_vram.rs` (`cascade-vram-ondemand-policy`),
  `crates/ramshared-block/src/origin_cache.rs` and `crates/ramshared-block/src/gpu_cache_worker.rs`
  and `crates/ramshared-wsl2d/src/gpu_budget.rs` (`isolated-gpu-cache-worker`,
  `wsl2-revocable-vram-origin-block`), `crates/ramshared-wsl2d/src/main.rs`
  (`memory-broker-wsl2d-entrypoint-contract`), and
  `crates/ramshared-cli/src/monitor.rs` (`wsl2-dual-plane-monitor`).
  (2026-09-30: confirmed — `plan-rust-slice-coverage.mjs --all` reports
  `RUST_SLICE_COVERAGE_STATUS=READY` with no `changed-rust-file-unmapped`.)
- [x] Every matrix row has a real test name and passes. (2026-10-01: 13 matrix rows, all 13 named `reserve_policy::tests` present and green; the module now has 20 tests total, 0 failed.)
- [ ] Kahneman critical rows have executable evidence (named tests above; ITEM-6 drill for #5).
      (2026-10-01: ITEM-1 through ITEM-5 are green with executable named tests —
      `cargo test -p ramshared-vram --lib` 42 passed / 0 failed,
      `cargo test -p ramshared-block --lib` 178 passed / 0 failed,
      `cargo test -p ramshared-wsl2d --lib` 179 passed / 0 failed,
      `cargo test -p ramshared-cli --bin ramshared` 537 passed / 0 failed,
      and `scripts/safety/test-preflight-reserve-floor.sh` exit 0 with
      `preflight_honors_raise_only_override: ok` and
      `preflight_refuses_below_sealed_floor: ok`. All 21 named tests in the
      required-tests matrix resolve to source. **ITEM-6 (#5) stays open**: the
      capacity-boundary run and live adapter session are lab-bound and are not
      claimed.)
- [x] No production path computes a reserve inline; grepping for `2 * GIB` as a reserve term, `1536 * 1024 * 1024` as an admission default, and `reserve_floor_bytes_from_env` returns nothing on an admission path. (2026-10-01: `2 * GIB` hits are test-only (`cascade.rs` validation tests, `origin_cache.rs` tests documenting the deleted `.max(2 * GIB)` term); `1536 * 1024 * 1024` hits are the broker's own `BROKER_DISPLAY_RESERVE_BYTES` (a different surface) and test fixtures in `gpu_cache_worker.rs`; `reserve_floor_bytes_from_env` returns zero hits — the function was deleted in `76a90c55`. `physical_target_bytes` line 200: "No inline reserve math remains.")
- [x] No path stamps provenance: grepping for `Instant::now()` and `GpuBudgetSource::DriverReported` written inside `physical_target_bytes` or its normalizer returns nothing. (2026-10-01: `physical_target_bytes` takes `now: Instant` as a parameter and never calls `Instant::now()`; it *refuses* `GpuBudgetSource::DriverReported` mismatches at line 178 rather than writing the variant; zero `DriverReported` writes inside the function or its normalizer.)
- [x] Startup log and status/monitor report the enforced floor and source; the source is `lab-override-raise` only when the override actually raised the enforced result. (2026-10-01: `ReserveFloorSource::source_label` in `crates/ramshared-vram/src/reserve_policy.rs` returns `LabOverrideRaise` → `"lab-override-raise"` only when `enforced > base.enforced`; the string is asserted by `source_labels_are_human_readable`. `crates/ramshared-cli/src/monitor.rs` reports the floor and its source and explicitly never claims `lab-override-raise` for a recomputed value that was not raised.)
- [ ] Capacity-boundary campaign: ≥3 runs, median + p99 + deviation, four-category table with directions, Tier 3 origin metrics, alarm thresholds from [`.claude/rules/benchmarks.md`](../../../../.claude/rules/benchmarks.md).
- [ ] Live path for this surface (userspace WSL2 adapter): before → action → after on an isolated canary origin; no unsupervised live-host pressure; `BINARY_MATCH` when `ramsharedd` is exercised.
- [ ] Until the live adapter and capacity-boundary gates pass, record the result as **partial**, never DONE.
