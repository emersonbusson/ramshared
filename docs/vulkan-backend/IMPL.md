# IMPL — Backend Vulkan do `VramProvider` (RF-G2)

> SSDV3 PASSO 3. Implementa estritamente `docs/vulkan-backend/SPEC.md` (DT-1..DT-11). Decisões que
> surgiram na IMPL viraram **DT-10/DT-11 no SPEC antes do código** (regra dura #3: zero criatividade
> fora do SPEC). PRD: `docs/vulkan-backend/PRD.md`.

## Status: implementado + verde (no que é validável por software aqui)

2ª implementação do trait `VramProvider`/`VramMemory` (a 1ª, CUDA, intacta) via `ash` (Vulkan), no
crate novo `ramshared-vulkan`, + shell `--backend vulkan` no daemon nos caminhos genéricos (broker +
NBD single). **A lógica completa do trait valida no lavapipe** (Vulkan em CPU) — o nível mais seguro
que ainda prova, igual ao `FakeVram` do ublk (DT-8). Só perf/VRAM-real/eviction ficam gated p/ host
NVIDIA-Vulkan nativo (a RTX 2060 não tem ICD Vulkan no WSL2 — `vulkaninfo` só vê `llvmpipe`).

## Arquivos (RF/ITEM → mudança)

| RF / ITEM | Arquivo | Mudança | Commit |
| --- | --- | --- | --- |
| RF-V1 (`open` + enumeração) | `crates/ramshared-vulkan/src/lib.rs` | loader + instância + physical device + heap `DEVICE_LOCAL` | `b19d8a3` |
| DT-10 (SPEC) | `docs/vulkan-backend/SPEC.md` | `VK_EXT_memory_budget` opcional + fallback (revisa DT-4/DT-8) | `36f3586` |
| RF-V2/RF-V3 | `crates/ramshared-vulkan/src/lib.rs` | device lógico + transfer queue + cmd pool/buffer + fence + staging; `alloc`/`mem_info`; `read_at`/`write_at`/`zero` + bounds-check + `Drop` | `d15bd82` |
| DT-11 (SPEC) | `docs/vulkan-backend/SPEC.md` | ublk+vulkan deferido (servidor de residência ublk é CUDA-fixo) | `d4617e7` |
| RF-V4 (shell daemon) | `crates/ramshared-wsl2d/src/main.rs`, `Cargo.toml` | `BackendKind::Vulkan` + parse + dispatch broker/NBD; `run_ublk` → `Err` claro (DT-11) | `afad4d5` |
| RF-V2 (registro de dep) | `docs/LIBRARIES.md` | `ash 0.38` ativo + `vulkano`/hand-roll descartados | (este) |

## Decisões pequenas durante a IMPL (não pediram nova ADR)

- **Cleanup RAII no idiom `goto out_err` (kernel.md):** `ResGuard` possui o `device` + handles `Option`;
  qualquer `?` na montagem destrói os children já criados **na ordem inversa** + o device. Em sucesso,
  `armed=false` e o `device` é clonado (handle leve do `ash`) p/ o `VulkanProvider` (o `destroy` real
  fica no `Drop` do provider). Substitui o aninhamento de `match`/cleanup manual por RAII verificável.
- **`alloc` arredonda o buffer p/ múltiplo de 4** (requisito do `vkCmdFillBuffer` com `WHOLE_SIZE` no
  `zero`); o `len` lógico continua `bytes` (bounds-check usa `len`, não o buffer).
- **`read_at`/`write_at` fatiam em chunks ≤ `STAGING_BYTES` (1 MiB)** reusando 1 staging buffer (DT-8,
  sem alloc no hot path). `HOST_COHERENT` → sem `vkFlushMappedMemoryRanges`.
- **`mem_info` usa só o fallback DT-10** (maior heap `DEVICE_LOCAL` − Σ alocado). O `VK_EXT_memory_budget`
  exato (que enxerga VRAM de outros processos) fica p/ o real-GPU — não muda a lógica, só a precisão do
  `free` num GPU compartilhado.
- **NBD com `--backend ram` → `Err` claro** (RAM não é `VramProvider`; só broker/ublk). Antes o arm NBD
  ignorava `backend` e caía sempre no CUDA.

## Validação (números)

- **Round-trip no lavapipe** (`cargo test -p ramshared-vulkan -- --ignored`, sandbox off p/ o loader):
  `2 passed; 0 failed`. `device='llvmpipe (LLVM 20.1.2, 256 bits)'`, `total=15993 MiB`.
  `vulkan_roundtrip_write_then_read`: `alloc(2 MiB)` → `write_at(off=4096, 1 MiB+4 KiB)` (2 chunks) →
  `read_at` **bytes-iguais**; `zero()` deixa tudo 0; `read_at` além do fim → `OutOfRange`; `mem_info`
  `free` cai exatamente 2 MiB (15993→15991 MiB) após o `alloc`.
- **Workspace:** `cargo test --workspace` → `205 passed; 0 failed; 22 ignored`. `cargo clippy --workspace
  --all-targets -- -D warnings` limpo; `cargo fmt --check` limpo. (`unwrap_used`/`expect_used = deny` no
  crate; testes com `#[allow]`.)
- **Parse/dispatch (daemon real):** `--backend vulkan --transport ublk` → mensagem DT-11 (sem
  side-effect); `--backend bogus` → erro do parser (`'vram', 'vulkan' ou 'ram'`).
