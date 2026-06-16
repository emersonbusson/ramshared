# SPEC — Backend Vulkan do `VramProvider` (RF-G2)

> SSDV3 PASSO 2, a partir de [`PRD.md`](PRD.md). Userspace (novo crate `ramshared-vulkan` + shell no
> daemon). 2ª impl do trait `VramProvider`/`VramMemory` (a 1ª, CUDA, fica intacta). **Sem uAPI de
> kernel.** Liga-se ao RF-G1 (`docs/vram-provider/SPEC.md`).

## Escopo fechado desta implementação

**Entra:** crate `ramshared-vulkan` com `VulkanProvider`/`VulkanMem` via `ash` (RF-V1/V2/V3); shell
`--backend vulkan` no daemon (RF-V4); teste round-trip `#[ignore]` (host Vulkan).
**Fica fora:** multi-GPU; D3D12/`/dev/dxg` (RF-G3); compute/shaders (só alloc+transfer); validação
dentro do WSL2 (Vulkan-em-WSL2 é imaturo — alvo é host Linux nativo).
**Dependências prontas:** trait `ramshared-vram::{VramProvider, VramMemory, VramError}` + daemon
genérico (`run_nbd`/`run_broker`/`serve_ublk_residency` sobre `P: VramProvider`, RF-G1).

## Matriz de rastreabilidade PRD → SPEC

| PRD  | Implementação no SPEC |
| ---- | --------------------- |
| RF-V1 (`open`) | ITEM-1 (`VulkanProvider::open`) |
| RF-V2 (`VramProvider`) | ITEM-2 (`alloc`/`mem_info`) |
| RF-V3 (`VramMemory`) | ITEM-3 (`read_at`/`write_at`/`zero`/`len`) |
| RF-V4 (shell daemon) | ITEM-4 (`--backend vulkan`) |

## Decisões técnicas

