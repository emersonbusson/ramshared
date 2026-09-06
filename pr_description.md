## Resumo
Refactor `AppArgs::parse_from` to use guard clauses, flattening the validation logic according to the Guard Clauses architectural principle.
## Commits
| Commit | O que fez | Por que fez | Detalhes |
| --- | --- | --- | --- |
| 7a5a2c1 | Refactor AppArgs::parse_from | Apply Guard Clauses principle | Flattened if statements |
## Issue
N/A
## Responsavel
Jules
## Labels
type:refactor
area:wsl2d
## Validacao
`cargo test -p ramshared-wsl2d && cargo clippy -p ramshared-wsl2d -- -D warnings`
## Rollback trigger
Test failures during CLI argument validation.
