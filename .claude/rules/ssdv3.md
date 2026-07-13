---
name: ssdv3
description: When SSDV3 (Spec-Driven Development) is mandatory vs optional.
paths:
  - docs/**
---

# SSDV3 rules

SSDV3 (Spec-Driven Development V3) is the spec methodology of `ramshared` (cf. `docs/`). Pipeline: **PRD → SPEC → IMPL** (with optional Step 2.5 audit).

Copy-paste prompts and full templates: [`docs/SSDV3-PROMPTS.md`](../../docs/SSDV3-PROMPTS.md).

## Mandatory

SSDV3 is **mandatory** for changes in:

1. **Locks / concurrency** — new lock order, spinlock↔mutex exchange, RCU, or memory barriers in hot path / IRQ context.
2. **DMA / IOMMU / MMIO** — new DMA mapping, `ioremap`, ReBAR, or PCIe cache coherence.
3. **Memory (mm)** — NUMA node semantics, HMM/`DEVICE_PRIVATE`, memory hotplug, or chronic allocation in hot path.
4. **uAPI / ABI** — new ioctl/sysfs/debugfs or layout change of struct exposed to user-space (irreversible after release).
5. **New hardware / subsystem** — support for new device, DRM/TTM integration, CXL.
6. **MMU / DRM** — any structural change touching the MMU or the DRM driver.

## Optional

Everything else is optional. Examples where SSDV3 is **overhead**:

- UI tweaks (spacing adjustment, color changes, component refactoring without changing contract).
- Internal refactors without public contract changes.
- Localized bugfixes (regression test + fix).
- Doc updates.
- Dependency updates (with entry in `docs/LIBRARIES.md`).

## Out of scope (not SSDV3)

Changes that are **infra / CI / host-safety / lab harness** — GitHub Actions, `scripts/p0/*`, `scripts/safety/*`, qemu drill wiring, disk reclaim, runner cache — do **not** require PRD→SPEC→IMPL. They have no product uAPI and no LKM contract.

Use instead:

- Kahneman **#15** (calibrated retry), **#16** (fail-safe + independent curator), **#18** (right-layer root cause + proven sunset)
- [`.claude/rules/benchmarks.md`](benchmarks.md) for measurement integrity and host safety (no thrash on live WSL2)
- Append empirical results to [`validation.md`](../../validation.md)

If a script change **also** invents a new kernel contract or lock/DMA path, that product side still needs SSDV3.

## Pipeline

### STEP 1 — PRD

Copy the prompt from `docs/SSDV3-PROMPTS.md` (Step 1), replace placeholders, paste into chat.

Output: `docs/specs/no-milestone/{slug}/PRD.md` (or under `docs/specs/M{NN}-…/{slug}/` if a formal milestone exists).

**14 fixed sections** (see prompts for full template):

1. Summary
2. Technical context
3. Recommended option
4. Functional requirements (`RF-N`)
5. Non-functional requirements (`NFR-N`)
6. Flows
7. Data model
8. API / Interfaces
9. Dependencies and risks
10. Implementation strategy
11. Documents to update
12. Out of scope
13. Acceptance criteria
14. Validation

Each item marked as:

- **Confirmed in codebase** — existing code was read
- **Confirmed in docs** — ADR/runbook read
- **Inference** — proposal without direct confirmation (should be scarce)

Discovery must include **abuse cases** (ioctl size/TOCTOU, race open/close×DMA, GFP in IRQ, capability bypass, info-leak, UAF on exit).

### STEP 2 — SPEC

Copy the Step 2 prompt; generate `SPEC.md` in the **same** folder as the PRD.

SPEC translates PRD into:

- Files to create/modify (paths from repo root)
- Handler validations (`copy_{from,to}_user`, bounds, alignment, `capable`)
- **Lock order table** and **context matrix** (process/softirq/hardirq × may-sleep × GFP) when applicable
- Kahneman blocks on critical steps (question + minimum evidence + abort trigger)
- Numerical rollback trigger for non-trivial mm/DMA/lock work
- Traceability: `RF-N` / `NFR-N` → `ITEM-N` / `DT-N`

**One `SPEC.md` only.** Step 2.5 no-go → revise `SPEC.md` **in-place**; git is history. Do **not** create `SPECv2.md` / `SPECvN.md` for new work.

**Unique SPEC (Advoq model):** only `SPEC.md` per feature. Step 2.5 revises it in-place; history is `git log`. **Never** create `SPECv2.md`/`SPECvN.md`. Flat `docs/{feature}/` trees are README stubs only.

### STEP 2.5 — Audit (risk-gated)

Use when locks, DMA, uAPI, privilege, Ring 0/3 isolation, or high oops/deadlock risk apply. Output `go` / `no-go`. On `no-go`, fix `SPEC.md` in the same turn.

**Persist the audit** (so go/no-go is not chat-only):

- Preferred: `docs/specs/…/{slug}/AUDIT-2.5.md` (same folder as SPEC)
- Or: `docs/reviews/YYYY-MM-DD-{slug}.md`

Minimum content: findings by severity, open questions, `go`/`no-go`, blockers fixed in SPEC if no-go.

### STEP 3 — IMPL

Implement strictly according to SPEC. **Zero creativity out of scope.** New decision → update SPEC first, then implement.

**Hard gates before close (aligned with Step 3 / Kahneman #13):**

1. **Cover ≥80%** on business-logic files/crates of the slice (`cargo llvm-cov -p …`). Workspace average does **not** count. Boilerplate wiring may be `N/A — boilerplate` in IMPL.
2. **E2E live + evidence** before `validation.md` close: deployed binary (`BINARY_MATCH=OK`), `ramshared status` / `cascade-health`, at least one legitimate path + SPEC refusals (ghost / used_kb / preflight). Order: unit/cover → E2E → validation/IMPL → commit.
3. Kahneman test types: #13 effect + paired refusal, #15 retry only transient, #16 exhaustion, #17 replay 2×=1× — see table in `docs/SSDV3-PROMPTS.md`.
4. Adversarial hang audit posture: root `superprompt.md`.

Output: `docs/specs/no-milestone/{slug}/IMPL.md` (same folder) documenting:

- status and green/red gates (include cover % + E2E commands)
- RF/ITEM → files table
- small decisions (no new ADR)
- validation with **numbers**
- env-bound gaps
- rollback trigger
- commit traceability

Template: `docs/SSDV3-PROMPTS.md` Step 3. All SSDV3 prose is **English**.

> **Benchmarks supporting a P0 numerical gate** follow [`.claude/rules/benchmarks.md`](benchmarks.md): automatic context, ≥3 runs (median + p99 + deviation), load tag (`idle`/`loaded`), record in `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl`. Never thrash swap/ublk on the live WSL2 host.

## Hard Rules

1. **Reuse before creation.** Before proposing new code, prove that an existing kernel API or helper does not suffice. Reference: subsystems (`mm/`, `drm/`, `lib/`), module helpers, workspace crates.
2. **Fact vs proposal separation.** Each PRD item is "Confirmed in codebase / Confirmed in docs / Inference". Inferences need validation in SPEC.
3. **Zero creativity in IMPL.** Code follows SPEC. New decision → SPEC update → re-approval.
4. **Traceability by requirement ID.** Each non-trivial commit references `RF-N`, `NFR-N`, etc. IMPL and PRs link covered IDs. (Legacy docs may say `FR-N` — treat as `RF-N`.)
5. **Kahneman discipline link.** Critical SPEC steps reference [`docs/methodology/kahneman-disciplines.md`](../../docs/methodology/kahneman-disciplines.md) (e.g. #2 counterfactual for lock/DMA; #15–#17 for retry/reconnect/command replay; #16 for demote/reclaim; #18 for shim sunset).
6. **Day-0.** No shims/dual-path/compat without documented exception.
7. **Host safety.** Real memory pressure only in isolated VM/qemu/civm.
8. **Index hygiene.** After creating/removing a feature folder under `docs/specs/` (or legacy flat), run `node tools/generate-docs-index.mjs` so `docs/INDEX.md` stays in sync.

## Location

| Artifact | Path |
| --- | --- |
| Copiable prompts | `docs/SSDV3-PROMPTS.md` |
| Specs (canonical) | `docs/specs/no-milestone/{slug}/{PRD,SPEC,IMPL}.md` |
| Kahneman Disciplines | `docs/methodology/kahneman-disciplines.md` |
| Benchmark rules | `.claude/rules/benchmarks.md` |

Slug: English kebab-case.

## How to link SPEC to code

C/Rust comments when the code implements a specific requirement:

```c
/* SPEC: docs/specs/no-milestone/vram-numa-node/SPEC.md ITEM-3 — RF-2 */
```

```rust
// SPEC: docs/specs/no-milestone/memory-broker/SPEC.md ITEM-4 — RF-B1
```

PR description cites SPEC + covered requirement IDs.

## Don't

- ❌ Skip PRD/SPEC for structural changes in locks/DMA/uAPI/mm.
- ❌ "I'll just do a small SPEC" — if the change is mandatory-scope, PRD is step 1.
- ❌ IMPL without approved SPEC (or without 2.5 `go` when risk-gated).
- ❌ "Inference" on >30% of PRD items — shallow investigation.
- ❌ Create a new utility without checking kernel APIs (`lib/`, subsystem helpers) or workspace crates.
- ❌ Commit in IMPL that does not trace to a requirement ID when non-trivial.
- ❌ Create `SPECv2.md` for new features; use in-place `SPEC.md`.
- ❌ Create new flat `docs/{feature-slug}/` trees; use `docs/specs/…`.
- ❌ SaaS-shaped PRD/SPEC (HTTP status, JSON REST, Prometheus-as-primary, curl-only validation) for LKM work.
