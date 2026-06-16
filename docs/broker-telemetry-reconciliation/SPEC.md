# SPEC — Coletor de Telemetria & Reconciliação do Memory Broker

> SSDV3 PASSO 2, a partir de [`PRD.md`](PRD.md). Fecha decisões e traduz os RF em mudanças exatas no
> repo. **Userspace** (`ramshared-broker`, `ramshared-wsl2d`, `ramshared-agent`) — sem uAPI de kernel,
> IRQ, DMA ou lock novo no kernel. Liga-se a [`.claude/rules/benchmarks.md`](../../.claude/rules/benchmarks.md).

## Escopo fechado desta implementação

**Entra agora:**
- Contadores de IO/bytes por slice no data-plane (atômicos compartilhados) + exposição no `StatusReply` (RF-1).
- Telemetria estendida do tenant: `memory.swap.current` (cgroup v2) + `/proc/diskstats` no agente, no `Msg::Psi` (RF-2).
- Atribuição de VRAM por subtração no host (gauge publicado pela amostragem de residência já existente) (RF-3).
- Invariante de reconciliação + flag de divergência, no `on_tick` do broker (RF-4).
- Linha unificada JSONL por amostra (`Outbound::Telemetry` → arquivo via flag `--telemetry-jsonl`) (RF-5).

**Fica fora agora (ver PRD §Fora de escopo):** crate `ramshared-nvml` / DXGI per-PID; exporter
Prometheus; atuar sobre a divergência (o coletor é observador); persistência em DB.

**Dependências assumidas prontas (Confirmado no codebase):** `Msg::Status`/`StatusReply`
(`protocol.rs:46,69`); `SliceMap`/`Slice` (`slices.rs`, `model.rs:30`); `BrokerCore`/`CoreEvent`/
`Outbound`/`on_tick`/`status_reply`/`core_loop` (`broker_srv.rs:40,50,70,128,473,492,726`); data-plane
`WMsg::Job{export}` (`conn.rs:48`); `Context::mem_info()->(free,total)` (`cuda/src/driver.rs:189`);
amostragem de residência (`residency.rs`, `mem_info` já chamada no worker); agente `read_psi`/`read_swaps`
(`agent/src/psi.rs:15,44`) + envio `Msg::Psi` 1 Hz (`agent/src/main.rs:277`).

## Matriz de rastreabilidade PRD → SPEC

| PRD  | Implementação no SPEC |
| ---- | --------------------- |
| RF-1 | ITEM-1, ITEM-2, ITEM-4 |
| RF-2 | ITEM-1, ITEM-5 |
| RF-3 | ITEM-3, ITEM-7 |
| RF-4 | ITEM-7 |
| RF-5 | ITEM-6, ITEM-7 |

## Decisões técnicas

