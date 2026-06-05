# SPEC — Issue #8 — Canário de residência dedicado (§9.4)

Fonte: [`PRD.md`](PRD.md). Implementa a SPECv3 [`§9.4`](../vram-as-ram/SPECv3-WSL2.md).
Decisões fechadas, sem ambiguidade para o Passo 3.

## Escopo fechado desta implementação

**Entra agora:**
- Região-canário dedicada na VRAM (`DeviceMem` separado).
- Sonda periódica: round-trip de tamanho fixo → `(latency_us, content_ok)`; `free` via
  `cuMemGetInfo`.
- Fiação no serve loop: a sonda (em cadência) substitui o proxy de latência do
  `serve()` como entrada do `Canary`; os 3 gatilhos passam a ser alcançáveis.
- `ResidencyConfig.free_floor_bytes` default > 0.

**Fica fora agora:** detecção em idle (thread amostradora / `cuCtxSetCurrent` — H1);
multi-conexão; writeback.

**Dependências prontas:** `ramshared-cuda` (`alloc`/`write_at`/`read_at`/`mem_info`),
`residency.rs` (decisão + 5 testes), `ramshared-integrity` (`fill_block`/`verify_block`).

## Matriz de rastreabilidade PRD → SPEC

| PRD  | Implementação no SPEC |
| ---- | --------------------- |
| RF-1 | ITEM-1, DT-1 |
| RF-2 | ITEM-2, DT-4 |
| RF-3 | ITEM-2, ITEM-4, DT-6 |
| RF-4 | ITEM-4, ITEM-5, DT-3 |
| RF-5 | ITEM-3, DT-2 |
| RF-6 | ITEM-4 (reusa o caminho de DEMOTE existente) |

## Decisões técnicas

| #    | Decisão | Justificativa |
| ---- | ------- | ------------- |
| DT-1 | `CANARY_BYTES = 4096` (1 bloco) | round-trip mínimo representativo (1 página), alinhado ao `BLOCK_SIZE`. |
| DT-2 | `CANARY_EVERY = 64` requests | amortiza overhead; sob pressão há tráfego abundante → detecção em < ~64 I/Os. |
| DT-3 | `free_floor_bytes` default = `64 * 1024 * 1024` | "GPU criticamente cheia" conservador; tunável; latência segue gatilho primário (PRD R1). |
| DT-4 | Sentinela = `ramshared_integrity::fill_block`/`verify_block` com `Pattern::Random` e `idx = seq` | reuso (regra SSDV3) + padrão reprodutível; `idx` por ciclo pega também **leitura stale**. |
| DT-5 | Erro CUDA na sonda (`run()`/`mem_info()` → `Err`) ⇒ **DEMOTE conservador** | disciplina #5 (worst-case): se não dá para *provar* residência, drena antes de arriscar. |
| DT-6 | A latência da **sonda** substitui a do `serve()` como entrada do `Canary`; baseline passa a ser da sonda | sinal de tamanho fixo, não confundido pelo tamanho do request (PRD RF-3). |

## Fronteira de atomicidade e política de rollback

- **Atomicidade:** cada ciclo de sonda é independente (1 ciclo = 1 amostra; sem
  multi-write transacional). A ação de DEMOTE (`swapoff`) é atômica no kernel e já é
  coberta pelo caminho existente (confirmação por canal + re-arm). Nenhum estado parcial
  novo é introduzido.
- **Rollback:** **app-only** (reverter os commits desta issue). Sem migration, sem
  dados persistidos, sem `forward-only`. Proibido em qualquer ambiente: nada (mudança
  reversível por revert).

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-4 (DEMOTE por free-floor / erro de sonda) | #5 Worst-case / pré-mortem + #2 Counterfactual/rollback | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "free baixo / erro de sonda é eviction real ou ruído do GPU-PV?" | teste injeta `free < floor` → `Demote(FreeFloor)`; ao vivo `vramhog` reduz free → DEMOTE no log | em operação normal (sem `vramhog`) o DEMOTE dispara por free-floor → subir `free_floor`/desabilitar (tunável) e reverter |
| ITEM-2 (gatilho de conteúdo) | #13 Ilusão de validade | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "e se a VRAM devolver dado stale/corrompido apesar de 'data-safe'?" | teste com leitura divergente → `Demote(Corruption)` | N/A (é guarda; não deve disparar em operação sã) |

## Checklist de segurança (pré-implementação)

- [x] Isolamento: a região-canário é um `DeviceMem` separado, **não endereçável** por
  requests NBD (o tamanho anunciado continua = região de swap).
- [x] Buffer overflow / OOB: `write_at`/`read_at` já fazem bounds-check (`driver.rs::bounds`);
  `wbuf`/`rbuf` têm exatamente `CANARY_BYTES`.
