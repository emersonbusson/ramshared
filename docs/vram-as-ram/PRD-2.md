---
slug: vram-userspace-block-swap
title: VRAM como Block Device em Userspace (ublk) para Swap
milestone: M01
issues: []
---

# PRD-2 — VRAM como Swap via Userspace Block Device (ublk)

## Resumo

Este PRD propõe uma solução mais pragmática e segura para utilizar VRAM como RAM do sistema. Em vez de hackear o gerenciador de memória do kernel, usaremos o *userspace* para alocar uma grande porção da VRAM (via OpenCL ou CUDA) e expor essa memória de volta para o kernel como um disco virtual ultra-rápido usando o subsistema `ublk` (Userspace Block Device). O sistema operacional então formata esse disco virtual como **Swap**.

## Contexto arquitetural e de Hardware

- **Subsistemas do Kernel:** `ublk` (Linux 6.0+), Block Layer, Swap Subsystem.
- **Hardware Alvo:** Qualquer GPU suportada por OpenCL 1.2+ ou CUDA (compatível com NVIDIA proprietário).
- **Proposta:** Um daemon escrito em Rust ou C (`ramshared-daemon`) que roda em userspace. Ele aloca VRAM via OpenCL. Quando o kernel tenta ler/escrever no disco de swap virtual `/dev/ublkb0`, o daemon intercepta o pedido e lê/escreve na VRAM.

## Opção recomendada (Neste contexto)

**Utilizar a API `ublk` com backend em OpenCL para máxima compatibilidade cruzada (AMD/NVIDIA/Intel).**

- **Motivo da escolha:** O `ublk` (comparado ao antigo NBD - Network Block Device) tem performance nativa quase igual a um disco físico, pois usa o framework `io_uring` para zerar a latência de troca de contexto entre kernel e userspace. Além disso, usar OpenCL garante que funcionará na RTX 2060 mesmo com drivers fechados.
- **Trade-offs:** Os dados não estão na "RAM" diretamente. Quando o sistema precisa da memória, ele faz o *page out* para o dispositivo de bloco. Isso envolve o overhead do Block Layer do Linux (montagem de BIOs, filas de I/O), o que é mais lento que um acesso direto à memória (Nó NUMA).

## Requisitos funcionais

- **RF-1:** O daemon deve alocar de 1GB a N GBs na VRAM sem travar a interface gráfica.
- **RF-2:** O daemon deve implementar os workers do `ublk` via `io_uring` para processar comandos de leitura/escrita do kernel.
- **RF-3:** O sistema deve fornecer um script auxiliar para montar o `/dev/ublkb0` com `mkswap` e `swapon` com a prioridade mais alta possível (`pri=32767`).

## Requisitos não-funcionais

- **Performance:** O throughput de leitura/escrita deve saturar o barramento PCIe (ex: atingir 10-15 GB/s em PCIe 3.0/4.0), superando a velocidade de SSDs NVMe topo de linha.
- **Estabilidade:** Evitar Deadlocks de Memória. Como o próprio daemon precisa de RAM para rodar, ele deve usar `mlockall()` para impedir que o kernel tente enviar as páginas do próprio daemon de VRAM para a área de swap da VRAM.

## Fluxos de Memória

### Happy path (Swap Out para VRAM)
1. Memória RAM primária enche.
2. Kernel decide evictar páginas inativas e escolhe o Swap de maior prioridade (`/dev/ublkb0`).
3. Kernel envia o pedido de escrita para a fila do `ublk`.
4. O `ramshared-daemon` lê a requisição via `io_uring`.
5. O daemon copia os dados da RAM para a VRAM usando `clEnqueueWriteBuffer` (OpenCL).
6. O daemon avisa o kernel que a escrita terminou.

### Fluxos de erro
1. **GPU Driver Crash:** Se o driver travar, o OpenCL retorna erro. O daemon deve sinalizar erro de I/O para o block layer. O kernel tentará matar os processos cujas páginas estavam no Swap corrompido (segfault).

## Dependências e riscos
- Depende fortemente de `io_uring` e kernels recentes (6.0+).
- **Risco:** Falta de suporte a "GPU First". Ao contrário do PRD-1, é difícil devolver a memória instantaneamente para um jogo. O usuário teria que rodar `swapoff` antes de jogar, o que demora (pois copia tudo de volta pra RAM/SSD).

#### Portão Cognitivo de Kahneman (Obrigatório antes do SPEC)
- **Disciplina Aplicável:** Disciplina 3 (Prevenção de Deadlock em Memória).
- **Justificativa:** O `ublk` precisa rodar para processar o swap do kernel. Se o kernel tentar fazer swap das páginas do próprio `ublk`, o processo travará para sempre esperando que ele mesmo escreva a própria memória no disco (OOM Deadlock fatal).
- **Evidência exigida para o SPEC:** O SPEC deve definir explicitamente a invocação de `mlockall(MCL_CURRENT | MCL_FUTURE)` antes de iniciar qualquer fila `io_uring` no userspace.
