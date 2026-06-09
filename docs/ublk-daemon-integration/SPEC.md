# SPEC — Integração do transporte ublk no daemon

> SSDV3 PASSO 2. Traduz `PRD.md` (Opção 1: residência dentro do worker DT-3). Implementação segue
> estritamente este SPEC; nova decisão → atualizar SPEC → re-aprovar.

## Arquivos a criar/modificar (paths absolutos)

1. `crates/ramshared-wsl2d/src/swap.rs` **(novo)** — extrai `spawn_swapoff`/`swapoff_bin` do
   `main.rs` para reuso pelo worker ublk. Sem `unsafe`.
2. `crates/ramshared-wsl2d/src/main.rs` **(mod)** — usa `crate::swap::spawn_swapoff`; adiciona
   `--transport ublk` (F2).
3. `crates/ramshared-wsl2d/src/ublk_server.rs` **(mod)** — novo
   `spawn_server_dt3_vram_with_residency(...)` + `worker_loop_with_residency(...)` (F1).
4. `crates/ramshared-wsl2d/src/lib.rs` **(mod)** — `pub mod swap;`.
5. `crates/ramshared-wsl2d/tests/ublk_residency.rs` **(novo)** — smoke F1 (DEMOTE sintético).

## F1 — worker DT-3 com residência (sem daemon)

### Refactor de reuso (Regra dura #1)

Mover de `main.rs` para `swap.rs` (idênticos, sem mudança de comportamento):
```rust
pub fn swapoff_bin() -> &'static str { /* /usr/sbin/swapoff | /sbin/swapoff | "swapoff" */ }
pub fn spawn_swapoff(dev: &str) -> std::sync::mpsc::Receiver<bool> { /* thread + status */ }
```
`main.rs` passa a `use crate::swap::spawn_swapoff;` (o caminho NBD não muda — RNF-4).

### Nova API pública (`ublk_server.rs`)

```rust
pub fn spawn_server_dt3_vram_with_residency(
    char_path: impl AsRef<Path>,
    queue_depth: u16,
    buf_size: usize,
    vram_bytes: usize,
    block_size: u32,
    swap_dev: String,                 // alvo do swapoff no DEMOTE
    residency: crate::ResidencyConfig,
) -> io::Result<ServerHandleDt3Vram>   // mesmo handle (ring + worker)
```
- O ring owner é **idêntico** ao de `spawn_server_dt3_vram` (`run_ring_owner`, inalterado).
- O worker (thread dona do CUDA) constrói, **na própria thread** (afinidade — Risco A do PRD):
  `Cuda::load` → `device(0)` → `create_context` → `ctx.alloc(vram_bytes)`+`zero` → `VramBackend` **e**
  `canary_region = ctx.alloc(crate::CANARY_BYTES)` → `CanaryProbe::new(canary_region)`. Depois roda
  `worker_loop_with_residency`.
- `spawn_server_dt3_vram` (sem residência) **permanece** para os smokes existentes.

### `worker_loop_with_residency` (espelha o loop do `main.rs`, dirigido pelo canal ublk)

Estado local (thread worker): `backend: VramBackend`, `probe: CanaryProbe`, `ctx`, `swap_dev`,
`canary: Option<Canary>`, `baseline: Vec<u64>`, `sampler: ResidencySampler`, `cadence: Cadence`,
`demoted: bool`, `demote_rx: Option<Receiver<bool>>`.

Por `IoWork` recebida:
1. `touches_vram = matches!(req.cmd, Read|Write)`.
2. `t0 = Instant::now(); result = serve_request(&req, &mut backend, &mut payload); lat_us = …`
   (**serve-only**, DT-16 — medir só a op de VRAM, não a espera na fila).
3. Responder `WorkerReply` ao ring owner (mantém o contrato no-alloc: devolve `buf`+`is_read`).
4. Poll não-bloqueante de `demote_rx` (re-arma se falhar) — idêntico ao `main.rs`.
5. Se `touches_vram && !demoted && demote_rx.is_none()`: canário §9 (baseline 16 amostras → `Canary::new`
   → `c.sample(lat_us, true, u64::MAX)`; `Verdict::Demote` → `demote_rx = Some(spawn_swapoff(&swap_dev))`).
6. Se `touches_vram && !demoted && demote_rx.is_none() && cadence.tick()`: sonda §9.4
   (`probe.check_content().ok()`, `ctx.mem_info().ok().map(|(f,_)| f)`, `sampler.sample(content, free)`;
   `Demote` → `spawn_swapoff`).

