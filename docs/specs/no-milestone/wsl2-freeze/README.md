# WSL2 freeze-elimination (scaffold)

## Status

**NOT CLAIMED.** Daily host dry-run only until an isolated lab runs the full protocol.

## Script

`scripts/safety/wsl2-freeze-campaign.sh`

| Mode | Flags | Behavior |
| --- | --- | --- |
| Dry-run (default) | none / `--dry-run` | Baseline capture; refuse thrash; `claim=NOT_CLAIMED` |
| Gate check | `--check-gates` | Exit 0 only if isolated-ready gates pass |
| Isolated run | `--allow-isolated-lab --run-isolated` + `RAMSHARED_ISOLATED_LAB=1` | 2× before→action→after with swap-sanitize, cgroup pressure probe, watchdog |

Static: `scripts/safety/Test-Wsl2FreezeCampaignStatic.sh`.

## Policy

Never thrash swap/ublk on the daily WSL2 host. See `.claude/rules/benchmarks.md`.
