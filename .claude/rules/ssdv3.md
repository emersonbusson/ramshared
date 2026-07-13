---
name: ssdv3
description: When SSDV3 (Spec-Driven Development) is mandatory vs optional.
paths:
  - docs/**
---

# SSDV3 rules

Pipeline: **PRD → SPEC → IMPL** (optional Step 2.5 audit). Full prompts/templates: [`docs/SSDV3-PROMPTS.md`](../../docs/SSDV3-PROMPTS.md). Do not restate that file here.

## Mandatory

1. Locks / concurrency (order, spinlock↔mutex, RCU, barriers in hot path / IRQ)
2. DMA / IOMMU / MMIO
3. Memory (mm): NUMA, HMM/`DEVICE_PRIVATE`, hotplug, hot-path allocation
4. uAPI / ABI (ioctl/sysfs/debugfs or exported struct layout)
5. New hardware / subsystem (device, DRM/TTM, CXL)
6. Structural MMU / DRM changes

## Optional / out of scope

Optional: pure refactors without contract change, localized bugfixes, doc-only, dependency bumps (log in `docs/LIBRARIES.md`).

**Not SSDV3:** CI, host-safety scripts (`scripts/safety/*`, `scripts/p0/*`), qemu/lab harness, disk reclaim. Use Kahneman #15/#16/#18 + [`.claude/rules/benchmarks.md`](benchmarks.md) + append [`validation.md`](../../validation.md). If a script also invents a kernel/uAPI contract, that product side still needs SSDV3.

## Artifacts

| Artifact | Path |
| --- | --- |
| Prompts | `docs/SSDV3-PROMPTS.md` |
| Specs | `docs/specs/no-milestone/{slug}/{PRD,SPEC,IMPL}.md` (+ optional `AUDIT-2.5.md`) |
| Kahneman | `docs/methodology/kahneman-disciplines.md` |

- One `SPEC.md` per feature; revise in-place (no `SPECvN.md`).
- After add/remove under `docs/specs/`, run `node tools/generate-docs-index.mjs`.

## Step 3 hard gates (summary)

1. Cover ≥80% on slice business-logic crates/files (`cargo llvm-cov -p …`); workspace average does not count.
2. Live E2E + evidence before closing `validation.md`: deployed binary (`BINARY_MATCH=OK`), `ramshared status` / `cascade-health`, legitimate path + SPEC refusals.
3. Kahneman test types #9/#13/#15/#16/#17 — table in `docs/SSDV3-PROMPTS.md`.
4. Hang-class audit posture: root `superprompt.md`.

Detail, PRD/SPEC templates, and IMPL shape: **only** `docs/SSDV3-PROMPTS.md`.

## Don't

- Skip PRD/SPEC for mandatory-scope changes
- IMPL without SPEC (or without 2.5 `go` when risk-gated)
- Inference on >30% of PRD items
- New helper without checking kernel/`lib/`/workspace crates
- Web/SaaS-shaped validation for LKM work (HTTP-only, JSON REST as primary proof)
- Thrash swap/ublk on the live WSL2 host
