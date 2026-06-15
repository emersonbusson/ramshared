# SPEC — `VramProvider`: abstração de backend de VRAM (prep da P3, RF-G1)

> **Tier SSDV3:** isto é um **refactor interno sem mudança de contrato público** (o daemon serve
> NBD/ublk de forma idêntica; nenhuma uAPI/protocolo muda). Pela regra, refactor → SSDV3 é
> opcional; este SPEC existe porque toca a **fronteira de abstração de hardware** e tem um problema
> de design real (lifetimes). O **backend Vulkan** (RF-G2) é um subsistema novo e ganhará seu
> próprio PRD quando for feito — **fora do escopo deste SPEC**.
>
## Estado da IMPL (2026-06-15)
- **DT-V1 revisado:** o `Arc` no `Context` **não** foi necessário. Caminho mais limpo: impl dos
  traits nos tipos CUDA existentes com **GAT** (`type Mem<'p> = DeviceMem<'p,'a>`). Sem reestruturar
  o `ramshared-cuda`, sem ripple de tipos. (Anula o ponto §2 sobre auto-referência.)
- **Feito + validado** (branch `feat/p1-hardening`): (1) crate `ramshared-vram` com os traits
  (`d898488`); (2) impl CUDA `VramProvider for Context`/`VramMemory for DeviceMem` (`74f4052`);
  (3) `VramBackend<M>`/`CanaryProbe<M>`/`residency_check<M>` genéricos (`ca5194d`). Tudo verde
  (clippy --all-targets, test --workspace, drill qemu PASS). **`VramMemory` está extraído E
  consumido**; `VramProvider` está definido + CUDA-impl'd.
- **Daemon genérico FEITO** (`c65e0de`): `run_nbd` e `run_broker` allocam via `VramProvider`
  (`provider.alloc`/`provider.mem_info`); o `Cuda::load`+`create_context` foi pra shells finos no
  `run()` (provider por valor; mem/canário/closure emprestam shared; afinidade de thread preservada).
  **`VramProvider` agora é consumido genericamente** → broker + NBD single são Vulkan-ready. Validado:
  clippy --all-targets, test --workspace 28 ok, drill qemu PASS, **smoke VRAM server-only no GPU real**.
- **ublk-vram FEITO** (2026-06-15): o loop do worker de `spawn_server_dt3_vram_with_residency` foi
  extraído na fn genérica `serve_ublk_residency<M: VramMemory, F: Fn()->Option<u64>>` (`ublk_server.rs`).
  A spawn mantém o **shell CUDA na thread** (`Cuda::load`+`create_context`+`alloc`) e chama o loop
  genérico passando `|| ctx.mem_info()` como `mem_free`. **Não precisou de `open()`/Arc:** o padrão
  "shell concreto + serve genérico" (igual ao `run_broker`) resolve a auto-referência — o loop é
  monomorfizado com `M = DeviceMem<'c,'a>` e o `ctx` thread-afim vive no chamador. `spawn_server_dt3_vram`
  (sem residência) já era genérico via `worker_loop<B>`. Um provider Vulkan futuro vira um sibling-spawn
  que cria o contexto Vulkan e chama o **mesmo** `serve_ublk_residency`.
  - **Validação (composição):** clippy `--all-targets`, `cargo test --workspace` (wsl2d lib 45 ok),
    **drill qemu ublk-RAM PASS** (ring/teardown), **smoke VRAM GPU** (`vram_backend_serves_nbd_write_then_read`
    + `gpu_roundtrip_256mib`). **Teste novo não-gated** `residency_tests` (backend `FakeVram` em RAM) roda
    `serve_ublk_residency` de verdade aqui (serve+§9.4+DEMOTE+teardown) **sem GPU/ublk/root** — seguro no WSL2.
  - **Gap deferido (ambiente):** o e2e ublk+VRAM combinado (`#[ignore]` `dt3_vram_residency_triggers_demote_synthetic`)
    só roda em host **não-WSL2** com root+CUDA+ublk — a GPU está presa no WSL2 (GPU-PV, sem passthrough p/ VM)
    e o WSL2 proíbe ublk (freeze 2026-06-09); qemu não tem GPU. A lógica do loop, porém, já tem teste executável aqui.
