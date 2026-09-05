<!--
PR Template — RamShared. All sections below are MANDATORY (.claude/rules/governance.md).
Commit visibility rule: EVERY commit line must be visible in the table.
Two-stage language rule: PT-BR is permitted during review/drafting; English is mandatory prior to final merge.
Sync rule: rules changing in governance must change in >=2 places in the same commit
(CLAUDE.md, AGENTS.md, .claude/rules/<topic>.md, .github/pull_request_template.md).
-->

## Resumo

<!--
Clear, human-readable summary explaining WHAT and WHY directly.
When touching performance, memory, or stress benchmarks, include a self-contained compact hardware comparison table
(Previous vs Current with physical hardware delta, directions [🔺/🔻], and tier tree ├─ Tier 1/2/3).
Tier 3 (Host SSD) qualification metrics are mandatory for performance PRs (CI blocks merge if omitted).
Prohibited: internal methodology buzzwords or external links to raw JSON files.
-->

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
