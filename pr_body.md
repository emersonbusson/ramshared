## Resumo
Added a guard clause to `scripts/windows/Manage-RamSharedOrigin.ps1` to ensure the `RamSharedWinSvc` service is strictly stopped before proceeding with `install` or `uninstall` transactions. This aligns with fail-fast principles, preventing file locks and consistency errors.
## Commits
| Commit | O que fez | Por que fez | Detalhes |
| 156355c4427b6e00321ebc68c7de9288dbf72e09 | Base | Base SHA | <details>N/A</details> |
| e5dc98fc75f7f74640707c81720a0f4c13ff75b4 | feat: guard clauses for service state transitions | Fail fast on invalid service state | <details>Added Get-Service validation prior to switch block.</details> |
## Issue
N/A
## Responsavel
@user
## Labels
type:scripts,area:infrastructure
## Validacao
pwsh -c "Invoke-ScriptAnalyzer -Path scripts/windows/Manage-RamSharedOrigin.ps1"
## Rollback trigger
If the guard clause improperly blocks valid uninstallation when the service is absent or legitimately stopped, revert the commit.
