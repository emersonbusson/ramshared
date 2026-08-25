# WSL2 freeze-elimination (scaffold)

## Status

**PARTIAL pending current requalification.** Default daily-host mode is still
dry-run only. The current candidate is disabled-staging only: it authorizes no
campaign, WSL lifecycle, VM, storage, swap, device, or pressure action.
Historical evidence does not prove the current source candidate or provide a
current execution path.

**Historical non-current / no execution:** 2026-07-22 close evidence is
retained only as a dated record; it must not be rerun from this document.
`SANITIZED_PATH_HOST_PRIVATE_ARTIFACT` passed with two
shared-host before/action/after rounds, validator
`WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS mode=shared-daily-host rounds=2`,
Windows watchdog not fired, telemetry JSONL all `ok=true`, no ghost, no recent
OOM/hung-task markers, and clean terminal state.

This is **not** `SANITIZED_VM_WINDOWS_STORPORT_LAB` and **not** the daily WSL2
desktop host. Sanitized markers that refuse thrash:

| Marker | Effect |
| --- | --- |
| `WSL_DISTRO_NAME=SANITIZED_WSL_DISTRO_DAILY` | daily host |
| `SANITIZED_PATH_SHARED_DESKTOP_MOUNT` present | shared Windows desktop (any distro, including `SANITIZED_WSL_DISTRO_CUSTOM_KERNEL`) |
| Missing `RAMSHARED_ISOLATED_LAB=1` | isolated flags refused |

**2026-07-17 probe:** `SANITIZED_WSL_DISTRO_CUSTOM_KERNEL` is a second WSL2
distro on the **same** Windows host (custom kernel `6.18.35.2`, still mounts
`SANITIZED_PATH_SHARED_DESKTOP_MOUNT`). It is **not** claim-ready isolab. Do
not set `RAMSHARED_FORCE_ISOLATED_LAB=1` on it casually — thrash still freezes
the desktop.

Direct/manual thrash on the daily/shared host is forbidden by
`.claude/rules/benchmarks.md`. **Disabled staging only / no execution:** The
retained watchdog interface below is inert documentation, not a current pressure
or WSL procedure. Historical shared-host evidence used
`scripts/windows/Invoke-SharedWslPressureCampaign.ps1` with an external Windows
watchdog and telemetry. The source definition retains three one-second Windows
commit samples and the `ceil(PressureAllocGiB*1024)+HostCommitReserveMiB`
(default 4096 MiB) refusal rule, but it authorizes no launch, disk telemetry, or
WSL action in this candidate.

Disabled-definition behavior only: historical records describe one-second host
commit samples and a PARTIAL reserve-breach result. They do not instruct a
current selected-distro termination, launcher stop, or workload stop. Broad WSL
shutdown, Windows reboot/shutdown, VM action, disk mutation, and an OOM-marker
override remain forbidden. The named admission/telemetry artifacts and
source-only manufactured tests are not live proof; live qualification remains
`PARTIAL` and a future campaign needs separate attended approval.

## Script

`scripts/safety/wsl2-freeze-campaign.sh`

**Disabled staging only / no execution:** The flags below describe retained
historical interfaces. They are not a current activation or campaign procedure.

| Mode | Flags | Behavior |
| --- | --- | --- |
| Dry-run (default) | none / `--dry-run` | Baseline capture; refuse thrash; `claim=NOT_CLAIMED` |
| Gate check | `--check-gates` | Exit 0 only if isolated-ready gates pass |
| Historical boundary | **Disabled staging only / no execution:** flags below are inert documentation, never a current activation path. | No invocation is authorized. |
| Isolated run | `--allow-isolated-lab --run-isolated` + `RAMSHARED_ISOLATED_LAB=1` | 2× before→action→after with swap-sanitize, cgroup pressure probe, watchdog |
| Shared daily-host run | `--approve-shared-daily-host --run-shared-daily-host` + `RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_WSL_TERMINATION` + `RAMSHARED_WINDOWS_WATCHDOG_ARMED=1` | 2× before→action→after with swap-sanitize, cgroup pressure probe, Windows watchdog, telemetry |
| Force isolab override | also `RAMSHARED_FORCE_ISOLATED_LAB=1` | Only for a true disposable lab VM that still exposes `/mnt/c` |

Static: `scripts/safety/Test-Wsl2FreezeCampaignStatic.sh`.

## Policy

Never run unsupervised swap/ublk pressure on the daily WSL2 host or any WSL
distro sharing that Windows desktop. See `.claude/rules/benchmarks.md`.
