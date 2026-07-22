# SPEC - WSL2 freeze-elimination campaign evidence gate

## Closed Scope

In now:

- Read-only artifact validator.
- Static safety test.
- Synthetic complete/incomplete fixture validation.

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
| NFR-1 | ITEM-2 |

## Technical Decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Validator reads artifact files only. | Keeps daily-host validation safe. |
| DT-2 | PASS requires either isolated completion or approved shared-host completion with Windows watchdog evidence. | Prevents false DONE from dry-run baselines and unsupervised daily-host pressure. |
| DT-3 | Synthetic PASS only proves validator logic. | Environment-bound claim still needs a real isolated-lab or shared-host watchdog artifact. |
| DT-4 | PASS requires per-round memory integrity JSON. | A killed pressure process can leave before/after logs but no proof that the pressured data survived. |

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

**MODIFY — `docs/reliability/GAP-REGISTER.md`**

- Add validator path to required close evidence.

## Implementation Order

1. ITEM-1: implement read-only validator.
2. ITEM-2: implement static safety test.
3. ITEM-3: add supervised shared-host wrapper.
4. ITEM-4: run synthetic PASS/PARTIAL fixtures and static PowerShell tests.
5. ITEM-5: add per-round integrity artifact production and validation.
6. ITEM-6: update docs without closing live claim until a real artifact passes.
