---
slug: wsl2-freeze-elimination-campaign
title: WSL2 freeze-elimination campaign evidence gate
milestone: —
issues: []
---

# PRD - WSL2 freeze-elimination campaign evidence gate

> **Disabled staging only / no execution:** This PRD preserves requirements and
> historical evidence. It authorizes no campaign, WSL lifecycle, VM, storage,
> swap, device, or pressure action; live qualification remains `PARTIAL`.

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
  `after*`, swap-sanitize logs, `integrity-result.json`, and
  `isolated-complete.txt`; shared-host mode records the same round artifacts
  plus `shared-daily-host-complete.txt`.
- Confirmed in docs: `docs/reliability/GAP-REGISTER.md` requires two isolated-lab
  before/action/after rounds with watchdog, binary match, ghost checks, D-state,
  hung-task evidence, swapoff-first proof, and clean terminal state.

## Requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | Validate campaign completeness from artifacts. | Validator PASS requires `summary.json`, an isolated or approved shared-host completion marker, and two complete `round-N` dirs. |
| RF-2 | Refuse unsafe daily-host or dry-run evidence as closure. | `daily_host=true` exits non-zero unless `shared_host_approved=true`, `windows_watchdog=true`, gates pass, and shared-host completion exists. |
| RF-3 | Require hang/freeze safety evidence. | Each round must include before/after captures, health JSON, sanitize logs, action rc, no watchdog file, and no hung-task/D-state markers in captures. |
| RF-4 | Require pressure-data integrity evidence. | Each round must include `integrity-result.json` with `status=PASS`, positive allocated MiB, positive verified chunk count, and matching before/after checksums. |
| RF-5 | Admit a shared-host campaign only when Windows commit headroom covers the planned allocation plus a protected reserve. | Before disk telemetry, WSL, RamShared, or a guest process, take three one-second locale-neutral CIM samples. The minimum must meet `ceil(PressureAllocGiB*1024)+HostCommitReserveMiB`; the default reserve is 4096 MiB. Invalid telemetry refuses with `host_memory_query_failed`; insufficient headroom refuses with `host_commit_headroom_insufficient`. |
| RF-6 | Guard Windows commit headroom for every host wait phase. | Sample each second. A value below the 4096 MiB reserve or three consecutive invalid samples trips once, stops the optional external workload and launcher, and targets only the selected distro with one `wsl.exe --terminate`. The terminal result is PARTIAL with `host_commit_reserve_breached` or `host_memory_telemetry_stale`. |
| NFR-1 | Read-only validation. | Validator never runs pressure, swapoff, VM, or disk commands. |
| NFR-2 | No host override or broad recovery route. | The guard has no bypass, never accepts an OOM-marker allowance, never uses broad WSL shutdown, host reboot/shutdown, VM action, or disk mutation, and records admission/runtime telemetry artifacts. |

## Validation Plan

- Static: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`.
- Manufactured PowerShell: `scripts/windows/Test-SharedWslPressureCampaignMemoryGate.ps1`
  covers below-plan refusal, exact boundary admission, invalid CIM refusal,
  reserve breach, and three-sample telemetry loss.
- Static PowerShell: `scripts/windows/Test-SharedWslPressureCampaignStatic.ps1`
  proves the guard is before disk telemetry and only the selected distro can be
  terminated; it forbids OOM allowances, broad shutdown/reboot, VM, and disk
  mutation routes.
- Synthetic PASS/PARTIAL fixture runs.
> **Historical non-current / no execution:** The retained live-close artifact
> descriptions below are evidence only; do not invoke either path.
- Live close: validator PASS over a real isolated-lab artifact produced by
  `scripts/safety/wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated`,
  or a real shared-host artifact produced by
  `scripts/windows/Invoke-SharedWslPressureCampaign.ps1 -ApproveSharedDailyHost`.
