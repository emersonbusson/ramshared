---
slug: gpu-reserve-floor-authority
title: Single authoritative GPU reserve floor for shared-host VRAM admission
milestone: —
issues: []
---

# PRD — Single authoritative GPU reserve floor for shared-host VRAM admission

## 1. Summary

RamShared currently computes the GPU free reserve with **three different formulas and four
different constants**, and the only value that is integrity-protected end-to-end — the sealed
host-origin manifest `gpu_reserve_min_mib` / `gpu_reserve_percent` — is **compared at seal
verification and then never read again**. Production admission therefore enforces a policy the
seal does not prove, and the most permissive path drops both the percentage share and the
runtime buffer.

This change makes one value the authority (the sealed manifest), one formula the only admission
calculation (`GpuBudgetSnapshot::required_free_bytes` / `safe_target_bytes`, which already
implement the correct shape), and routes every VRAM admission surface through it. The outcome
is a reserve floor that a seal can actually attest, enforced identically on every path.

## 2. Technical context

- **Confirmed in codebase:** `crates/ramshared-vram/src/lib.rs` already implements the correct
  shape in `GpuBudgetSnapshot::required_free_bytes(configured, runtime_headroom)` =
  `max(configured, ceil(min(total, budget)/5)) + runtime_headroom`, and `safe_target_bytes()`
  subtracts the same reserve from both capacity and live headroom.
- **Confirmed in codebase:** three admission surfaces do **not** all use it:
  - `crates/ramshared-block/src/sparse_vram.rs` compares `free < reserve_floor_bytes + chunk`
    with `reserve_floor_bytes = reserve_floor_bytes_from_env()` — no 20% share, no runtime
    buffer. This is the most permissive path in production.
  - `crates/ramshared-block/src/origin_cache.rs :: physical_target_bytes()` uses
    `budget − external − max(ceil(total/5), 2 GiB)` — a hardcoded 2 GiB constant that exists
    in no policy document.
  - `crates/ramshared-block/src/gpu_cache_worker.rs` calls `required_free_bytes(...)` correctly
    but feeds it `reserve_floor_bytes_from_env()`, not the sealed value.
  - `crates/ramshared-wsl2d/src/gpu_budget.rs` calls `required_free_bytes`/`safe_target_bytes`
    correctly; it inherits the same unbound constant.
- **Confirmed in codebase:** `GpuSample` (`crates/ramshared-block/src/origin_cache.rs:125-129`)
  has only `budget_bytes`, `external_usage_bytes`, `total_vram_bytes` — **no source, no adapter
  identity, no timestamp**. `GpuBudgetSnapshot`'s staleness and consistency preconditions are
  precisely those three fields (`source == DriverReported`, `adapter.is_some()`, sample age),
  and they are enforced by `can_admit_at` / `trusted_available_at` and by the broker caller —
  **not** by `safe_target_bytes`, whose doc comment says "Callers must first validate source,
  identity, freshness, and budget consistency." The origin path currently has no way to carry
  the provenance those checks need.
- **Confirmed in codebase:** `reserve_floor_bytes_from_env()` reads
  `RAMSHARED_MIN_VRAM_FREE_MIB` or `MIN_VRAM_HEADROOM_MIB`, defaults to **512 MiB**, and
  clamps to **128–4096 MiB**. A value of 128 MiB is accepted on the production path.
- **Confirmed in codebase — a fifth surface, and an operator go/no-go gate:**
  `scripts/safety/preflight.sh:19` sets `MIN_VRAM_FREE_MIB="${RAMSHARED_MIN_VRAM_FREE_MIB:-256}"`
  and lines 46–47 abort the start when free VRAM is below it. A **256 MiB** default will print
  `[ok] VRAM livre=300 MiB` on a host whose enforced floor is ~2688 MiB. It never allocates, so
  an acceptance criterion phrased as "production **admission** paths" misses it — but it is the
  surface an operator trusts before starting the daemon.
- **Confirmed in codebase:** `crates/ramshared-wsl2d/src/main.rs` seals the host origin manifest
  with `gpu_reserve_min_mib != 2048 || gpu_reserve_percent != 20` as a **comparison only**.
  No admission path reads those fields. The seal currently attests a policy that does not run.
