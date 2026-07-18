# RamShared Gap Register

This file tracks open product claims that must stay **PARTIAL** until their
listed proof exists. It is not a backlog for speculative features; it is a
guardrail against false DONE status.

## Current Open Gates

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| External GPU workload WDDM pressure | PARTIAL | 2026-07-17 generic CUDA workload gate passed on the physical RTX 2060, proving aggregate WDDM/CUDA pressure and recovery. 2026-07-18 `C:\ramshared\artifacts\exhaustive-20260718-003811` passed a 3 GiB Windows LUN plus 768 MiB synthetic external CUDA workload. `scripts/p0/Invoke-ExternalGpuWddmPressureAudit.ps1` now enforces the missing correlation gate and synthetic PASS/PARTIAL fixtures are green, but no live daemon/cascade DEMOTE event was captured under external pressure yet. | Live evidence from `scripts/p0/Invoke-ExternalGpuWddmPressureAudit.ps1` with real `Invoke-GpuWorkloadGate.ps1` output and matching `ramshared diagnose --events` daemon telemetry showing `DEMOTES>0`, no corruption, and no process attribution claim during an app-agnostic external VRAM pressure gate. |
| GiB-scale VRAM reclaim matrix | PARTIAL | 2026-07-18 Windows 3 GiB storage-only LUN passed cleanly, and 3 GiB plus 768 MiB external CUDA workload passed cleanly. The exact simultaneous Windows 3 GiB + 1 GiB external workload case was refused again at `C:\ramshared\artifacts\vram-reclaim-matrix-20260718-012157` because RTX 2060 free VRAM was 5203 MiB, below the 5120 MiB plan plus 256 MiB operational margin. WSL2 1 GiB, WSL2 4 GiB, and split-owner reclaim under external GPU pressure remain unproven. | `docs/specs/no-milestone/vram-reclaim-pressure-matrix/PRD.md` matrix evidence: before/action/after for Windows 3 GiB + external workload, WSL2 1 GiB, WSL2 4 GiB, and split 4 GiB+1 GiB, with generic external workload, reserve preservation, checksum integrity, DEMOTE/teardown proof, and clean terminal state. |
| WSL2 freeze-elimination campaign | PARTIAL | The daily WSL2 desktop must not be thrashed. 2026-07-18 `--check-gates --json` produced `claim=NOT_CLAIMED` and refused action with `daily_host_refused_without_isolated_lab_flag`; QEMU/ublk and broker drills are green, and `scripts/safety/validate-wsl2-freeze-campaign-artifact.sh` now enforces the required artifact shape, but no real isolated WSL2 campaign has passed it yet. | Two isolated-lab `before -> action -> after` campaign runs that pass `scripts/safety/validate-wsl2-freeze-campaign-artifact.sh`, with watchdog timeout, binary match, swapoff-first proof, no ghost swap, no hung task/D-state evidence, and clean terminal state. |
| Custom-kernel/ublk as day-1 product transport | DEFERRED | NBD remains the day-1 WSL2 product path. ublk root and QEMU smokes are historical capability evidence, not product transport closure. `scripts/windows/Invoke-LinuxKernelLabCapabilityAudit.ps1` now gates the current `linux-kernel-lab` capability state without disk mutation or swap pressure; the last recorded lab boot was Ubuntu `6.8.0-134-generic` with `/dev/ublk-control` absent and `modprobe ublk_drv` missing. | Dedicated custom-kernel lab SPEC with full up/down wire-up, swapoff-first teardown, crash/drain drills, and terminal no-ghost proof after a PASS from `scripts/windows/Invoke-LinuxKernelLabCapabilityAudit.ps1`. |

## Closed In This Session

| Gap | Close evidence |
| --- | --- |
| App-specific GPU workload naming | Public tracked scan for specific example application names and old render-specific terms is clean. Generic names are `gpu_workload`, `dcc`, `host_agent`, `vram_reclaim`, and `gpu_budget`. |
| Historical signing password literal | `validation.md` now references `RAMSHARED_TESTSIGN_PFX_PASSWORD`; `tools/ci/check-public-hygiene.mjs` blocks the old literal and inline signing-password patterns. |
| P1 broker isolated drill freshness | `scripts/kernel/qemu-broker-drill.sh` PASS with `KTEST-DAEMON-BINARY-MATCH=ok`, `KTEST-AGENT-BINARY-MATCH=ok`, `KTEST-SWAP-ACTIVE=ok`, `KTEST-TELEMETRY=ok`, `KTEST-SWAPOFF=ok`, and `KTEST-DAEMON-TERMINATED=ok`. |
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