Ao fechar o canal (`work_rx` retorna `Err`): teardown DT-17 — espera (bounded 5s) o `demote_rx` em
voo, `backend.zero()`, `probe.zero()`, retorna `io::Result<()>` (o `ServerHandleDt3Vram` já existe).

### Validações (handlers)

- `result` do `serve_request` é o contrato existente (bytes ≥ 0 ou `-errno`); inalterado.
- `lat_us`: `t0.elapsed().as_micros()` saturado em `u64` (`try_from`/`unwrap_or(u64::MAX)`), sem panic.
- `mem_info`/`check_content` em `Option` (None = erro de sonda) → o `sampler` aplica histerese
  (streak) antes de DEMOTE — **evita demote espúrio** (Kahneman #2, counterfactual já no `main.rs`).

### Ordem de threads / seções críticas

- **Invariante DT-3 mantido:** só o ring owner toca o `UblkServer`/io_uring; só o worker toca CUDA.
- O canário roda **na thread worker** (mesma que serve) → nenhuma chamada CUDA cross-thread (Risco A).
- `spawn_swapoff` roda numa **terceira** thread (não bloqueia worker nem ring) — Disciplina 3 /
  [KAHNEMAN-DISCIPLINES.md] #3 (anti-deadlock: o swapoff não pode bloquear o caminho que serve o swap).

## F1 — teste (`tests/ublk_residency.rs`, root+GPU, `#[ignore]`)

`dt3_vram_residency_triggers_demote_synthetic`:
- Sobe device (smoke_auto) + `spawn_server_dt3_vram_with_residency` com um `ResidencyConfig` de
  **gatilho sintético** (limiar de latência baixíssimo OU um swap_dev = um device de swap real
  pequeno que possa ser `swapoff`-ado), START_DEV.
- Gera I/O suficiente para o canário armar a baseline e então disparar `Demote` (latência forçada,
  ex.: limiar = baseline×1 → qualquer jitter dispara, de modo determinístico para o smoke).
- Assert: o `swapoff` foi invocado (observável via o swap_dev sair de `/proc/swaps`, ou um hook de
  teste). `/dev` antes==depois; VRAM zerada no teardown.
- **Fechar o fd do block dev antes do STOP_DEV** (gotcha `del_gendisk`).

> Nota: o gatilho determinístico de DEMOTE no smoke é a parte sensível — o SPEC prefere um
> `ResidencyConfig` com limiar explícito a depender de eviction WDDM real (não reproduzível sob
> demanda). A validação da falha REAL (eviction → spike ~330×) fica para o teste manual §14.

## F2 — wire no `main.rs`

- Args: `--transport {nbd,ublk}` (default nbd), `--swap-dev /dev/ublkbN`, `--queue-depth N`.
- Modo ublk: após CUDA+mlockall+oom (reuso do bloco atual), em vez do acceptor NBD:
  ADD_DEV(smoke_auto com `queue_depth`) → SET_PARAMS(`basic_disk(dev_sectors, 12, 12)`; opcional
  `with_max_sectors` para multipágina) → `spawn_server_dt3_vram_with_residency(...)` → START_DEV →
  aguarda sinal de término → fecha fds → STOP_DEV → `handle.join()` → DEL_DEV → zera → remove nós.
- O bloco CUDA do `main.rs` (alloc/zero/canary) **migra** para dentro do worker no modo ublk (o
  worker é o dono do contexto); no modo NBD permanece no `main.rs`. Decisão: dois caminhos claros
  atrás da flag, sem dual-path escondido (Day-0).

## F3 — validação swap end-to-end + bench

- `mkswap`/`swapon`/`swapoff` pelo daemon ublk (ciclo limitado, sem pressão — risco WSL2).
- Bench p50/p99 do daemon ublk; comparar com o bench de teste (~241µs) — RNF-2.
- `/dev` + `/proc/swaps` antes==depois; `dmesg` sem OOPs.

## Rastreabilidade

F1 → RF-3, RNF-1/3, Risco A/B. F2 → RF-1/2/4/5, RNF-4. F3 → RF-3, RNF-2, Critérios §13.
Cada commit cita o RF coberto. Kahneman: [docs/methodology/KAHNEMAN-DISCIPLINES.md] #2
(counterfactual do canário) e #3 (anti-deadlock do swapoff).
