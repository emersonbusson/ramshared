# IMPL — gpu-reserve-floor-authority

## Tracking Record

- **SPEC**: [SPEC.md](SPEC.md) — one configured-reserve authority for the GPU free floor.
- **PRD**: [PRD.md](PRD.md)
- **AUDIT-2.5**: [AUDIT-2.5.md](AUDIT-2.5.md) (2026-09-30, step 2.5 pass 3: go with the
  DT-6 one-ownership-chain fix, `GpuSample` without `Copy`, and `preflight.sh` under the
  authority).
- **Current state**: ITEM-1..ITEM-5 implemented and live. ITEM-6 is **env-bound**
  (capacity-boundary and live-adapter hardware runs) and remains **partial**, verdict 🟡.
- **Verdict**: 🟡 **partial** — code slice complete and proven by named tests; hardware
  requalification not executed on this host.

## Problem

Four surfaces enforced four different GPU free floors:

| Surface | Old default / formula | Silent consequence |
| --- | --- | --- |
| `crates/ramshared-block/src/sparse_vram.rs` | `reserve_floor_bytes_from_env()` = `512 MiB` clamped to `[128, 4096]` | An operator could silently get a **128 MiB** floor on a shared host. |
| `crates/ramshared-block/src/origin_cache.rs` | `physical_target_bytes` added a hardcoded `.max(2 * GIB)` | A 2 GiB reserve survived every policy decision and was invisible to the seal. |
| worker / broker | `640 MiB` + `20%` | Different threshold from the sparse tier on the same adapter. |
| `scripts/safety/preflight.sh` | `${RAMSHARED_MIN_VRAM_FREE_MIB:-256}` | Gated daemon startup at a **fifth** value. |

The host-origin manifest **attested** `gpu_reserve_min_mib = 2048` and `gpu_reserve_percent = 20`
and **enforced none of them**.

## What was done

### ITEM-1 — `ReserveFloorPolicy` (DT-1, DT-2, DT-3)

`crates/ramshared-vram/src/reserve_policy.rs` is the single configured-reserve authority.
Resolved once at startup, passed by value to every admission surface. Immutable after
resolution. Carries **no** runtime headroom.

```rust
configured_reserve_bytes(capacity) =
    max(min_floor_bytes, floor(capacity * sealed_percent / 100))
```

The three-term enforced free floor over **one** capacity
(`capacity = total_bytes.unwrap_or(budget_bytes).min(budget_bytes)`):

```text
max(min_floor_bytes, floor(capacity * sealed_percent / 100), ceil(capacity/5))
  + runtime_headroom_bytes
```

The third term is the helper's existing non-negotiable 20% safety floor (DT-2); seal
verification rejects `gpu_reserve_percent < 20`, so a reseal can never lower the reserve.

`enforced_free_floor_from_configured(configured, capacity, runtime)` is a free function so a
caller with no `GpuBudgetSnapshot` (the sparse tier) gets the same threshold without
inventing budget provenance. A named test asserts it is arithmetically equal to
`GpuBudgetSnapshot::required_free_bytes`.

**Why:** one resolution point means one place to test, audit, and log. Passing by value makes
the policy immutable and removes any dual reader.

### ITEM-2 — sparse admission (DT-5, DT-11)

`SparseVramConfig.reserve_floor_bytes: u64` → `reserve_policy: ReserveFloorPolicy`.
Admission and the demotion-probe floor both use the shared formula. The old
`unwrap_or(512).clamp(128, 4096)` reader is **deleted** — `reserve_floor_bytes_from_env` no
longer exists as a second surface.

### ITEM-3 — `GpuSample` provenance (DT-6, DT-12)

One ownership chain:

| Step | Owner | Does **not** do |
| --- | --- | --- |
| produce | sampler | validate |
| `observe_gpu` | `WriteThroughCacheBackend` | validate |
| `physical_target_bytes` | `origin_cache` | stamp provenance |

`GpuSample` drops `Copy` and carries `sampled_at: Instant` (one clock), `source: GpuBudgetSource`,
and `adapter: Option<GpuAdapterIdentity>`. `physical_target_bytes` validates and returns **0**
on: stale sample (`> SAMPLE_MAX_AGE`), non-`DriverReported` source, missing adapter,
`external_usage_bytes > budget_bytes`, `budget_bytes > total_vram_bytes`, or a future
`sampled_at`. Only then does it return `safe_target_bytes(...)`.

**The hardcoded `.max(2 * GIB)` is deleted.**

### ITEM-4 — worker and broker admission (DT-5)

Both surfaces call `required_free_bytes(configured, RUNTIME_FREE_BUFFER_BYTES)` with the one
named constant `RUNTIME_FREE_BUFFER_BYTES = 640 MiB` in `gpu_cache_worker.rs`. A stale budget
fails `can_admit` and yields target 0.