- **Backend Vulkan (RF-G2)**: subsistema novo, PRD próprio.

## 1. Objetivo e escopo
Extrair um trait `VramProvider` (+ `VramMemory`) que abstraia o **plano de controle** de VRAM hoje
acoplado direto ao CUDA, com `ramshared-cuda` virando a **1ª implementação**. O daemon
(`run_nbd`/`run_broker`/ublk + canário) passa a ser genérico sobre o provider. **Comportamento
idêntico**, zero mudança de protocolo. Destrava um futuro backend Vulkan sem reescrever o daemon.

O `BlockBackend` já abstrai o **plano de dados** (`read_at`/`write_at`/`size_bytes`/`flush`). O que
falta abstrair (do mapa de acoplamento): **ciclo de vida** (`load`/`device`/`create_context`),
**alocação** (`alloc`+`Drop`), **wipe** (`zero` com sync, DT-17/§11) e **free-floor** (`mem_info`
para a residência, DT-3/9/11).

## 2. Problema de design central — a cadeia de lifetimes
Hoje (`crates/ramshared-cuda/src/driver.rs`):
```
Cuda                       // dono do dlopen + syms
Context<'a>  { cuda: &'a Cuda, raw }              // empresta &Cuda
DeviceMem<'c,'a> { ctx: &'c Context<'a>, ptr, len }   // empresta &Context
```
Juntar `Cuda`+`Context` num único "provider" seria **auto-referencial** (Context empresta Cuda). Um
trait genérico sobre essa cadeia de 3 níveis é inviável de forma limpa.

**Decisão (DT-V1): quebrar a cadeia de borrows com `Arc`.** Reestruturar o `ramshared-cuda`:
- `Context` passa a **possuir** `Arc<CudaInner>` (os `syms`) em vez de `&'a Cuda` → deixa de ser
  auto-referencial; o provider pode possuir o contexto.
- `DeviceMem` possui `Arc<ContextInner>` (ou mantém `&Context` via GAT — ver DT-V2).
Resultado: a cadeia vira **Provider (dono) → Memory**, 2 níveis, abstraível.

**Decisão (DT-V2): a memória como GAT do trait** (evita `Arc` por-alocação no hot path):
```rust
pub trait VramProvider {
    type Mem<'p>: VramMemory where Self: 'p;
    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError>;
    fn mem_info(&self) -> Result<(u64, u64), VramError>; // (free, total)
}
pub trait VramMemory {
    fn len(&self) -> usize;
    fn zero(&mut self) -> Result<(), VramError>;           // wipe + sync (DT-17/§11)
    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError>;
    fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError>;
}
```
O provider já é o "contexto thread-afim" (criado via construtor que faz `load`+`device`+
`create_context`). `Mem<'p>` empresta `&'p Provider` → mesma semântica de hoje (`DeviceMem` empresta
`Context`), mas atrás do trait. GATs são estáveis no rustc do projeto (1.93).
> Alternativa rejeitada: `Mem` com `Arc<Context>` (sem lifetime). Mais simples para os consumidores,
> mas adiciona refcount e atrapalha a **afinidade de thread** do CUDA (`!Send`); GAT preserva a
> afinidade via `&Provider` na mesma thread.

## 3. Construtor (substitui `load`+`device`+`create_context` espalhados)
```rust
pub trait VramProvider: Sized {
    fn open(device_ordinal: u32) -> Result<Self, VramError>;  // load+device+context, thread-afim
    fn device_name(&self) -> &str;
    // + alloc/mem_info/Mem (acima)
}
```
`CudaProvider::open(0)` faz hoje o que `Cuda::load()? + cuda.device(0)? + create_context(&dev)?` fazem.

