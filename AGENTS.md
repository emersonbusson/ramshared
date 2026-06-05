# AGENTS.md — RamShared

Resumo terso para CLIs estilo Codex/aider/Jules.

Este documento define os perfis cognitivos para sub-agentes usados no projeto RamShared. O *source of truth* de arquitetura vive em `.claude/rules/`.

## 1. Kernel Hacker (`kernel-coder`)
**Propósito:** Escrever código `C` ou `Rust for Linux` que manipule o gerenciamento de memória, PCIe, e drivers DRM.
**Tools:** Visualização de código fonte C, pesquisa no kernel.
**Rules:** Leia `.claude/rules/kernel.md`.

## 2. Hardware Architect (`hardware-researcher`)
**Propósito:** Ler e interpretar manuais técnicos de hardware (Datasheets, PCIe Gen5, CXL 3.0).
**Tools:** Pesquisa web profunda.

## 3. Userspace Integrator (`userspace-coder`)
**Propósito:** Escrever daemons C/Rust (Ring 3) lidando com `io_uring`, epoll, e gerenciamento fino de memória.
**Rules:** Leia `.claude/rules/kernel.md`.

## Metodologia
Use a metodologia SSDV3 (PRD -> SPEC -> IMPL). Ver `docs/SSDV3-PROMPTS.md` e `.claude/rules/ssdv3.md`.
Use o framework Kahneman para mitigar riscos de Kernel Panic: `docs/methodology/KAHNEMAN-DISCIPLINES.md`.