- **Confirmed in codebase:** the generic `GpuWorkerConfig` default is
  `reserve_floor_bytes: 1536 * 1024 * 1024`, matching the parent PRD mitigation constant,
  while the production caller overrides it with the env default of 512 MiB.
- **Confirmed in docs:** parent PRD (`wsl2-isolated-gpu-cache-worker`) NFR-2 states
  `reserve = max(configured reserve, 20% of min(total, budget))` with a separate 640 MiB
  runtime buffer; its risk mitigation names `max(1536 MiB, 20%)`. Its validation checklist
  records the drift: "the production origin-cache caller currently defaults the configured
  floor to 512 MiB while the PRD mitigation requires 1536 MiB. Reconcile the contract and
  production default, then rerun capacity-boundary and live-adapter qualification."
- **Confirmed in docs:** `docs/specs/no-milestone/cuda-rust-native-tiering/SPEC.md` Step 2.5
  pass 3 (2026-09-30) corrected its own admission *shape* to consume `required_free_bytes` and
  left the floor *value* as an open dependency on this reconciliation.
- **Qualification limit:** no capacity-boundary measurement on this host has established which
  constant leaves a usable cache while protecting Windows display and foreground GPU work. The
  number is selected from the integrity-protected surface, not from a comfort estimate.

## 3. Recommended option

**Bind the runtime reserve to the sealed manifest value and route every admission path through
the existing `GpuBudgetSnapshot` helpers.**

Concretely: `gpu_reserve_min_mib` / `gpu_reserve_percent` become the source of the
`configured_reserve_bytes` argument — the minimum term and the percentage term both come from
the sealed manifest, and the shared helper's existing `ceil(capacity/5)` term remains as a
non-negotiable 20% safety floor beneath them (DT-2) — the env override becomes a raise-only
lab seam that can raise the minimum term and never touch the percentage, and
`sparse_vram.rs`, `origin_cache.rs`, `gpu_cache_worker.rs`, and `gpu_budget.rs` all admit
through `required_free_bytes` / `safe_target_bytes`. Competing constants are deleted.

### Why this option

1. **The seal becomes true.** Today it attests 2048 MiB / 20% while 512 MiB with no percentage
   share is enforced. Honoring the sealed value is the only option that makes the existing
   integrity proof meaningful without a reseal.
2. **No host configuration change.** 2048 / 20 is already sealed and checksummed end-to-end
   (`configuration_sha256`). Raising or lowering the number would require resealing the host
   origin configuration; binding to it requires none.
3. **The formula already exists.** `required_free_bytes` / `safe_target_bytes` implement
   `max(configured, ceil(capacity/5)) + runtime_headroom` today and are already unit-tested.
   This change is a rewiring, not a new allocator.
4. **Host safety first.** 2048 MiB / 20% is the strictest of the declared policies. On a shared
   WSL2 host where Windows display and foreground GPU work must never be starved, the
   conservative bound is the correct default; capacity can be recovered later by a deliberate,
   measured, resealed reduction — never by a silent env default.

### Discarded alternatives

1. *Raise the env default from 512 MiB to 1536 MiB and stop.* Minimal diff, but leaves the
   dead manifest seal, the divergent `sparse_vram` and `origin_cache` formulas, and the 128 MiB
   clamp-floor hole intact. It fixes the symptom the child spec found and leaves the class of
   bug in place. Rejected on Day-0 grounds: it preserves dual-path disagreement.
2. *Adopt `max(1536 MiB, 20%)` from the parent PRD mitigation as the constant.* Requires
   resealing the host manifest to 1536, weakening the strictest declared policy on a shared
   host, and still does not unify the three formulas. Rejected: it changes a sealed value to
   match a document instead of matching enforcement to a seal.
3. *Remove the seal fields entirely.* Would admit that no policy is attested. Rejected: the
   host-safety cushion must be provable (Principle 11), not merely present.
4. *Keep per-path formulas and document them.* Rejected: three formulas will diverge again, and
   the parent SPEC already records this as a reconciliation obligation.

### Trade-offs accepted

