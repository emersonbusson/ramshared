---
slug: wsl2-freeze-elimination-campaign
title: WSL2 freeze-elimination campaign evidence gate
milestone: —
issues: []
---

# PRD - WSL2 freeze-elimination campaign evidence gate

## Summary

The WSL2 freeze-elimination claim must only close from a complete isolated-lab
campaign. Daily-host baselines, QEMU-only drills, and single-round pressure runs
are useful evidence, but they must remain PARTIAL.

## Technical Context

- Confirmed in codebase: `scripts/safety/wsl2-freeze-campaign.sh` refuses live
  pressure on the daily WSL2 desktop unless isolated-lab gates are explicit.
- Confirmed in codebase: isolated mode records `round-N/before*`, `action-rc.txt`,
  `after*`, swap-sanitize logs, and `isolated-complete.txt`.
- Confirmed in docs: `docs/reliability/GAP-REGISTER.md` requires two isolated-lab
  before/action/after rounds with watchdog, binary match, ghost checks, D-state,
  hung-task evidence, swapoff-first proof, and clean terminal state.

## Requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | Validate campaign completeness from artifacts. | Validator PASS requires `summary.json`, `isolated-complete.txt`, and two complete `round-N` dirs. |
| RF-2 | Refuse daily-host or dry-run evidence as closure. | `daily_host=true`, `gates_ok=false`, or missing isolated completion exits non-zero. |
| RF-3 | Require hang/freeze safety evidence. | Each round must include before/after captures, health JSON, sanitize logs, action rc, no watchdog file, and no hung-task/D-state markers in captures. |
| NFR-1 | Read-only validation. | Validator never runs pressure, swapoff, VM, or disk commands. |

## Validation Plan

- Static: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`.
- Synthetic PASS/PARTIAL fixture runs.
- Live close: validator PASS over a real isolated-lab artifact produced by
  `scripts/safety/wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated`.

