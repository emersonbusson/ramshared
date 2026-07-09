---
name: ssdv3
description: When SSDV3 (Spec-Driven Development) is mandatory vs optional.
paths:
  - docs/**
---

# SSDV3 rules

SSDV3 (Spec-Driven Development V3) is the spec methodology of `ramshared` (cf. `docs/`). Pipeline in 3 steps: **PRD → SPEC → IMPL**.

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

## Pipeline

### STEP 1 — PRD

Copy the prompt from `docs/SSDV3-PROMPTS.md` (STEP 1 PRD), replace placeholders, paste into chat.

Output: `docs/<feature-slug>/PRD.md`. 14 fixed sections:

1. Summary
2. Technical context
3. Recommended option
4. Functional requirements (FR)
5. Non-functional requirements (NFR)
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

### STEP 2 — SPEC

Copy the STEP 2 SPEC prompt, paste the approved PRD, generate `docs/<feature-slug>/SPEC.md`.

SPEC translates PRD into:

- Files to create/modify (absolute paths)
- Files/patches with absolute paths
- Validations in handlers (ioctl/sysfs: `copy_{from,to}_user`, bounds, alignment)
- Lock order and critical sections
- Links to `docs/methodology/KAHNEMAN-DISCIPLINES.md` for critical steps

### STEP 3 — IMPL

Implement strictly according to SPEC. **Zero creativity out of scope.** If a new decision arises → go back to SPEC, update, then implement.

Output: `docs/<feature-slug>/IMPL.md` documenting what was done (commits, files, small decisions that did not require a new ADR, validation metrics).

> **Benchmarks supporting the P0 numerical gate and validation metrics** follow [`.claude/rules/benchmarks.md`](benchmarks.md): context captured automatically, ≥3 runs (median + p99 + deviation), load tag (`idle`/`loaded`), and complete record in `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl`.

## Hard Rules

1. **Reuse before creation.** Before proposing new code, prove that an existing kernel API or helper does not suffice. Reference: subsystems (`mm/`, `drm/`, `lib/`), module helpers, workspace crates.
2. **Fact vs proposal separation.** Each item of the PRD is "Confirmed in codebase / Confirmed in docs / Inference". Inferences need validation in SPEC.
3. **Zero creativity in IMPL.** Code follows SPEC. New decision → SPEC update → re-approval.
4. **Traceability by requirement ID.** Each commit references `FR-3`, `NFR-2`, etc. IMPL PR links to covered IDs.
5. **Kahneman discipline link.** Critical steps in SPEC reference `docs/methodology/KAHNEMAN-DISCIPLINES.md` (e.g., for lock/DMA changes, link discipline #2 counterfactual).

## Location

- Copiable prompts: `docs/SSDV3-PROMPTS.md`.
- Artifacts: `docs/<feature-slug>/{PRD,SPEC,IMPL}.md`. Slug in English kebab-case.
- Kahneman Disciplines: `docs/methodology/KAHNEMAN-DISCIPLINES.md`.

## How to link SPEC to code

C/Rust comments when the code implements a specific requirement:

```rust
// SPEC: docs/vram-as-ram/SPECv3-WSL2.md §9 — DEMOTE of VRAM due to latency
fn demote(&mut self) -> Result<(), Error> {
    // ...
}
```

PR description cites SPEC + covered requirement IDs.

## Don't

- ❌ Skip PRD/SPEC for structural changes in locks/DMA/uAPI/mm.
- ❌ "I'll just do a small SPEC" — if the change fits in a SPEC without a PRD, it is likely optional, not mandatory; and if it is mandatory, PRD is step 1.
- ❌ IMPL without approved SPEC.
- ❌ "Inference" in the PRD on >30% of items — a sign that the investigation was shallow.
- ❌ Create a new utility without checking kernel APIs (`lib/`, subsystem helpers) or workspace crates.
- ❌ Commit in IMPL that does not trace to a requirement ID.
