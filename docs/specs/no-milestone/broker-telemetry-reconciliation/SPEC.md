# SPEC — Coletor de Telemetria & Reconciliação do Memory Broker

> **Arquivo único** `SPEC.md` (modelo Advoq). Passo 2.5 revisa in-place; histórico = `git log` — sem `SPECvN.md`.
> **Revisado após auditoria do Passo 2.5:** corrige os blockers F1 (ponto de incremento exato), F2/F12 (invariante conflacionava
> capacidade × ocupação × throughput; `reconcile()` referenciava sinais fora do `ReconcileInput`),
> F3 (timestamp/`PartialEq` da amostra), F8 (`swap_dev` do diskstats), F9 (fonte do `swap_used`),
> além de F4/F5/F6/F7.

> **SPEC auditado:** `SPEC.md`. **Blockers endereçados (1ª rodada):** F1, F2, F3, F5, F8, F9 (+ F4, F6, F7).
> **Re-auditado (Passo 2.5, 2ª rodada) → `no-go`; corrigido in-place:** F-v2-1 (ordem do `delta` em
> `reconcile`), F-v2-2 (`TenantState.occupied_bytes` p/ o invariante), F-v2-3 (unidades KiB→bytes),
> F-v2-4 (`branch`/`commit` via env, não `git`), F-v2-5 (fiação do sink no `core_loop`).
> **Esta versão é o candidato ativo** para nova auditoria (Passo 2.5) / implementação (Passo 3).
>
> SSDV3 PASSO 2. Userspace (`ramshared-broker`, `ramshared-wsl2d`, `ramshared-agent`). Sem uAPI de
> kernel/IRQ/DMA/lock de kernel. Liga-se a [`.claude/rules/benchmarks.md`](../../../../.claude/rules/benchmarks.md).

## Escopo fechado desta implementação

**Entra agora:** RF-1 (contadores de IO/bytes por slice, atômicos compartilhados, no data-plane);
RF-2 (telemetria estendida do tenant: `/proc/swaps` filtrado + `memory.swap.current` + `/proc/diskstats`);
RF-3 (VRAM por subtração via gauge publicado pela closure de residência); RF-4 (invariante de
**ocupação** + flag); RF-5 (linha JSONL por amostra).

**Fora agora:** `ramshared-nvml`/DXGI per-PID; exporter Prometheus; atuar sobre a divergência
(observador); persistência em DB.

**Dependências prontas (Confirmado no codebase):** `Msg::Status`/`StatusReply` (`protocol.rs:46,69`);
`SliceMap`/`Slice` (`slices.rs`, `model.rs:30`); `BrokerCore`/`CoreEvent`/`Outbound`/`on_tick`/
`status_reply`/`core_loop`/`dev_to_slice` (`broker_srv.rs:40,50,70,128,473,492,726,+`); **worker do
broker `serve_broker_jobs<B: BlockBackend>(backend, rt: &BrokerRuntime, residency: impl FnMut(u64)->Option<DemoteReason>)`**
(`main.rs:665`), serve em `serve(&job.req,&job.payload,&mut view)` (:705) com `job.export` = slice;
`run_broker`/`run_nbd` alocam `canary_region`+`CanaryProbe` e a closure de residência chama
`provider.mem_info()` (`main.rs:420,520,420`); `WMsg::Job(Job{export,req,payload,reply})` (`conn.rs:48`);
agente `read_psi`/`read_swaps` (`agent/psi.rs:15,44`), envio `Msg::Psi` 1 Hz (`agent/main.rs:277`).

## Matriz de rastreabilidade PRD → SPEC

| PRD  | Implementação no SPEC |
| ---- | ----------------------- |
| RF-1 | ITEM-1, ITEM-2, ITEM-5 |
| RF-2 | ITEM-1, ITEM-6 |
| RF-3 | ITEM-3, ITEM-7 |
| RF-4 | ITEM-4 (`telemetry.rs`), ITEM-7 |
| RF-5 | ITEM-4, ITEM-7, ITEM-8 |

## Decisões técnicas

