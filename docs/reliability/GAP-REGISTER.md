# RamShared Gap Register

This file tracks open product claims that must stay **PARTIAL** until their
listed proof exists. It is not a backlog for speculative features; it is a
guardrail against false DONE status.

## Current Open Gates

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| External GPU workload WDDM pressure | PARTIAL | 2026-07-17 generic CUDA workload gate passed on the physical RTX 2060, proving aggregate WDDM/CUDA pressure and recovery. 2026-07-18 `C:\ramshared\artifacts\exhaustive-20260718-003811` passed a 3 GiB Windows LUN plus 768 MiB synthetic external CUDA workload. The daemon/cascade was not active, so DEMOTE/recovery is not correlated yet. | Evidence run with matching `ramshared diagnose --events` or daemon telemetry showing DEMOTE/recovery without corruption during an app-agnostic external VRAM pressure gate. |
| GiB-scale VRAM reclaim matrix | PARTIAL | 2026-07-18 Windows 3 GiB storage-only LUN passed cleanly, and 3 GiB plus 768 MiB external CUDA workload passed cleanly. The exact simultaneous Windows 3 GiB + 1 GiB external workload case was refused again at `C:\ramshared\artifacts\vram-reclaim-matrix-windows3gib-run-20260718-0055` because RTX 2060 free VRAM was 5193 MiB, below the 5120 MiB plan plus 256 MiB operational margin. WSL2 1 GiB, WSL2 4 GiB, and split-owner reclaim under external GPU pressure remain unproven. | `docs/specs/no-milestone/vram-reclaim-pressure-matrix/PRD.md` matrix evidence: before/action/after for Windows 3 GiB + external workload, WSL2 1 GiB, WSL2 4 GiB, and split 4 GiB+1 GiB, with generic external workload, reserve preservation, checksum integrity, DEMOTE/teardown proof, and clean terminal state. |
| WSL2 freeze-elimination campaign | PARTIAL | The daily WSL2 desktop must not be thrashed. 2026-07-18 `--check-gates --json` produced `claim=NOT_CLAIMED` and refused action with `daily_host_refused_without_isolated_lab_flag`; QEMU/ublk and broker drills are green, but they do not prove repeated live WSL2 desktop freeze elimination under host pressure. | Two isolated-lab `before -> action -> after` campaign runs with watchdog timeout, binary match, swapoff-first proof, no ghost swap, no hung task/D-state evidence, and clean terminal state. |
| Windows Task Manager disk counters | PARTIAL | 2026-07-18 `C:\ramshared\artifacts\disk-counter-audit-20260718-005325` passed the dedicated CIM/direct-I/O audit with non-zero PerfDisk activity and checksum I/O while the LUN was mounted. Task Manager can still present StorPort virtual disks as `100% / 0 KB/s / 0 ms`, so UI parity with a physical SSD is not claimed. | A dedicated Windows UI proof for Task Manager read/write/active-time behavior across RAW, formatted-idle, formatted-write, teardown, and residual-PnP states, or an explicit product decision that Task Manager parity is out of scope and CIM/direct metrics are authoritative. |
| Custom-kernel/ublk as day-1 product transport | DEFERRED | NBD remains the day-1 WSL2 product path. ublk root and QEMU smokes are green, but `linux-kernel-lab` currently boots Ubuntu `6.8.0-134-generic`; `/dev/ublk-control` is absent and `modprobe ublk_drv` reports the module is missing. | Dedicated custom-kernel lab SPEC with full up/down wire-up, swapoff-first teardown, crash/drain drills, and terminal no-ghost proof. |

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

## Rules

- Do not mark an environment-bound gate DONE from unit tests, parser checks,
  docs, QEMU-only evidence, or a different machine class.
- Do not encode one example application as a product feature, directory,
  script, policy, or generic docs heading.
- Do not commit local VM credentials, signing passwords, key material, or
  generated package artifacts.