- **Sem regressão (drills qemu, host intacto):** `qemu-broker-drill.sh` → **PASS** (broker assina 2
  slices, swap via NBD, telemetria JSONL, teardown limpo); `qemu-ublk-daemon.sh` → **PASS** (insmod ublk,
  `/dev/ublkb0`, serve I/O, teardown limpo). Os caminhos broker/RAM/ublk-RAM que o shell tocou seguem
  idênticos.

## Segurança (RNF)

- Todo `unsafe` (FFI `ash`) **isolado** no `ramshared-vulkan` com `// SAFETY:` por bloco; a fronteira do
  trait (`ramshared-vram`) é `#![forbid(unsafe_code)]`. Sem `unwrap`/`expect`/panic em produção (erros →
  `VramError::Provider`/`OutOfRange`). Bounds-check `off+len ≤ len` antes de qualquer cópia.
- **Supply chain (RNF) — verde (2026-07-01):** `cargo audit` → **0 advisories** nas 29 deps do
  workspace (`ash 0.38` + transitivas `libc`/`bitflags`/…); `cargo deny check` →
  `advisories ok, bans ok, licenses ok, sources ok`. Gate versionado em **`deny.toml`** na raiz:
  advisories sem `ignore`; allow-list de licenças **estrita** (só MIT/Apache-2.0/ISC/Unicode-3.0,
  presentes na árvore — nova licença barra até revisão); `sources` só crates.io; `wildcards = deny`
  com `allow-wildcard-paths` habilitado pelas crates internas marcadas `publish = false`. Reproduzir:
  `cargo audit && cargo deny check`.

## Gaps genuinamente env-bound (gated, `#[ignore]` / host NVIDIA-Vulkan nativo)

Mesma natureza do trap do ublk+VRAM — **não dá pra validar neste WSL2** (sem ICD NVIDIA Vulkan
**carregável pelo loader Linux** — ver detalhe abaixo):

- **Perf vs CUDA** (Kahneman ITEM-3 #5: staging copy < 2× o `cuMemcpy` no mesmo HW) — medir no real-GPU.
- **VRAM real** (`alloc` numa GPU física + `mem_info` via `VK_EXT_memory_budget` exato).
- **ublk+VRAM e2e e eviction-sob-carga** com `--backend vulkan` — bloqueado também pelo DT-11 (servidor
  de residência ublk é CUDA-fixo; generificá-lo sobre `VramProvider` é o próximo passo, validável só em
  host nativo onde o ublk roda).

### Rota futura conhecida p/ real-GPU **dentro do WSL2** — Dozen (`dzn`), investigada e não-trivial

Investigado em 2026-06-16 (pra não re-investigar do zero): a RTX 2060 **não** é alcançável por Vulkan
**pelo lado Linux** deste WSL2 (`vulkaninfo --summary` só lista `llvmpipe`). Precisão importante (a NVIDIA
**tem** Vulkan Linux nativo de primeira — `nvidia_icd.json`→`libGLX_nvidia.so` num Linux normal): no WSL2
existem ICDs Vulkan da NVIDIA (`/usr/lib/wsl/drivers/.../nv-vk64.json`), mas eles apontam pra **DLL do
Windows** (`nvoglv64.dll`) — o loader Vulkan do **Linux** (`libvulkan.so.1`) só carrega `.so` (ELF), não
`.dll` (PE). As libs NVIDIA **Linux** projetadas no WSL (`/usr/lib/wsl/lib`) são só CUDA/NVENC/OptiX/NGX
(`ldconfig` confirma: **nenhuma `.so` Vulkan/GLX**). O `nouveau`/NVK está instalado mas não acha a GPU
(precisa de DRM nativo, não `/dev/dxg`). Ou seja: o Vulkan→2060 no WSL2 existe **só pelo lado Windows**
(DLL), não pelo Linux. **MAS** as
libs D3D12 do WSL estão presentes (`/usr/lib/wsl/lib/libd3d12core.so`, `libdxcore.so`, no linker path) —
o substrato do **Dozen (`dzn`)** da Mesa (Vulkan→D3D12→`/dev/dxg`→GPU). Viabilidade:

- **Sem pacote pronto:** `mesa-vulkan-drivers` (25.2.8 e 24.0.5) **não** entregam `libvulkan_dzn.so`
  (Ubuntu/Debian excluem o `microsoft-experimental`; PPAs seguem a mesma regra). Única rota = **build de
  fonte** com `-Dvulkan-drivers=microsoft-experimental` (instalar `meson`/`ninja`/`glslang` + `apt
  build-dep mesa` — modifica o sistema; clone + ~10–30 min de compile CPU-bound).
- **Cobre só o funcional:** mesmo dando certo, valida `alloc`/`mem_info`/round-trip na **VRAM física
  real** — mas a **perf** seria Vulkan-traduzido-pra-D3D12, **não** NVIDIA nativo, então o gate de
  perf-vs-CUDA (#5) **continua** precisando de host nativo. Decisão (2026-06-16): **não buildar agora**
  (custo/benefício marginal; a lógica já está provada no lavapipe).

## Rastreabilidade

`b19d8a3` (RF-V1) · `36f3586` (DT-10) · `d15bd82` (RF-V2/RF-V3) · `d4617e7` (DT-11) · `afad4d5` (RF-V4).
SPEC: `docs/vulkan-backend/SPEC.md`. PRD: `docs/vulkan-backend/PRD.md`. Kahneman:
`docs/methodology/KAHNEMAN-DISCIPLINES.md` (#1 `open`, #5 perf gated, #13 `unsafe` + round-trip).
