---
name: ssdv3
description: When SSDV3 (Spec-Driven Development) is mandatory vs optional.
paths:
  - docs/**
---

# SSDV3 rules

**RamShared only.** Pipeline: **PRD → SPEC → (2.5 when risk) → IMPL**. Canonical prompts, fixed section skeletons, test matrix, E2E evidence protocol: [`docs/SSDV3-PROMPTS.md`](../../docs/SSDV3-PROMPTS.md). Do **not** restate those sections here. Do **not** import foreign-repo process, service names, or API shapes.

Cognitive disciplines (RamShared examples): [`docs/methodology/kahneman-disciplines.md`](../../docs/methodology/kahneman-disciplines.md) (test *types* #9/#13/#15–#17 live there).

## Mandatory

1. Locks / concurrency (order, spinlock↔mutex, RCU, barriers, IRQ)
2. DMA / IOMMU / MMIO
3. Memory (mm): NUMA, HMM/`DEVICE_PRIVATE`, hotplug, hot-path allocation
4. uAPI / ABI (ioctl/sysfs/debugfs or exported layout)
5. New hardware / subsystem (device, DRM/TTM, CXL)
6. Structural MMU / DRM changes

## Optional / out of scope

Optional: pure refactors without contract change, localized bugfixes, doc-only, dependency bumps (log in `docs/LIBRARIES.md`).

**Not SSDV3:** CI, host-safety script tweaks (`scripts/safety/*`, `scripts/p0/*`), qemu/lab harness, disk reclaim. Use Kahneman #15/#16/#18 + [benchmarks.md](benchmarks.md) + append [`validation.md`](../../validation.md). If a script invents a kernel/uAPI contract, that product side still needs SSDV3.

## Artifacts

| Artifact | Path |
| --- | --- |
| Prompts + skeletons | `docs/SSDV3-PROMPTS.md` |
| Specs | `docs/specs/no-milestone/{slug}/{PRD,SPEC,IMPL}.md` (+ optional `AUDIT-2.5.md`, `evidence/`) |
| Kahneman | `docs/methodology/kahneman-disciplines.md` |

- One `SPEC.md` per feature; revise in-place (no `SPECvN.md`). After 2.5 `no-go`, fix SPEC same turn.
- After add/remove under `docs/specs/`: `node tools/generate-docs-index.mjs`.

## Step 3 hard gates (summary)

1. Cover ≥80% on slice **business-logic** files via `node tools/ci/check-rust-slice-coverage.mjs -p … --files … --min 80` (paths from SPEC matrix). Workspace average does not count. Shell-only may be E2E-only if SPEC marks it + live proof recorded.
2. Live E2E on **this** surface before DONE: **before → action → after**; platform-correct tools (cascade-health / kernel drill / WDK lab — as SPEC); `BINARY_MATCH` when `ramsharedd`; ≥1 legitimate + SPEC refusals. Env-bound → **partial**, not DONE.
3. Every SPEC matrix row has a real test name; Kahneman critical evidence is executable (not “code exists”).
4. Hang-class: [`superprompt.md`](../../superprompt.md) (audit only).

## Don't

- Skip PRD/SPEC for mandatory-scope changes
- IMPL without SPEC (or without 2.5 `go` when risk-gated)
- Inference on >30% of PRD facts
- New helper without checking kernel / `crates/` / existing SPECs
- Prove LKM/cascade with foreign-app HTTP smoke — need cargo test, drills, dmesg, `/proc/swaps` as appropriate
- Close Step 3 on index `DONE` / `IMPL.md` presence alone
- Run unsupervised swap/ublk pressure on the live WSL2 host
- Create `SPECvN.md`
- Paste SSDV3/Kahneman process from other monorepos (tenant, OpenAPI, web e2e) into this tree
