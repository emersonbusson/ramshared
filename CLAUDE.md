# CLAUDE.md — RamShared

## Agent Source Of Truth

Para regras de codificação, arquitetura de memória, módulos do Kernel (LKM), PCIe e Rust for Linux, leia **`.claude/rules/kernel.md`**.

Suas prioridades em código são a segurança do barramento (prevenir stalls no PCIe) e a estabilidade da árvore de páginas do kernel.

## Metodologia SSDV3
Nunca escreva implementação direta baseada no PRD. Siga a metodologia SSDV3 descrita em `docs/SSDV3-PROMPTS.md` e `.claude/rules/ssdv3.md`.

## Disciplinas Kahneman
Toda mudança estrutural deve documentar mitigação de Kernel Panic ou corrupção de memória (Sistema 2) usando `docs/methodology/KAHNEMAN-DISCIPLINES.md`.
