---
slug: wsl2-freeze-elimination-campaign
title: WSL2 freeze-elimination campaign evidence gate
milestone: —
issues: []
---

# PRD - WSL2 freeze-elimination campaign evidence gate

## Summary

The WSL2 freeze-elimination claim must only close from a complete supervised
campaign. Preferred evidence is an isolated-lab campaign. When the explicit
target is the real shared WSL2 host, the only acceptable path is the Windows
shared-host watchdog harness with approval, telemetry, bounded pressure, and
cleanup artifacts. Daily-host baselines, QEMU-only drills, and single-round
pressure runs remain PARTIAL.

## Technical Context

- Confirmed in codebase: `scripts/safety/wsl2-freeze-campaign.sh` refuses live
  pressure on the daily WSL2 desktop unless isolated-lab gates or the explicit
  shared-host approval/watchdog gates are present.
- Confirmed in codebase: isolated mode records `round-N/before*`, `action-rc.txt`,
  `after*`, swap-sanitize logs, and `isolated-complete.txt`; shared-host mode
  records the same round artifacts plus `shared-daily-host-complete.txt`.
- Confirmed in docs: `docs/reliability/GAP-REGISTER.md` requires two isolated-lab
  before/action/after rounds with watchdog, binary match, ghost checks, D-state,
  hung-task evidence, swapoff-first proof, and clean terminal state.

## Requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | Validate campaign completeness from artifacts. | Validator PASS requires `summary.json`, an isolated or approved shared-host completion marker, and two complete `round-N` dirs. |
| RF-2 | Refuse unsafe daily-host or dry-run evidence as closure. | `daily_host=true` exits non-zero unless `shared_host_approved=true`, `windows_watchdog=true`, gates pass, and shared-host completion exists. |
| RF-3 | Require hang/freeze safety evidence. | Each round must include before/after captures, health JSON, sanitize logs, action rc, no watchdog file, and no hung-task/D-state markers in captures. |
| NFR-1 | Read-only validation. | Validator never runs pressure, swapoff, VM, or disk commands. |

## Validation Plan

- Static: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`.
- Synthetic PASS/PARTIAL fixture runs.
- Live close: validator PASS over a real isolated-lab artifact produced by
  `scripts/safety/wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated`,
  or a real shared-host artifact produced by
  `scripts/windows/Invoke-SharedWslPressureCampaign.ps1 -ApproveSharedDailyHost`.
