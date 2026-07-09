<!--
Template de PR — RamShared. As 7 secoes abaixo sao OBRIGATORIAS (.claude/rules/governance.md).
Regra de visibilidade: TODA linha de commit fica visivel na tabela. Use <details> per-row
no campo "Detalhes" — nunca um <details> agrupador que esconda commits do preview do PR.
Sync rule: regra que mudar na governanca deve mudar em >=2 lugares no mesmo commit
(CLAUDE.md, AGENTS.md, .claude/rules/<topic>.md, .github/pull_request_template.md).
-->

## Resumo

<!-- PT-BR, suficiente para alguem fora da conversa entender o QUE e o PORQUE. -->

## Commits

<!-- Toda linha de commit visivel. <details> per-row obrigatorio no campo Detalhes. -->

| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| `hash` | ... | ... | <details><summary>detalhes</summary>**Arquivos:** ...<br>**Validacao:** ...<br>**Risco/rollback:** ...</details> |

## Issue

<!-- Closes #NNN | Fixes #NNN | Resolves #NNN. Crie a issue ANTES do PR. -->
Closes #

## Responsavel

<!-- @usuario. PR e issue linkada compartilham o assignee. -->
@

## Labels

<!-- Pelo menos uma type:* e uma area:* (ex.: type:feat, area:mm, area:drm, area:core). -->

## Validacao

<!--
Gates relevantes ao que mudou:
- Codigo C de LKM: ./scripts/checkpatch.pl -f, make modules, dmesg sem OOPs, kselftest.
- Rust userspace: cargo fmt --all --check, cargo clippy --workspace -D warnings, cargo test.
- Docs: ./scripts/docs-check.sh se tocou docs/specs ou INDEX.
- SSDV3: se mudança estrutural, path em docs/specs/… e IDs RF/NFR no body.
-->
- [ ] Gates de build/test do escopo tocado
- [ ] `./scripts/docs-check.sh` (se tocou docs/specs ou gerou PRD/SPEC/IMPL)
- [ ] SSDV3: SPEC/IMPL atualizados e citados (ou N/A — mudança não estrutural / só scripts)

## Rollback trigger

<!--
Condicao NUMERICA/observavel que justifica reverter o patch (ex.: stall > 1ms, kernel panic,
latencia > Nx baseline por M amostras). Proibido "se der errado, reverter".
-->
