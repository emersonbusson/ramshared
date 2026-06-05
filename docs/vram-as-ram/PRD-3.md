---
slug: vram-zswap-backend
title: VRAM como Backend para zswap/zpool
milestone: M01
issues: []
---

# PRD-3 — VRAM como Backend Customizado de Memória Comprimida (zswap/zpool)

## Resumo

Esta abordagem mescla a velocidade do espaço de kernel com a praticidade de uma solução focada em Swap. O Linux possui um recurso nativo chamado `zswap`, que intercepta páginas indo para o Swap no disco e tenta comprimi-las, guardando-as em um "pool" na própria RAM. A solução aqui é criar um módulo de kernel que forneça uma nova API de alocação de memória (zpool) para o `zswap`, onde os dados comprimidos são transferidos via DMA para a VRAM da GPU, e não mantidos na RAM da CPU.

## Contexto arquitetural e de Hardware

- **Subsistemas do Kernel:** Frontswap, Zswap, Zpool API, DMA (Direct Memory Access), PCI.
- **Hardware Alvo:** GPUs discretas acessíveis via barramento PCI.
- **Proposta:** Um módulo de kernel (`ramshared_zpool.ko`) que se registra como um allocator válido para o Zswap (assim como os já existentes `zbud`, `z3fold`, `zsmalloc`).

## Opção recomendada (Neste contexto)

**Implementar um zpool driver que aloca páginas MMIO da placa de vídeo via PCIe.**

- **Motivo da escolha:** É uma integração de kernel "limpa". Não tenta enganar o alocador principal (Buddy Allocator) dizendo que a VRAM é RAM pura (PRD-1), evitando kernel panics fáceis. Ele atua apenas quando a RAM enche (condição de Swap). Como os dados são comprimidos pela CPU antes de irem para a VRAM, economiza-se banda do PCIe.
- **Trade-offs:** O processador gasta ciclos comprimindo e descomprimindo dados antes de enviá-los para a placa de vídeo. 

## Requisitos funcionais

- **RF-1:** O módulo deve registrar um novo zpool driver chamado `vrampool`.
- **RF-2:** O sistema permitirá que o administrador configure o zswap para usá-lo via comando: `echo vrampool > /sys/module/zswap/parameters/zpool`.
- **RF-3:** O módulo deve mapear um bloco de VRAM via acesso PCI nativo (`pci_iomap`).
- **RF-4:** Quando o zswap pedir alocação, o módulo deve retornar um endereço virtual correspondente à memória física da GPU.

## Requisitos não-funcionais

- **Performance:** Redução drástica na utilização da banda PCIe, já que os dados viajam comprimidos (geralmente taxa de compressão de 2:1 a 3:1).
- **Estabilidade:** Se a VRAM falhar, o zswap tem um mecanismo de *fallback* automático onde ele desiste do pool e joga a página diretamente para o SSD real, aumentando a resiliência do sistema em relação ao PRD-1 e PRD-2.

## Fluxos de Memória

### Happy path
1. Sistema atinge pressão de memória.
2. Kernel tenta escrever página inativa na partição de Swap no SSD.
3. O `frontswap/zswap` intercepta a requisição.
4. A CPU comprime os 4KB da página para, por exemplo, 1.5KB.
5. Zswap solicita 1.5KB ao `vrampool` (nosso módulo).
6. O módulo usa DMA para jogar esses 1.5KB na VRAM da RTX 2060.
7. A requisição de disco é abortada, o SSD nem é tocado.

## Dependências e riscos
- **Riscos:** Depende de o driver da NVIDIA ou AMD permitir o mapeamento PCI livre por outro módulo do kernel enquanto eles operam. Caso o driver proprietário bloqueie acesso exclusivo à BAR, o módulo falhará no boot.

#### Portão Cognitivo de Kahneman (Obrigatório antes do SPEC)
- **Disciplina Aplicável:** Disciplina 1 (Segurança de Barramento e DMA).
- **Justificativa:** Para injetar dados comprimidos (zswap) na VRAM de forma assíncrona, faremos extensivo uso de DMA entre a RAM principal e a placa de vídeo. Configurar DMA incoerente irá corromper os blocos comprimidos.
- **Evidência exigida para o SPEC:** O SPEC deve demonstrar a assinatura da API de DMA mapping do kernel do Linux que será usada e o tratamento para `dma_mapping_error` caso o IOMMU rejeite o acesso PCI no momento do boot.
