# PRD — Backend Vulkan do `VramProvider` (RF-G2)

> SSDV3 PASSO 1. Camada(s): **Userspace** (novo crate `ramshared-vulkan` + shell no daemon). Sem uAPI
> de kernel/IRQ/DMA. **Mudança estrutural** (abstração de hardware / novo subsistema) → PRD obrigatório.

## Resumo

Implementar uma **2ª implementação** do trait `VramProvider`/`VramMemory` (hoje só CUDA) usando
**Vulkan**, destravando "**qualquer GPU**" (AMD/Intel/NVIDIA, Windows/Linux nativos) sem reescrever o
daemon — `run_nbd`/`run_broker`/`serve_ublk_residency` já são genéricos sobre `VramProvider` (RF-G1).
Valor: (a) tier de VRAM-as-swap em placas não-NVIDIA; (b) um **host Linux nativo com GPU** onde o
`ublk+VRAM` e o `eviction`-sob-carga (gaps env-bound do broker) finalmente rodam e2e — coisa
impossível no WSL2 (GPU presa no GPU-PV; daemon ublk inseguro) e no qemu (sem GPU).

## Contexto técnico

- **Confirmado no codebase:** o trait já existe e é consumido genericamente —
  `ramshared-vram::{VramProvider, VramMemory, VramError}` (`crates/ramshared-vram/src/lib.rs`):
  `VramProvider::alloc(bytes)->Mem<'_>` (GAT) + `mem_info()->(free,total)`; `VramMemory::{len, zero,
  read_at(off,&mut[]), write_at(off,&[])}`. A impl CUDA está em `ramshared-cuda/src/vram_impl.rs`
  (padrão a espelhar). O daemon é genérico: `run_nbd`/`run_broker`/`serve_ublk_residency` sobre
  `P: VramProvider` (`ramshared-wsl2d/src/main.rs`, `ublk_server.rs`).
