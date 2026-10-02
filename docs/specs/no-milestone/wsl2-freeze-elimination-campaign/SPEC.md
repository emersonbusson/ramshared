# SPEC - WSL2 freeze-elimination campaign evidence gate

> **Disabled staging only / no execution:** This SPEC is source/static planning
> and historical evidence only. It authorizes no campaign, WSL lifecycle, VM,
> storage, swap, device, or pressure action; live qualification stays `PARTIAL`.

## Closed Scope

In now:

- Read-only artifact validator.
- Static safety test.
- Synthetic complete/incomplete fixture validation.
- Windows physical-memory and commit admission/runtime guards for the approved
  shared-host harness, using manufactured tests only in this source slice.
- WSL2 guest `MemAvailable` and `SwapFree` admission before any RamShared
  mutation in either the shared pressure campaign or three-tier wrapper; the
  Windows supervisor only observes and contains the guest.
- Guest memory, swap, and PSI runtime guards inside the bounded freeze probe;
  cgroup memory and swap limits are finite, recalculated once per second, and
  the worker cannot allocate before cgroup attachment.
- Standalone probe runs refuse unless the enclosing campaign passes its
  isolated-lab or approved shared-host gates and supplies the admission marker.
- WSL2/cascade Rust stress requires valid PSI before allocation and rechecks it
  during ramp, recovery waits, and hold; invalid `min_free_kbytes` telemetry
  cannot lower the documented physical-memory floor.
- The Linux CI workflow runs the pure guest-guard and probe-ordering fixtures;
  no live cgroup or pressure path is part of CI.

Out now:

- Running pressure directly without a watchdog harness.
- Creating or configuring a WSL2 isolated lab.
- Claiming WSL2 freeze elimination from synthetic fixtures.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-1 |
| RF-2 | ITEM-1, ITEM-2 |
| RF-3 | ITEM-1 |
| RF-4 | ITEM-1, ITEM-5 |
| RF-5 | ITEM-6 |
| RF-6 | ITEM-6 |
| RF-7 | ITEM-8, ITEM-9 |
| NFR-1 | ITEM-2 |
| NFR-2 | ITEM-6, ITEM-7 |
| NFR-3 | ITEM-8, ITEM-9 |

## Technical Decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Validator reads artifact files only. | Keeps daily-host validation safe. |
| DT-2 | PASS requires either isolated completion or approved shared-host completion with Windows watchdog evidence. | Prevents false DONE from dry-run baselines and unsupervised daily-host pressure. |
| DT-3 | Synthetic PASS only proves validator logic. | Environment-bound claim still needs a real isolated-lab or shared-host watchdog artifact. |
| DT-4 | PASS requires per-round memory integrity JSON. | A killed pressure process can leave before/after logs but no proof that the pressured data survived. |
| DT-5 | Admission uses three one-second `GetPerformanceInfo` snapshots and the lowest valid physical and commit headroom; commit headroom is `CommitLimit - CommitTotal`. | One native snapshot supplies both counters, avoids the WMI virtual-memory approximation, and minimum samples prevent a transient high reading from admitting pressure. |
| DT-6 | The runtime guardian is a pure decision function plus a one-second harness loop. | Manufactured boundary/error/state tests cover every decision branch without a live-host bypass. |
| DT-7 | The guardian has one idempotent route: stop optional work and launcher, then exactly one selected-distro termination. | It contains a bad host state without broad WSL, reboot, VM, or disk action. |
| DT-8 | Both the shared pressure campaign and three-tier wrapper run their RAM allocators only in the WSL2 guest and check guest `MemAvailable` plus `SwapFree` before any RamShared mutation. The separate Windows CUDA VRAM workload remains opt-in with a zero default. | WSL2 shares host physical RAM; the host supervisor must reserve and watch Windows memory while avoiding a Windows-side RAM pressure allocator. |
| DT-9 | WSL2/cascade Rust stress fails closed on unavailable PSI and missing `min_free_kbytes`; the freeze-probe worker runs in a unique cgroup with finite memory and swap caps derived from guest headroom after 600 MiB `MemAvailable` and 1024 MiB `SwapFree` reserves. Fresh PSI/memory/swap samples and cgroup usage recalculate both caps each second; missing inputs, failed writes, or probe PSI full avg10 >=10% stop the worker. A start gate keeps the allocator blocked until its process is attached. The probe refuses standalone runs; only the outer campaign passes its marker after the selected lab/shared-host gates succeed. | A fabricated zero-pressure sample or reserve weakens the Rust stress guard; entry checks alone do not contain shell pressure after the worker starts; fixed/unbounded cgroup limits, direct unsupervised entry, and process attachment races can exhaust guest memory or swap. |

## Files To Create / Modify

**CREATE — `scripts/safety/validate-wsl2-freeze-campaign-artifact.sh`**