| #    | Decisão | Justificativa |
| ---- | ------- | ------------- |
| DT-1 | Contadores de IO **NÃO** vão no `struct Slice` (como o PRD sugeriu); vão num `Arc<Vec<SliceIoCounters>>` (atômicos), indexado por `SliceId`. | O IO flui na thread do **data-plane** (`conn.rs` `WMsg::Job` → worker), mas `Slice`/`SliceMap` é estado **control-plane single-thread** (sem locks, invariante DT-27/ITEM-8). Mutar `Slice` de outra thread quebraria isso. |
| DT-2 | `StatusReply` ganha um vetor paralelo `slice_io: Vec<SliceIo>` (`{id, bytes_served, io_count}`); `Slice` **não** muda. | `Slice` é o struct de estado+wire com `Eq` derivado (roundtrip tests, `model.rs:29`); manter IO separado evita tocar a máquina de estados e seus testes. |
| DT-3 | Cadência da telemetria = **tick do broker** (`on_tick`, **2 s**, DT-24), não 1 Hz. | O core já tem `on_tick`; reusar. 2 s basta p/ reconciliação; eviction rápida é pega pelo canário (caminho separado, por-request). |
| DT-4 | `vram_alloc_daemon = Σ slice.len (Active\|Draining\|Leased) + CANARY_BYTES`; `vram_outros = max(0, vram_total_used − vram_alloc_daemon)`, com clamp em 0. | Atribuição coarse sem NVML per-PID (RF-3). Clamp absorve skew de amostragem (as 3 fontes não são lidas no mesmo instante atômico). |
| DT-5 | O **gauge de VRAM** (`Arc<VramGauge>` = `{free: AtomicU64, total: AtomicU64}`) é publicado pela amostragem `mem_info()` que o worker **já faz** na residência (§9.4). | `mem_info` tem afinidade de thread CUDA; o worker já a chama; só publicar no gauge evita chamada CUDA cross-thread (R-V2 do SPEC VramProvider). |
| DT-6 | Tolerância da reconciliação: `tol_frac` + `streak` **configuráveis**, defaults provisórios `tol_frac=0.10` (10% de Σslices) e `streak=3` ticks; **calibrados no P0**. | Evitar falso-positivo; número final via medição (Kahneman #3), igual ao `delta_psi` do árbitro (`P0-RESULTS.md §5`). |
| DT-7 | Saída JSONL via novo `Outbound::Telemetry(TelemetrySample)`; a camada de IO (`core_loop`) serializa e **append** no arquivo de `--telemetry-jsonl`; sem flag → no-op. | Mantém o core **puro** (emite dado; não faz IO) — testável; espelha o padrão `Outbound::Log` (`broker_srv.rs:808`). |
| DT-8 | `Msg::Psi.mem: Option<TenantMem>` com `#[serde(default)]`; agente sem cgroup/diskstats manda `None`. | Degrade-graceful (RNF resiliência); roundtrip JSON tolera ausência; **Day-0:** sem produção viva, mas `Option` mantém os testes de roundtrip e dev de versões mistas. |
| DT-9 | `vram_total_used`/`free` vêm de `cuMemGetInfo` (device-wide), **não** per-PID. A atribuição "outros" é por subtração (DT-4), explicitamente anônima. | NVML/DXGI per-PID exige host Windows (GPU-PV) → fora de escopo; sem PII (RNF-LGPD). |

## Fronteira de atomicidade e política de rollback

**Fronteira de atomicidade desta implementação:**
- **Atômico:** cada `WMsg::Job` servido faz `bytes_served += served; io_count += 1` no contador atômico
  daquela slice (`Ordering::Relaxed`, por-op). O incremento individual é atômico.
- **Fora da atomicidade (eventual):** a **reconciliação** lê as 3 fontes em instantes ligeiramente
  distintos (ledger do core, `mem` dos `Psi` mais recentes, gauge de VRAM) → snapshot *eventually
  consistent*; o `tol_frac`+`streak` (DT-6) absorvem o skew. O `StatusReply` é um snapshot coerente o
  suficiente (atomics lidos `Relaxed`).
- **Estados parciais aceitos nesta fase:** amostra com fonte ausente (agente sem cgroup, `mem_info`
  falhou) → campo `None` + `flag = partial`; nunca aborta o broker.

**Política de rollback:**
- **Rollback de aplicação:** desligar `--telemetry-jsonl` → zero emissão (os atomics continuam
  incrementando, custo desprezível). Reversão total dos contadores = `git revert` do ITEM-2.
- **Rollback de migration:** N/A — não há migration (sem DB/esquema).
- **Rollback de dados:** N/A — o único "dado" é o arquivo JSONL **append-only**; deletável sem efeito
  no data-plane. Sem perda de dado de produção possível.
- **Proibido em staging/production:** N/A (feature local, sem produção viva — Day-0).
- **Forward-only:** N/A.

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina Kahneman | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-2 (contadores no hot path) | #5 Availability heuristic (worst-case) + #3 Número | [`KAHNEMAN-DISCIPLINES.md#5-availability-heuristic`](../methodology/KAHNEMAN-DISCIPLINES.md) | O incremento atômico no serve degrada a latência p50/p99 sob carga? | `cargo test --workspace` + smoke VRAM no host: p50 de serve (canário) com vs sem contadores, ≥3 rodadas | p50 de serve > **2×** o baseline (P0 §3, 241 µs) → `git revert` ITEM-2 |
| ITEM-3 / RF-3 (subtração de VRAM) | #1 WYSIATI + #3 Número | [`KAHNEMAN-DISCIPLINES.md#1-wysiati--what-you-see-is-all-there-is`](../methodology/KAHNEMAN-DISCIPLINES.md) | `vram_alloc_daemon` bate com o ledger (Σ len + canário)? O "outros" é estado contingente (registrar)? | smoke VRAM: `vram_alloc_daemon ≈ Σ slice.len ± 1 página`; `vram_outros ≥ 0` | `vram_outros < 0` sistemático → cálculo de daemon-alloc errado, não avançar |
| ITEM-7 (invariante + flag) | #13 Ilusão de validade + #1 WYSIATI | [`KAHNEMAN-DISCIPLINES.md#13-ilusão-de-validade`](../methodology/KAHNEMAN-DISCIPLINES.md) | A divergência é sinal real ou ruído de amostragem? O flag dispara pelo **motivo certo**? | teste `reconcile()` com fixtures: slice presa → `stuck_slice`; convergência idle → `none`; sem falso-positivo na janela idle do P0 | falso-positivo na janela idle medida (P0) → recalibrar `tol_frac`/`streak` (DT-6) antes de avançar |
| ITEM-6 (rollout via flag) | #6 Confiança calibrada | [`KAHNEMAN-DISCIPLINES.md#6-overconfidence--confiança-calibrada`](../methodology/KAHNEMAN-DISCIPLINES.md) | A flag default-off não altera comportamento atual? | drill qemu + smoke sem a flag = comportamento idêntico (sem regressão) | qualquer regressão no drill/smoke sem a flag → bloquear |

## Checklist de segurança (pré-implementação)

- [x] **Isolamento:** o coletor é **read-only** sobre o ledger e `/proc`/cgroup do próprio tenant; não
  altera estado do árbitro/SliceMap (RF-4 observador). `WMsg::Job` já valida `len ≤ export` (`conn.rs:155`).
- [x] **Buffer overflow / OOB:** sem cópia user↔kernel nova (userspace). `parse_memcg`/`parse_diskstats`
  são parsers tolerantes a linha malformada (espelham `parse_swaps`).
- [x] **Permissões:** sem caminho privilegiado novo. `memory.swap.current`/`diskstats` são leitura
  read-only; falham-graceful se sem permissão (`Option=None`).
- [x] **Preempção/IRQ:** N/A (userspace). Hot path ganha só 2 `fetch_add(Relaxed)`.
- [x] **Input validation:** campos do `Msg::Psi.mem` validados no parse (números; `Option` se ausente).
- [x] **Ponteiros/segredos:** a linha de telemetria não carrega endereços de kernel nem segredos
  (KASLR — regra `coding.md`); só bytes/IO/VRAM/PSI agregados.
- [x] **Kernel Oops:** N/A (userspace); erros viram `Option=None`/`flag=partial`, sem panic.

## Arquivos a CRIAR

### `crates/ramshared-wsl2d/src/telemetry.rs`
- **Propósito:** tipos e lógica **pura** de contadores, gauge, amostra e reconciliação (testável sem GPU/rede).
- **Requisitos cobertos:** RF-1, RF-3, RF-4, RF-5, DT-1, DT-4, DT-6, DT-7.
- **Structs/Types:**
  ```rust
  use std::sync::atomic::{AtomicU64, Ordering};

  /// Contadores de IO por slice (data-plane escreve, control-plane lê). DT-1.
  #[derive(Default)]
  pub struct SliceIoCounters { pub bytes_served: AtomicU64, pub io_count: AtomicU64 }

  /// Gauge de VRAM publicado pelo worker (amostragem de residência). DT-5.
  #[derive(Default)]
  pub struct VramGauge { pub free: AtomicU64, pub total: AtomicU64 }

  /// Veredito da reconciliação (RF-4).
  #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
  #[serde(rename_all = "snake_case")]
  pub enum ReconcileFlag { None, Partial, Eviction, StuckSlice, Unaccounted }

  /// Linha unificada por amostra (RF-5). Serializada como 1 objeto JSON por linha.
  #[derive(Clone, Debug, serde::Serialize)]
  pub struct TelemetrySample {
      pub t: u64,                       // epoch secs (injetado pela camada de IO; core não lê relógio)
      pub tenant: Option<String>,
      pub slice: Option<u16>,
      pub swap_used: u64,               // Σ used dos tenants (KiB→bytes)
      pub page_io_s: Option<u64>,
      pub vram_alloc_daemon: u64,
      pub vram_total_used: Option<u64>,
      pub vram_outros: Option<u64>,
      pub canario_demotes: u64,
      pub demote_reason: Option<String>,
      pub reconcile_delta: f64,         // |Σslices − ΣSwapUsed| / Σslices
      pub flag: ReconcileFlag,
      pub branch: Option<String>,
      pub commit: Option<String>,
  }

  /// Entrada da reconciliação (snapshot já coletado do core/gauge). RF-4.
  pub struct ReconcileInput {
      pub sum_slice_bytes: u64,         // Σ len de slices Active|Draining|Leased (ledger)
      pub sum_swap_used: u64,           // Σ used dos tenants (Psi/swaps)
      pub vram_alloc_daemon: u64,       // DT-4
      pub vram_total_used: Option<u64>,
      pub stuck_draining: bool,         // alguma slice Draining além de ZERO_RETRY_ERROR ticks
      pub demotes_delta: u64,           // demotes desde a última amostra
      pub any_source_missing: bool,
  }
  ```
- **Funções:**
  - `pub fn reconcile(inp: &ReconcileInput, tol_frac: f64) -> (f64, ReconcileFlag)` — **pura**:
    1. se `any_source_missing` → `(delta, Partial)`.
    2. `delta = |sum_slice_bytes − sum_swap_used| as f64 / max(1, sum_slice_bytes) as f64`.
    3. desambiguação: `demotes_delta>0` ⇒ `Eviction`; senão `stuck_draining` ⇒ `StuckSlice`;
       senão `vram_outros` cresce sem ledger correspondente (`delta>tol_frac` por causa de VRAM) ⇒
       `Unaccounted`; senão se `delta>tol_frac` ⇒ `Unaccounted`; caso contrário `None`.
    (O `streak` é aplicado **fora** — no `on_tick` — sobre o flag retornado; `reconcile` é stateless.)
  - `pub fn vram_outros(total_used: u64, alloc_daemon: u64) -> u64 { total_used.saturating_sub(alloc_daemon) }` (DT-4 clamp).
- **Dependências internas:** nenhuma (tipos puros). **Externas:** `serde`.
- **Padrão de referência:** `crates/ramshared-wsl2d/src/residency.rs` (lógica pura + testes sem GPU).
- **Testes requeridos:** `#[cfg(test)] mod tests` (com `#![allow(clippy::unwrap_used, clippy::expect_used)]`):
  `reconcile_idle_is_none`, `reconcile_stuck_slice`, `reconcile_eviction_when_demotes`,
  `reconcile_partial_when_source_missing`, `vram_outros_clamps_at_zero`.
- **Disciplina Kahneman:** suporta ITEM-7 → ver mapa (ITEM-7: #13 Ilusão de validade).

## Arquivos a MODIFICAR

### `crates/ramshared-broker/src/protocol.rs`  *(ITEM-1 — RF-1, RF-2)*
- **O que muda:** estender o wire-format (aditivo, retrocompat por `#[serde(default)]`/`Option`).
- **Função/bloco afetado:** `enum Msg` (`:19`), `struct TenantStatus` (`:98`); **novos** `TenantMem`, `SliceIo`.
- **Antes:** `Psi { sample, swaps }` (`:26`); `TenantStatus { id, name, psi, slices, present }` (`:98`);
  `StatusReply { tenants, slices, last_rebalance_secs }` (`:69`).
- **Depois:**
  ```rust
  Psi { sample: PsiSample, swaps: Vec<SwapEntry>, #[serde(default)] mem: Option<TenantMem> },
  // ...
  StatusReply { tenants: Vec<TenantStatus>, slices: Vec<Slice>,
                #[serde(default)] slice_io: Vec<SliceIo>, last_rebalance_secs: Option<u64> },

  #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
  pub struct TenantMem { pub swap_current: u64, pub diskstats_io: u64 } // bytes, ios acumulados

  #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
  pub struct SliceIo { pub id: SliceId, pub bytes_served: u64, pub io_count: u64 }
  // TenantStatus += pub bytes_served: u64  (agregado por tenant)
  ```
- **Por quê:** RF-1 (IO por slice/tenant) + RF-2 (cgroup/diskstats no Psi).
- **Impacto:** **ABI userspace JSON** — aditivo; `#[serde(default)]` mantém roundtrip. **Quebra os
  testes `roundtrip_each_variant` (`:157`)** que constroem `Psi`/`StatusReply`/`TenantStatus` por
  campo → atualizar os literais no mesmo commit (DT-12 docs vivas). Não há ABI de kernel.
- **Testes requeridos:** atualizar `roundtrip_each_variant`; novo `psi_mem_default_when_absent`
  (desserializar `{"type":"psi",...}` sem `mem` → `None`).

### `crates/ramshared-wsl2d/src/main.rs`  *(ITEM-2, ITEM-3, ITEM-6 — RF-1, RF-3, RF-5)*
- **O que muda:** (a) criar `Arc<Vec<SliceIoCounters>>` e `Arc<VramGauge>` em `run_broker`; (b) passar
  o `Arc` dos contadores ao **worker** e incrementar por `Job` servido; (c) publicar `mem_info()` no
  gauge na amostragem de residência já existente; (d) `core_loop` dispatcher trata `Outbound::Telemetry`;
  (e) flag `--telemetry-jsonl <path>`.
- **Função/bloco afetado:** `run_broker` (genérico sobre `VramProvider`); o worker serve loop (onde
  `WMsg::Job` é servido via `SliceView`); o laço de residência (`residency_check`/§9.4 que chama
  `mem_info`); o parsing de args (`:220–326`).
- **Antes:** worker serve `Job` e devolve resultado; residência chama `provider.mem_info()` e descarta
  o `total`; args sem flag de telemetria; dispatcher trata `ToSession/CloseSession/ZeroSlice/Log`.
- **Depois:** worker faz `slice_io[job.export].bytes_served.fetch_add(served as u64, Relaxed)` +
  `io_count.fetch_add(1, Relaxed)` após servir; residência faz `gauge.free.store(free); gauge.total.store(total)`;
  `Outbound::Telemetry(s)` → serializa `s` (com `t`/`branch`/`commit` carimbados aqui) e **append** no
  arquivo (`OpenOptions::append`), best-effort (erro = `eprintln` warn, não aborta); flag nova
  `--telemetry-jsonl` (default `None` = desligado).
- **Por quê:** RF-1 (contar bytes onde fluem), RF-3 (gauge), RF-5 (sink JSONL), DT-3/DT-5/DT-7.
- **Impacto:** hot path do serve ganha 2 atomics (Kahneman ITEM-2). Sem mudança de ABI. `--backend ram`
  (drill qemu) não tem gauge de VRAM → telemetria com `vram_*=None` (degrade).
- **Testes requeridos:** drill qemu ublk-RAM PASS (sem regressão); smoke VRAM com `--telemetry-jsonl`
  gera linhas válidas; cobertura da serialização via teste de `telemetry.rs`.
- **Disciplina Kahneman:** ITEM-2 (#5) e ITEM-3 (#1/#3) — ver mapa.

### `crates/ramshared-wsl2d/src/broker_srv.rs`  *(ITEM-4, ITEM-7 — RF-1, RF-4, RF-5)*
- **O que muda:** `BrokerCore` passa a ter referências aos contadores/gauge + estado de demote +
  `mem` por tenant; `status_reply` mescla `slice_io` e `bytes_served`/`mem` por tenant; `on_demote`
  conta; o handler de `Msg::Psi` guarda `mem`; `on_tick` calcula a reconciliação e empurra
  `Outbound::Telemetry`. Novo variante `Outbound::Telemetry`.
- **Função/bloco afetado:** `struct BrokerCore` (`:70`), `struct TenantState` (`:60`), `enum Outbound`
  (`:50`), `fn status_reply` (`:473`), `fn on_demote` (`:435`), handler `Msg::Psi` (em `handle`/`on_msg`),
  `fn on_tick` (`:492`), `BrokerCore::new` (`:` construtor).
- **Antes:** `Outbound { ToSession, CloseSession, ZeroSlice, Log }`; `TenantState { ..., psi, reconciled }`;
  `status_reply` monta `tenants`+`slices`; `on_demote` só loga+`DemoteAll`; `on_tick` faz árbitro+R4.
- **Depois:**
  - `enum Outbound` += `Telemetry(TelemetrySample)`.
  - `TenantState` += `mem: Option<TenantMem>` (último do `Psi`).
  - `BrokerCore` += `slice_io: Arc<Vec<SliceIoCounters>>`, `vram: Arc<VramGauge>`,
    `demotes_total: u64`, `last_demote_reason: Option<String>`, `demotes_at_last_sample: u64`,
    `recon_streak: u32`, `tol_frac: f64`.
  - handler `Msg::Psi { sample, swaps, mem }`: guarda `mem` no `TenantState` (além do que já faz).
  - `on_demote`: `self.demotes_total += 1; self.last_demote_reason = Some(reason.into())` (mantém o
    `DemoteAll`).
  - `status_reply`: inclui `slice_io` (lendo `self.slice_io[i]` `Relaxed`) e `TenantStatus.bytes_served`
    (Σ dos `slice_io` das slices do tenant) e o `mem`.
  - `on_tick`: monta `ReconcileInput` (Σ len de slices Active|Draining|Leased; Σ swap used dos tenants;
    `vram_alloc_daemon`=Σ len+`CANARY_BYTES`; `vram_total_used`=`total−free` do gauge; `stuck_draining`
    de `pending_zero`≥`ZERO_RETRY_ERROR`; `demotes_delta`); chama `telemetry::reconcile`; aplica `streak`
    (DT-6) e empurra `Outbound::Telemetry(sample)` (campos `t`/`branch`/`commit` ficam `None` aqui —
    a camada de IO carimba, DT-7).
  - `BrokerCore::new`: recebe os `Arc` + `tol_frac` (novos params).
- **Por quê:** RF-1 (expor IO), RF-4 (reconciliar+flag), RF-5 (emitir amostra).
- **Impacto:** `BrokerCore::new` muda assinatura → ajustar callers (`main.rs run_broker` + os testes
  `broker_srv.rs:1078+`). `Outbound` ganha variante → o `match` do dispatcher (`:808`) precisa do braço
  novo (exaustivo).
- **Testes requeridos:** `status_reply_includes_slice_io`; `on_tick_emits_telemetry_with_flag`;
  `eviction_flag_when_demote_seen` (injeta `CoreEvent::Demote` antes do tick). Atualizar
  `status_reply_lists_tenants_and_slices` (`:1086`).
- **Disciplina Kahneman:** ITEM-7 (#13/#1) — ver mapa.

### `crates/ramshared-agent/src/psi.rs`  *(ITEM-5 — RF-2)*
- **O que muda:** adicionar leitura+parse de `memory.swap.current` (cgroup v2) e `/proc/diskstats`.
- **Função/bloco afetado:** novas `read_memcg_swap`/`parse_memcg_swap`, `read_diskstats`/
  `parse_diskstats(content, dev)`; padrão idêntico a `read_swaps`/`parse_swaps` (`:44,50`).
- **Antes:** só `read_psi`, `read_swaps`, `read_euid`.
- **Depois:**
  ```rust
  /// Lê memory.swap.current do cgroup v2 do escopo atual (via /proc/self/cgroup → caminho). None se ausente.
  pub fn read_memcg_swap() -> Option<u64>;
  pub fn parse_memcg_swap(content: &str) -> Option<u64>; // 1 inteiro (bytes)
  /// Soma sectors-read+written do device de swap em /proc/diskstats (campos 6 e 10). None se ausente.
  pub fn read_diskstats(dev: &str) -> Option<u64>;
  pub fn parse_diskstats(content: &str, dev: &str) -> Option<u64>;
  ```
- **Por quê:** RF-2 — dar ao broker `swap_used` (cgroup) e `page_io` por tenant.
- **Impacto:** nenhum em ABI; leitura read-only; `Option` quando ausente (DT-8 degrade).
- **Testes requeridos:** `parse_memcg_swap_reads_integer`, `parse_memcg_swap_max_is_none` (`"max"`),
  `parse_diskstats_sums_rw_for_dev`, `parse_diskstats_unknown_dev_is_none` (fixtures, sem tocar `/proc`).

### `crates/ramshared-agent/src/main.rs`  *(ITEM-5 — RF-2)*
- **O que muda:** montar `Msg::Psi { sample, swaps, mem }` com `mem` das novas leituras.
- **Função/bloco afetado:** `session` loop, construção do `Msg::Psi` (`:277`).
- **Antes:** `Msg::Psi { sample, swaps }` (`:277`).
- **Depois:** `let mem = psi::read_memcg_swap().map(|s| TenantMem { swap_current: s, diskstats_io: psi::read_diskstats(&swap_dev).unwrap_or(0) });` → `Msg::Psi { sample, swaps, mem }`.
- **Por quê:** RF-2. **Impacto:** retrocompat (DT-8); se cgroup ausente, `mem=None`.
- **Testes requeridos:** coberto pelos parsers (`psi.rs`); o envio é exercitado pelo e2e civm (Q1d).

## Arquivos a DELETAR

| Arquivo | Motivo |
| --- | --- |
| — | nenhum (Day-0: sem código morto/compat a remover) |

## Observabilidade

**Métricas Prometheus:** N/A no MVP (DT — sem exporter). A observabilidade é a **própria saída** JSONL
+ o `StatusReply` (pull).

**Saída estruturada (JSONL, RF-5):** 1 objeto `TelemetrySample` por linha em `--telemetry-jsonl`
(default `docs/benchmarks/results.jsonl` quando rodando como benchmark, por `.claude/rules/benchmarks.md`).

**Logs estruturados (eprintln existente, estendido):**

| Evento | Level | Campos |
| --- | --- | --- |
| Divergência detectada | `error` (`Outbound::Log`) | `flag`, `reconcile_delta`, `demote_reason` |
| Amostra parcial | `warn` | fonte ausente (`mem`/`vram`) |
| Telemetria desligada | — | sem flag → silencioso |

## Contratos e documentação viva

| Documento | Atualização | Motivo |
| --- | --- | --- |
| `Documentation/` (uAPI/ABI) | N/A | userspace; sem ioctl/sysfs/ABI de kernel |
| `Kconfig` | N/A | sem CONFIG_/module param |
| `CLAUDE.md` | N/A | nenhum padrão estrutural novo |
| `.claude/rules/benchmarks.md` | N/A (já referencia) | o `results.jsonl` é o destino que a regra já prevê |
| `docs/decisions/ADR-NNN` | N/A | sem decisão arquitetural de kernel; DTs ficam aqui |
| `docs/methodology/KAHNEMAN-DISCIPLINES.md` | N/A | usa disciplinas existentes |
| `docs/broker-telemetry-reconciliation/IMPL.md` | **Criar** (PASSO 3) | registrar commits/decisões/métricas |
| `docs/memory-broker/P0-RESULTS.md` | **Alterar** | calibração de `tol_frac`/`streak` (DT-6), nova célula |
| `docs/memory-broker/SPECv2.md` | **Alterar** | registrar `TenantMem`/`SliceIo`/`Outbound::Telemetry` nos DTs do broker |

## Ordem de implementação

1. **ITEM-1** — tipos de wire em `protocol.rs` (`TenantMem`, `SliceIo`, `Psi.mem`, `StatusReply.slice_io`,
   `TenantStatus.bytes_served`) + atualizar roundtrip tests. *(compila isolado)*
2. **ITEM (telemetry.rs)** — criar `telemetry.rs` (tipos puros + `reconcile` + testes). *(compila isolado)*
3. **ITEM-2** — `SliceIoCounters` + incremento no worker de `run_broker` + `Arc` em `main.rs`.
4. **ITEM-3** — `VramGauge` + publicação na amostragem de residência.
5. **ITEM-4** — `BrokerCore` (campos + `status_reply` + `on_demote` + handler `Psi`) + `Outbound::Telemetry`.
6. **ITEM-5** — agente: `read_memcg_swap`/`read_diskstats` (`psi.rs`) + `Msg::Psi.mem` (`main.rs`).
7. **ITEM-6** — sink JSONL no dispatcher + flag `--telemetry-jsonl`.
8. **ITEM-7** — reconciliação no `on_tick` + `streak` + emissão da amostra.
9. Validação (testes + drill + smoke) e docs vivas.

## Plano de testes

**Backend (Rust):**
- **Unitários:** `telemetry::reconcile` (idle/stuck/eviction/partial), `vram_outros` clamp;
  `parse_memcg_swap`/`parse_diskstats` (fixtures); `protocol` roundtrip (incl. `Psi.mem` default).
- **Integração:** `status_reply_includes_slice_io`; `on_tick_emits_telemetry`; `eviction_flag_when_demote`
  (in-process no `BrokerCore`, sem GPU/rede).
- **Isolamento/atomicidade:** teste concorrente — N threads incrementando `SliceIoCounters` →
  soma final exata (sem perda); o core lê snapshot coerente.
- **Concorrência:** o worker (1 thread) escreve, o core (1 thread) lê → `Relaxed` é suficiente
  (sem ordering entre contadores distintos); documentar no `telemetry.rs`.

**Drivers/GPU:**
- smoke VRAM server-only (RTX 2060) com `--telemetry-jsonl /tmp/t.jsonl`: linhas válidas;
  `vram_alloc_daemon ≈ Σ len`; `vram_outros ≥ 0` (sobe ao abrir app gráfico).

**Manuais:**
- `nc` + `jq` no socket do broker mandando `{"type":"status"}` → `StatusReply` com `slice_io` (ADR-0005).
- e2e civm (Q1d, sessão "juntos"): flag `eviction`/`stuck_slice` sob carga real; evidência objetiva do
  mapa Kahneman (ITEM-7).

## Checklist de validação

**Backend:**
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `./scripts/checkpatch.pl` — N/A (sem C)

**Drivers/GPU:**
- [ ] smoke VRAM server-only com `--telemetry-jsonl` (linhas JSON válidas, `jq` parseia)
- [ ] drill qemu ublk-RAM PASS **sem** a flag (zero regressão, RNF-4)

**Docs:**
- [ ] `IMPL.md` criado (PASSO 3); `P0-RESULTS.md` com a célula de calibração `tol_frac`/`streak`

**Gates cognitivos:**
- [ ] ITEM-2, ITEM-3, ITEM-7, ITEM-6 apontam disciplina + link em `KAHNEMAN-DISCIPLINES.md`
- [ ] Cada etapa crítica tem pergunta obrigatória + evidência mínima + abort trigger (mapa acima)
- [ ] Sem linguagem vaga em ponto crítico (a tolerância da reconciliação é número, DT-6, calibrado no P0)