- [x] Permissões: o daemon já roda como root (subido pelo `up`); sem novo caminho privilegiado.
- [x] Input validation: a sonda não recebe input externo (sentinela sintética).
- [x] Ponteiros: nenhum endereço de VRAM é logado.
- [x] Sem `unsafe` novo (usa `ramshared-cuda`; `lib.rs` segue `forbid(unsafe_code)`).

## Arquivos a CRIAR

### `crates/ramshared-wsl2d/src/canary_probe.rs`

- **Propósito:** sonda da região-canário (round-trip + integridade) e cadência pura.
- **Requisitos cobertos:** RF-2, RF-3, RF-5, DT-1, DT-2, DT-4, DT-6.
- **Structs/Types:**
  ```rust
  use ramshared_cuda::{CudaError, DeviceMem};
  use ramshared_integrity::{Pattern, fill_block, verify_block};

  pub const CANARY_BYTES: usize = 4096; // DT-1
  pub const CANARY_EVERY: u32 = 64;     // DT-2

  /// Cadência pura (testável sem GPU): dispara a cada `every` ticks.
  pub struct Cadence {
      every: u32,
      counter: u32,
  }

  /// Resultado de um ciclo de sonda.
  pub struct ProbeOutcome {
      pub latency_us: u64,
      pub content_ok: bool,
  }

  /// Sonda da região-canário. Empresta o `DeviceMem` da região (separada da swap).
  pub struct CanaryProbe<'c, 'a> {
      region: DeviceMem<'c, 'a>,
      wbuf: Vec<u8>,
      rbuf: Vec<u8>,
      seq: u64,
  }
  ```
- **Funções:**
  ```rust
  impl Cadence {
      pub fn new(every: u32) -> Self;          // { every, counter: 0 }
      pub fn tick(&mut self) -> bool;          // counter+=1; if counter>=every {counter=0; true} else {false}
  }

  impl<'c, 'a> CanaryProbe<'c, 'a> {
      pub fn new(region: DeviceMem<'c, 'a>) -> Self; // wbuf/rbuf = vec![0u8; CANARY_BYTES], seq: 0
      /// Um ciclo: fill(seq) -> write_at(0) -> read_at(0) -> verify(seq); cronometra o round-trip.
      pub fn run(&mut self) -> Result<ProbeOutcome, CudaError>;
  }
  ```
  `run` (passos exatos): `self.seq += 1`; `fill_block(&mut self.wbuf, self.seq, Pattern::Random)`;
  `let t0 = std::time::Instant::now()`; `self.region.write_at(0, &self.wbuf)?`;
  `self.region.read_at(0, &mut self.rbuf)?`; `let latency_us = t0.elapsed().as_micros() as u64`;
  `let content_ok = verify_block(&self.rbuf, self.seq, Pattern::Random)`;
  `Ok(ProbeOutcome { latency_us, content_ok })`.
- **Dependências internas:** `ramshared-cuda`, `ramshared-integrity`.
- **Padrão de referência:** `crates/ramshared-wsl2d/src/residency.rs` (lógica pura + testes).
- **Testes requeridos** (`#[cfg(test)]` no próprio arquivo, sem GPU):
  - `cadence_fires_every_n`: `Cadence::new(64)` → `tick()` retorna `true` exatamente no 64º.
  - `cadence_resets`: após disparar, recomeça do zero.
  - (round-trip real de `run()` é coberto por teste `--ignored` em `lib.rs`/composição — exige GPU.)

## Arquivos a MODIFICAR

### `crates/ramshared-wsl2d/Cargo.toml`

- **O que muda:** adiciona dep de path `ramshared-integrity`.
- **Requisitos:** DT-4.
- **Antes:** deps = `ramshared-cuda`, `ramshared-block`, `ramshared-tier`.
- **Depois:** + `ramshared-integrity = { path = "../ramshared-integrity" }`.
- **Impacto:** nenhum (workspace path dep; sem dep externa).

### `crates/ramshared-wsl2d/src/lib.rs`

- **O que muda:** `pub mod canary_probe;` + re-export.
- **Antes:** `pub mod backend; pub mod residency; pub mod state;` + re-exports.
- **Depois:** + `pub mod canary_probe;` e `pub use canary_probe::{Cadence, CanaryProbe, ProbeOutcome, CANARY_BYTES, CANARY_EVERY};`.
- **Impacto:** API de lib cresce (interna ao crate/bin); `forbid(unsafe_code)` mantido.

### `crates/ramshared-wsl2d/src/residency.rs`

