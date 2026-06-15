# VISION — RamShared Memory Broker: uma plataforma, N tiers

> Documento-guarda-chuva (2026-06-09), origem: "a melhor abordagem seria uma coisa só que
> resolvesse todos esses problemas — Windows, VM, WSL2". Une Fase B (`docs/ublk-backend/`),
> o árbitro (`docs/vram-arbiter/PRD.md`) e a Fase C (`docs/dcc-out-of-core/PRD.md`).

## A resposta honesta: uma plataforma, não um binário

Os três problemas são o **mesmo problema abstrato**: *um host físico (EMEDEV) com um pool fixo
(VRAM 6 GB + RAM), repartido estaticamente entre consumidores (WSL2, VM civm, apps Windows/DCC) —
e sempre há um consumidor sofrendo enquanto o recurso do vizinho está ocioso.*

O que **pode e deve ser um só** (a plataforma):
1. **Um árbitro** — um único cérebro que vê a pressão de todos os consumidores e decide quem ganha
   o quê. (A comparação de PSI + histerese do vram-arbiter, generalizada.)
2. **Um protocolo** — agente em cada ambiente reporta pressão (PSI no Linux, contadores de memória
   + NVML no Windows) e executa comandos (`swapon/swapoff`, `release/grow`).
3. **Um primitivo universal: o *lease* de VRAM com revogação.** Todo byte de VRAM que o tier de
   swap usa é emprestado e revogável — e a revogação **já existe**: é o DEMOTE da Fase B
   (canário §9/§9.4 + `swapoff`). O mesmo mecanismo que hoje protege contra eviction WDDM vira a
   moeda de troca entre consumidores.
4. **Uma camada CUDA** (`ramshared-cuda`, FFI por dlopen — portável p/ `nvcuda.dll`) e **uma
   disciplina** (telemetria, canário, SSDV3/Kahneman, gates numéricos).

O que **não pode ser um só** (física e SO, não escolha de design):
- O **mecanismo de entrega** por consumidor: kernel Linux quer block device (ublk local / NBD
  remoto — prontos); app DCC quer alocador/cache (interposer/managed — Fase C); Windows-como-
  consumidor-de-swap exigiria um driver de disco Windows (fora de escopo por ora).
- A **latência**: RAM (ns) ≫ PCIe (µs). Nenhum tier muda isso; o broker administra *capacidade*,
  não física.

## O cenário que mostra tudo funcionando junto

1. **Alex abre um render grande no Windows** → o addon (Fase C) pede VRAM ao broker → o broker
   **revoga o lease** do tier de swap (demove slices no WSL2/civm) → o render ganha a VRAM inteira
   + o tier RAM-backing do DCC.
2. **Render termina** → broker re-arrenda a VRAM ao tier de swap.
3. **CI roda na civm + build no WSL2** → árbitro move slices para quem tem PSI maior (vram-arbiter).
4. Tudo com o mesmo árbitro, o mesmo protocolo e o mesmo primitivo de revogação.

## Topologia

- **Cérebro (broker/árbitro):** no WSL2 primeiro (onde a stack vive; Day-0 Linux). Agentes:
  civm (Linux, trivial), Windows (Rust roda nativo; agente lê pressão + NVML e hospeda o tier DCC).
- **Evolução sem retrabalho:** Fase B (feita) → vram-arbiter (slices + agente + árbitro) → Fase C
  pluga como mais um *tenant* do mesmo broker (um tenant que **pede** VRAM em vez de oferecer).

## Sequência (cada passo útil sozinho)

1. **vram-arbiter F0-F3** (Linux↔Linux): constrói o broker real — protocolo, agente, árbitro,
   slices, lease/revogação. É o coração da plataforma.
2. **Fase C F0/MVP** (Windows, addon): mede com as cenas do Alex; o MVP nem precisa do broker
   (configura o out-of-core nativo do Cycles) — mas o **release-VRAM-antes-do-render** já pode
   falar com o broker via o agente Windows (primeira ponte Windows↔broker).
3. **Fase C v2** (interposer) e/ou **Windows-swap-driver**: só com gates numéricos.

## "Qualquer placa de vídeo com VRAM" — a estratégia cross-vendor

CUDA é NVIDIA-only. Para **qualquer GPU** (AMD/Intel/NVIDIA), a camada de GPU vira um **trait**
(`VramProvider`: alloc/free/read_at/write_at/budget) com backends por API — e a costura **já
existe**: o tier de swap só fala `BlockBackend` (alloc + memcpy), nada CUDA-específico.

| Backend | Cobre | Estado |
|---|---|---|
| **CUDA** (dlopen) | NVIDIA (Windows, WSL2, Linux) | **pronto** (Fase B) |
| **Vulkan** (`DEVICE_LOCAL` + `VK_EXT_memory_budget` + transfer queue) | AMD/Intel/NVIDIA em Windows e Linux nativos | próximo — destrava "qualquer placa" |
| **D3D12** (`/dev/dxg`) | Windows nativo; possível caminho p/ GPU não-NVIDIA dentro do WSL2 | pesquisa |

Honestidade de matriz: **WSL2 + não-NVIDIA é o caso fraco hoje** (Vulkan no WSL2 via Mesa/Dozen é
imaturo); WSL2+NVIDIA (CUDA) já funciona, e Windows/Linux nativos com qualquer placa ficam coberto
pelo Vulkan. O tier de swap não usa shader nenhum — só alloc + cópia — então o backend Vulkan é
pequeno e estável.

## Produto instalável — como seria

**Um workspace Rust → três artefatos:**

1. **`ramsharedd`** (o broker/daemon) — binário nativo por plataforma:
   - **Windows:** `ramshared-setup.exe` (instalador/winget) → serviço do Windows + CLI. Vulkan vem
     com o driver da GPU (zero dependência extra). É o cérebro quando o host é o desktop do artista.
   - **Linux/WSL2/civm:** binário único (CUDA via dlopen = sem dependência de build) + `.deb`/
     systemd unit. Transporte: **ublk onde o kernel tem** (`CONFIG_BLK_DEV_UBLK`, mainline ≥6.0;
     no WSL2 stock exige nosso kernel custom) e **NBD como fallback universal** (funciona em
     qualquer kernel; medimos: ~26% mais lento — aceitável como fallback).
2. **`ramshared-agent`** (fino) — roda em cada ambiente consumidor (civm, WSL2, Windows), reporta
   pressão (PSI/NVML) e executa swapon/swapoff/release. Mesmo protocolo em todo lugar.
3. **Addon Blender** (Python, Fase C) — fala com o broker local ("libera VRAM, vou renderizar").

Instalação-tipo do usuário final (caso Alex): roda o `ramshared-setup.exe` no Windows + instala o
addon no Blender. Caso dev (nós): `.deb` no WSL2 + agente na civm.

## Anti-visões (o que isto NÃO é)

- Não é "um binário que roda igual em todo lugar" — é um protocolo + árbitro com mecanismos nativos
  por ambiente.
- Não é promessa de "RAM virar VRAM rápida" (PCIe manda).
- Não substitui os PRDs por-tier: cada mecanismo segue seu SSDV3 (PRD→SPEC→IMPL) próprio.
