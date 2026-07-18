# SPEC - WSL2 freeze-elimination campaign evidence gate

## Closed Scope

In now:

- Read-only artifact validator.
- Static safety test.
- Synthetic complete/incomplete fixture validation.

Out now:

- Running isolated pressure.
- Creating or configuring a WSL2 isolated lab.
- Claiming WSL2 freeze elimination from synthetic fixtures.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-1 |
| RF-2 | ITEM-1, ITEM-2 |
| RF-3 | ITEM-1 |
| NFR-1 | ITEM-2 |

## Technical Decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Validator reads artifact files only. | Keeps daily-host validation safe. |
| DT-2 | PASS requires exactly the evidence class named in the gap register. | Prevents false DONE from dry-run baselines. |
| DT-3 | Synthetic PASS only proves validator logic. | Environment-bound claim still needs real isolated-lab artifact. |

## Files To Create / Modify

**CREATE — `scripts/safety/validate-wsl2-freeze-campaign-artifact.sh`**

- Purpose: validate complete isolated-lab campaign artifacts.
- Required tests: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`
  plus synthetic fixture PASS/PARTIAL runs.
- Cover target: N/A — shell evidence validator.

**CREATE — `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`**

- Purpose: static guard that validator is read-only and checks required tokens.
- Required tests: itself.
- Cover target: N/A — static shell test.

**MODIFY — `docs/reliability/GAP-REGISTER.md`**

- Add validator path to required close evidence.

## Implementation Order

1. ITEM-1: implement read-only validator.
2. ITEM-2: implement static safety test.
3. ITEM-3: run synthetic PASS/PARTIAL fixtures.
4. ITEM-4: update docs without closing live claim.

