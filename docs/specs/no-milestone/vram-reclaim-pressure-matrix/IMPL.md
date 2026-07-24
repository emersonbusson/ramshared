# IMPL - VRAM reclaim pressure matrix

## Status

**DONE on the calibrated RTX 2060 surface.** Windows, WSL2 1 GiB, WSL2 4 GiB,
and split-owner rows have live before/action/after evidence. See
`evidence/matrix-close-20260724.md`.

## Implemented

| Item | Result |
| --- | --- |
| Matrix cases | `windows-smoke`, `windows-3gib`, `wsl2-1gib`, `wsl2-4gib`, `split-3gib-1gib` |
| Plan-only default | `PLAN_ONLY=1` without `-Run` |
| Safe headroom refusal | `PARTIAL` before LUN creation when free VRAM is below plan plus margin |
| Calibrated split preflight | 1 GiB Windows + 3 GiB WSL2 owners plus 256 MiB setup margin; 1 GiB external pressure is staged afterward |
| Machine-readable summary | `matrix-summary.json` |
| Windows 3 GiB + 1 GiB external subcase | `C:\ramshared\artifacts\vram-reclaim-matrix-20260718-135319` passed with `reserve_mib=768` plus the fixed 256 MiB margin, preserving a 1 GiB effective floor |
| WSL2 integrity gate | `scripts/safety/cascade_pressure_integrity_worker.py` plus `validate-wsl2-freeze-campaign-artifact.sh` now require a per-round checksum artifact before a WSL2 pressure artifact can validate |

## Closing evidence

- WSL2 1 GiB: `shared-wsl-pressure-20260723-232558`.
- WSL2 4 GiB: `shared-wsl-pressure-20260724-031615`.
- Split: `vram-reclaim-matrix-20260724-032344`,
  `exhaustive-20260724-032344`, and
  `shared-wsl-pressure-20260724-032358`.

## 2026-07-22 supervised split run

`C:\ramshared\artifacts\exhaustive-20260722-030004` established the calibrated
1 GiB Windows + 3 GiB WSL2 owners and staged 1 GiB external pressure. The WSL
watchdog artifact `shared-wsl-pressure-20260722-030018` passed external DEMOTE;
the private-mounted Windows LUN passed three checksums and direct I/O. Closure
remains partial because the installed older driver required configured letter
`S` during teardown, refused the private mount, and left an orphan virtual LUN.
The source fix now permits administrator-only `DESTROY_DISK` after owner exit,
reports non-rotating media through SCSI VPD `0xB1`, and the replacement driver
builds with WDK `/W4 /WX`. Disk telemetry now generates aligned uncached,
write-through reads and writes instead of measuring the Windows file cache.
Reboot and deployment are required before rerunning the row.

Rollback trigger: revert if a partial matrix case is reported as DONE without
the required live campaign evidence.