| #    | Decisão | Justificativa |
| ---- | ------- | ------------- |
| DT-1 | Binding **`ash`** (não `vulkano`, não hand-roll FFI). | `ash` é o binding Vulkan battle-tested e fino (regra dura #1: reuso). `vulkano` esconde o controle de memória/fila; hand-roll de Vulkan é superfície grande demais p/ Day-0. |
| DT-2 | `VulkanMem<'p>` empresta `&'p VulkanProvider` (GAT, igual `DeviceMem<'c,'a>` do CUDA). | Mesma semântica thread-afim do RF-G1; sem `Arc`/refcount no hot path (espelha DT-V2 do VramProvider). |
| DT-3 | **1 staging buffer `HOST_VISIBLE|HOST_COHERENT` por provider** + 1 transfer queue + 1 `VkFence`, reusados. | Sem alloc no hot path (DT-8); GPUs discretas exigem staging p/ tocar `DEVICE_LOCAL`. `HOST_COHERENT` evita flush/invalidate manual. |
| DT-4 | `mem_info` via **`VK_EXT_memory_budget`** (`heapBudget`-`heapUsage`, `heapBudget`) do heap `DEVICE_LOCAL`. **REVISTO pelo DT-10:** a extensão é opcional + tem fallback (não falha-claro). | Equivalente do `cuMemGetInfo`; extensão padrão em AMD/NVIDIA/Intel modernos. |
| DT-5 | `read_at`/`write_at`/`zero` são **síncronos** (submit + `vkWaitForFences`), iguais ao `cuMemcpy`/`cuMemsetD8`+`cuCtxSynchronize`. | O `BlockBackend::flush` é no-op porque a escrita é durável no ack (H1/DT-10); manter a sincronicidade do CUDA. |
| DT-6 | `zero()` = `vkCmdFillBuffer(0)` no buffer `DEVICE_LOCAL` (não staging). | `vkCmdFillBuffer` zera direto na VRAM (wipe+sync, DT-17/§11), sem cópia de zeros via staging. |
| DT-7 | **Afinidade de thread:** o provider (device/queue/cmd-pool/staging) é criado e usado **na mesma thread** (o worker único do daemon). Filas Vulkan são externamente sincronizadas. | Igual à afinidade CUDA (RF-G1); o trait não exige `Send` na `Mem`. |
| DT-8 (revisto) | **A LÓGICA valida no lavapipe (Vulkan em CPU) AQUI** (round-trip `#[ignore]`, rodado com `--ignored`); só **perf / VRAM-real / eviction** precisam de host NVIDIA-Vulkan nativo (a RTX 2060 **não** tem ICD Vulkan no WSL2 — verificado por `vulkaninfo`). | Testar no nível mais seguro que ainda prova (igual ao `FakeVram` do ublk); não deferir o que dá pra validar por software. |
| DT-9 | Mapeamento de erro: `From<vk::Result>`/erros do `ash` → `VramError::Provider(String)`; OOB → `VramError::OutOfRange` (espelha `From<CudaError>` do `ramshared-cuda`). | Contrato do trait (sem `unwrap`/panic; tudo vira `VramError`). |
| DT-10 (IMPL) | **`VK_EXT_memory_budget` é OPCIONAL** (revisa DT-4). Presente → `mem_info` exato (`budget-usage`, `budget`). Ausente (ex.: lavapipe) → `total` = maior heap `DEVICE_LOCAL`; `free` = `total − Σ alocado pelo provider` (contador `AtomicU64`). | Sem o fallback a lógica não roda no lavapipe; o contador dá um `free` correto p/ o próprio daemon (o ajuste p/ VRAM de outros processos vem do budget, só no real-GPU). |
| DT-11 (IMPL) | `--backend vulkan` é ligado nos caminhos **genéricos** (`run_broker` + `run_nbd` single, ambos `P: VramProvider`). O **ublk** com Vulkan fica **deferido**: o `spawn_server_dt3_vram_with_residency` (`ublk_server.rs`) é **CUDA-fixo** (faz `Cuda::load()`/`create_context` dentro da thread do worker), não genérico — generificá-lo sobre `VramProvider` é um refactor à parte. `run_ublk` com `BackendKind::Vulkan` retorna `Err` claro (não panica). | Revisa o RF-V4/ITEM-4 ("`run_ublk` não muda"): o arm VRAM do ublk não é genérico. Day-0 honesto: nada de caminho Vulkan-com-forma-de-CUDA meia-feito; ublk só roda em host nativo (gated), onde a generificação será validada. |

## Fronteira de atomicidade e política de rollback

- **Atômico/escopo:** cada `read_at`/`write_at`/`zero` é uma submissão+fence completa (síncrona) antes
  de retornar. `alloc`/`Drop` são RAII (libera `VkDeviceMemory`/`VkBuffer` na ordem inversa).
- **Fora de escopo:** concorrência multi-thread na mesma `VulkanMem` (proibida — thread-afim, DT-7);
  multi-GPU.
- **Rollback:** **app** = remover/não-compilar a flag `--backend vulkan` (CUDA é o default e o caminho
  de produção atual); o crate `ramshared-vulkan` é aditivo. **Migration/dados:** N/A. **forward-only:** N/A.

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-3 (staging copy no hot path) | #5 Availability (worst-case) + #3 Número | [`KAHNEMAN-DISCIPLINES.md#5-availability-heuristic`](../methodology/KAHNEMAN-DISCIPLINES.md) | O staging+fence degrada o serve p50 vs CUDA no MESMO HW? | smoke round-trip + fio 4K Vulkan vs CUDA, ≥3 rodadas (host nativo) | p50 > **2×** o CUDA → revisar (staging persistente / ring de fences) antes de seguir |
| ITEM-1 (`open` + `VK_EXT_memory_budget`) | #1 WYSIATI | [`#1-wysiati--what-you-see-is-all-there-is`](../methodology/KAHNEMAN-DISCIPLINES.md) | A extensão de budget existe no driver-alvo? Vulkan-em-WSL2 funciona ou só nativo? | `open()` num host nativo retorna Ok + `mem_info().1 ≈ VRAM total` | sem `VK_EXT_memory_budget` / Vulkan ausente → `Err` claro, **não** silenciar com fallback |
| ITEM-3 (`unsafe` FFI `ash`) | #13 Ilusão de validade | [`#13-ilusão-de-validade`](../methodology/KAHNEMAN-DISCIPLINES.md) | Cada bloco `unsafe` tem invariante provada (handles vivos, tamanhos batem)? | `// SAFETY:` em cada bloco; `cargo audit`/`deny` no `ash`; round-trip bytes-iguais | round-trip não devolve os bytes escritos → bug de sincronização/cópia, parar |

## Checklist de segurança (pré-implementação)

- [ ] `unsafe` (FFI `ash`) **isolado** no crate `ramshared-vulkan` com `// SAFETY:` por bloco; o
  trait/fronteira fica `#![forbid(unsafe_code)]` onde possível (a impl re-exporta).
- [ ] Bounds-check `off+len ≤ len()` em `read_at`/`write_at` → `VramError::OutOfRange` (espelha CUDA).
- [ ] Sem `unwrap`/`expect` em produção; todo erro Vulkan → `VramError` (DT-9).
- [ ] `cargo audit` + `cargo deny check` cobrindo `ash` (nova dep, regra de security).
- [ ] Sem segredos/endereços no log; handles Vulkan não logados crus.
- [ ] Teardown libera recursos na ordem inversa (RAII `Drop`), sem leak de `VkDeviceMemory`.

## Arquivos a CRIAR

### `crates/ramshared-vulkan/Cargo.toml`
- **Propósito:** crate da 2ª impl. **Deps:** `ramshared-vram` (path), `ash` (Vulkan FFI).
  `[lints.clippy] unwrap_used="deny", expect_used="deny"` (igual aos outros crates).

### `crates/ramshared-vulkan/src/lib.rs`
- **Propósito:** `VulkanProvider` + `VulkanMem` implementando `VramProvider`/`VramMemory` (RF-V1/2/3).
- **Requisitos:** RF-V1, RF-V2, RF-V3, DT-1..DT-9.
- **Structs/Types (assinaturas):**
  ```rust
  pub struct VulkanProvider {
      entry: ash::Entry, instance: ash::Instance, device: ash::Device,
      phys: ash::vk::PhysicalDevice, transfer_queue: ash::vk::Queue, queue_family: u32,
      cmd_pool: ash::vk::CommandPool, fence: ash::vk::Fence,
      staging: VkRegion,            // HOST_VISIBLE|HOST_COHERENT, mapeado
      device_local_type: u32,       // índice do memory type DEVICE_LOCAL
      heap_index: u32,              // heap p/ VK_EXT_memory_budget
  }
  pub struct VulkanMem<'p> { provider: &'p VulkanProvider, region: VkRegion }
  struct VkRegion { buffer: ash::vk::Buffer, memory: ash::vk::DeviceMemory, len: u64, mapped: *mut u8 }
  ```
- **Funções (assinatura + lógica):**
  - `pub fn open(ordinal: u32) -> Result<Self, VramError>` (RF-V1): `Entry::load` → `create_instance`
    (com `VK_EXT_memory_budget` se disponível no device) → escolher `phys` `ordinal` (preferir
    `DISCRETE_GPU`) → achar queue family com `TRANSFER` → `create_device` + `get_device_queue` →
    `create_command_pool` + alocar 1 cmd buffer → `create_fence` → alocar staging
    `HOST_VISIBLE|HOST_COHERENT` (tamanho = `BLOCK_SIZE`*K? usar um teto, ex. 1 MiB; cópias maiores
    iteram) + `map_memory`. Falha-claro (`VramError::Provider`) sem GPU/extensão.
  - `impl VramProvider for VulkanProvider`:
    - `type Mem<'p> = VulkanMem<'p> where Self: 'p;`
    - `alloc(bytes)`: `create_buffer`(`TRANSFER_SRC|TRANSFER_DST`) → `get_buffer_memory_requirements`
      → escolher o memory type em **`requirements.memory_type_bits ∩ DEVICE_LOCAL`** (não um índice
      fixo — o tipo precisa ser compatível com o buffer; 2.5-fix) → `allocate_memory` +
      `bind_buffer_memory` → `VulkanMem`. `Err` se `allocate_memory` falhar (OOM VRAM).
    - `mem_info() -> (u64,u64)`: `get_physical_device_memory_properties2` com
      `PhysicalDeviceMemoryBudgetPropertiesEXT` → `(budget[heap]-usage[heap], budget[heap])`.
  - `impl VramMemory for VulkanMem<'_>`:
    - `len()` = `region.len`.
    - `write_at(off, src)`: bounds-check; `copy` src→staging (`mapped`, em chunks ≤ staging len);
      por chunk: `cmd: vkCmdCopyBuffer(staging→region.buffer, dst_off=off)` + submit na
      `transfer_queue` + `wait_for_fences` + `reset_fences`.
    - `read_at(off, dst)`: simétrico (region→staging→`dst`).
    - `zero()`: `vkCmdFillBuffer(region.buffer, 0, WHOLE_SIZE, 0)` + submit + wait (DT-6).
  - `impl Drop` p/ `VulkanMem` (free buffer+memory) e `VulkanProvider` (destrói fence/pool/device/instance, ordem inversa).
- **Dependências:** internas: `ramshared_vram`; externas: `ash`.
- **Padrão de referência:** `crates/ramshared-cuda/src/{driver.rs, vram_impl.rs}` (mesma forma:
  provider thread-afim + GAT Mem + `From<erro>→VramError`).
- **Testes:** `#[cfg(test)]` com `#![allow(clippy::unwrap_used, clippy::expect_used)]`:
  `vulkan_roundtrip_write_then_read` (`#[ignore = "requer GPU Vulkan (host nativo)"]`) — `open`→`alloc`
  →`write_at`→`read_at` bytes-iguais; `mem_info_reports_total` (`#[ignore]`).

## Arquivos a MODIFICAR

### `crates/ramshared-wsl2d/Cargo.toml`
- **+ dep** `ramshared-vulkan = { path = "../ramshared-vulkan" }`. **Por quê:** shell do daemon (RF-V4).

### `crates/ramshared-wsl2d/src/main.rs`  *(RF-V4)*
- **O que muda:** `BackendKind` ganha `Vulkan`; o parsing de `--backend` aceita `"vulkan"`; o dispatch
  do modo broker/single/ublk cria o shell Vulkan (espelha o shell CUDA: hoje `Cuda::load()` →
  `create_context` → `run_broker(ctx, …)`; novo: `VulkanProvider::open(0)? → run_broker(provider, …)`).
- **Função/bloco:** `enum BackendKind` (+`Vulkan`), `BackendKind::label`, o `match args "--backend"`,
  e os `match backend { Vram => {shell CUDA}, Ram => …, Vulkan => {shell Vulkan} }` no modo broker e no
  `Transport::Nbd` single de `run()`. **ublk:** `run_ublk` ganha um arm `Vulkan => Err(claro)` (DT-11:
  o servidor de residência ublk é CUDA-fixo; generificá-lo é refactor à parte, só validável em host).
- **Impacto:** aditivo; CUDA/RAM intactos. `run_broker`/`run_nbd` **não mudam** (já genéricos). `run_ublk`
  ganha só o arm de erro (DT-11). `run_ublk` no WSL2 segue barrado por `guard_not_wsl2` de qualquer forma.
- **Testes:** `--backend vulkan` parseia; e2e = smoke round-trip (host Vulkan).

## Arquivos a DELETAR

| Arquivo | Motivo |
| --- | --- |
| — | nenhum (Day-0; 2ª impl aditiva) |

## Observabilidade

Sem métricas novas — o `VramGauge`/telemetria (RF-3) já consome `mem_info()` genérico; com Vulkan, o
`vram_alloc_daemon`/`vram_outros` passam a refletir a GPU Vulkan automaticamente. Erros de bring-up →
`eprintln` (igual ao shell CUDA).

## Contratos e documentação viva

| Documento | Atualização | Motivo |
| --- | --- | --- |
| `docs/LIBRARIES.md` | **Alterar** | nova dep `ash` (registro de lib, regra ssdv3) |
| `docs/vulkan-backend/IMPL.md` | **Criar** (PASSO 3) | commits/decisões/métricas (quando houver host Vulkan) |
| `docs/memory-broker/PRD.md` | **Alterar** | marcar RF-G2 com SPEC pronto |
| `Documentation/`, `Kconfig`, `CLAUDE.md`, `.claude/rules/*` | N/A | userspace; sem uAPI/convenção nova |

## Ordem de implementação

1. Crate `ramshared-vulkan` + `Cargo.toml` + `VulkanProvider::open` (RF-V1). *(build do `ash` — ver
   risco R-ash abaixo)*
2. `impl VramProvider` (`alloc`/`mem_info`) — RF-V2.
3. `impl VramMemory` (`read_at`/`write_at`/`zero`/`len`) + `Drop` — RF-V3 + teste round-trip `#[ignore]`.
4. Shell `--backend vulkan` no daemon — RF-V4.
5. Validação num **host Linux nativo** com GPU Vulkan: round-trip + smoke server-only + (destrava)
   ublk+VRAM e2e + eviction-sob-carga.

## Plano de testes

- **Unit/round-trip (`#[ignore]`, host Vulkan):** `vulkan_roundtrip_write_then_read` (bytes-iguais),
  `mem_info_reports_total`, `zero_wipes`. Espelham os testes CUDA (`backend.rs`).
- **Perf (host nativo):** fio 4K Vulkan vs CUDA (gate Kahneman ITEM-3, <2×).
- **e2e (host nativo):** `--backend vulkan --transport nbd` smoke server-only; `--transport ublk`
  (não-WSL2) → ublk+VRAM e2e; eviction sob carga gráfica.
- **Regressão:** `cargo test --workspace` (CUDA/RAM/telemetria intactos); drills qemu PASS.

## Checklist de validação

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` (inclui `ramshared-vulkan`)
- [ ] `cargo test --workspace` (sem regressão; testes Vulkan `#[ignore]` salvo host)
- [ ] round-trip Vulkan PASS num host nativo (`--ignored`)
- [ ] `cargo audit` / `cargo deny` cobrindo `ash`
- [ ] cada etapa crítica com disciplina + evidência + abort (mapa Kahneman)

## Risco operacional explícito (ambiente de IMPL)

- **R-ash (build):** adicionar `ash` é uma dep nova; **compilar no WSL2 pode ser pesado** (cargo
  caution) — usar `-j2`, escopo `-p ramshared-vulkan`, **sem `--release`**. Se o build ameaçar
  travar, fazer num host nativo.
- **R-validação:** o e2e Vulkan **exige host Linux nativo com GPU** (DT-8). No WSL2 só dá pra
  compilar; a validação (round-trip/perf/e2e) fica gated nesse host — mesma natureza do ublk+VRAM.
  → **A IMPL (PASSO 3) só fecha o "validado" num host Vulkan; aqui ela seria compile-only.**
