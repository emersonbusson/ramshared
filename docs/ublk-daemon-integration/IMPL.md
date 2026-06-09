# IMPL — Integração do transporte ublk no daemon

> SSDV3 PASSO 3. Implementa `SPEC.md`. Rastreabilidade por RF.

## Status

- **F1 — worker DT-3 com residência: FEITO e validado** (2026-06-09, commit `31f8395`, RF-3).
- **F2 — `--transport ublk` no `main.rs`: CÓDIGO ESCRITO, _NÃO_ VALIDADO.** Compila/clippy OK, mas
  rodar o daemon standalone **CONGELOU o WSL2** (2026-06-09 — device órfão no teardown → I/O em
  D-state). **Bloqueado por duas travas** (ver abaixo); validação só em VM/qemu.
- **F3 — swap e2e pelo daemon + bench: pendente** (depois da validação do F2 em qemu).

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

## F2 (código escrito, NÃO validado — congelou WSL2)

`run()` faz branch por `--transport {nbd,ublk}` (default nbd) → `run_nbd` (corpo NBD inalterado) /
`run_ublk`; `--queue-depth N`. `run_ublk`: `guard_not_wsl2` → `lock_memory` (extraída, reuso) →
registra SIGINT/SIGTERM (handler seta `SHUTDOWN`) → ADD_DEV/SET_PARAMS →
`spawn_server_dt3_vram_with_residency` → START_DEV → `while !SHUTDOWN { sleep(200ms) }` → STOP_DEV →
`join` → DEL_DEV. `ramshared_uring::wait_and_drain` ganhou retry em `EINTR` (sinal na thread do ring
owner não é erro). Compila + clippy `-D warnings` OK.

### Por que NÃO foi validado e as travas

Rodar o smoke de processo (`daemon_ublk_serves_and_terminates_on_signal`, que sobe o daemon e manda
SIGTERM) **CONGELOU o WSL2**: o teardown não fechou limpo, o `kill` deixou o `/dev/ublkbN` **sem
servidor** com I/O em voo → D-state no caminho de writeback/memória + `mlockall(MCL_FUTURE)` +
`drop_caches` → stall global → freeze (reboot forçado). Causa-raiz (bug em STOP_DEV/join _vs._
corrida SIGTERM-tarde→SIGKILL) **só é depurável em qemu**, nunca no WSL2.

**Duas travas independentes (default = tudo trancado):**
1. **Teste:** `daemon_ublk_serves_and_terminates_on_signal` pula sem `RAMSHARED_DANGEROUS_DAEMON_SMOKE=1`
   (não roda nem com `--ignored`); sem `drop_page_cache()`.
2. **Daemon:** `run_ublk` chama `guard_not_wsl2()` — **recusa** servir ublk se
   `/proc/sys/kernel/osrelease` contém `microsoft`/`wsl`, a menos de `RAMSHARED_ALLOW_UBLK_ON_WSL2=1`.
   Logo, mesmo o binário rodado à mão **não cria device no WSL2**.

**Validar o F2 só em VM/qemu** (`scripts/kernel/qemu-validate.sh`), onde um stall é recuperável sem
derrubar o host. Lá: abrir os dois gates, rodar o smoke, depurar o teardown se ainda travar.

## F3 (próximo)

`mkswap`/`swapon`/`swapoff` pelo daemon ublk (ciclo limitado) + bench p50/p99 vs o de teste (~241µs).
`/dev` + `/proc/swaps` antes==depois; `dmesg` sem OOPs.
