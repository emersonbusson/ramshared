# VRAM reclaim pressure matrix close — 2026-07-24

## Verdict

**PASS** for the calibrated GiB-scale matrix on the RTX 2060 shared host.

| Row | Authoritative artifact | Result |
| --- | --- | --- |
| WSL2 1 GiB | `C:\ramshared\artifacts\shared-wsl-pressure-20260723-232558` | `matrix_row_close=true`, `DEMOTE=1`, two integrity rounds, final clean |
| WSL2 4 GiB | `C:\ramshared\artifacts\shared-wsl-pressure-20260724-031615` | `PREALLOCATE_VRAM=true`, `matrix_row_close=true`, `DEMOTE=2`, two integrity rounds, final clean |
| Split 1 GiB Windows + 3 GiB WSL2 + 1 GiB external | `C:\ramshared\artifacts\vram-reclaim-matrix-20260724-032344` | matrix `PASS`; correlated StorPort and WSL2 artifacts |

The split Windows artifact is
`C:\ramshared\artifacts\exhaustive-20260724-032344`. It reports
`ALL_MATCH=true`, `GRACEFUL=true`, `LUN_GONE=true`, `WIN32_GONE=true`,
`PNP_GONE=true`, `LEASE_RELEASED=true`, and `DISK_IO_MEASURE_OK=true`.

The correlated split WSL2 artifact is
`C:\ramshared\artifacts\shared-wsl-pressure-20260724-032358`. It reports
`matrix_row_close=true`, `freeze_campaign_validated=true`, `DEMOTE=1`, and
`final_clean=true`.

## Terminal state

After the split run, `ramshared status --json` reported `phase=Off`, no zram or
VRAM tier, no daemon, and no ghost. Host GPU free memory was 5348 MiB.

## Scope

This closes the calibrated matrix on the tested RTX 2060 environment. It does
not make the deferred ublk transport a product path and does not promote the
Windows miniport beyond its signed-distribution policy.
