# CLAUDE.md — RamShared

> **ATENÇÃO:** Mantenha este arquivo minúsculo. Todas as regras específicas do projeto foram movidas para [`.claude/rules/*.md`](.claude/rules/*.md). Não copie longos dossiers aqui.

## Agent Source Of Truth

[`.claude/rules/*.md`](.claude/rules/*.md) são os documentos autoritativos de regras de código. `AGENTS.md` (e `.cursor/rules/*`, `.windsurf/rules/*` se houver) espelham essas diretrizes.

Antes de alterar código:

1. Leia este arquivo e `MEMORY.md`.
2. Para módulos de kernel (LKM), HMM, Rust for Linux e CXL, leia [`.claude/rules/kernel.md`](.claude/rules/kernel.md).
3. Se envolver mudança estrutural, manipulação de locks, alocação crônica ou novo hardware, siga a metodologia **SSDV3** ([`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md) e [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md)).
4. Siga sempre [`.claude/rules/coding.md`](.claude/rules/coding.md) para formatação, checkpatch e testes.
5. Em Pull Requests, siga o formato de tabela de commits de [`.claude/rules/governance.md`](.claude/rules/governance.md).
6. Para benchmarks/medições que embasam decisão, siga [`.claude/rules/benchmarks.md`](.claude/rules/benchmarks.md) (contexto auto + ≥3 rodadas + log append-only em [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md)).

## Metodologias Core

- **Kahneman Disciplines**: Toda decisão arquitetural ou de lock/DMA deve seguir as 14 disciplinas de Kahneman ([`docs/methodology/KAHNEMAN-DISCIPLINES.md`](docs/methodology/KAHNEMAN-DISCIPLINES.md)). Evite decisões rápidas de "Sistema 1"; registre counterfactuals e triggers de reversão de patch explícitos.
- **SSDV3**: Spec-Driven Development. Pipeline: PRD → SPEC → IMPL. Veja [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md).

## Day-0 Policy

O RamShared exige que todo código enviado para o Ring 0 seja a versão definitiva para o Day-0. É proibido:
- Shims de compatibilidade que introduzam latência.
- Workarounds provisórios para contornar falhas de hardware ou coerência de cache.
- Módulos que ignoram os avisos do `checkpatch.pl`.

## Commits & Patches

- **Inglês** em todo código C/Rust, branches, Makefiles, e títulos de commit (Conventional Commits: `feat(scope): title`).
- **PT-BR** no corpo de commits, PRs, issues e documentos na raiz / `docs/`.
- Commits estruturais ou que afetem a MMU/DRM requerem um `Rollback trigger:` no body.

## Tech Stack Overview

- **Kernel Linux**: Desenvolvimento de LKM (Loadable Kernel Modules) focados em CXL, PCIe Gen5.
- **Linguagens**: C11 (Padrões do Kernel) e Rust for Linux.
- **Subsistemas**: HMM (Heterogeneous Memory Management), DRM (Direct Rendering Manager), MMU.
- **Validação**: kselftest, checkpatch.pl, sparse, lockdep, kmemleak.

Consulte os arquivos em `.claude/rules/` para as diretrizes profundas sobre cada tópico.