- **Confirmado no codebase:** a afinidade de thread é exigida pelo trait (doc do `VramMemory`: "use na
  mesma thread que alocou"; o daemon roda todo I/O de VRAM numa thread só). O `VramBackend<M>`/
  `CanaryProbe<M>` não exigem `Send` na `Mem` — compatível com Vulkan (fila externamente sincronizada).
- **Confirmado em docs:** RF-G2 no PRD unificado (`docs/memory-broker/PRD.md` §4): "Backend Vulkan
  (`DEVICE_LOCAL` + `VK_EXT_memory_budget` + transfer queue)". `docs/vram-provider/SPEC.md` deixa o
  Vulkan explicitamente **fora** do RF-G1 (subsistema novo, este PRD).
- **Confirmado na documentação oficial (Vulkan):** (a) memória **`DEVICE_LOCAL`** (`VkDeviceMemory`
  num heap device-local) = a "VRAM" alocável; (b) **`VK_EXT_memory_budget`**
  (`VkPhysicalDeviceMemoryBudgetPropertiesEXT` via `vkGetPhysicalDeviceMemoryProperties2`) dá
  `heapBudget`/`heapUsage` → o `mem_info()` (free/total); (c) cópias host↔device via **staging buffer**
  `HOST_VISIBLE` + `vkCmdCopyBuffer` numa **transfer queue** + `VkFence` (equivalente do `cuMemcpy`
  síncrono); (d) `vkCmdFillBuffer`/copy-de-zeros para o `zero()` (wipe + sync, DT-17/§11).
- **Proposto (Inferência):** crate novo `ramshared-vulkan` com binding **`ash`** (FFI Vulkan
  battle-tested; reuso > hand-roll, regra dura #1) implementando os traits. O daemon ganha um **shell
  Vulkan** no `run()` (espelha o shell CUDA) que cria o provider e entra no caminho genérico.

## Opção recomendada

**Crate `ramshared-vulkan` via `ash`, implementando `VramProvider`/`VramMemory`, com staging buffer
único por provider (thread-afim) + transfer queue dedicada.** O daemon não muda no plano genérico; só
ganha o shell de bring-up.

- **Por quê:** o trait + o daemon genérico já existem (RF-G1); a Day-0 é só adicionar a 2ª impl. `ash`
  é o binding Vulkan padrão (seguro o suficiente, mantido), evitando reescrever loader/FFI à mão.
- **Alternativas descartadas:** (1) `vulkano` (alto nível) — esconde o controle de memória/filas que
  precisamos e adiciona peso; (2) hand-roll FFI do `libvulkan` (como o `ramshared-cuda` faz com CUDA)
  — possível, mas Vulkan tem superfície grande demais p/ hand-roll Day-0 (regra dura #1: reuso); (3)
  **D3D12/`/dev/dxg`** p/ não-NVIDIA dentro do WSL2 = RF-G3 (pesquisa, fora deste PRD).
- **Trade-offs aceitos:** staging buffer = 1 cópia extra host↔staging↔device (Vulkan não tem "unified
  copy" como `cuMemcpyHtoD` direto p/ DEVICE_LOCAL sem staging em GPUs discretas); aceitável (o
  data-plane já é cópia). `mem_info` via `VK_EXT_memory_budget` exige a extensão (presente em
  drivers modernos AMD/NVIDIA/Intel; se ausente → erro claro no `open()`).

## Requisitos funcionais

- **RF-V1 — `VulkanProvider::open(ordinal)`**: carrega instância Vulkan, escolhe o physical device
  `ordinal`, cria device lógico + **transfer queue** + staging buffer `HOST_VISIBLE|HOST_COHERENT`.
  - **Aceite:** `open(0)` num host com GPU Vulkan retorna `Ok`; sem GPU/sem `VK_EXT_memory_budget` →
    `Err(VramError::Provider(msg))` claro. Isolamento: só leitura/alloc na própria GPU; sem rede.
- **RF-V2 — `impl VramProvider for VulkanProvider`**: `alloc(bytes)` → `VkDeviceMemory` `DEVICE_LOCAL`
  (+ `VkBuffer` ligado) devolvido como `VulkanMem<'p>` (GAT, empresta `&provider`); `mem_info()` →
  `(budget-usage, budget)` do heap device-local via `VK_EXT_memory_budget`.
  - **Aceite:** `alloc(N)` reserva N bytes device-local (ou `Err` se faltar); `mem_info().1` ≈ VRAM
    total do heap; `.0` cai após `alloc` (mesma semântica do CUDA `cuMemGetInfo`).
- **RF-V3 — `impl VramMemory for VulkanMem`**: `read_at`/`write_at` via staging + `vkCmdCopyBuffer` na
  transfer queue + `VkFence` (síncrono); `zero()` wipe + sync; `len()`.
  - **Aceite:** round-trip `write_at`→`read_at` devolve os bytes escritos (igual ao teste
    `vram_backend_serves_nbd_write_then_read` do CUDA); `zero()` deixa a região zerada.
  - **Isolamento:** bounds-check de `off+len ≤ len()` (devolve `VramError::OutOfRange`, espelha CUDA).
- **RF-V4 — Shell Vulkan no daemon**: `run()` ganha um caminho (ex.: `--backend vulkan`) que cria o
  `VulkanProvider` e chama o **mesmo** `run_nbd`/`run_broker`/`run_ublk` genérico.
  - **Aceite:** `ramsharedd --backend vulkan --transport nbd ...` sobe e serve idêntico ao CUDA.

## Requisitos não-funcionais

- **Performance:** o staging copy não deve degradar o serve >2× vs CUDA no mesmo HW (medir; gate
  Kahneman #5). Reusar 1 staging buffer por provider (sem alloc no hot path, DT-8).
- **Segurança:** `#![forbid(unsafe_code)]` na fronteira do trait; o `unsafe` (FFI Vulkan via `ash`)
  isolado e com `// SAFETY:` em cada bloco; `cargo audit`/`deny` sobre o `ash`.
- **Observabilidade:** o gauge/telemetria já existentes (`VramGauge` via `mem_info`) funcionam sem
  mudança — o `vram_outros` passa a valer p/ Vulkan também.
- **Resiliência:** `open()` falha-claro sem GPU/extensão; erros de fila/fence viram `VramError`
  (nunca panic). Afinidade de thread documentada (fila externamente sincronizada).
- **Escalabilidade:** 1 provider por daemon (1 GPU); fora de escopo multi-GPU.

## Fluxos

**Happy path:** `run()` (`--backend vulkan`) → `VulkanProvider::open(0)` → `run_broker(provider, …)`
(genérico) → worker aloca VRAM via `provider.alloc`, serve NBD via `VramBackend<VulkanMem>`, residência
via `mem_info` (gauge). Idêntico ao CUDA daqui pra baixo.

**Alternativos:** `--backend vulkan --transport ublk` num host **não-WSL2** → `serve_ublk_residency`
genérico roda o ublk+VRAM e2e (o gap env-bound do ublk-vram).

**Erro:** sem GPU Vulkan / sem `VK_EXT_memory_budget` → `open()` devolve `Err` → daemon sai com
mensagem clara (não panica). Falha de alloc → `VramError::Provider`; OOB → `VramError::OutOfRange`.

## Modelo de dados

`VulkanProvider { entry, instance, phys, device, transfer_queue, staging: VkBuffer+VkDeviceMemory,
cmd_pool, fence, mem_props }` (thread-afim). `VulkanMem<'p> { provider: &'p VulkanProvider, buffer:
VkBuffer, memory: VkDeviceMemory, len }` (GAT, espelha `DeviceMem<'c,'a>`). Ciclo de vida: RAII —
`Drop` libera `VkDeviceMemory`/`VkBuffer` na ordem inversa; o provider destrói device/instance no fim.
Sem ABI exposta a user-space.

## API / Interfaces

Sem ioctl/sysfs novo. A "API" é o trait `VramProvider`/`VramMemory` (já existente) + a flag CLI
`--backend vulkan`. Crate novo `ramshared-vulkan` (dep `ash`); `ramshared-wsl2d` ganha a dep + o shell.
**Nenhuma uAPI de kernel.**

## Dependências e riscos

| Risco | Mitigação |
|---|---|
| `VK_EXT_memory_budget` ausente em driver velho | `open()` checa a extensão e falha-claro; é padrão em AMD/NVIDIA/Intel modernos |
| Staging copy mais lento que `cuMemcpy` (Kahneman #5) | medir vs CUDA; reusar staging; aceitar se <2× |
| Vulkan **dentro do WSL2** imaturo (ICD via dxg) | o alvo é **host nativo**; WSL2 fica no CUDA. Validar Vulkan em Linux bare-metal (matriz honesta do doc-pai R5) |
| `ash`/FFI `unsafe` | isolar + `// SAFETY:` + `cargo audit`/`deny`; trait sem `unsafe` |
| Afinidade de thread (fila Vulkan) | documentar; o daemon já roda I/O de VRAM numa thread só (RF-G1) |

**Sem breaking change** (2ª impl aditiva). **Rollout:** atrás de `--backend vulkan` (CUDA continua
default). **Rollback:** remover a flag/crate; o CUDA é o caminho de produção atual.

## Estratégia de implementação

1. Crate `ramshared-vulkan` + `VulkanProvider::open` (instância/device/queue/staging) — RF-V1.
2. `impl VramProvider` (alloc/mem_info) — RF-V2.
3. `impl VramMemory` (read_at/write_at/zero via staging+fence) — RF-V3 + teste round-trip (`--ignored`,
   host Vulkan), espelhando `vram_backend_serves_nbd_write_then_read`.
4. Shell `--backend vulkan` no daemon — RF-V4.
5. Validação e2e num host Linux nativo com GPU: smoke server-only + (destrava) ublk+VRAM + eviction.

## Fora de escopo

- **Multi-GPU**; **D3D12/`/dev/dxg`** (RF-G3, pesquisa); compute Vulkan (só transfer/alloc — "o tier
  não usa shader", doc-pai §3); otimização de latência abaixo do CUDA. Validação dentro do WSL2 (alvo
  = host nativo).
