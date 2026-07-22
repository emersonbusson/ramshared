# PRD - VRAM reclaim pressure matrix

## Status

**PARTIAL.** The 64 MiB Windows StorPort campaign is only a low-risk storage smoke. A
3 GiB Windows storage-only LUN passed on 2026-07-18, and the Windows 3 GiB plus
1 GiB external CUDA workload subcase passed on the same host after freeing desktop
VRAM and preserving a 1 GiB effective floor. WSL2 and split-owner reclaim remain open.

## Goal

Prove that RamShared keeps the Linux/WSL2 core safe while returning GPU memory when an
external GPU workload needs VRAM. The external workload is app-agnostic: browser/video,
DCC, game, CUDA, Direct3D, or another GPU consumer are all modeled as aggregate VRAM
pressure.

## Required Matrix

| Case | RamShared allocation | External pressure | Expected behavior |
| --- | --- | --- | --- |
| Windows storage smoke | 64 MiB Windows LUN | none | Online, checksum I/O, graceful teardown, LUN gone |
| Windows large LUN | 3 GiB Windows LUN | synthetic external GPU workload | No data corruption; clean teardown; no minidump; visible I/O counters under backend-active writes |
| WSL2 small cascade | 1 GiB WSL2 VRAM tier | external GPU workload over reserve | DEMOTE or commit refusal before reserve is exhausted; swapoff-first; no ghost swap |
| WSL2 product cascade | 4 GiB WSL2 VRAM tier | external GPU workload over reserve | VRAM returned via DEMOTE; zram/disk absorb pages; no freeze/hung task |
| Split consumers | 3 GiB WSL2 + 1 GiB Windows | 1 GiB staged external workload | Establish both owners with a 256 MiB setup margin, then require reclaim to restore the 1 GiB target floor; all RamShared data remains intact |

## Acceptance

- Use generic names: `external_gpu_workload`, `vram_reclaim`, `gpu_budget`, `wsl2_cascade`,
  and `windows_lun`. Do not encode example application names into files, policies, or claims.
- Record before -> action -> after for every case: `nvidia-smi`, RamShared status/events,
  swap state, pagefile state, minidumps, and checksum evidence.
- A 64 MiB pass must never be reported as GiB-scale reclaim proof.
- Physical host runs require clean preflight, concrete `PagingFiles`, no stale RAMSHARE LUN,
  explicit size, and explicit approval.
- Shared-host WSL2 pressure requires `Invoke-SharedWslPressureCampaign.ps1`,
  explicit approval, the Windows-side watchdog, bounded pressure, telemetry, and
  cleanup artifacts. Direct or unsupervised pressure remains forbidden.
- Split preflight requires resident owner allocations plus a fixed 256 MiB setup
  margin. External pressure is staged after both owners are established; the
  1 GiB reserve is the post-reclaim target, not a simultaneous allocation.
- Each WSL2 pressure round must include `integrity-result.json` with `status=PASS`,
  non-zero allocated MiB, non-zero verified chunks, and matching before/after
  checksums.

## Current Evidence

- 2026-07-18 `C:\ramshared\artifacts\exhaustive-20260718-003148`: Windows 3 GiB
  storage-only LUN passed with three SHA rounds, graceful teardown, lease release, no
  residual `Get-Disk`/`Win32_DiskDrive`/PnP nodes, and post-run preflight PASS.
- 2026-07-18 `C:\ramshared\artifacts\exhaustive-20260718-003811`: Windows 3 GiB LUN
  plus 768 MiB synthetic external CUDA workload passed with three SHA rounds, graceful
  teardown, lease release, no residual `Get-Disk`/`Win32_DiskDrive`/PnP nodes, and
  post-run preflight PASS. This is an intermediate proof; it does not close the exact
  1 GiB external workload case.
- 2026-07-18 `C:\ramshared\artifacts\vram-reclaim-matrix-20260718-135319`: matrix
  runner passed the exact Windows 3 GiB LUN plus 1 GiB external CUDA workload subcase
  after closing GPU-heavy desktop apps. The run used `reserve_mib=768`, which combines
  with the fixed 256 MiB operational margin to preserve a 1 GiB effective floor. The
  delegated exhaustive artifact `C:\ramshared\artifacts\exhaustive-20260718-135319`
  reported `ALL_MATCH=true`, `GRACEFUL=true`, `EXTERNAL_WORKLOAD_OK=true`,
  `LUN_GONE=true`, `WIN32_GONE=true`, `PNP_GONE=true`, `LEASE_RELEASED=true`, and
  `DISK_IO_MEASURE_OK=true`. Host System log later recorded `Kernel-Power` 41
  and `EventLog` 6008 after a manual reboot, so this evidence must not be used
  to close WSL2/desktop freeze elimination.
- 2026-07-18 `C:\ramshared\artifacts\exhaustive-20260718-004215`: 64 MiB Windows smoke
  passed with `DISK_IO_MEASURE_OK=true`. During the PerfDisk sampling window the direct
  load wrote 304 MiB and read 304 MiB with `match=True`; PerfDisk matched `5 S:`, showed
  non-zero busy/write/queue counters, and the direct probe reported checksum match.
- 2026-07-18 `C:\ramshared\artifacts\vram-reclaim-matrix-windows3gib-run-20260718`:
  simultaneous Windows 3 GiB + 1 GiB external workload refused safely because free VRAM
  was 5150 MiB for a 5120 MiB plan plus required 256 MiB operational margin.
- 2026-07-18 `C:\ramshared\artifacts\vram-reclaim-matrix-windows3gib-run-20260718-0055`:
  rerun with `-Run -ApprovePhysicalHost` refused safely before creating a LUN because
  free VRAM was 5193 MiB for a 5120 MiB plan plus required 256 MiB operational margin.
  Post-refusal storage-only preflight PASSed with no residual LUN, Win32 disk, or PnP node.
- 2026-07-18 `C:\ramshared\artifacts\vram-reclaim-matrix-20260718-012157`:
  matrix runner emitted `matrix-summary.json` with `windows-3gib` as `PARTIAL`
  before LUN creation because free VRAM was 5203 MiB for a 5120 MiB plan plus
  required 256 MiB operational margin.
- 2026-07-22 `C:\ramshared\artifacts\shared-wsl-pressure-20260722-015303`:
  supervised shared-host pressure proved aggregate external GPU DEMOTE telemetry
  with C:/I: host disk telemetry, but the pressure worker was killed and the
  artifact has no per-round checksum integrity proof. It is external-pressure
  evidence only; it does not close WSL2 1 GiB, WSL2 4 GiB, or split-owner rows.
- 2026-07-22 the split row was recalibrated from 4 GiB + 1 GiB to 3 GiB +
  1 GiB for the 6144 MiB RTX 2060. The former row incorrectly treated the
  post-reclaim reserve as simultaneously resident. The calibrated preflight
  requires 4096 MiB of owners plus a 256 MiB setup margin, then stages 1024 MiB
  of external pressure under the Windows watchdog.

## Open Questions

- Whether physical Windows large-LUN testing should format a 3 GiB VRAM-backed volume, or use
  unformatted block I/O only. Formatting is acceptable only with exact RAMSHARE identity gates.
- Whether split-consumer proof runs on one physical host or across `win11-drill` plus WSL2 with
  GPU-PV limitations documented.
