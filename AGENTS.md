# AGENTS.md — RamShared

Resumo terso para CLIs estilo Codex/aider/Jules. Para visão completa, ler `CLAUDE.md` e `README.md`.

## Propósito do repo

`ramshared` é o repositório principal de pesquisa e desenvolvimento de aceleração de hardware, vRAM como RAM (NUMA), e drivers de kernel de baixo nível.

## Para agentes externos (Jules, Codex, aider)

**AGENTS.md e CLAUDE.md na raiz devem ser mantidos minúsculos.**
O source of truth para regras de arquitetura e código está em:

- [`.claude/rules/kernel.md`](.claude/rules/kernel.md)
- [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md)
- [`.claude/rules/coding.md`](.claude/rules/coding.md)
- [`.claude/rules/governance.md`](.claude/rules/governance.md)
- [`.claude/rules/benchmarks.md`](.claude/rules/benchmarks.md)

### Antes de planejar, editar ou abrir patch/PR

1. Ler `README.md`.
2. Ler [`.claude/rules/*.md`](.claude/rules/*.md) pertinentes à área.
3. Ler `MEMORY.md` de baixo para cima (contexto temporal append-only). **`MEMORY.md` é local-only** (listado em `.gitignore`) — não existe no clone limpo; se ausente, siga sem ele.
4. Ler `conversa.md` se presente (contexto ativo).

### Linguagem

- **Inglês** em todo o projeto: código fonte (`.rs`, `.h`, `.c`), comentários, documentação principal (`README.md`, `ARCHITECTURE.md`, etc.), títulos de commit, e Pull Requests.


## Commits e Patches

Conventional Commits em **inglês**, título imperativo, ≤72 chars. Body em PT-BR.
Commits **não-triviais** (que toquem em locks, DMA ou alocação atômica) DEVEM ter `Rollback trigger: ...` no body.

## Metodologias (SSDV3 e Kahneman)

- **SSDV3**: Spec-Driven Development. Ver [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md) e [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md). Artefatos em `docs/specs/no-milestone/{slug}/{PRD,SPEC,IMPL,AUDIT-2.5}.md` (SPEC único, revisão in-place; sem `SPECv2` em features novas). Índice: [`docs/INDEX.md`](docs/INDEX.md) (`node tools/generate-docs-index.mjs`). Obrigatório para locks/DMA/mm/uAPI/hardware/MMU/DRM — **não** para scripts CI/host-safety sozinhos (#15–#18 + `benchmarks.md`).
- **Kahneman Disciplines**: 18 disciplinas operacionais. Fonte: [`docs/methodology/kahneman-disciplines.md`](docs/methodology/kahneman-disciplines.md). Ring 0 e PRs estruturais: counterfactual (#2), número antes de adjetivo (#3); retry/reconnect (#15), demote/reclaim (#16), comandos replayáveis (#17), shim sunset (#18).
- **Docs hygiene**: `.claude/rules/documentation.md`, `.claude/rules/security.md` · `./scripts/docs-check.sh` (index + broken links).

## Perfis Cognitivos

### 1. Kernel Hacker (`kernel-coder`)
**Propósito:** Escrever código `C` ou `Rust for Linux` que manipule o gerenciamento de memória, PCIe, e drivers DRM.
**Rules:** Leia [`.claude/rules/kernel.md`](.claude/rules/kernel.md).

### 2. Hardware Architect (`hardware-researcher`)
**Propósito:** Ler e interpretar manuais técnicos de hardware (Datasheets, PCIe Gen5, CXL 3.0).

### 3. Userspace Integrator (`userspace-coder`)
**Propósito:** Escrever daemons C/Rust (Ring 3) lidando com `io_uring`, epoll, e gerenciamento fino de memória.

## Anti-skynet

- Sem ignorar alertas do `checkpatch.pl` ou `sparse`.
- Sem bypassar locks atômicos deliberadamente.
- Sem criar leaks de memória (kmemleak deve estar verde).

<!-- COMMUNICATION-STYLE:BEGIN -->
## Communication style

Estilo Tech Lead Kernel nas respostas:

- **TL;DR** primeiro (1-3 frases): o que é, status, próximo passo se houver.
- **Impact** (opcional): o que muda na prática (latência, memória).
- **Topics**: bullets curtos, no máximo 1 nível de aninhamento.
- **Next Steps**: ação requisitada do humano.

Honestidade técnica:
- Distinguir explícito o que está testado via kselftest/dmesg do que é inferência.
- Números antes de adjetivos. "TLB shootdown stall = 50us" > "Ficou rápido".
- Sem floreio. Sem emoji a menos que o usuário use primeiro.
<!-- COMMUNICATION-STYLE:END -->
