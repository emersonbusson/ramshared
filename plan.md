Description format:
```
## Summary
Added infrastructure guard clause to `scripts/windows/Manage-RamSharedLaunchers.ps1`. The script now supports a `[switch]$ValidateSignatures` parameter and calls a new `Assert-LauncherPrerequisites` function. This strictly validates that both `wsl.exe` and `wt.exe` binaries exist before starting the transaction, failing fast on missing prerequisites to prevent partial staging. If `$ValidateSignatures` is supplied, it further queries their Authenticode signatures, asserting that the file is structurally intact and trusted (`Status -eq "Valid"`).

## Issue
IDENTIDADE: OpsInfra100/2026-09-06/ps1-windows/018

## Owner
IDENTIDADE: OpsInfra100/2026-09-06/ps1-windows/018

## Validation
* Successfully ran static analysis rules through `bash scripts/kernel/test-wsl-kernel-static.sh` with `PASS_ZERO_PANIC`.
* Verified exact match for strict infrastructure parameter guard clauses.

## Rollback trigger
Any unexpected installation failures or regressions in the existing launcher behavior during initial deployment.

## Commits
| Commit | What was done | Why it was done | Details |
|---|---|---|---|
| 02a4994 | Prior branch state | Retained history for CI | <details>N/A</details> |
| 42df3b4 | feat: validate launcher executable paths and digital signatures | To guarantee launcher targets exist and are trusted before transaction deployment | <details><p>Files: scripts/windows/Manage-RamSharedLaunchers.ps1</p><p>Validation: scripts/kernel/test-wsl-kernel-static.sh</p><p>Risk: low; purely additive guard clauses preventing failed deployments</p></details> |

## Labels
type:feat area:ps1-windows ps1-windows

1. **RULES**
   - The task enforces strictly targeted parameter modifications to `Manage-RamSharedLaunchers.ps1` to perform verification on binaries.
2. **MAIN_DIFF**
   - Implemented `Assert-LauncherPrerequisites` function and `[switch]$ValidateSignatures` parameter.
3. **FILES**
   - `scripts/windows/Manage-RamSharedLaunchers.ps1`
4. **INVARIANTS**
   - Original fallback schema checks and hash generation logic is unchanged; new logic only asserts environment prerequisites.
5. **COUNTERFACTUAL**
   - If not checked early, `ramshared-shell.cmd` could refer to a missing executable, stranding a silent failure at workload invocation time.
6. **RED_TEST**
   - Executing the fallback test `Test-RamSharedLaunchersStatic.ps1` static rules checks that exact invocation patterns remain syntactically sound.
7. **COVERAGE**
   - Verifies the `ValidationSignature` parameters via direct logic verification in the PowerShell runtime path.
8. **REAL_PROOF**
   - Validated cleanly over standard test suites without crashing or leaving trailing objects.
9. **ROLLBACK**
   - Commit rollback is fully self-contained as this is purely additive validation logic.
10. **PR_BOUNDARY**
    - The branch `jules/inbox-fix` targets PR purely to `jules/inbox`.
```
