# SPEC - WSL2 freeze-elimination campaign evidence gate

> **Disabled staging only / no execution:** This SPEC is source/static planning
> and historical evidence only. It authorizes no campaign, WSL lifecycle, VM,
> storage, swap, device, or pressure action; live qualification stays `PARTIAL`.

## Closed Scope

In now:

- Read-only artifact validator.
- Static safety test.
- Synthetic complete/incomplete fixture validation.
- Permanent Windows commit admission and runtime guardian for the approved
  shared-host harness, using manufactured tests only in this source slice.

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
| NFR-1 | ITEM-2 |
| NFR-2 | ITEM-6, ITEM-7 |

## Technical Decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Validator reads artifact files only. | Keeps daily-host validation safe. |
| DT-2 | PASS requires either isolated completion or approved shared-host completion with Windows watchdog evidence. | Prevents false DONE from dry-run baselines and unsupervised daily-host pressure. |
| DT-3 | Synthetic PASS only proves validator logic. | Environment-bound claim still needs a real isolated-lab or shared-host watchdog artifact. |
| DT-4 | PASS requires per-round memory integrity JSON. | A killed pressure process can leave before/after logs but no proof that the pressured data survived. |
| DT-5 | Admission uses three one-second `Win32_OperatingSystem` CIM snapshots and the lowest valid commit headroom. | Locale-neutral counters and a minimum sample prevent a transient high reading from admitting pressure. |
| DT-6 | The runtime guardian is a pure decision function plus a one-second harness loop. | Manufactured boundary/error/state tests cover every decision branch without a live-host bypass. |
| DT-7 | The guardian has one idempotent route: stop optional work and launcher, then exactly one selected-distro termination. | It contains a bad host state without broad WSL, reboot, VM, or disk action. |

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

- Purpose: collect locale-neutral host commit snapshots and expose pure
  admission/runtime decisions for the shared-host harness.
- Required tests: `scripts/windows/Test-SharedWslPressureCampaignMemoryGate.ps1`.
- Cover target: N/A — PowerShell campaign harness; all decision branches have
  named manufactured cases.

**CREATE — `scripts/windows/Test-SharedWslPressureCampaignMemoryGate.ps1`**

- Purpose: prove `host_memory_admission_refuses_below_plan_plus_reserve`,
  `host_memory_admission_passes_at_exact_boundary`,
  `host_memory_query_failure_refuses_before_wsl_launch`,
  `runtime_guard_trips_once_below_reserve`, and
  `telemetry_loss_trips_after_three_samples`.

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
