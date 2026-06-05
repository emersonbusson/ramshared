---
name: governance
description: PR template, sync rule, e regra de visibilidade de commits.
paths:
  - .github/**
  - CLAUDE.md
  - AGENTS.md
  - .claude/rules/**
---

# Governance rules — RamShared

Estas regras existem para que toda PR (Patch/Pull Request) carregue contexto revisável e para que mudanças em regras de agente vivam sincronizadas entre `CLAUDE.md`, `AGENTS.md` e `.claude/rules/*`.

## Template de PR (formato canônico)

Todo PR usa `.github/pull_request_template.md`. Seções **obrigatórias**:

1. `## Resumo` — PT-BR, suficiente para alguém fora da conversa.
2. `## Commits` — tabela com `Commit | O que fez | Por que fez | Detalhes`. Cada linha tem hash + `<details>` clicável com contexto, impacto, arquivos, validação e risco/rollback. **Toda linha de commit é visível na tabela**, mesmo em PRs com 20+ commits — proibido envolver múltiplas linhas dentro de um `<details>` agrupador que esconda commits do preview inicial. Per-row `<details>` no campo `Detalhes` continua obrigatório e cumpre o papel de esconder o contexto profundo. Agrupamento por categoria editorial vai no `summary` da linha ou em texto curto no `Detalhes`, nunca em `<details>` que oculte commits.
3. `## Issue` — `Closes #NNN`, `Fixes #NNN` ou `Resolves #NNN`.
4. `## Responsavel` — `@usuario`. PR e issue linkada compartilham assignee.
5. `## Labels` — pelo menos uma `type:*` e uma `area:*` (ex: `area:mm`, `area:drm`).
6. `## Validacao` — checklist com gates relevantes (`checkpatch.pl`, `make modules`, `dmesg` limpo de OOPs, `kselftest`).
7. `## Rollback trigger` — condição numérica/observável que justifica reverter o patch do kernel (ex: stall > 1ms, kernel panic).

## Regra de visibilidade dos commits

**Por que existe:** um PR com 16 commits dobrados num `<details>` agrupador mostrava só 5 linhas no preview; o reviewer humano não viu os outros e perguntou onde estavam. A regra garante que isso não aconteça.

## Sync rule

Toda regra que muda aqui deve mudar em pelo menos 2 destes lugares no mesmo commit:

- `CLAUDE.md`
- `AGENTS.md`
- `.claude/rules/<topic>.md`
- `.github/pull_request_template.md`

Skip via `[sync-skip-justified]` no commit body com explicação.

## Don't

- ❌ Abrir PR sem preencher as 7 seções.
- ❌ Tabela de commits sem `<details>` per-row e sem hash.
- ❌ `<details>` agrupador escondendo múltiplas linhas de commits do preview do PR.
- ❌ Labels sem `type:*` e `area:*`.
- ❌ PR sem assignee compartilhado com a issue.
- ❌ Rollback trigger em forma de "se der errado, reverter" — precisa de número/janela observável no Kernel.
- ❌ Mudar `CLAUDE.md` sem sincronizar `AGENTS.md` no mesmo commit.
