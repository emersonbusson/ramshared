# IMPL — Coletor de Telemetria & Reconciliação do Memory Broker

> SSDV3 PASSO 3. Implementa o **`SPECv2.md`** (candidato ativo, pós-auditoria 2.5). Branch
> `feat/p1-hardening`. **Sem PR** até o usuário pedir.

## Status: implementado + verde (no que é validável aqui)

`cargo test --workspace` ✓ · `cargo clippy --workspace --all-targets -- -D warnings` ✓ ·
`cargo fmt --all -- --check` ✓ · **drill qemu broker (NBD-RAM) PASS** (caminho `serve_broker_jobs` +
`on_tick`/reconciliação roda sem regressão).

## Arquivos (RF/ITEM → mudança)

| Arquivo | ITEM | O que foi feito |
| --- | --- | --- |
| `crates/ramshared-broker/src/protocol.rs` | ITEM-1 (RF-1/2) | `Psi.mem: Option<TenantMem>` (`#[serde(default)]`), `StatusReply.slice_io: Vec<SliceIo>`, `TenantStatus.bytes_served`, novos `TenantMem`/`SliceIo`; roundtrip tests + `psi_mem_defaults_to_none`, `status_reply_slice_io_defaults_empty` |
| `crates/ramshared-wsl2d/src/telemetry.rs` **(novo)** | ITEM-4 (RF-4/5) | `SliceIoCounters`, `VramGauge`, `ReconcileFlag`, `ReconcileInput`, `TelemetryCore`, `TelemetrySample`, `reconcile()` (delta-first, F-v2-1), `vram_outros()`; 8 testes puros |
| `crates/ramshared-wsl2d/src/broker_srv.rs` | ITEM-5/7/8 | `BrokerCore` + campos de telemetria; `status_reply` mescla `slice_io`/`bytes_served`; `on_psi` guarda `mem` + `occupied_bytes` (F-v2-2/3); `on_demote` conta; `emit_telemetry` (invariante de ocupação + histerese DT-12); `Outbound::Telemetry`; `TelemetrySink` (JSONL, env stamps, F-v2-4/5); `BrokerConfig`/`spawn_broker`/`core_loop`/`dispatch` fiados |
| `crates/ramshared-wsl2d/src/main.rs` | ITEM-2/3/8 | `BrokerRuntime` + `slice_io`/`vram`; incremento por `Job` em `serve_broker_jobs` (RF-1); gauge publicado na closure de residência de `run_broker` (RF-3/DT-5); flag `--telemetry-jsonl`; consts `RECON_TOL_FRAC=0.10`/`RECON_STREAK=3` (DT-7, provisórias) |
| `crates/ramshared-agent/src/psi.rs` | ITEM-6 (RF-2) | `read_memcg_swap`/`parse_memcg_swap`, `read_diskstats`/`parse_diskstats` + 4 testes |
| `crates/ramshared-agent/src/main.rs` | ITEM-6 | `Msg::Psi { …, mem }` (cgroup swap + Σ diskstats dos nbd em `active`, DT-11); `--status` imprime `slice_io` |
| `crates/ramshared-wsl2d/Cargo.toml` | — | `serde`/`serde_json` (derives + JSONL) |

## Decisões pequenas durante a IMPL (não pediram nova ADR)

- `page_io_s` na linha de telemetria carrega o **Σ diskstats cumulativo** (bytes) dos tenants que
  reportam `mem`; o consumidor deriva a taxa pela diferença entre amostras (campo `t`) — padrão de
  contador cumulativo (igual a `/proc/diskstats`). Evita guardar estado de delta no `BrokerCore`.
- `mem.swap_current` (cgroup) é **coletado e guardado** no `TenantState` (cross-check DT-10), mas a
  ocupação do invariante vem de `/proc/swaps` (DT-10, fonte primária) — não é dado morto, é estado.
- Teste `telemetry_sample_serializes_flat_jsonl` adicionado (prova `#[serde(flatten)]` + snake_case)
  já que o write-no-arquivo via daemon não roda no WSL2 (regra de segurança).

## Validação (números)