### ITEM-5 — seal verification, raise-only override, observability (DT-7, DT-8, DT-10)

- **DT-7(a) seal integrity:** manifest verification rejects reserve fields that disagree with
  the sealed literals.
- **DT-7(b) enforcement binding** is live in **production** startup, not test-only:
  `resolved.min_floor_bytes >= verified_sealed_min` AND `resolved.sealed_percent == verified_percent`.
- **DT-8 raise-only:** `RAMSHARED_MIN_VRAM_FREE_MIB` / `MIN_VRAM_HEADROOM_MIB` may only **raise**
  the floor. The clamp-to-128 is gone in Rust and in preflight. A below-seal override is a
  startup failure on the existing error path, not a new exit-code class. Non-numeric values are
  refused, never coerced to zero.
- **DT-10 startup log:** one line with enforced floor, source, sealed minimum, override,
  percentage, and runtime buffer. Source label is honest: `lab-override-raise` **only** when the
  override actually raises the enforced result at the **measured** adapter capacity; a subsumed
  override logs `sealed-manifest` with `(override subsumed)`.
- **DT-10 monitor/status:** `GpuObservation` gained five `vram_reserve_*` fields with an explicit
  VRAM-reserve label so the GPU free floor can never be read as a system-RAM threshold.

### Pre-existing defect found and fixed during this slice

`refuse_half_cascade` in `crates/ramshared-cli/src/cascade/mod.rs` read the live
`/run/ramshared` record paths with no test seam. On a host whose daemon is running, the unit
test `up_with_config_refuses_missing_safety_net_before_runtime_setup` hit the operator's real
half-state and could not reach the DEMOTE-safety-net ordering it is meant to prove.