- Available cache capacity on this host drops relative to the current 512 MiB path. On a 6 GiB
  adapter the enforced free floor becomes `max(2048, ceil(6144/5)) + 640 = 2688 MiB` instead of
  the current sparse-path `512 + chunk`. Whether the remaining cache is useful is a **measured**
  question (NFR-5), not an assumed one.
- The 2 GiB constant in `origin_cache.rs` disappears. If a caller depended on it for a reason
  not recorded in any document, the capacity-boundary requalification will surface it.

## 4. Functional requirements (RF)

- **RF-1 (Single authority):** The GPU reserve floor used by every VRAM admission surface
  **and by every operator startup gate** must derive from the sealed host-origin manifest
  `gpu_reserve_min_mib` and `gpu_reserve_percent`. No other constant may be introduced as a
  default — including safety scripts that never allocate.
  *Acceptance:* grepping the tree for competing reserve constants
  (`512 * 1024 * 1024`, `2 * GIB`, `1536 * 1024 * 1024`, and the 256 MiB preflight default)
  yields no production admission path and no startup gate; a named test asserts each surface
  receives the sealed value; a named drill asserts `scripts/safety/preflight.sh` refuses below
  the sealed floor.

- **RF-2 (Single formula):** Every VRAM admission decision — sparse tier free floor, origin
  physical target, isolated cache worker chunk admission, and broker slice admission — must
  compute the reserve through `GpuBudgetSnapshot::required_free_bytes` /
  `safe_target_bytes` (or one wrapper that calls them). Per-path inline formulas are removed.
  *Acceptance:* a named test per surface asserts the surface refuses at the same free-bytes
  threshold as the shared helper for an identical snapshot.

- **RF-3 (Seal binds enforcement):** Manifest seal verification must fail if the sealed
  reserve fields disagree with the verifier's sealed literals, and the runtime must refuse to
  resolve a policy below those literals. The seal may no longer attest a value that no code
  path consumes, and no override may undercut it.
  *Acceptance:* a named test feeds a manifest whose reserve fields disagree with the sealed
  literals and asserts the seal rejects; a matching manifest passes; a separate named test
  asserts the resolved policy satisfies `min_floor_bytes >= verified_sealed_min_bytes` and
  `resolved.sealed_percent == verified_sealed_percent` — both sides named, so the comparison
  cannot collapse into an identity.

- **RF-4 (Raise-only lab override):** Any configuration override of the reserve floor must be
  raise-only in *effect*, not merely in comparison. An operator may make the host more
  conservative; a value below the sealed authority must be rejected at startup with a nonzero
  exit, not silently clamped upward into an unsafe default. The override raises the minimum
  term only — the sealed percentage share is always applied, so no override can lower the
  enforced floor by dropping that share.
  *Acceptance:* named tests assert a lower value fails closed with a distinct message; a
  higher value is honored; and an override between the sealed minimum and the percentage share
  leaves the enforced floor unchanged (the share wins) while being logged as subsumed, never
  as a raise.

- **RF-5 (Abuse — no silent weakening):** A caller must not be able to pass `0`, `u64::MAX`, or
  a stale budget snapshot and obtain a larger allocation. Overflow and staleness are refusals.
  *Acceptance:* named tests cover zero, saturating, and stale-snapshot refusals at the shared
  helper boundary.

## 5. Non-functional requirements (NFR)

- **NFR-1 (Host safety bound):** The enforced free floor on any adapter is at least the
  three-term maximum `max(sealed_min_bytes, floor(capacity * sealed_percent / 100),
  ceil(min(total, budget)/5)) + runtime_headroom_bytes`, where `runtime_headroom_bytes` is the
  existing 640 MiB buffer and `capacity = min(total_bytes.unwrap_or(budget_bytes), budget_bytes)`
  derived from the **same snapshot** the admission helper is given — never `total_vram_bytes`
  alone, never a second definition. The first term is the sealed
  minimum, the second is the sealed percentage share, the third is the shared helper's
  non-negotiable 20% safety floor. The bound is subtracted from both total capacity and live
  available headroom, not from capacity alone. **All three terms are required** — omitting the
  percentage-share term would understate the bound on any reseal above 20%.
