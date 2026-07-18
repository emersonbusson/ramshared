# IMPL - VRAM reclaim pressure matrix

## Status

**PARTIAL.** The matrix runner exists and emits per-case PASS/PARTIAL/FAIL
artifacts. Windows smoke and Windows 3 GiB cases run through the host exhaustive
harness; WSL2 and split-owner cases remain environment-bound.

## Implemented

| Item | Result |
| --- | --- |
| Matrix cases | `windows-smoke`, `windows-3gib`, `wsl2-1gib`, `wsl2-4gib`, `split-4gib-1gib` |
| Plan-only default | `PLAN_ONLY=1` without `-Run` |
| Safe headroom refusal | `PARTIAL` before LUN creation when free VRAM is below plan plus margin |
| Machine-readable summary | `matrix-summary.json` |
| Windows 3 GiB + 1 GiB external subcase | `C:\ramshared\artifacts\vram-reclaim-matrix-20260718-135319` passed with `reserve_mib=768` plus the fixed 256 MiB margin, preserving a 1 GiB effective floor |

## Remaining

- WSL2 1 GiB and 4 GiB isolated-lab campaigns.
- Split 4 GiB WSL2 + 1 GiB Windows owner orchestration.
- Live DEMOTE telemetry correlation under external GPU pressure.

Rollback trigger: revert if a partial matrix case is reported as DONE without
the required live campaign evidence.
