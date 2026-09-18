## Summary
Flattened connection loops using guard clauses

## Commits
| Commit | What was done | Why it was done | Details |
|---|---|---|---|
| 0e8e143 | Flatten connection loops | Guard clauses | <details><summary>detalhes</summary>**Arquivos:** conn.rs<br>**Validacao:** cargo test<br>**Risco/rollback:** connection issues</details> |

## Issue
Closes #1

## Responsavel
@jules

## Labels
type:refactor,area:wsl2d

## Validacao
- [x] Gates de build/test do escopo tocado
- [ ] `./scripts/docs-check.sh` (se tocou docs/specs ou gerou PRD/SPEC/IMPL)
- [ ] SSDV3: SPEC/IMPL atualizados e citados (ou N/A — mudança não estrutural / só scripts)

## Rollback trigger
connection issues