- **NFR-2 (Numbers before adjectives):** The requalification campaign reports, per adapter,
  the enforced free floor in MiB, the resulting usable cache bytes, and the four canonical
  hardware categories with directions and Tier 3 origin metrics
  ([`.claude/rules/benchmarks.md`](../../../../.claude/rules/benchmarks.md)).
- **NFR-3 (Observability):** Startup logs the enforced reserve floor, its source
  (`sealed-manifest` | `lab-override-raise`), the capacity share used, and the runtime buffer.
  A monitor or status surface exposes the same value so an operator can see what is enforced
  rather than what is configured.
- **NFR-4 (No latency shim):** The change adds no runtime indirection on the allocation hot
  path beyond a function call to the existing helper (Day-0).
- **NFR-5 (Capacity is measured, not promised):** No minimum usable-cache ratio is claimed.
  If the sealed floor leaves less cache than the declared workload needs, the result is
  recorded and the number is revised by an explicit reseal with measurements — never by
  relaxing the formula.

## 6. Flows

### Happy path — startup admission

1. `ramshared-wsl2d` loads and verifies the sealed host-origin manifest
   (`main.rs` manifest gate). Reserve fields are checked against the runtime authority (RF-3).
2. The configured reserve is taken from the sealed manifest (RF-1). An env override is
   accepted only if it is strictly greater (RF-4); otherwise startup exits nonzero.
3. Each admission surface constructs its budget snapshot and calls
   `required_free_bytes` / `safe_target_bytes` (RF-2).
4. Allocation proceeds only while `available_bytes >= required_free_bytes + request`.
5. Startup logs the enforced floor and its source (NFR-3).

### Alternate — lab override raise

1. Operator sets `RAMSHARED_MIN_VRAM_FREE_MIB` above the sealed minimum.
2. Startup logs `lab-override-raise` with both values and uses the higher one.
3. Seal verification still passes: the runtime is strictly more conservative than the seal.

### Errors

| Trigger | Exit / errno | Log | State |
| --- | --- | --- | --- |
| Override below sealed authority | existing daemon failure path (`ExitCode::from(1)`) | `reserve floor override below sealed authority: <env> < <sealed>` | no allocation started |
| Both configuration names set to different values | existing daemon failure path (`ExitCode::from(1)`) | `conflicting reserve floor overrides: <primary> != <alias>` | no allocation started |
| Manifest reserve fields disagree with runtime authority | manifest gate rejects (existing sealed-policy error path) | `host and guest origin manifests disagree on sealed policy or identity` | startup aborts |
| Budget snapshot stale or inconsistent (`source` not driver-reported, `used > budget`, `budget > total`, sample older than `WDDM_BUDGET_MAX_AGE`) | allocation refused | existing worker/budget refusal path | surface stays at last safe target or goes unavailable; no allocation |
| Overflow in `request + required_free_bytes` | allocation refused (saturating/checked) | refusal at helper boundary | no state change |

## 7. Data / state model

```rust
// crates/ramshared-vram/src/lib.rs — already present, becomes the only admission formula
impl GpuBudgetSnapshot {
    pub fn required_free_bytes(
        &self,
        configured_reserve_bytes: u64,
        runtime_headroom_bytes: u64,
    ) -> u64; // max(configured, ceil(min(total, budget)/5)) + runtime_headroom

    pub fn safe_target_bytes(
        &self,
        requested_bytes: u64,
        configured_reserve_bytes: u64,
        runtime_headroom_bytes: u64,
    ) -> u64; // min(request, capacity - reserve, available - reserve - runtime_headroom)
}
```

```rust
// New single entry for the configured component. The policy owns the configured
// component only; the 640 MiB runtime headroom stays the caller's second helper
// argument (RUNTIME_FREE_BUFFER_BYTES) and is not a policy field.
pub struct ReserveFloorEnv {
    pub env_mib: Option<u64>,          // RAMSHARED_MIN_VRAM_FREE_MIB
    pub alias_mib: Option<u64>,        // MIN_VRAM_HEADROOM_MIB (cascade.conf key exported
                                       // to the environment by the cascade boot path; this
                                       // type does not parse that file)
}

pub struct ReserveFloorPolicy {
    pub min_floor_bytes: u64,  // sealed gpu_reserve_min_mib, raised by a lab override
    pub sealed_percent: u64,   // sealed gpu_reserve_percent, never overridable
}

impl ReserveFloorPolicy {
    pub fn from_manifest(gpu_reserve_min_mib: u64, gpu_reserve_percent: u64)
        -> Result<Self, ReserveFloorError>;
    /// Raise-only: raises min_floor_bytes, never lowers it, never touches sealed_percent.
    pub fn resolve_with_env(base: &Self, env: &ReserveFloorEnv)
        -> Result<Self, ReserveFloorError>;
    /// max(min_floor_bytes, floor(capacity * sealed_percent / 100)).
    pub fn configured_reserve_bytes(&self, capacity: u64) -> u64;
}
```

