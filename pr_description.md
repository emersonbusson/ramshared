## Resumo
Refactored the `print_text_report` function in `crates/ramshared-cli/src/main.rs` to improve maintainability and readability by splitting it into smaller helper functions.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| `hash` | Split `print_text_report` | Improve readability and maintainability | <details><summary>detalhes</summary>**Arquivos:** `crates/ramshared-cli/src/main.rs`<br>**Validacao:** `cargo check`, `cargo fmt`, `cargo test`<br>**Risco/rollback:** Low risk, pure refactoring.</details> |

## Issue
N/A - Code health improvement

## Responsavel
@jules

## Labels
type:refactor
area:cli

## Validacao
- [x] Gates de build/test do escopo tocado
- [ ] `./scripts/docs-check.sh` (se tocou docs/specs ou gerou PRD/SPEC/IMPL)
- [ ] SSDV3: SPEC/IMPL atualizados e citados (ou N/A — mudança não estrutural / só scripts)

## Rollback trigger
Nenhum - Refactoring only, functionally equivalent.
