# RamShared Gap Register

This file tracks open product claims that must stay **PARTIAL** until their
listed proof exists. It is not a backlog for speculative features; it is a
guardrail against false DONE status.

## Current Open Gates

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| GiB-scale VRAM reclaim matrix | PARTIAL | The Windows 3 GiB + 1 GiB external subcase passed previously. On 2026-07-22, `exhaustive-20260722-030004` established the calibrated 1 GiB Windows + 3 GiB WSL2 owners; `shared-wsl-pressure-20260722-030018` passed staged 1 GiB external DEMOTE, and the Windows LUN passed three checksums plus direct I/O. The installed older driver then required letter `S` for teardown, refused the private mount, and left an orphan virtual LUN. Source now supports administrator-only orphan destroy after owner exit and builds `/W4 /WX`, but reboot/deploy/rerun remain. | Reboot to clear the old virtual LUN, deploy the signed replacement driver, then rerun WSL2 1 GiB, WSL2 4 GiB, and split 3 GiB+1 GiB with integrity, reserve restoration, teardown, and clean terminal-state proof. |
| Custom-kernel/ublk as day-1 product transport | DEFERRED | NBD remains the day-1 WSL2 product path. ublk root and QEMU smokes are historical capability evidence, not product transport closure. 2026-07-18 `C:\ramshared\artifacts\linux-kernel-lab-capability-20260718-131502` reached `linux-kernel-lab` over SSH with passwordless sudo, installed/loaded `ublk_drv`, and passed capability audit with `/dev/ublk-control` present. The VM still has no GPU surface, and no product ublk lifecycle, swapoff-first teardown, crash/drain, or no-ghost proof exists yet. | Dedicated custom-kernel lab SPEC with full up/down wire-up, swapoff-first teardown, crash/drain drills, and terminal no-ghost proof using the now-capable `linux-kernel-lab` surface. |

## Closed In This Session

| Gap | Close evidence |
| --- | --- |
| App-specific GPU workload naming | Public tracked scan for specific example application names and old render-specific terms is clean. Generic names are `gpu_workload`, `dcc`, `host_agent`, `vram_reclaim`, and `gpu_budget`. |
| External GPU workload WDDM pressure | `C:\ramshared\artifacts\shared-wsl-pressure-20260722-015303` PASS from `scripts/windows/Invoke-SharedWslPressureCampaign.ps1 -ApproveSharedDailyHost -ExternalWorkloadMiB 4096 -PostCampaignObserveSec 120 -HostDiskLetters C,I`: generic external CUDA workload completed (`external_workload_ok=true`), `ramshared diagnose --events --json` observed `demotes=2` with timeline reason `GlobalGpuFreeFloor` and no process attribution, global GPU free fell to 348 MiB while GPU used peaked at 5607 MiB, final health was clean (`ghost=false`, daemon dead, no zram/VRAM swap left), and host disk telemetry was collected for `C:` and `I:`. This closes the aggregate external VRAM pressure claim; it does not close the GiB reclaim matrix rows. |
| Historical signing password literal | `validation.md` now references `RAMSHARED_TESTSIGN_PFX_PASSWORD`; `tools/ci/check-public-hygiene.mjs` blocks the old literal and inline signing-password patterns. |
| P1 broker isolated drill freshness | `scripts/kernel/qemu-broker-drill.sh` PASS with `KTEST-DAEMON-BINARY-MATCH=ok`, `KTEST-AGENT-BINARY-MATCH=ok`, `KTEST-SWAP-ACTIVE=ok`, `KTEST-TELEMETRY=ok`, `KTEST-SWAPOFF=ok`, and `KTEST-DAEMON-TERMINATED=ok`. |
| WSL2 freeze-elimination campaign | `C:\ramshared\artifacts\shared-wsl-pressure-20260722-002748` PASS from `scripts/windows/Invoke-SharedWslPressureCampaign.ps1 -ApproveSharedDailyHost`: two shared-host before/action/after rounds, validator `WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS mode=shared-daily-host rounds=2`, `wsl_exit_code=0`, Windows watchdog not fired, no round watchdog files, `BINARY_MATCH=true`, no ghost, no recent OOM/hung-task markers, telemetry JSONL all `ok=true`, zram max 262140 KiB, VRAM/NBD max 45396 KiB, and terminal state clean with only `/dev/sdc` disk swap active and `ramsharedd` stopped. VM status remains documented separately: `win11-wsl2-lab` WSL runtime is still unavailable, but it is no longer the required path for this claim. |
| Linux/WSL2 installable bundle | `scripts/package/build-linux-bundle.sh` builds release binaries, stages safety scripts/docs, and emits a validating `SHA256SUMS` manifest. |
| GPU-PV CUDA inside `win11-drill` | 2026-07-17 `scripts/windows/Run-StorportCudaPartial.ps1` started `win11-drill`, loaded `poolstress` and `ramshared`, and `ramshared-winsvc.exe probe-cuda` passed in guest with RTX 2060 and a 512 MiB CUDA allocation/roundtrip. Artifact: `C:\Users\emedev\ramshared-drill\agent-storport-cuda-20260717-224717\guest.json`. |
| Windows physical product Online | 2026-07-18 `C:\ramshared\artifacts\exhaustive-20260718-001154`: clean preflight, product Online on RTX 2060, 64 MiB `RAMSHARE VRAMDISK`, three SHA rounds match, `RuntimeSummary exit_code: 0`, `LEASE_RELEASED=true`, `LUN_GONE=true`, `WIN32_GONE=true`, `PNP_GONE=true`, no minidumps, and post-run preflight PASS. |
| Windows 3 GiB storage-only LUN | 2026-07-18 `C:\ramshared\artifacts\exhaustive-20260718-003148`: artifact-local config `size_bytes=3221225472`, product Online on RTX 2060, formatted only `RAMSHARE VRAMDISK` disk 5 as `S:`, three SHA rounds match, graceful teardown, `EXIT=0`, `LEASE_RELEASED=true`, `LUN_GONE=true`, `WIN32_GONE=true`, `PNP_GONE=true`, no minidumps, and post-run preflight PASS. |
| Windows supported disk counters | 2026-07-18 `C:\ramshared\artifacts\disk-counter-audit-20260718-005325`: dedicated CIM/direct-I/O audit passed with `DISK_IO_MEASURE_OK=true`, `DIRECT_LOAD_MATCH=true`, `DIRECT_PROBE_MATCH=true`, `PERFDISK_MATCH=true`, `NONZERO_ACTIVITY=true`, and clean `LUN_GONE`/`WIN32_GONE`/`PNP_GONE`. Task Manager UI parity is explicitly out of scope; CIM/direct metrics are authoritative. |

## Rules

- Do not mark an environment-bound gate DONE from unit tests, parser checks,
  docs, QEMU-only evidence, or a different machine class.
- Do not encode one example application as a product feature, directory,
  script, policy, or generic docs heading.
- Do not commit local VM credentials, signing passwords, key material, or
  generated package artifacts.