- **O que muda:** `ResidencyConfig::default().free_floor_bytes` de `0` para `64 * 1024 * 1024` (DT-3).
- **Requisitos:** RF-4.
- **Antes:** `free_floor_bytes: 0,`
- **Depois:** `free_floor_bytes: 64 * 1024 * 1024,`
- **Impacto:** os testes existentes passam `free = u64::MAX` (não cruzam o floor) e o
  `free_floor_demotes` fixa `1 << 30` explicitamente → **sem quebra**. Atualizar o
  comentário do default.
- **Testes requeridos:** os 5 existentes continuam verdes (rodar `cargo test -p ramshared-wsl2d`).

### `crates/ramshared-wsl2d/src/main.rs`

- **O que muda:** (a) alocar a região-canário; (b) construir `Cadence` + `CanaryProbe`;
  (c) **substituir** o bloco de amostragem por latência de `serve()` pela sonda em
  cadência, alimentando `(latency_us, content_ok, free)`; erro de sonda/`mem_info` ⇒
  DEMOTE conservador (DT-5).
- **Requisitos:** RF-3, RF-4, RF-5, RF-6, DT-5, DT-6.
- **Função/bloco afetado:** `run()` — alocação após `let mut backend = ...` e o bloco do
  serve loop que hoje faz `c.sample(lat_us, true, u64::MAX)`.
- **Antes (resumo do estado atual):**
  ```rust
  // ... let mut backend = VramBackend::new(mem, BLOCK_SIZE);
  // serve loop:
  let touches_vram = matches!(req.cmd, Command::Read | Command::Write);
  let t0 = std::time::Instant::now();
  let out = serve(&req, &payload, &mut backend);
  let lat_us = t0.elapsed().as_micros() as u64;
  // ... escreve reply ... ; poll de demote_rx ...
  if touches_vram && !demoted && demote_rx.is_none() {
      match canary.as_mut() {
          None => { baseline.push(lat_us); /* arma na 16ª */ }
          Some(c) => { if let Verdict::Demote(reason) = c.sample(lat_us, true, u64::MAX) { /* spawn swapoff */ } }
      }
  }
  ```
- **Depois (decisões fechadas):**
  - Após `backend`: `let canary_region = ctx.alloc(CANARY_BYTES)?;` e
    `let mut probe = CanaryProbe::new(canary_region);` e `let mut cadence = Cadence::new(CANARY_EVERY);`.
  - Remover a cronometragem de `serve()` como fonte do canário (o `serve()` continua,
    sem `t0`/`lat_us` para o canário).
  - O bloco de amostragem passa a:
    ```rust
    if touches_vram && !demoted && demote_rx.is_none() && cadence.tick() {
        // DT-5: não conseguir medir residência ⇒ DEMOTE conservador.
        let sample = match (probe.run(), ctx.mem_info()) {
            (Ok(o), Ok((free, _total))) => Some((o.latency_us, o.content_ok, free as u64)),
            _ => None, // erro CUDA ⇒ trata como Demote
        };
        let verdict = match (sample, canary.as_mut()) {
            (Some((lat, ok, free)), None) => { /* baseline.push(lat); arma na 16ª */ Verdict::Ok }
            (Some((lat, ok, free)), Some(c)) => c.sample(lat, ok, free),
            (None, _) => Verdict::Demote(DemoteReason::Corruption), // conservador (DT-5)
        };
        if let Verdict::Demote(reason) = verdict {
            // mesmo spawn de swapoff confirmado por canal já existente
        }
    }
    ```
    (A montagem exata do baseline e do `spawn` reusa o código atual; só a **fonte** da
    amostra e o gate `cadence.tick()` mudam.)
- **Por quê:** PRD RF-3/RF-4/RF-6 — ativa os 3 gatilhos a partir de uma sonda dedicada.
- **Impacto:** sem uAPI/ABI; o `serve()` e o protocolo NBD não mudam; o caminho de
  DEMOTE (swapoff + canal) é o mesmo.
- **Testes requeridos:** `cargo test -p ramshared-wsl2d` (cadência + decisão);
  `cargo clippy --workspace -D warnings`; re-rodar `cascade-validate.sh`/`cascade-demote.sh`
  (sem regressão §14).
  - **Disciplina Kahneman:** #5/#2 (DT-5 DEMOTE conservador) — ver Mapa acima.

## Documentos a atualizar no mesmo commit do IMPL

- `docs/008-vram-residency-canary/IMPL.md` (Passo 3).
- `docs/vram-as-ram/SPECv3-WSL2.md` §9.4 → implementado.
- `docs/vram-as-ram/IMPL.md`, `ARCHITECTURE.md` (limitação C1 resolvida), `MEMORY.md`.
