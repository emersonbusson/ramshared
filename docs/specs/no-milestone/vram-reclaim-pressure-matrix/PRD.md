# PRD - VRAM reclaim pressure matrix

## Status

**PARTIAL.** The 64 MiB Windows StorPort campaign is only a low-risk storage smoke. A
3 GiB Windows storage-only LUN passed on 2026-07-18, and a 3 GiB Windows LUN plus
768 MiB synthetic external CUDA workload passed on the same host. The exact 3 GiB
Windows plus 1 GiB external workload case, WSL2, and split-owner reclaim remain open.

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
| Split consumers | 4 GiB WSL2 + 1 GiB Windows | external workload requiring more than remaining free VRAM | At least one RamShared owner releases or refuses growth; external workload gets headroom; all RamShared data remains intact |

## Acceptance

- Use generic names: `external_gpu_workload`, `vram_reclaim`, `gpu_budget`, `wsl2_cascade`,
  and `windows_lun`. Do not encode example application names into files, policies, or claims.
- Record before -> action -> after for every case: `nvidia-smi`, RamShared status/events,
  swap state, pagefile state, minidumps, and checksum evidence.
- A 64 MiB pass must never be reported as GiB-scale reclaim proof.
- Physical host runs require clean preflight, concrete `PagingFiles`, no stale RAMSHARE LUN,
  explicit size, and explicit approval.
- WSL2 pressure runs require an isolated lab or an explicit shared-desktop override. Daily
  WSL2 must remain read-only/dry-run.

## Current Evidence

- 2026-07-18 `C:\ramshared\artifacts\exhaustive-20260718-003148`: Windows 3 GiB
  storage-only LUN passed with three SHA rounds, graceful teardown, lease release, no
  residual `Get-Disk`/`Win32_DiskDrive`/PnP nodes, and post-run preflight PASS.
- 2026-07-18 `C:\ramshared\artifacts\exhaustive-20260718-003811`: Windows 3 GiB LUN
  plus 768 MiB synthetic external CUDA workload passed with three SHA rounds, graceful
  teardown, lease release, no residual `Get-Disk`/`Win32_DiskDrive`/PnP nodes, and
  post-run preflight PASS. This is an intermediate proof; it does not close the exact
  1 GiB external workload case.
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

## Open Questions

- Whether physical Windows large-LUN testing should format a 3 GiB VRAM-backed volume, or use
  unformatted block I/O only. Formatting is acceptable only with exact RAMSHARE identity gates.
- Whether split-consumer proof runs on one physical host or across `win11-drill` plus WSL2 with
  GPU-PV limitations documented.
