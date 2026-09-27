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
| RF-5 | Admit a shared-host campaign only when Windows physical and commit headroom cover the planned allocation plus protected reserves. | Before disk telemetry, WSL, RamShared, or a guest process, take three one-second `GetPerformanceInfo` samples. Calculate commit headroom as `CommitLimit - CommitTotal` pages and physical headroom from `PhysicalAvailable` pages using the reported page size. The minima must meet the planned allocation plus their separate reserves; both default to 4096 MiB. Invalid telemetry refuses with `host_memory_query_failed`; insufficient headroom reports `host_physical_headroom_insufficient` or `host_commit_headroom_insufficient`. |
| RF-6 | Guard Windows physical and commit headroom during every host wait phase. | Sample `GetPerformanceInfo` each second. Physical or commit headroom below its 4096 MiB reserve, or three consecutive invalid samples, trips once, stops optional work and the launcher, and targets only the selected distro with one `wsl.exe --terminate`. The terminal result is PARTIAL with `host_physical_reserve_breached`, `host_commit_reserve_breached`, or `host_memory_telemetry_stale`. |
| RF-7 | Admit and contain pressure only while the selected Linux guest has valid memory, swap, and PSI headroom. | Before any `ramshared down`, `check`, or `up`, require `/proc/meminfo` `MemAvailable` and `SwapFree` to each meet the fixed 1024 MiB reserve. Before the bounded freeze-probe worker starts, require valid guest `MemAvailable`, `SwapFree`, and memory PSI with positive headroom above 600 MiB and 1024 MiB reserves respectively, and PSI full avg10 below 10%. Give the worker finite cgroup `memory.max` and `memory.swap.max` limits that preserve those reserves, recalculate them once per second using current guest telemetry and cgroup usage, and stop on malformed/stale telemetry, reserve breach, failed limit writes, or pressure threshold. Missing cgroup accounting or controller support refuses before worker allocation. Standalone probe invocation without the campaign admission marker refuses before cgroup creation; the marker is passed only by the gated isolated/shared campaign paths. WSL2/cascade CLI stress also requires valid PSI before allocation and during each active phase; a missing/invalid `min_free_kbytes` value is treated as zero known reserve so the 600 MiB WSL2 floor is not weakened. |
| NFR-1 | Read-only validation. | Validator never runs pressure, swapoff, VM, or disk commands. |
| NFR-2 | No host override or broad recovery route. | The guard has no bypass, never accepts an OOM-marker allowance, never uses broad WSL shutdown, host reboot/shutdown, VM action, or disk mutation, and records admission/runtime telemetry artifacts. |
| NFR-3 | Keep planned RAM pressure inside the selected WSL2 guest. | The shared freeze probe and three-tier allocator run through `wsl.exe`; Windows supervises and samples host telemetry without allocating the planned RAM workload. The optional external CUDA VRAM workload is a separate Windows GPU test, defaults to zero, and requires explicit nonzero input. WSL2 guest allocations still consume shared physical host RAM, so host pressure is expected and guarded. |

## Validation Plan

- Static: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`.
- Manufactured PowerShell: `scripts/windows/Test-SharedWslPressureCampaignMemoryGate.ps1`
  covers below-plan refusal, exact boundary admission, invalid CIM refusal,
  physical/commit reserve breaches, and three-sample telemetry loss.
- Static PowerShell: `scripts/windows/Test-SharedWslPressureCampaignStatic.ps1`
  proves the guard is before disk telemetry and only the selected distro can be
  terminated; it forbids OOM allowances, broad shutdown/reboot, VM, and disk
  mutation routes.
- Guest admission: `scripts/safety/test-ramshared-guest-memory-admission.sh`
  covers exact-boundary admission, low-memory and low-swap refusal, malformed
  telemetry, and reserve floors.
- Three-tier static PowerShell: `scripts/windows/Test-RamSharedThreeTierStressStatic.ps1`
  proves host admission precedes WSL launch, guest admission precedes activation
  and stress, and the wrapper contains no Windows pressure allocator.
- Shared campaign static PowerShell:
  `scripts/windows/Test-SharedWslPressureCampaignStatic.ps1` proves guest
  admission and refusal happen before the cleanup trap, RamShared mutations,
  or pressure probe, and the optional Windows CUDA workload remains off by
  default.
- Guest runtime guard: `scripts/safety/test-guest-pressure-runtime-guard.sh`
  covers malformed/duplicate memory and PSI samples, exact reserve boundaries,
  limit parsing/overflow, finite dynamic cgroup budgets, and evaluator failure.
- Pressure-probe integration: `scripts/safety/test-cascade-pressure-probe-static.sh`
  proves admission precedes cgroup/worker creation, the worker is attached
  before allocation is released, each-second guest checks update finite memory
  and swap limits, direct invocation refuses, and owned cgroup state is cleaned
  up. `scripts/safety/Test-Wsl2FreezeCampaignStatic.sh` verifies the outer
  campaign supplies admission only after its gates.
- Hosted CI runs the shell syntax and pure fixture/static tests only; it does
  not run a cgroup worker or allocate pressure.
- Synthetic PASS/PARTIAL fixture runs.
> **Historical non-current / no execution:** The retained live-close artifact
> descriptions below are evidence only; do not invoke either path.
- Live close: validator PASS over a real isolated-lab artifact produced by
  `scripts/safety/wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated`,
  or a real shared-host artifact produced by
  `scripts/windows/Invoke-SharedWslPressureCampaign.ps1 -ApproveSharedDailyHost`.
