# WSL2 freeze-elimination (scaffold)

## Status

**CLAIMED via supervised shared-host evidence.** Default daily-host mode is
still dry-run only. A complete campaign can run either in a disposable isolated
lab or, when explicitly authorized, through the Windows shared-host watchdog
harness.

2026-07-22 close evidence:
`C:\ramshared\artifacts\shared-wsl-pressure-20260722-002748` passed with two
shared-host before/action/after rounds, validator
`WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS mode=shared-daily-host rounds=2`,
Windows watchdog not fired, telemetry JSONL all `ok=true`, no ghost, no recent
OOM/hung-task markers, and clean terminal state.

This is **not** `win11-drill` (Windows Hyper-V guest for StorPort) and **not** the daily WSL2
desktop host. Markers that refuse thrash:

| Marker | Effect |
| --- | --- |
| `WSL_DISTRO_NAME=Ubuntu-24.04` | daily host |
| `/mnt/c/Users` present | shared Windows desktop (any distro, including `RamShared-Kernel`) |
| Missing `RAMSHARED_ISOLATED_LAB=1` | isolated flags refused |

**2026-07-17 probe:** `RamShared-Kernel` is a second WSL2 distro on the **same** Windows host
(custom kernel `6.18.35.2`, still mounts `/mnt/c/Users`). It is **not** claim-ready isolab.
Do not set `RAMSHARED_FORCE_ISOLATED_LAB=1` on it casually — thrash still freezes the desktop.

Direct/manual thrash on the daily/shared host is forbidden by
`.claude/rules/benchmarks.md`. Shared-host pressure is only valid through
`scripts/windows/Invoke-SharedWslPressureCampaign.ps1`, which arms the external
Windows watchdog and records telemetry.

## Script

`scripts/safety/wsl2-freeze-campaign.sh`

| Mode | Flags | Behavior |
| --- | --- | --- |
| Dry-run (default) | none / `--dry-run` | Baseline capture; refuse thrash; `claim=NOT_CLAIMED` |
| Gate check | `--check-gates` | Exit 0 only if isolated-ready gates pass |
| Isolated run | `--allow-isolated-lab --run-isolated` + `RAMSHARED_ISOLATED_LAB=1` | 2× before→action→after with swap-sanitize, cgroup pressure probe, watchdog |
| Shared daily-host run | `--approve-shared-daily-host --run-shared-daily-host` + `RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_WSL_TERMINATION` + `RAMSHARED_WINDOWS_WATCHDOG_ARMED=1` | 2× before→action→after with swap-sanitize, cgroup pressure probe, Windows watchdog, telemetry |
| Force isolab override | also `RAMSHARED_FORCE_ISOLATED_LAB=1` | Only for a true disposable lab VM that still exposes `/mnt/c` |

Static: `scripts/safety/Test-Wsl2FreezeCampaignStatic.sh`.

## Policy

Never run unsupervised swap/ublk pressure on the daily WSL2 host or any WSL
distro sharing that Windows desktop. See `.claude/rules/benchmarks.md`.
