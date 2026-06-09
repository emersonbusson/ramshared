# IMPL — Integração do transporte ublk no daemon

> SSDV3 PASSO 3. Implementa `SPEC.md`. Rastreabilidade por RF.

## Status

- **F1 — worker DT-3 com residência: FEITO e validado** (2026-06-09, commit `31f8395`, RF-3).
- **F2 — `--transport ublk` no `main.rs`: pendente.**
- **F3 — swap e2e pelo daemon + bench: pendente.**

## F1 (feito)

- **Refactor de reuso (Regra dura #1):** `src/swap.rs` (novo) extrai `spawn_swapoff`/`swapoff_bin`
  do `main.rs`, idênticos. `main.rs` e o worker ublk passam a `use crate::swap::spawn_swapoff`. O
  caminho NBD não muda (RNF-4). Disciplina 3: o swapoff segue numa thread separada (não bloqueia
  quem serve o swap).
- **`spawn_server_dt3_vram_with_residency`** (`ublk_server.rs`): igual a `spawn_server_dt3_vram`, mas
  o worker (dono do contexto CUDA — Opção 1 do PRD) constrói também a `canary_region` +
  `CanaryProbe` e roda a máquina de residência **inline** no loop:
  - serve-only latency (DT-16): `Instant` em volta do `serve_request` apenas;
  - canário §9: baseline (16 amostras) → `Canary::new` → `c.sample(lat, true, u64::MAX)`;
  - sonda §9.4 em cadência: `probe.check_content()` + `ctx.mem_info()` → `ResidencySampler::sample`;
  - DEMOTE → `spawn_swapoff(swap_dev)` + poll não-bloqueante (re-arma se falhar).
  - teardown DT-17: espera (5s) o swapoff em voo, `backend.zero()` + `probe.zero()`.
- **Observabilidade:** `ServerHandleDt3VramResidency::demote_count` (`Arc<AtomicU32>`) — o DEMOTE é
  contável sem swap real.
- **Invariante DT-3 mantido:** só o ring owner toca io_uring; só o worker toca CUDA (o canário roda
  na thread worker). Nenhuma chamada CUDA cross-thread.
- **Validação (RTX 2060):** `dt3_vram_residency_triggers_demote_synthetic` — config sintética
  (`latency_mult=0, consecutive=1`) dispara DEMOTE determinístico após a baseline; o `swapoff` é
  invocado (swap_dev inexistente → falha esperada) e `demote_count >= 1`. `/dev` limpo, sem
  regressão nos smokes VRAM. clippy lib `-D warnings` limpo; 40 testes não-root verdes.

## F2 (próximo) — esboço confirmado no SPEC

`--transport {nbd,ublk}` (default nbd), `--swap-dev`, `--queue-depth`. No modo ublk: mlockall+oom
(reuso) → ADD_DEV/SET_PARAMS → `spawn_server_dt3_vram_with_residency` → START_DEV → **aguarda sinal
de término** (SIGINT/SIGTERM via flag) → fecha fds do block dev → STOP_DEV → `join` → DEL_DEV. A
alocação CUDA migra para o worker (no modo NBD permanece no `main.rs`); dois caminhos claros atrás
da flag (Day-0, sem dual-path escondido). Ponto sensível: ciclo de vida do daemon (sinal) e teardown
ordenado (gotcha `del_gendisk`).

## F3 (próximo)

`mkswap`/`swapon`/`swapoff` pelo daemon ublk (ciclo limitado) + bench p50/p99 vs o de teste (~241µs).
`/dev` + `/proc/swaps` antes==depois; `dmesg` sem OOPs.
