## Resumo
Refactors the monolith TCP `session` loop inside the `ramshared-agent` crate into a modular state machine named `SessionDispatcher`. This improves readability and encapsulates state mutations (such as PSI tracking, execution drains, message processing, and watchdog checks) into explicit methods without sacrificing zero-cost single-thread dispatch invariants.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| `HEAD` | Decomposed session loop into `SessionDispatcher` methods | To cleanly modularize the agent event dispatcher | <details><summary>detalhes</summary>**Arquivos:** `crates/ramshared-agent/src/main.rs`<br>**Validacao:** `cargo test -p ramshared-agent`<br>**Risco/rollback:** Regressao no event loop / perda de timeouts</details> |

## Issue
Closes #N/A

## Responsavel
@jules

## Labels
type:refactor, area:core

## Validacao
- [x] Gates de build/test do escopo tocado
- [x] `./scripts/docs-check.sh` (se tocou docs/specs ou gerou PRD/SPEC/IMPL)
- [x] SSDV3: SPEC/IMPL atualizados e citados (ou N/A — mudança não estrutural / só scripts)

## Rollback trigger
Revert if agent connection dropping loop regresses.