- Purpose: validate complete isolated-lab or approved shared-host campaign artifacts.
- Required tests: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`
  plus synthetic fixture PASS/PARTIAL runs.
- Cover target: N/A — shell evidence validator.

**CREATE — `scripts/safety/cascade_pressure_integrity_worker.py`**

- Purpose: hold deterministic pressure memory and emit a JSON checksum result
  during cleanup.
- Required tests: `scripts/safety/Test-CascadePressureIntegrityWorker.sh`.
- Cover target: N/A — campaign helper with executable contract test.

**CREATE — `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`**

- Purpose: static guard that validator is read-only and checks required tokens.
- Required tests: itself.
- Cover target: N/A — static shell test.

**CREATE — `scripts/windows/Invoke-SharedWslPressureCampaign.ps1`**

- Purpose: run the real shared WSL2 campaign under a Windows-side watchdog.
- Required tests: `scripts/windows/Test-SharedWslPressureCampaignStatic.ps1`.
- Cover target: N/A — Windows harness.

**CREATE — `scripts/windows/Test-SharedWslPressureCampaignStatic.ps1`**

- Purpose: prove the shared-host harness requires approval/watchdog tokens and
  does not contain disk/VM mutation commands.
- Cover target: N/A — static PowerShell test.

**CREATE — `scripts/windows/SharedWslHostMemoryGate.psm1`**

- Purpose: collect native `GetPerformanceInfo` snapshots for physical
  availability and exact commit headroom, then expose pure admission/runtime
  decisions for the shared-host harness.
- Required tests: `scripts/windows/Test-SharedWslPressureCampaignMemoryGate.ps1`.
- Cover target: N/A — PowerShell campaign harness; all decision branches have
  named manufactured cases.

**CREATE — `scripts/windows/Test-SharedWslPressureCampaignMemoryGate.ps1`**

- Purpose: prove `host_memory_admission_refuses_below_plan_plus_reserve`,
  `host_memory_admission_passes_at_exact_boundary`,
  `host_memory_query_failure_refuses_before_wsl_launch`,
  `runtime_guard_trips_once_below_reserve`, and
  `telemetry_loss_trips_after_three_samples`.

**MODIFY — `scripts/windows/Invoke-RamSharedThreeTierStress.ps1`**

- Purpose: require Windows physical/commit headroom and a fresh guest
  `MemAvailable`/`SwapFree` sample before activating RamShared; keep the
  planned allocation in the guest stress command.
- Required tests: `scripts/windows/Test-RamSharedThreeTierStressStatic.ps1`
  and `scripts/windows/Test-SharedWslPressureCampaignMemoryGate.ps1`.

**CREATE — `scripts/safety/ramshared-guest-memory-admission.sh`**

- Purpose: fail closed on missing or insufficient guest memory/swap telemetry
  before `ramshared up`.
- Required tests: `scripts/safety/test-ramshared-guest-memory-admission.sh`.

**MODIFY — `scripts/safety/cascade-pressure-probe.sh`,
`scripts/safety/wsl2-freeze-campaign.sh`, and
`scripts/safety/Test-Wsl2FreezeCampaignStatic.sh`; CREATE —
`scripts/safety/guest-pressure-runtime-guard.sh`**

- Purpose: fail closed on invalid guest pressure telemetry and dynamically
  clamp the owned cgroup's finite memory/swap limits while the worker runs.
- Required tests: `scripts/safety/test-guest-pressure-runtime-guard.sh` and
  `scripts/safety/test-cascade-pressure-probe-static.sh`, plus
  `scripts/safety/Test-Wsl2FreezeCampaignStatic.sh`.

**MODIFY — `crates/ramshared-cli/src/stress.rs` and
`crates/ramshared-cli/src/supervisor.rs`**

- Purpose: require trustworthy PSI during WSL2/cascade stress and preserve the
  minimum-memory floor when the Linux reserve sysctl is missing or malformed.
- Required tests: `stress::tests::parse_psi_full_avg10_fails_closed_on_missing_or_invalid_samples`,
  `stress::tests::required_stress_psi_does_not_turn_missing_telemetry_into_zero_pressure`,
  `stress::tests::missing_min_free_sysctl_does_not_lower_the_physical_memory_floor`,
  and `supervisor::tests::sample_parsers_and_atomic_publication_are_bounded`.

**CREATE — `scripts/windows/Test-RamSharedThreeTierStressStatic.ps1`**

- Purpose: prove plan mode cannot launch WSL pressure, host admission precedes
  launch, guest admission precedes activation/stress, and Windows does not
  contain the three-tier pressure allocator.

**MODIFY — `scripts/windows/Invoke-SharedWslPressureCampaign.ps1` and
`scripts/windows/Test-SharedWslPressureCampaignStatic.ps1`**

- Purpose: require the guest memory/swap gate before the cleanup trap, any
  `ramshared down/up`, and the bounded pressure probe; prove the optional
  Windows CUDA VRAM workload is disabled by default.
- Required tests: the campaign static test and
  `scripts/safety/test-ramshared-guest-memory-admission.sh`.

**MODIFY — `docs/reliability/GAP-REGISTER.md`**

- Add validator path to required close evidence.

## Implementation Order

1. ITEM-1: implement read-only validator.
2. ITEM-2: implement static safety test.
3. ITEM-3: add supervised shared-host wrapper.
4. ITEM-4: run synthetic PASS/PARTIAL fixtures and static PowerShell tests.
5. ITEM-5: add per-round integrity artifact production and validation.
6. ITEM-6: update docs without closing live claim until a real artifact passes.
7. ITEM-7: add host commit admission and one-shot runtime guardian; retain
   source-only PARTIAL until a separately approved attended campaign provides
   before/action/after evidence.
8. ITEM-8: add guest memory/swap admission to both shared-host campaign paths
   and prove their RAM allocators run inside WSL2, with no planned Windows-side
   RAM allocation.
9. ITEM-9: keep the bounded guest probe inside runtime memory, swap, and PSI
   reserves; refuse direct probe invocation, pass admission only after outer
   campaign gates, validate manufactured boundaries and worker ordering, and
   retain PARTIAL until supervised live evidence exists.
