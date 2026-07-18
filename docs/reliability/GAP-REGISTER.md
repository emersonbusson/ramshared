# RamShared Gap Register

This file tracks open product claims that must stay **PARTIAL** until their
listed proof exists. It is not a backlog for speculative features; it is a
guardrail against false DONE status.

## Current Open Gates

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| External GPU workload WDDM pressure | PARTIAL | 2026-07-17 generic CUDA workload gate passed on the physical RTX 2060, proving aggregate WDDM/CUDA pressure and recovery. The daemon/cascade was not active, so DEMOTE/recovery is not correlated yet. | Evidence run with matching `ramshared diagnose --events` or daemon telemetry showing DEMOTE/recovery without corruption during an app-agnostic external VRAM pressure gate. |
| WSL2 freeze-elimination campaign | PARTIAL | The daily WSL2 desktop must not be thrashed. 2026-07-17 `--check-gates --json` produced `claim=NOT_CLAIMED` and refused action with `daily_host_refused_without_isolated_lab_flag`; QEMU/ublk and broker drills are green, but they do not prove repeated live WSL2 desktop freeze elimination under host pressure. | Two isolated-lab `before -> action -> after` campaign runs with watchdog timeout, binary match, swapoff-first proof, no ghost swap, no hung task/D-state evidence, and clean terminal state. |
| Windows physical product Online | PARTIAL | 2026-07-17 storage-only preflight passed and `Run-HostExhaustive.ps1` reached `product Online`, but no RAMSHARE disk enumerated because `ramshared-winsvc` could not connect to broker `127.0.0.1:19876`. PnP root device was removed afterward; the miniport remained loaded until host reboot/unload, so terminal cleanup is not clean. | Approved physical/lab run with `BINARY_MATCH`, three fresh SHA rounds, no bugcheck/dump, correlated `LeaseRelease`, CUDA free restoration, and terminal cleanup. |
| Custom-kernel/ublk as day-1 product transport | DEFERRED | NBD remains the day-1 WSL2 product path. ublk root and QEMU smokes are green, but `linux-kernel-lab` currently boots Ubuntu `6.8.0-134-generic`; `/dev/ublk-control` is absent and `modprobe ublk_drv` reports the module is missing. | Dedicated custom-kernel lab SPEC with full up/down wire-up, swapoff-first teardown, crash/drain drills, and terminal no-ghost proof. |

## Closed In This Session

| Gap | Close evidence |
| --- | --- |
| App-specific GPU workload naming | Public tracked scan for specific example application names and old render-specific terms is clean. Generic names are `gpu_workload`, `dcc`, `host_agent`, `vram_reclaim`, and `gpu_budget`. |
| Historical signing password literal | `validation.md` now references `RAMSHARED_TESTSIGN_PFX_PASSWORD`; `tools/ci/check-public-hygiene.mjs` blocks the old literal and inline signing-password patterns. |
| P1 broker isolated drill freshness | `scripts/kernel/qemu-broker-drill.sh` PASS with `KTEST-DAEMON-BINARY-MATCH=ok`, `KTEST-AGENT-BINARY-MATCH=ok`, `KTEST-SWAP-ACTIVE=ok`, `KTEST-TELEMETRY=ok`, `KTEST-SWAPOFF=ok`, and `KTEST-DAEMON-TERMINATED=ok`. |
| Linux/WSL2 installable bundle | `scripts/package/build-linux-bundle.sh` builds release binaries, stages safety scripts/docs, and emits a validating `SHA256SUMS` manifest. |
| GPU-PV CUDA inside `win11-drill` | 2026-07-17 `scripts/windows/Run-StorportCudaPartial.ps1` started `win11-drill`, loaded `poolstress` and `ramshared`, and `ramshared-winsvc.exe probe-cuda` passed in guest with RTX 2060 and a 512 MiB CUDA allocation/roundtrip. Artifact: `C:\Users\emedev\ramshared-drill\agent-storport-cuda-20260717-224717\guest.json`. |

## Rules

- Do not mark an environment-bound gate DONE from unit tests, parser checks,
  docs, QEMU-only evidence, or a different machine class.
- Do not encode one example application as a product feature, directory,
  script, policy, or generic docs heading.
- Do not commit local VM credentials, signing passwords, key material, or
  generated package artifacts.
