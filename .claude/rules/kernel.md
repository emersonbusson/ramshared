# Regras Globais para LLMs no RamShared

Este documento define como IAs e Assistentes devem gerar código C/Rust para o projeto RamShared.

## Linguagem e Formatação
- **C Kernel Style:** Use rigorosamente o estilo padrão do Linux Kernel (indentação com TABs de 8 espaços, linhas de 80 caracteres máximo, brackets abertos na mesma linha de `if`/`for`).
- **Rust for Linux:** Se utilizar Rust, siga estritamente os padrões da `alloc` e `core`, evitando a `std`. Use o `rustfmt` com as configurações do mainline.
- Todo output de IA para arquivos `.c` ou `.rs` deve estar pronto para passar no script `checkpatch.pl` do kernel Linux.

## Semântica e Memória
- Nunca assuma que ponteiros virtuais podem ser passados diretamente para hardware. Mapeie e desmapeie (ex: `dma_map_single`, `pci_iomap`) explicitamente.
- Todo lock (spinlock, mutex) deve ter um escopo claro e justificado. Atenção extrema à inversão de prioridade e deadlocks em contexto de interrupção.
- Não deixe vazamentos de memória e libere recursos na exata ordem inversa da alocação nas rotinas de erro (`goto out_err;` idiom).

## Documentação e SPEC
- Nunca escreva implementação direta baseada no PRD. Use a metodologia SSDV3 (PRD -> SPEC -> IMPL).
- Os PRDs e SPECs devem ter disciplinas de mitigação de Kernel Panic documentadas usando o framework Kahneman.
