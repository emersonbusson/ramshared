# WSL2 freeze-elimination (scaffold)

## Status

**NOT CLAIMED (env-bound).** Daily host dry-run only until a **disposable isolated lab**
(true separate VM / machine — not a second WSL distro on the desktop) runs the full protocol.

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

Thrash on the daily/shared host is forbidden by `.claude/rules/benchmarks.md`.

## Script

`scripts/safety/wsl2-freeze-campaign.sh`

| Mode | Flags | Behavior |
| --- | --- | --- |
| Dry-run (default) | none / `--dry-run` | Baseline capture; refuse thrash; `claim=NOT_CLAIMED` |
| Gate check | `--check-gates` | Exit 0 only if isolated-ready gates pass |
| Isolated run | `--allow-isolated-lab --run-isolated` + `RAMSHARED_ISOLATED_LAB=1` | 2× before→action→after with swap-sanitize, cgroup pressure probe, watchdog |
| Force isolab override | also `RAMSHARED_FORCE_ISOLATED_LAB=1` | Only for a true disposable lab VM that still exposes `/mnt/c` |

Static: `scripts/safety/Test-Wsl2FreezeCampaignStatic.sh`.

## Policy

Never thrash swap/ublk on the daily WSL2 host or any WSL distro sharing that Windows desktop.
See `.claude/rules/benchmarks.md`.
