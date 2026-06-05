---
name: ssdv3
description: Quando SSDV3 (Spec-Driven Development) é obrigatório vs opcional.
paths:
  - docs/specs/**
  - docs/**
---

# SSDV3 rules

SSDV3 (Spec-Driven Development V3) é a metodologia de spec do `ramshared` (cf. `docs/`). Pipeline em 3 passos: **PRD → SPEC → IMPL**.

## Obrigatório

SSDV3 é **mandatório** para mudanças em:

1. **Locks / concorrência** — nova ordem de lock, troca spinlock↔mutex, RCU, ou barreiras de memória em hot path / contexto de IRQ.
2. **DMA / IOMMU / MMIO** — novo mapeamento DMA, `ioremap`, ReBAR ou coerência de cache PCIe.
3. **Memória (mm)** — semântica de NUMA node, HMM/`DEVICE_PRIVATE`, memory hotplug, ou alocação crônica em hot path.
4. **uAPI / ABI** — nova ioctl/sysfs/debugfs ou mudança de layout de struct exposta a user-space (irreversível após release).
5. **Novo hardware / subsistema** — suporte a novo device, integração DRM/TTM, CXL.
6. **MMU / DRM** — qualquer mudança estrutural que toque a MMU ou o driver DRM.

## Opcional

Tudo o mais é opcional. Exemplos onde SSDV3 é **overhead**:

- UI tweaks (ajuste de spacing, troca de cor, refactor de component sem mudar contrato).
- Refactors internos sem mudança de contrato público.
- Bugfixes localizados (regression test + fix).
- Atualização de docs.
- Atualização de deps (com entrada em `docs/LIBRARIES.md`).

## Pipeline

### PASSO 1 — PRD

Copie o prompt de `docs/SSDV3-PROMPTS.md` (PASSO 1 PRD), substitua placeholders, cole no chat.

Output: `docs/specs/<feature-slug>/PRD.md`. 14 seções fixas:

1. Resumo
2. Contexto técnico
3. Opção recomendada
4. Requisitos funcionais (RF)
5. Requisitos não-funcionais (RNF)
6. Fluxos
7. Modelo de dados
8. API / Interfaces
9. Dependências e riscos
10. Estratégia de implementação
11. Documentos a atualizar
12. Fora de escopo
13. Critérios de aceitação
14. Validação

Cada item marcado como:

- **Confirmado no codebase** — código existente foi lido
- **Confirmado em docs** — ADR/runbook lido
- **Inferência** — proposta sem confirmação direta (deve ser escassa)

### PASSO 2 — SPEC

Copie o prompt PASSO 2 SPEC, cole o PRD aprovado, gere `docs/specs/<feature-slug>/SPEC.md`.

SPEC traduz PRD em:

- Arquivos a criar/modificar (paths absolutos)
- Diffs SQL com placeholders
- Validações em handlers (request/response shape)
- Middleware chains
- Links para `disciplines/KAHNEMAN-DISCIPLINES.md` para os passos críticos

### PASSO 3 — IMPL

Implementa estritamente conforme SPEC. **Zero criatividade fora do escopo.** Se nova decisão arise → volta para SPEC, atualiza, depois implementa.

Output: `docs/specs/<feature-slug>/IMPL.md` documentando o que foi feito (commits, arquivos, decisões pequenas que não pediram nova ADR, métricas de validação).

## Regras duras

1. **Reuso antes de criação.** Antes de propor código novo, prove que uma API do kernel ou helper existente não atende. Reference: subsistemas (`mm/`, `drm/`, `lib/`), helpers do módulo, crates do workspace.
2. **Separação fato vs proposta.** Cada item do PRD é "Confirmado em codebase / Confirmado em docs / Inferência". Inferências precisam de validação em SPEC.
3. **Zero criatividade no IMPL.** Code segue SPEC. Nova decisão → SPEC update → re-aprovação.
4. **Rastreabilidade por requirement ID.** Cada commit cita `RF-3`, `RNF-2`, etc. PR de IMPL liga aos IDs cobertos.
5. **Kahneman discipline link.** Passos críticos no SPEC referenciam `disciplines/KAHNEMAN-DISCIPLINES.md` (ex.: para mudança de schema, link disciplina #2 counterfactual).

## Localização

- Prompts copiáveis: `docs/SSDV3-PROMPTS.md`.
- Artifacts: `docs/specs/<feature-slug>/{PRD,SPEC,IMPL}.md`. Slug em kebab-case inglês.
- Disciplinas Kahneman: `disciplines/KAHNEMAN-DISCIPLINES.md`.

## Como linkar SPEC ao código

Comentários Go/TS quando o código implementa requisito específico:

```go
// SPEC: docs/specs/auth-port/SPEC.md §RF-3 — Token versioning bump no logout-all
func (s *AuthService) LogoutAll(ctx context.Context, userID string) error {
    // ...
}
```

PR description cita SPEC + requirement IDs cobertos.

## Don't

- ❌ Pular PRD/SPEC para mudança estrutural em locks/DMA/uAPI/mm.
- ❌ "Vou só fazer um SPEC pequeno" — se a mudança cabe em SPEC sem PRD, ela provavelmente é opcional, não obrigatória; e se é obrigatória, PRD é passo 1.
- ❌ IMPL sem SPEC aprovado.
- ❌ "Inferência" no PRD em >30% dos itens — sinal de que a investigação foi rasa.
- ❌ Criar utilitário novo sem checar APIs do kernel (`lib/`, helpers do subsistema) ou os crates do workspace.
- ❌ Commit no IMPL que não rastreia para requirement ID.