| #    | Decisão | Justificativa |
| ---- | ------- | ------------- |
| DT-1 | Contadores de IO em `Arc<Vec<SliceIoCounters>>` (atômicos), **não** no `struct Slice`. | IO flui na thread do data-plane (`serve_broker_jobs`); `Slice`/`SliceMap` é control-plane single-thread lock-free (DT-27). |
| DT-2 | `StatusReply` ganha `slice_io: Vec<SliceIo>` paralelo; `Slice` não muda. | `Slice` é estado+wire com `Eq` (roundtrip tests, `model.rs:29`). |
| DT-3 | Cadência da telemetria = `on_tick` do broker (2 s, DT-24). | Reusa o tick existente; eviction rápida vem do canário (por-request). |
| **DT-4 (revisado F2)** | O **invariante é de OCUPAÇÃO**, não de throughput: compara `alloc_active = Σ slice.len (Active\|Draining)` (capacidade emprestada) com `occupied = Σ used` dos **nossos** nbd devices. `bytes_served`/`io_count` (RF-1) são **throughput** (alimentam `page_io_s`), **fora** do invariante. | Capacidade, ocupação e throughput são grandezas distintas; misturá-las dava reconciliação sem sentido (F2). |
| **DT-5 (revisado F5)** | `vram_alloc_daemon = alloc_active + CANARY_BYTES` (só backend VRAM; RAM não tem canário). O `Arc<VramGauge>{free,total}` é publicado **dentro da closure de residência** de `run_broker` (onde `provider.mem_info()` já roda, cadência §9.4). `total==0` ⇒ sem dado de VRAM ⇒ campos `None` (sentinela, F6). | A closure de residência é o único ponto thread-afim que chama `mem_info`; RAM (qemu) não publica → `None`. |
| **DT-6 (revisado F2/F12)** | **Eviction é detectada pelo canário** (`demotes_delta>0`), **não** por subtração de VRAM (WDDM-evicted continua "alocada" no `cuMemGetInfo`). `vram_outros` (subtração) é indicador **informativo** de pressão gráfica, emitido mas **não** usado em `reconcile()`. | `cuMemGetInfo` não distingue VRAM residente de evicção; o sinal real é a latência (canário). |
| DT-7 | `tol_frac` + `streak` configuráveis; defaults provisórios `tol_frac=0.10`, `streak=3` ticks; **calibrados no P0**. | Número, não adjetivo (#3); igual ao `delta_psi` (`P0-RESULTS §5`). |
| **DT-8 (revisado F3)** | Dois tipos: **`TelemetryCore`** (o core emite; `Clone+Debug+PartialEq`; **sem** `t`/`branch`/`commit`) e **`TelemetrySample`** (a camada de IO embrulha, **adicionando** `t`=epoch e `branch`/`commit`; `Serialize`). `Outbound::Telemetry(TelemetryCore)`. | O core tem só `now: Instant` (monotônico, não epoch) e não deve ler relógio; a IO carimba o wall-clock. Resolve o campo obrigatório + o `PartialEq` exigido por `Outbound` (`broker_srv.rs:50`). |
| DT-9 | `Msg::Psi.mem: Option<TenantMem>` com `#[serde(default)]`. | Degrade-graceful; roundtrip tolera ausência. |
| **DT-10 (revisado F9)** | `occupied` conta **só os nossos NBD devices**: filtra `Msg::Psi.swaps` por identidade exata `nbd[0-9]+` nos formatos aceitos (`/dev/nbdN`, `/nbdN`, `nbdN`) e casa com slices `Active`. Nomes apenas parecidos, outros block devices e entradas `(deleted)` não são slices. `/proc/swaps` (`used_kb`) é a **fonte primária**; `memory.swap.current` (cgroup) é cross-check **opcional** (só significativo sob cgroup confinado). | O invariante é sobre o que ocupamos NAS NOSSAS slices; swap local do tenant é outra coisa (reportada à parte). A identidade exata impede que `/dev/sda5` readote ou contabilize a slice 5. |
| **DT-11 (revisado F8)** | O agente rastreia o conjunto de nbd devices que ele fez `swapon` (dos `SwapOn` executados); `diskstats_io = Σ read_diskstats(dev)` sobre eles. | `read_diskstats` precisa de um device concreto; o agente é quem sabe quais montou. |
| **DT-12 (revisado F4)** | `streak`: o `on_tick` mantém `(last_flag, count)`. Conta ticks consecutivos com o **mesmo** flag não-`None`; só **emite o flag** (≠`None`) quando `count ≥ streak`; reseta em `None` ou troca de flag. Abaixo do limiar, a amostra sai com `flag=None` (pendente). | Remove a vagueza "aplica o streak"; histerese explícita (igual ao árbitro). |

## Fronteira de atomicidade e política de rollback

**Atômico:** cada `WMsg::Job` servido em `serve_broker_jobs` faz `bytes_served.fetch_add(req.len, Relaxed)`
+ `io_count.fetch_add(1, Relaxed)` na slice `job.export`. Incremento individual atômico.
**Fora da atomicidade (eventual, F7):** a reconciliação lê 3 fontes em instantes distintos; `tol_frac`+
`streak` absorvem o skew. `status_reply`/`on_tick` leem os pares `(bytes,io)` `Relaxed` **sem** garantia
de leitura conjunta atômica — skew de um tick é **aceito** (telemetria, não contabilidade financeira).
**Estados parciais aceitos:** fonte ausente → campo `None` + `flag=Partial`; nunca aborta o broker.

**Rollback:**
- **App:** desligar `--telemetry-jsonl` → zero emissão (atomics seguem, custo desprezível). Reverter
  contadores = `git revert` ITEM-2.
- **Migration:** N/A (sem DB/esquema). **Dados:** N/A — só o JSONL append-only, deletável.
- **Proibido staging/prod / forward-only:** N/A (feature local, sem produção viva — Day-0).

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-2 (contadores no hot path de `serve_broker_jobs`) | #5 Availability + #3 Número | [`kahneman-disciplines.md#5-availability-heuristic`](../../../methodology/kahneman-disciplines.md#disc-5) | 2 `fetch_add(Relaxed)`/op degradam p50/p99 do serve? | smoke VRAM: p50 de serve com vs sem contadores, ≥3 rodadas | p50 > **2×** baseline (P0 §3, 241 µs) → `git revert` ITEM-2 |
| ITEM-3 / RF-3 (gauge + subtração) | #1 WYSIATI + #3 | [`#1`](../../../methodology/kahneman-disciplines.md#disc-1) | `vram_alloc_daemon` casa com `Σ len + canário`? "outros" é estado contingente (registrar)? | smoke VRAM: `vram_alloc_daemon ≈ Σ len ± 1 página`; `vram_outros ≥ 0` | `vram_outros<0` sistemático → cálculo errado, não avançar |
| ITEM-4/7 (invariante de ocupação + flag) | #13 Ilusão de validade + #1 | [`#13`](../../../methodology/kahneman-disciplines.md#disc-13) | A divergência é sinal real ou ruído? O flag dispara pelo motivo certo (eviction=canário, não subtração)? | testes `reconcile()` fixtures: `occupied>alloc → Unaccounted`; `demotes>0 → Eviction`; idle → `None`; sem falso-positivo na janela idle do P0 | falso-positivo na janela idle (P0) → recalibrar `tol_frac`/`streak` (DT-7) antes de avançar |
| ITEM-8 (rollout via flag) | #6 Confiança calibrada | [`#6`](../../../methodology/kahneman-disciplines.md#disc-6) | Flag default-off não muda o comportamento atual? | drill qemu + smoke **sem** a flag = idêntico | qualquer regressão sem a flag → bloquear |

## Checklist de segurança (pré-implementação)

- [x] **Isolamento:** coletor read-only sobre ledger + `/proc`/cgroup do próprio tenant; **não** muta
  árbitro/SliceMap (RF-4 observador). `serve` já valida `len ≤ export` (`conn.rs:155`).
- [x] **OOB:** sem cópia user↔kernel nova; `parse_memcg_swap`/`parse_diskstats` toleram linha malformada
  (espelham `parse_swaps`, `psi.rs:50`).
- [x] **Permissões:** sem caminho privilegiado novo; leituras read-only; falham-graceful (`None`).
- [x] **Hot path:** só 2 `fetch_add(Relaxed)`/op (gate ITEM-2).
- [x] **Segredos/KASLR:** a linha não carrega endereços de kernel nem segredos (regra `coding.md`).
- [x] **Sem panic:** erros viram `None`/`flag=Partial`.

## Arquivos a CRIAR

### `crates/ramshared-wsl2d/src/telemetry.rs`  *(ITEM-4 — RF-4, RF-5, DT-1/4/6/8/12)*
- **Propósito:** tipos compartilhados + lógica **pura** de reconciliação (testável sem GPU/rede).
- **Requisitos:** RF-1 (tipos de contador), RF-3 (gauge + `vram_outros`), RF-4 (`reconcile`), RF-5 (amostra).
- **Structs/Types:**
  ```rust
  use std::sync::atomic::AtomicU64;

  #[derive(Default)]
  pub struct SliceIoCounters { pub bytes_served: AtomicU64, pub io_count: AtomicU64 } // DT-1
  #[derive(Default)]
  pub struct VramGauge { pub free: AtomicU64, pub total: AtomicU64 }                  // DT-5

  #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
  #[serde(rename_all = "snake_case")]
  pub enum ReconcileFlag { None, Partial, Eviction, StuckSlice, Unaccounted }

  /// Entrada PURA da reconciliação (já coletada do core/gauge). DT-4/DT-6.
  pub struct ReconcileInput {
      pub alloc_active_bytes: u64,   // Σ slice.len (Active|Draining)
      pub occupied_swap_bytes: u64,  // Σ used das NOSSAS nbd devices (DT-10)
      pub stuck_draining: bool,      // alguma slice em pending_zero ≥ ZERO_RETRY_ERROR
      pub demotes_delta: u64,        // demotes do canário desde a última amostra (DT-6)
      pub any_source_missing: bool,  // alguma fonte ausente → Partial
  }

  /// Amostra emitida pelo CORE (sem t/branch/commit — DT-8). PartialEq p/ entrar em Outbound.
  #[derive(Clone, Debug, PartialEq, serde::Serialize)]
  pub struct TelemetryCore {
      pub tenant: Option<String>, pub slice: Option<u16>,
      pub swap_used: u64, pub alloc_active: u64, pub page_io_s: Option<u64>,
      pub vram_alloc_daemon: u64, pub vram_total_used: Option<u64>, pub vram_outros: Option<u64>,
      pub canario_demotes: u64, pub demote_reason: Option<String>,
      pub reconcile_delta: f64, pub flag: ReconcileFlag,
  }

  /// Linha final (a camada de IO embrulha o TelemetryCore — DT-8). 1 objeto JSON/linha (RF-5).
  #[derive(Clone, Debug, serde::Serialize)]
  pub struct TelemetrySample {
      pub t: u64, pub branch: Option<String>, pub commit: Option<String>,
      #[serde(flatten)] pub core: TelemetryCore,
  }
  ```
- **Funções (puras):**
  - `pub fn reconcile(inp: &ReconcileInput, tol_frac: f64) -> (f64, ReconcileFlag)` (F-v2-1: `delta`
    computado **primeiro**):
    1. `let delta = (inp.occupied_swap_bytes as f64 - inp.alloc_active_bytes as f64) / inp.alloc_active_bytes.max(1) as f64;`
    2. `if inp.any_source_missing { return (delta, Partial) }`
    3. `if inp.demotes_delta > 0 { return (delta, Eviction) }`  // canário é a autoridade (DT-6)
    4. `if inp.stuck_draining { return (delta, StuckSlice) }`
    5. `if delta > tol_frac { return (delta, Unaccounted) }`     // ocupou mais do que emprestamos
    6. `(delta, None)`
  - `pub fn vram_outros(total_used: u64, alloc_daemon: u64) -> u64 { total_used.saturating_sub(alloc_daemon) }` (DT-5 clamp; F6: chamado só quando `total>0`).
- **Dependências:** internas: nenhuma; externas: `serde`.
- **Padrão de referência:** `residency.rs` (lógica pura + testes sem GPU).
- **Testes:** `reconcile_idle_none`, `reconcile_unaccounted_when_occupied_gt_alloc`,
  `reconcile_eviction_when_demotes`, `reconcile_stuckslice`, `reconcile_partial_when_missing`,
  `vram_outros_clamps`. (`#![allow(clippy::unwrap_used, clippy::expect_used)]`.)
- **Disciplina Kahneman:** suporta ITEM-4/7 (#13) — ver mapa.

## Arquivos a MODIFICAR

### `crates/ramshared-broker/src/protocol.rs`  *(ITEM-1 — RF-1, RF-2)*
- **O que muda / Depois:** (aditivo, retrocompat por `#[serde(default)]`/`Option`)
  ```rust
  Psi { sample: PsiSample, swaps: Vec<SwapEntry>, #[serde(default)] mem: Option<TenantMem> },
  StatusReply { tenants: Vec<TenantStatus>, slices: Vec<Slice>,
                #[serde(default)] slice_io: Vec<SliceIo>, last_rebalance_secs: Option<u64> },
  // novos:
  pub struct TenantMem { pub swap_current: Option<u64>, pub diskstats_io: u64 } // DT-10/DT-11
  pub struct SliceIo { pub id: SliceId, pub bytes_served: u64, pub io_count: u64 }
  // TenantStatus += pub bytes_served: u64
  ```
- **Antes:** `Psi { sample, swaps }` (:26); `StatusReply { tenants, slices, last_rebalance_secs }` (:69);
  `TenantStatus { id, name, psi, slices, present }` (:98).
- **Por quê:** RF-1 + RF-2. **Impacto:** ABI JSON aditiva; **quebra `roundtrip_each_variant` (:157)** →
  atualizar literais no mesmo commit. Sem ABI de kernel.
- **Testes:** atualizar roundtrip; novo `psi_mem_defaults_to_none`.

### `crates/ramshared-wsl2d/src/broker_srv.rs`  *(ITEM-5, ITEM-7 — RF-1, RF-4, RF-5)*
- **O que muda:** `Outbound` += `Telemetry(TelemetryCore)`; `TenantState` += `mem: Option<TenantMem>` **e** `occupied_bytes: u64` (F-v2-2: dado que o invariante soma no tick);
  `BrokerCore` += `slice_io: Arc<Vec<SliceIoCounters>>`, `vram: Arc<VramGauge>`, `demotes_total: u64`,
  `last_demote_reason: Option<String>`, `demotes_at_last_sample: u64`, `recon: (ReconcileFlag, u32)`
  (streak DT-12), `tol_frac: f64`, `streak_cfg: u32`; `BrokerCore::new` recebe `slice_io`,`vram`,`tol_frac`,`streak_cfg`.
- **`status_reply` (:473):** inclui `slice_io` (lê `self.slice_io[i]` `Relaxed`) + `TenantStatus.bytes_served`
  (Σ dos `slice_io` das slices Active do tenant) + `mem`.
- **handler `Msg::Psi { sample, swaps, mem }`:** guarda `mem` e **recomputa `occupied_bytes`** =
  Σ `used_kb*1024` (F-v2-3) das `swaps` cujo `dev_to_slice(dev)` casa uma slice `Active` deste tenant
  (DT-10/F-v2-2).
- **`on_demote` (:435):** `self.demotes_total += 1; self.last_demote_reason = Some(reason.to_string())` (mantém `DemoteAll`).
- **`on_tick` (:492):** monta `ReconcileInput` — `alloc_active_bytes`=Σ len de Active|Draining;
  `occupied_swap_bytes`=Σ `TenantState.occupied_bytes` dos tenants presentes (já filtrado+convertido no
  handler do `Psi`, DT-10/F-v2-2); `stuck_draining`=algum `pending_zero ≥ ZERO_RETRY_ERROR`; `demotes_delta`=
  `demotes_total - demotes_at_last_sample` (e atualiza); `any_source_missing` se `vram.total==0` ou sem `mem`.
  Chama `telemetry::reconcile`; aplica `streak` (DT-12); calcula `vram_total_used = (total>0).then(total-free)`,
  `vram_outros = vram_total_used.map(|u| telemetry::vram_outros(u, alloc_active+CANARY_BYTES))`; monta
  `TelemetryCore` e empurra `Outbound::Telemetry(core)`.
- **Por quê:** RF-1/RF-4/RF-5. **Impacto:** `BrokerCore::new` muda assinatura → ajustar `run_broker` +
  testes (`:1078+`); `match` de `Outbound` no dispatcher ganha braço (exaustivo).
- **Testes:** `status_reply_includes_slice_io`; `on_tick_emits_telemetry`; `eviction_flag_after_demote`
  (injeta `CoreEvent::Demote` → tick); `unaccounted_when_occupied_exceeds_alloc`. Atualizar `:1086`.
- **Disciplina Kahneman:** ITEM-7 (#13/#1) — ver mapa.

### `crates/ramshared-wsl2d/src/main.rs`  *(ITEM-2, ITEM-3, ITEM-8 — RF-1, RF-3, RF-5)*
- **ITEM-2 (RF-1):** em `serve_broker_jobs` (`:665`), após `serve(...)` (:705), guardado por `touches`:
  `rt.slice_io[job.export].bytes_served.fetch_add(job.req.len as u64, Relaxed); .io_count.fetch_add(1, Relaxed);`
  → adicionar `slice_io: Arc<Vec<SliceIoCounters>>` ao `BrokerRuntime` (`rt`, struct ~`:551`).
- **ITEM-3 (RF-3, DT-5):** em `run_broker`, a closure de residência passada a `serve_broker_jobs` (hoje
  `|| provider.mem_info().ok().map(|(f,_)| f)`, espelha `:520`) vira: `{ let (f,t)=provider.mem_info().ok()?;
  gauge.free.store(f,Relaxed); gauge.total.store(t,Relaxed); Some(f) }`. Criar `Arc<VramGauge>` em `run_broker`
  e `Arc::clone` para o `BrokerCore` (RAM: gauge fica `total=0` ⇒ `None`).
- **ITEM-8 (RF-5, DT-8):** `core_loop` recebe um `sink: Option<TelemetrySink>` (`{ file: File, branch:
  Option<String>, commit: Option<String> }`) — `None` quando a flag está off (F-v2-5). No dispatcher
  (`:808`), braço `Outbound::Telemetry(core)` → se `Some(sink)`: embrulha em `TelemetrySample { t:
  SystemTime epoch_secs, branch: sink.branch.clone(), commit: sink.commit.clone(), core }`, serializa
  (`serde_json::to_string` + `\n`), **append** (`sink.file`); erro = `eprintln` warn (não aborta). Os
  stamps vêm de **env var** `RAMSHARED_BUILD_BRANCH`/`RAMSHARED_BUILD_COMMIT` (launcher/harness; `None`
  se ausentes — qemu/initramfs não tem `git`, F-v2-4). Flag nova `--telemetry-jsonl <path>` (default
  `None` = silencioso).
- **Antes/Impacto:** hot path +2 atomics; sem ABI; `--backend ram` → `vram_*=None`. `BrokerCore::new`/
  `BrokerRuntime` mudam → ajustar chamadas.
- **Testes:** drill qemu ublk-RAM PASS **sem** a flag (RNF-4); smoke VRAM com a flag → linhas `jq`-válidas.
- **Disciplina Kahneman:** ITEM-2 (#5), ITEM-3 (#1/#3), ITEM-8 (#6) — mapa.

### `crates/ramshared-agent/src/psi.rs`  *(ITEM-6 — RF-2, DT-10/DT-11)*
- **Depois:** novas funções (padrão de `read_swaps`/`parse_swaps`):
  ```rust
  pub fn read_memcg_swap() -> Option<u64>;            // None se cgroup v2 ausente
  pub fn parse_memcg_swap(content: &str) -> Option<u64>; // inteiro; "max" → None
  pub fn read_diskstats(dev: &str) -> Option<u64>;    // sectors (rd+wr) * 512 do dev
  pub fn parse_diskstats(content: &str, dev: &str) -> Option<u64>;
  ```
- **Por quê:** RF-2. **Impacto:** read-only; `Option` quando ausente (DT-9). 
- **Testes:** `parse_memcg_swap_integer`, `parse_memcg_swap_max_is_none`, `parse_diskstats_sums_rw`,
  `parse_diskstats_unknown_dev_none` (fixtures).

### `crates/ramshared-agent/src/main.rs`  *(ITEM-6 — RF-2, DT-11)*
- **O que muda:** rastrear os nbd devices `swapon`'d (set atualizado quando executa `SwapOn`/`SwapOff`);
  no `Msg::Psi` (`:277`): `mem = Some(TenantMem { swap_current: psi::read_memcg_swap(),
  diskstats_io: active_swap_devs.iter().filter_map(|d| psi::read_diskstats(d)).sum() })` (DT-9/DT-11).
- **Impacto:** retrocompat; sem devices ativos → `diskstats_io=0`.
- **Testes:** parsers em `psi.rs`; envio exercitado no e2e civm (Q1d).

## Arquivos a DELETAR

| Arquivo | Motivo |
| --- | --- |
| — | nenhum (Day-0) |

## Observabilidade

**Prometheus:** N/A no MVP. A observabilidade É a saída JSONL + `StatusReply` (pull).
**JSONL (RF-5):** 1 `TelemetrySample`/linha em `--telemetry-jsonl` (harness de benchmark passa
`docs/benchmarks/results.jsonl`, por `.claude/rules/benchmarks.md`).

| Evento | Level | Campos |
| --- | --- | --- |
| Divergência (flag≠None após streak) | `error` (`Outbound::Log`) | `flag`, `reconcile_delta`, `demote_reason` |
| Amostra parcial | `warn` | fonte ausente (`mem`/`vram`) |

## Contratos e documentação viva

| Documento | Atualização | Motivo |
| --- | --- | --- |
| `Documentation/`, `Kconfig`, `CLAUDE.md`, `.claude/rules/*` | N/A | userspace; sem uAPI/CONFIG; convenção já coberta por `benchmarks.md` |
| `docs/specs/no-milestone/broker-telemetry-reconciliation/IMPL.md` | **Feito** (IMPL.md presente) | commits/decisões/métricas |
| `docs/reliability/memory-broker-p0-results.md` | **Alterar** | calibração `tol_frac`/`streak` (DT-7) |
| `docs/methodology/kahneman-disciplines.md` | N/A | usa disciplinas existentes (#1/#3/#5/#6/#13) |

## Ordem de implementação

1. **ITEM-1** — tipos de wire (`protocol.rs`) + roundtrip tests. *(isolado)*
2. **ITEM-4** — `telemetry.rs` (tipos + `reconcile` puro + testes). *(isolado, validável já)*
3. **ITEM-2** — `SliceIoCounters` no `BrokerRuntime` + incremento em `serve_broker_jobs`.
4. **ITEM-3** — `VramGauge` publicado na closure de residência de `run_broker`.
5. **ITEM-5** — `BrokerCore` (campos + `status_reply` + `on_demote` + handler `Psi`) + `Outbound::Telemetry`.
6. **ITEM-6** — agente (`read_memcg_swap`/`read_diskstats` + `Msg::Psi.mem` + tracking de devices).
7. **ITEM-8** — sink JSONL + flag `--telemetry-jsonl`.
8. **ITEM-7** — reconciliação no `on_tick` + `streak` (DT-12) + emissão.
9. Validação (testes + drill + smoke) e docs vivas.

## Plano de testes

**Backend (Rust):**
- **Unitários:** `telemetry::reconcile` (idle/unaccounted/eviction/stuckslice/partial), `vram_outros`;
  `parse_memcg_swap`/`parse_diskstats` (fixtures); `protocol` roundtrip (+`Psi.mem` default).
- **Integração (in-process, sem GPU/rede):** `status_reply_includes_slice_io`; `on_tick_emits_telemetry`;
  `eviction_flag_after_demote`; `unaccounted_when_occupied_exceeds_alloc`.
- **Atomicidade:** N threads incrementando `SliceIoCounters` → soma exata por counter (skew cross-counter
  documentado como aceito, F7).

**GPU/Drivers:** smoke VRAM server-only (RTX 2060) com `--telemetry-jsonl /tmp/t.jsonl`: linhas
`jq`-válidas; `vram_alloc_daemon ≈ Σ len + canário`; `vram_outros ≥ 0`.

**Manuais:** `nc`+`jq` → `{"type":"status"}` → `StatusReply` com `slice_io` (ADR-0005); e2e civm (Q1d):
flag `eviction`/`unaccounted` sob carga real (evidência objetiva do mapa Kahneman ITEM-7).

## Checklist de validação

**Backend:**
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

**GPU:**
- [ ] smoke VRAM com `--telemetry-jsonl` (linhas válidas)
- [ ] drill qemu ublk-RAM PASS **sem** a flag (zero regressão, RNF-4)

**Docs:**
- [ ] `IMPL.md` (PASSO 3) + `P0-RESULTS.md` (célula `tol_frac`/`streak`)

**Gates cognitivos:**
- [ ] ITEM-2/3/7/8 com disciplina + link + pergunta + evidência + abort (mapa acima)
- [ ] Sem linguagem vaga em ponto crítico (tolerância é número, DT-7; `streak` definido, DT-12;
  invariante é ocupação, DT-4; eviction = canário, DT-6)