State: the resolved policy is computed once at startup and passed by value to each admission
surface. It is immutable for the process lifetime. There is no runtime re-read of the
environment and no dual reader.

## 8. Interfaces

- **CLI:** none added. Existing `ramshared` / `ramsharedd` flags are unchanged.
- **Environment / configuration:** `RAMSHARED_MIN_VRAM_FREE_MIB` (environment) and
  `MIN_VRAM_HEADROOM_MIB` (a live key of `/etc/ramshared/cascade.conf`) both become raise-only
  inputs to one resolver (RF-4). Semantics change from "clamp into 128–4096" to "must be
  >= sealed, else fail startup with a distinct message". **Neither name is deprecated or
  removed in this slice**; renaming or removing the cascade.conf key is a separate
  cascade-config decision.
- **uAPI / sysfs / ioctl:** none. No kernel or ABI surface.
- **Manifest:** `gpu_reserve_min_mib` / `gpu_reserve_percent` keep their sealed schema and
  values (`2048` / `20`). They gain meaning; they do not change shape.
- **Status/monitor:** the enforced floor is exposed in the existing status JSON or monitor
  output (NFR-3), labelled as VRAM reserve, never as RAM.

## 9. Dependencies and risks

**Prerequisites**
- `GpuBudgetSnapshot::required_free_bytes` / `safe_target_bytes` (exists, tested).
- Host-origin manifest seal path in `crates/ramshared-wsl2d/src/main.rs` (exists).
- Parent SPEC `wsl2-isolated-gpu-cache-worker` capacity-boundary and live-adapter
  qualification procedures (exist; this change re-runs them).

**Risks and mitigations**

| Risk | Mitigation |
| --- | --- |
| Cache becomes too small to be useful at 2048 MiB / 20% on a 6 GiB adapter | NFR-5 measures it. If useless, the number is revised by explicit reseal with the measurement attached — the formula is not relaxed. |
| A caller relied on `origin_cache`'s 2 GiB constant for an undocumented reason | Capacity-boundary requalification covers the origin physical-target path; if it regresses, the constant's purpose is recorded before any reintroduction. |
| Raise-only change breaks an operator script that sets a low value deliberately | Startup fails through the existing error path with an explicit message rather than silently changing behavior. Affected scripts and the runbook are updated in the same change. |
| Percentage term ambiguity (`ceil(capacity/5)` is 20% already, but the manifest also seals `gpu_reserve_percent`) | DT-2 closes this: the sealed percent and the helper's `div_ceil(5)` must agree or the seal rejects. |

**Numeric rollback trigger:** any of the following reverts the caller to the previous
admission constant and opens a defect — one allocation observed below the enforced floor in a
capacity-boundary run; one Windows display/foreground-GPU starvation event attributable to the
new floor; one seal verification that accepts a manifest whose reserve fields disagree with
the runtime authority; one path still computing a reserve inline.

## 10. Implementation strategy

1. Introduce `ReserveFloorPolicy` (resolve + configured_reserve_bytes) with unit tests,
   including raise-only refusal and the percent/`div_ceil(5)` agreement check.
2. Rewire `sparse_vram.rs` admission to the shared helper (removes the most permissive path).
3. Rewire `origin_cache.rs :: physical_target_bytes` to the shared helper (removes the 2 GiB
   constant).
4. Rewire `gpu_cache_worker.rs` and `gpu_budget.rs` to take the resolved policy instead of
   `reserve_floor_bytes_from_env()`.
5. Bind the manifest seal to the runtime authority (RF-3); make the env seam raise-only (RF-4).
6. Re-run capacity-boundary and live-adapter qualification; publish the measured numbers;
   close the parent SPEC checklist gap and the GAP-REGISTER entry.