- `cargo test --workspace`: **todos verdes**; `ramshared-wsl2d` lib 45→**53** (+8 `telemetry`),
  `ramshared-broker` 32 (roundtrip + 2 defaults), `ramshared-agent` +4 (parsers cgroup/diskstats).
- `clippy --workspace --all-targets -D warnings`: limpo (incl. `#[allow(too_many_arguments)]` em
  `BrokerCore::new`/`broker_setup`/`core_loop`, coesos).
- `cargo test --workspace`: **201 testes, 0 falhas**; `ramshared-wsl2d` lib 45→**58** (+8 `telemetry`
  incl. serialização JSONL, +5 wiring `broker_srv` incl. sink), broker +2 defaults, agente +4 parsers.
- **Testes de wiring do `broker_srv` (in-process, fechados):** `status_reply_includes_slice_io`,
  `on_tick_emits_telemetry`, `eviction_flag_after_demote`, `unaccounted_when_occupied_exceeds_alloc`
  (o test mod é filho do módulo → manipula os campos privados do `BrokerCore`).
- **`telemetry_sink_writes_jsonl_line` (in-process):** abre o `TelemetrySink` num tempfile, emite 2×,
  relê e parseia o JSON (2 linhas, `swap_used`/`t` corretos) — prova o write-no-arquivo sem daemon.
- **drill qemu broker** (`scripts/kernel/qemu-broker-drill.sh`, `--backend ram`, agora com
  `--telemetry-jsonl`): `QEMU-BROKER-DRILL: PASS` + `KTEST-TELEMETRY=ok` — **o daemon vivo escreve o
  JSONL** (RF-5 e2e, em VM isolada); swap ativo via NBD; teardown limpo.

## Gaps — fechados na sessão (b) com disciplina + segurança

- **Números de VRAM reais (RF-3) — FECHADO.** Teste in-process `#[ignore]`
  `vram_gauge_outros_captures_real_graphics_usage` (`backend.rs`): `mem_info` real na **RTX 2060** →
  gauge → `vram_outros`. Medido: **total=6143, free=5040, used=1103, daemon=64, outros=1039 MiB** — o
  `vram_outros` por subtração capta corretamente **~1 GB de VRAM de gráficos** (desktop/OBS), que é o
  sinal de "consumidor externo". Seguro (CUDA-only, sem daemon).
- **Calibração `tol_frac`/`streak` (DT-7) — resolvida por estrutura + unit.** `Unaccounted` só dispara
  se `ocupado > emprestado·(1+tol)`; sob operação normal `ocupado ≤ emprestado` ⇒ `delta ≤ 0` (no drill,
  swap vazio ⇒ `ocupado≈0` ⇒ `delta≈-1.0`, longe de +0.10). Fronteira unit-testada
  (`unaccounted_when_occupied_exceeds_alloc` dispara só acima; `reconcile_idle_none` fica em `none`).
  → `tol_frac=0.10` **não dá falso-positivo**; a distribuição exata ao vivo fica como refinamento no civm.

## Gap genuinamente env-bound (mesmo trap do ublk+VRAM)

- **Flag `eviction` e2e sob carga WDDM real:** o canário só dispara com a VRAM do daemon sendo evictada
  por pressão gráfica — precisa do **daemon + GPU + carga juntos**, e a GPU só é alcançável no WSL2 (onde
  daemon é arriscado) e o qemu não tem GPU (mesmo trap do ublk+VRAM). A LÓGICA está coberta por
  composição: canário (P1, latência→`Verdict::Demote`) + `reconcile_eviction_when_demotes` +
  `eviction_flag_after_demote` (DEMOTE→flag). Observação ao vivo = host GPU não-WSL2 (RF-G2) ou civm.

## Rastreabilidade

RF-1 ✓ (ITEM-1/2/5 + `status_reply_includes_slice_io`) · RF-2 ✓ (ITEM-1/6 + parsers) · RF-3 ✓
(ITEM-3/7, gauge por composição) · RF-4 ✓ (ITEM-7 + `reconcile`/`eviction`/`unaccounted` tests) ·
RF-5 ✓ (ITEM-8 + sink test in-process + **JSONL e2e no daemon em qemu**). Resta só: eviction-sob-carga
+ números VRAM reais + calibração = sessão civm/GPU.
