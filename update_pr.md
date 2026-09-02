## Resumo
Add guard clauses to ensure product services are stopped before mutating storage identity or origin manifests.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
| --- | --- | --- | --- |
| 1 | Adicionadas validações de estado para `RamSharedWinSvc` e `RamSharedBroker` durante operações de `install` e `uninstall`. | Prevenir corrupção de estado falhando rapidamente se os serviços do produto ainda estiverem ativos. | Modificado `scripts/windows/Manage-RamSharedOrigin.ps1` com verificações `-ErrorAction SilentlyContinue` e lançamentos de exceções. |

## Issue
N/A

## Responsavel
jules

## Labels
type:scripts, type:security, type:ci

## Validacao
`echo "Skipping PSScriptAnalyzer as pwsh is not installed"`

## Rollback trigger
Se as transações de instalação ou desinstalação começarem a falhar devido a exceções de estado inesperadas (e.g., validações pendentes ou travamentos na SCM) originadas do `$Action` switch block.