Early validation: steps 1–3 are pure userspace and fully unit-testable without hardware.
Step 6 is env-bound and produces **partial** if no adapter session is available.

## 11. Documents to update

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Record the single reserve authority |
| `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/SPEC.md` validation checklist | Close the reserve-drift item once step 6 lands |
| `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/IMPL.md` | Update the reserve-floor gap note |
| `docs/specs/no-milestone/cuda-rust-native-tiering/AUDIT-2.5.md` / `IMPL.md` | Close the "floor value unreconciled" open item |
| `docs/reliability/GAP-REGISTER.md` | Record/close the reserve-floor contract gap |
| `docs/decisions/ADR-NNN-gpu-reserve-floor-authority.md` | Create — the authority decision |
| `scripts/safety/preflight.sh` | Replace the 256 MiB startup-gate default with the sealed authority; record in the operator runbook |
| `validation.md` | Append the requalification result |
| `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl` | Register the capacity-boundary campaign |

## 12. Out of scope

- Changing the sealed numeric value (`2048` MiB or `20`). A change is a separate, measured,
  resealed decision.
- Any kernel, LKM, HMM, DRM, CXL, uAPI, or Windows driver change.
- Swap-format, origin-format, pagefile, or block-ABI changes.
- Cache compression (`cuda-rust-native-tiering`), cascade tiering policy, and reclaim policy.
- Removing the 640 MiB runtime buffer or redefining `RUNTIME_FREE_BUFFER_BYTES`.
- GPU capacity autotuning or workload-aware reserve sizing.

## 13. Acceptance criteria

1. No production VRAM admission path computes a reserve inline; all four surfaces call
   `required_free_bytes` / `safe_target_bytes` (or one wrapper). No startup gate — including
   `scripts/safety/preflight.sh` — decides go/no-go on a constant of its own.
2. The configured reserve component derives from the sealed manifest fields; the seal rejects
   a manifest whose reserve fields disagree with the verifier's sealed literals; the resolved
   policy is never below those literals.
3. A configuration override below the sealed authority fails startup with a distinct message
   and a nonzero exit through the existing error path; a higher override is honored and logged
   as `lab-override-raise`. Setting both documented names to different values also fails.
4. The competing constants (512 MiB env default as a production floor, 2 GiB in
   `origin_cache.rs`, 1536 MiB as an unbound default, and the 256 MiB preflight startup gate)
   are gone from admission paths and from operator go/no-go surfaces.
5. Capacity-boundary and live-adapter requalification produce numbers for the enforced floor,
   usable cache bytes, and the four hardware categories, with Tier 3 origin metrics and
   `PASS_ZERO_PANIC`.
6. The origin path refuses stale, untrusted, and inconsistent samples and never invents
   provenance: no `Instant::now()` and no `GpuBudgetSource::DriverReported` is written inside
   the path, and a sample that cannot prove freshness yields a zero target.
7. The parent SPEC checklist gap and the GAP-REGISTER entry are closed or explicitly carried
   with a reason.

## 14. Validation plan

- **Unit:** `ReserveFloorPolicy` resolution (raise-only, percent agreement, overflow, zero);
  each rewired surface refuses at the shared threshold for an identical snapshot; the seal
  rejects a disagreeing manifest; stale/oversized budget snapshots refuse.
- **Integration:** startup with a low override fails with a nonzero exit and the distinct
  message; startup with a high override logs `lab-override-raise`; conflicting values for the
  two documented names fail; status output reports the enforced floor and source.
- **Live path (userspace WSL2 surface):** capacity-boundary run on an isolated canary origin
  with a real adapter — before (forced allocation down to the old floor) → action (enforce the
  sealed floor) → after (allocation refused above the floor, host display and a predeclared
  foreground GPU workload unaffected). `BINARY_MATCH` when `ramsharedd` is exercised.
- **Host safety:** no unsupervised swap/ublk or GPU pressure on the live WSL2 host. Any
  pressure component runs only through the approved watchdog harness or an isolated lab VM.
- **Env-bound gaps:** if no adapter session is available, record **partial**, never DONE.