**Classification: reproduced defect in test isolation** (not a product bug — the product
correctly refuses on half-open state). The existing seam at `has_live_records` ("injected
`/proc/swaps` must not couple to live `/run` records") was extended to `refuse_half_cascade`,
clearing only the hardcoded host paths; the injected `entries` still decide `has_vram`. The
narrowing matters: skipping the whole function would have masked
`refuse_half_cascade_when_vram_live_without_health`.

## Validation — real data

Host: `Linux 6.18.40.1-microsoft-standard-WSL2+`, WSL2 GPU-PV, RTX 2060 6 GiB.
Branch `feat/ramshared-v0.15.0-readiness`, 2026-09-30. `CARGO_BUILD_JOBS=1`.

### Static checks

| Check | Result |
| --- | --- |
| `cargo fmt` (`ramshared-vram`, `ramshared-block`, `ramshared-wsl2d`, `ramshared-cli`) | clean |
| `cargo clippy --all-targets` (same four crates) | clean — 0 errors, 0 warnings |

### Unit and integration tests

| Package | Passed | Failed |
| --- | ---: | ---: |
| `ramshared-vram` | 26 | 0 |
| `ramshared-block` (lib) | 141 | 0 |
| `ramshared-block` (tests) | 6 | 0 |
| `ramshared-wsl2d` (lib) | 172 | 0 |
| `ramshared-wsl2d` (bin `ramsharedd`) | 119 | 0 |
| `ramshared-cli` (bin `ramshared`) | 436 | 0 |
| other workspace targets (tiers, integration) | 59 | 0 |
| **Total** | **959** | **0** |

**Post-closure re-run, 2026-09-30** (after the contract-closure tests landed):
`cargo test --workspace --exclude ramshared-winsvc` → **1390 passed / 0 failed / 25 ignored**
across 53 suites. `cargo test -p ramshared-vram --lib` → 40 passed / 0 failed.

### SPEC-named tests

| Named test | Surface | Result |
| --- | --- | --- |
| `enforced_free_floor_bytes_matches_required_free_bytes` | `reserve_policy.rs` | PASS |
| `sparse_admission_uses_shared_reserve_floor` | `sparse_vram.rs` | PASS |
| `sparse_probe_floor_matches_shared_reserve` | `sparse_vram.rs` | PASS |
| `sealed_reserve_policy_rejects_override_below_sealed` | `sparse_vram.rs` | PASS |
| `origin_physical_target_uses_shared_reserve_floor` | `origin_cache.rs` | PASS |
| `origin_physical_target_has_no_hardcoded_two_gib_term` | `origin_cache.rs` | PASS |
| `origin_sample_maps_external_usage_to_used_bytes` | `origin_cache.rs` | PASS |
| `origin_unnormalizable_sample_returns_zero` | `origin_cache.rs` | PASS |
| `origin_stale_sample_refuses` | `origin_cache.rs` | PASS |
| `origin_untrusted_sample_refuses` | `origin_cache.rs` | PASS |
| `origin_path_never_stamps_provenance` | `origin_cache.rs` | PASS |
| `physical_target_bytes_refuses_inconsistent_sample` | `origin_cache.rs` | PASS |
| `worker_admission_uses_resolved_policy` | `gpu_cache_worker.rs` | PASS |
| `worker_admission_refuses_on_stale_budget` | `gpu_cache_worker.rs` | PASS |
| `broker_admission_uses_resolved_policy` | `gpu_budget.rs` | PASS |
| `candidate_target_applies_reserve_freshness_and_request_cap` | `gpu_budget.rs` | PASS |
| `manifest_seal_rejects_reserve_mismatch` | `main.rs` | PASS |
| `enforcement_binding_matches_verified_seal` | `main.rs` | PASS |
| `low_reserve_override_fails_startup` | `main.rs` | PASS |
| `high_reserve_override_logs_raise_only` | `main.rs` | PASS |
| `monitor_labels_enforced_reserve_floor_as_vram` | `monitor.rs` | PASS |
| `preflight_refuses_below_sealed_floor` | `preflight.sh` | PASS (shell) |
| `preflight_honors_raise_only_override` | `preflight.sh` | PASS (shell) |
| `safe_target_keeps_the_floor_for_trusted_snapshots` | `lib.rs` | PASS |
| `safe_target_can_violate_the_floor_without_budget_consistency` | `lib.rs` | PASS |

### Contract closure — the two helpers agree on the reachable domain

AUDIT-2.5 pass 2 left a load-bearing open question: `safe_target_bytes` subtracts
`runtime_headroom_bytes` from its live-headroom term but not from `within_capacity`, while
`required_free_bytes` adds it to the reserve, so the two do not bound the same expression on
paper. **Closed 2026-09-30.** Both production callers refuse `budget_bytes > total_bytes`
before the arithmetic (`physical_target_bytes` per DT-6, `safe_cache_target_with_runtime`), so
`capacity == budget_bytes` on every reachable call and `within_live_headroom <= within_capacity`
always. The live-headroom term binds; the omitted capacity-side headroom is never the limiting
term.

- `safe_target_keeps_the_floor_for_trusted_snapshots` asserts
  `capacity - used - target >= required_free_bytes` for six trusted snapshots, including the
  live host figure `total 6144 MiB / budget 4270 MiB / used 1549 MiB` and a 48 GiB adapter.
- `safe_target_can_violate_the_floor_without_budget_consistency` asserts the counter-case with
  `budget 8000 > total 3000`: the capacity term binds and capacity-relative free space falls
  below the floor. This is the Kahneman #16 evidence that the caller-side consistency check is
  load-bearing, not decorative.

**ITEM-6 measures `required_free_bytes`** (the floor). The target-side quantity is
`safe_target_bytes`; the two are equal in bound over the trusted domain.

### Slice coverage gate

`tools/ci/check-rust-slice-coverage.mjs`, metric `lines`, min 80% on business-logic files
(excluding `main.rs`, whose cover target is "extracted business logic only" per SPEC).

Command (2026-09-30 re-measure, `CARGO_BUILD_JOBS=1`):

```text
node tools/ci/check-rust-slice-coverage.mjs \
  -p ramshared-vram,ramshared-block,ramshared-wsl2d \
  --files crates/ramshared-vram/src/reserve_policy.rs,crates/ramshared-block/src/sparse_vram.rs,crates/ramshared-block/src/origin_cache.rs,crates/ramshared-block/src/gpu_cache_worker.rs,crates/ramshared-wsl2d/src/gpu_budget.rs \
  --min 80
```

| File | Lines covered | % |
| --- | ---: | ---: |
| `crates/ramshared-vram/src/reserve_policy.rs` | 118/126 | **93.7%** |
| `crates/ramshared-block/src/sparse_vram.rs` | 365/398 | **91.7%** |
| `crates/ramshared-block/src/origin_cache.rs` | 529/577 | **91.7%** |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | 680/839 | **81.0%** |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | 217/258 | **84.1%** |

All five **PASSED**. `gpu_cache_worker.rs`'s production-line denominator grew from 391 to 839
after the worker surface expanded; 81.0% is the honest current figure, above the 80% floor.

### Hardcoded-reserve verification

```text
grep -rn "max(2 \* GIB)\|max(2\*GIB)\|div_ceil(5)).max" crates/   → empty
grep -rn "reserve_floor_bytes_from_env" --include="*.rs" crates/   → empty
grep -rn ":-256" scripts/safety/preflight.sh                       → empty (comment only)
```

`div_ceil(5)` now lives only in the two shared authorities
(`GpuBudgetSnapshot` in `ramshared-vram/src/lib.rs`, `reserve_policy.rs`) plus one test that
asserts against it.

## Kahneman critical rows

| Discipline | Evidence |
| --- | --- |
| #16 from exhaustion | `override_below_sealed_is_refused`, `zero_sealed_field_is_refused`, `zero_configured_never_raises_allocation`, `overflow_is_refused`, `percent_below_twenty_is_rejected`, `override_writes_min_floor_only` |
| #13 not happy-path | every origin refusal test is a *refusal* test; preflight exercises refuse **and** accept in both directions (128 / 2048 / 3072 / 4096 / `abc` / alias 512 / alias 4096) |
| #3 number before adjective | coverage and test counts above carry `n`, unit and file |
| #15 retry only transient | no retry path added; DT-8 failures are deterministic and fail-fast |
| #17 idempotent | `resolve_with_env` is pure; `sealed_reserve_policy_from_env` is re-runnable |

## ITEM-6 — env-bound, not closed

**Status: partial, verdict 🟡.**

ITEM-6 requires a capacity-boundary campaign and a live-adapter before→action→after session with
a predeclared foreground GPU workload, producing the four-category hardware table with Tier 3
origin metrics and a `PASS_ZERO_PANIC` verdict. That is a **hardware run**, not an in-session
code fix.

| Gate | State |
| --- | --- |
| ITEM-1..ITEM-5 code + named tests | ✅ done, 0 failures |
| Cover gate ≥80% on business-logic files | ✅ 5/5 passed |
| ITEM-6 capacity-boundary campaign (`scripts/p0/measure-gpu-reserve-floor.sh --drill measure_gpu_reserve_floor::capacity_boundary_campaign`) | ⏳ **not run** |
| ITEM-6 live-adapter before→action→after (`scripts/p0/measure-gpu-reserve-floor.sh --drill measure_gpu_reserve_floor::live_adapter_before_action_after`) | ⏳ **not run** |
| Four-category table + Tier 3 metrics + `PASS_ZERO_PANIC` | ⏳ **pending** |

Both ITEM-6 rows are drills owned by `scripts/p0/measure-gpu-reserve-floor.sh`. They allocate,
so each needs `--allocating`, `RAMSHARED_ALLOW_PRESSURE=1`, and a predeclared `--workload-cmd`;
the harness exits 77 and refuses otherwise. The read-only `--drill snapshot` mode is not either
campaign and cannot close these rows.

Per SSDV3 step 3, an env-bound gap yields **partial**, never a false DONE. ITEM-6 is not
claimed as closed.

## Residual gaps (honestly unresolved)

| Gap | Why it stays open |
| --- | --- |
| ITEM-6 hardware requalification | Needs a supervised GPU capacity campaign; not in-session-fixable. |
| Cache re-growth after pressure release | Unproven since EVD-0122. |
| Idle `b_avail` ceiling 2721 MiB | The WDDM per-process term binds the `min`; explained, not removed. |
| `SoleAdapter` single-adapter assumption | Multi-adapter and multi-vendor (AMD/Intel) unproven. |
| CoCo (SEV-SNP / TDX / Arm CCA) | No lab. Kept honestly unresolved. |
| `cuda-rust-native-tiering` ITEM-3 | No nvCOMP / `libnvcomp` on this host, no `lz4` crate. |

## Files touched

| File | Change |
| --- | --- |
| `crates/ramshared-vram/src/reserve_policy.rs` | **new** — `ReserveFloorPolicy`, `ReserveFloorEnv`, `ReserveFloorError`, `ReserveFloorSource`, `enforced_free_floor_from_configured`, `SEALED_PERCENT_SAFETY_FLOOR` |
| `crates/ramshared-vram/src/lib.rs` | re-export the policy surface |
| `crates/ramshared-block/src/sparse_vram.rs` | policy-by-value admission; sealed constants; delete the clamped env reader |
| `crates/ramshared-block/src/origin_cache.rs` | `GpuSample` provenance (DT-12); `physical_target_bytes` validation (DT-6); delete `.max(2 * GIB)` |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | `RUNTIME_FREE_BUFFER_BYTES`; admission uses the shared formula |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | broker admission uses the shared formula |
| `crates/ramshared-wsl2d/src/main.rs` | DT-7(b) enforcement binding; DT-10 startup log; `source_label` at measured capacity |
| `crates/ramshared-cli/src/monitor.rs` | DT-10 `vram_reserve_*` fields |
| `crates/ramshared-cli/src/cascade/mod.rs` | extend the injected-swap seam to `refuse_half_cascade` |
| `scripts/safety/preflight.sh` | sealed raise-only reserve gate (DT-8) |
| `scripts/safety/test-preflight-reserve-floor.sh` | **new** — two named preflight cases |

## Rollback

Every change is confined to the reserve-floor slice and its tests. Reverting the
`gpu-reserve-floor-authority` commits restores the previous per-surface defaults; no schema,
on-disk format, or protocol frame changed. **Rollback trigger:** any admission refusal that
fires on a healthy adapter above the sealed floor, or any startup that fails the DT-7
enforcement binding with a valid manifest.
