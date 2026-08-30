git commit --amend -m "feat(mm): implement specific and semantic errors for priority weights

## Resumo
Introduces PriorityError enum to ramshared-tier to provide typed semantic
errors (InvalidWeight for -EINVAL equivalents and ThresholdOutOfRange for
-ERANGE equivalents) instead of loose generic strings, strictly enforcing the
architectural principle of specific domain-typed errors.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| HEAD | Added PriorityError, validate_weight, and validate_threshold | To replace loose errors with semantic typed ones (EINVAL, ERANGE) | <details><summary>detalhes</summary>**Arquivos:** crates/ramshared-tier/src/priority.rs<br>**Validacao:** cargo clippy, cargo test<br>**Risco/rollback:** Breaks priority errors</details> |

## Issue
Closes #000

## Responsavel
@jules

## Labels
type:refactor
type:test
area:mm

## Validacao
- [x] Gates de build/test do escopo tocado
- [ ] ./scripts/docs-check.sh
- [ ] SSDV3: SPEC/IMPL atualizados e citados

## Rollback trigger
If this breaks priority error handling for existing consumers of the library, rollback this PR."