## 4. Onde mora o trait
Crate novo **`ramshared-vram`** (`#![forbid(unsafe_code)]` no trait; o `unsafe` fica só na impl CUDA
que re-exporta de `ramshared-cuda`). Alternativa: módulo `ramshared-block::vram`. Recomendado o crate
novo (mantém `ramshared-block` focado em I/O de bloco; evita `ramshared-block` depender de CUDA).

## 5. Arquivos a CRIAR / MODIFICAR
- **CRIAR** `crates/ramshared-vram/` — os traits `VramProvider`/`VramMemory` + `VramError`.
- **MODIFICAR** `crates/ramshared-cuda/src/driver.rs` — `Context` possui `Arc<…>` (DT-V1); `impl
  VramProvider for CudaProvider` + `impl VramMemory for DeviceMem`.
- **MODIFICAR** `crates/ramshared-wsl2d/src/backend.rs` — `VramBackend<M: VramMemory>` (genérico
  sobre a memória, não `DeviceMem` concreto). `SliceView`/`RamBackend` inalterados (já genéricos/independentes).
- **MODIFICAR** `crates/ramshared-wsl2d/src/canary_probe.rs` — `CanaryProbe<M: VramMemory>` (hoje
  `DeviceMem` direto).
- **MODIFICAR** `crates/ramshared-wsl2d/src/main.rs` — `run_nbd`/`run_broker` genéricos sobre
  `P: VramProvider` (alloc/mem_info/zero via trait; o closure de free-floor vira `p.mem_info()`).
- **MODIFICAR** `crates/ramshared-wsl2d/src/ublk_server.rs` — `spawn_server_dt3_vram*` genéricos.

## 6. Ordem de migração (incremental, cada passo compila + testes verdes)
1. Crate `ramshared-vram` + traits (sem consumidores ainda).
2. DT-V1 no `ramshared-cuda` (Arc) — testes do cuda crate verdes.
3. `impl VramProvider/VramMemory for Cuda*` — adita, sem migrar consumidores.
4. `VramBackend`/`CanaryProbe` genéricos sobre `M: VramMemory`.
5. `run_nbd`/`run_broker`/`ublk_server` genéricos sobre `P: VramProvider`; trocar os usos diretos de
   `Cuda::load/alloc/mem_info/zero`.
6. Remover a superfície CUDA-direta do `ramshared-wsl2d` (só via trait).

## 7. Validação (comportamento idêntico)
- `cargo test --workspace` (todos os testes existentes passam **sem mudança** — é refactor).
- `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all --check`.
- **Drill qemu** (`scripts/kernel/qemu-broker-drill.sh`, `--backend ram`): PASS (RAM path não usa o
  provider → garante que a genericização não quebrou o data-plane).
- **Smoke VRAM server-only** no host (RTX 2060): bring-up + `VRAM zerada` no teardown **idênticos**.
- **e2e VRAM cross-host** (`MADV_PAGEOUT`): integridade 0/N páginas ruins (igual ao baseline atual).

## 8. Riscos
- **Lifetimes/GAT** (R-V1): a genericização de `run_nbd`/`run_broker` (closures de free-floor +
  `SliceView<&mut backend>`) pode bater em borrow/lifetime. Mitigação: migração incremental (§6) com
  compile a cada passo; manter a afinidade de thread (provider+mem na thread do worker).
- **Afinidade de thread CUDA** (R-V2): o contexto é thread-local. O trait NÃO deve exigir `Send` na
  `Mem`; o worker já é single-thread dono do provider. Documentar no trait.
- **`Arc` no `Context`** (R-V3): muda o `ramshared-cuda`; risco de regressão no caminho VRAM (só
  validável no host/qemu). Validar com o smoke VRAM server-only.
- **Escopo**: NÃO implementar Vulkan aqui (só o trait + CUDA). Vulkan = PRD próprio depois.

## 9. Rollback trigger
Se a genericização degradar a latência de serve (canário) em >2× o baseline (Fase 0) por GAT/dyn
indireção, ou se a afinidade de thread quebrar (CUDA errors), reverter o refactor (`git revert`) —
o caminho CUDA-direto atual é o fallback. O trait é puro overlay; reversão é limpa.
