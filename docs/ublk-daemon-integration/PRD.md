# PRD — Integração do transporte ublk no daemon `ramshared-wsl2d`

> SSDV3 PASSO 1. Slug: `ublk-daemon-integration`. Pré-req lido: `docs/ublk-backend/SPEC-ring-loop.md`,
> `docs/ublk-backend/IMPL.md`, `crates/ramshared-wsl2d/src/main.rs`, `.../ublk_server.rs`,
> `docs/daemon-multiconn/SPECv3.md` (referência do worker NBD).

## 1. Resumo

Hoje o transporte ublk (`/dev/ublkbN` servindo VRAM via DT-3) só roda nos smokes de teste; o daemon
`ramshared-wsl2d` (`main.rs`) é **NBD-only**. Este PRD especifica ligar o ublk no daemon como um
`--transport ublk`, servindo VRAM-as-swap end-to-end **com** a máquina de residência (canário §9 de
latência, sonda §9.4 de conteúdo/free, DEMOTE via `swapoff`) — mantendo a Day-0 Policy (sem shims).

## 2. Contexto técnico

- **Confirmado no codebase** (`main.rs`): o daemon NBD usa um **worker CUDA único** = a thread
  principal, que é dona do `Context`, do `VramBackend`, da `CanaryProbe` e roda o loop de serviço +
  os dois canários + o poll do `swapoff`. `mlockall(MCL_CURRENT|MCL_FUTURE)` + `oom_score_adj=-1000`
  protegem contra o deadlock de swap (Disciplina 3). O DEMOTE = `spawn_swapoff(dev)` numa thread,
  com poll não-bloqueante do resultado.
- **Confirmado no codebase** (`ublk_server.rs`): o transporte ublk usa **DT-3** — uma thread *ring
  owner* (dona do `UblkServer`/io_uring) e uma thread *worker* dona do `VramBackend`.
  `spawn_server_dt3_vram` cria todo o stack CUDA (`Cuda`/`Context`/`DeviceMem`/`VramBackend`)
  **dentro da thread worker**, porque `DeviceMem<'c,'a>` empresta o `Context` e é `!Send`/`!'static`.
  O worker serve via `serve_request` (no-alloc, pool de buffers) e responde ao ring owner.
- **Confirmado no codebase**: `ramshared_block::serve()` (NBD) e `serve_request()` (ublk) são dois
  caminhos sobre o mesmo `BlockBackend`; o `VramBackend` implementa o trait para ambos.
- **Inferência**: o swap real exercita os dois canários (latência sob eviction WDDM e free-floor);
  a Fase 0 mediu spike de ~330× no serve sob eviction (a validar no caminho ublk).

### O conflito central (a decisão deste PRD)

O canário §9 (latência) e a sonda §9.4 (conteúdo/free via `ctx.mem_info()` + `CanaryProbe`) exigem
**afinidade com o contexto CUDA**. No NBD, isso é trivial: a thread que serve **é** a dona do
contexto. No ublk DT-3, a dona do contexto é a **thread worker**, mas o loop de canário/demote do
`main.rs` roda na thread principal, que **não tem** o contexto. Não dá para simplesmente reusar o
loop do `main.rs` sobre o ublk.

## 3. Opção recomendada

**Opção 1 — mover a máquina de residência para dentro do worker DT-3** (a thread dona do contexto
CUDA passa a servir **e** se auto-monitorar). Recomendada.

- O worker já tem `Context` + `VramBackend`; aloca também a `canary_region` e constrói `CanaryProbe`,
  `Canary`, `ResidencySampler`, `Cadence` na própria thread.
- No loop: a cada `IoWork` servida mede a latência **serve-only** (DT-16) e alimenta o canário §9;
  em cadência, roda a sonda §9.4 (`check_content` + `mem_info`); no veredito `Demote`, dispara
  `spawn_swapoff(block_dev)` e faz poll não-bloqueante (idêntico ao `main.rs`).
- O ring owner não muda (continua só drenando CQE ↔ canais).

**Opções rejeitadas:**
- **Opção 2 (compartilhar o contexto entre threads via push/pop):** exigiria refazer o modelo de
  lifetimes do `ramshared-cuda` (`DeviceMem` deixaria de emprestar o `Context`) — mudança grande,
  arriscada, fora do escopo Day-0.
- **Opção 3 (segundo contexto só para o canário):** o canário veria uma visão de memória diferente
  da região servida; o sinal primário (latência) nasce no serve (worker), então separar é incoerente.

## 4. Requisitos funcionais (RF)

- **RF-1** Flag `--transport ublk` no `main.rs` (default segue `nbd`); `--swap-dev /dev/ublkbN` e
  `--queue-depth N` (opcional). *(Confirmado no codebase: o parser de args existe e é extensível.)*
- **RF-2** No modo ublk: ADD_DEV → SET_PARAMS → `spawn` do worker+ring → START_DEV → servir; no
  encerramento STOP_DEV → join → DEL_DEV, com teardown limpo (`/dev` antes==depois). *(Confirmado:
  `ublk_control` + `ublk_server` já expõem tudo.)*
- **RF-3** O worker ublk roda o canário §9 (latência serve-only) e a sonda §9.4 (conteúdo/free) e
  executa DEMOTE via `swapoff(swap_dev)` — reusando `Canary`, `ResidencySampler`, `CanaryProbe`,
  `spawn_swapoff`. *(Reuso — Regra dura #1.)*
- **RF-4** `mlockall` + `oom_score_adj=-1000` aplicados **antes** de servir (igual ao NBD), com o
  mesmo gate `--force`. *(Confirmado no codebase.)*
- **RF-5** Teardown fecha qualquer fd de `/dev/ublkbN` antes do STOP_DEV (`del_gendisk` espera os
  openers — gotcha confirmado no smoke multipage) e zera a VRAM + a região-canário no fim.

## 5. Requisitos não-funcionais (RNF)

- **RNF-1** `unsafe` continua confinado a `ramshared-uring`/`ramshared-cuda`; daemon-lib
  `#![forbid(unsafe_code)]`. *(Confirmado.)*
- **RNF-2** Latência p50 ≥ a do bench atual (~241µs ublk-VRAM); o canário não pode adicionar custo
  mensurável no hot path (sonda em cadência, não por-request). *(Inferência — validar.)*
- **RNF-3** Sem alloc no hot path (pool no-alloc já garante); o canário §9.4 só aloca fora da cadência.
- **RNF-4** Zero regressão no caminho NBD (o `--transport nbd` continua idêntico).

## 6. Fluxos

1. **Boot ublk:** parse args → CUDA load/ctx/alloc/zero → mlockall+oom → ADD_DEV/SET_PARAMS →
   spawn worker (canário dentro) + ring owner → START_DEV → loop.
2. **Serviço:** kernel → io_uring CQE → ring owner → `IoWork` → worker serve (mede latência) →
   `WorkerReply` → ring owner COMMIT. Worker, fora do serve: canário/sonda/demote.
3. **DEMOTE:** veredito `Demote` no worker → `spawn_swapoff(swap_dev)` → poll não-bloqueante →
   confirma/re-arma.
4. **Shutdown:** sinal/última conexão → fecha fds do block dev → STOP_DEV → join (ABORT drena o
   ring) → DEL_DEV → zera VRAM+canário → remove nós.

## 7. Modelo de dados

Reuso integral (sem structs novos no caminho feliz): `IoWork`/`WorkerReply` (ublk),
`Canary`/`ResidencyConfig`/`Verdict`/`ResidencySampler`/`CanaryProbe`/`Cadence` (residência),
`VramBackend`. Possível novo: um `UblkDaemonConfig` (struct de args) — decisão de SPEC.

## 8. API / Interfaces

- Nova função pública em `ublk_server.rs` (a definir no SPEC): um worker DT-3 que aceita os
  parâmetros de residência e roda o canário internamente — ex.
  `spawn_server_dt3_vram_with_residency(char_path, qd, buf_size, vram_bytes, block_size, swap_dev, residency_cfg)`.
  Mantém `spawn_server_dt3_vram` (sem canário) para os smokes.
- Nenhuma mudança de uAPI/ABI do kernel (params/ioctls já existentes). *(→ não dispara o gatilho
  SSDV3 de uAPI.)*

## 9. Dependências e riscos

- **Risco A (afinidade CUDA):** toda chamada CUDA do canário tem de rodar na thread worker. Mitiga:
  Opção 1 (construir `CanaryProbe`/`ctx` no worker). Kahneman #2 (counterfactual): se a latência do
  canário poluir o serve, mover a sonda para cadência maior.
- **Risco B (teardown/`del_gendisk`):** fd aberto trava o STOP_DEV. Mitiga: RF-5.
- **Risco C (DEMOTE sob swap real):** `swapoff` sob pressão pode demorar; já é assíncrono
  (`spawn_swapoff` + poll). Validar com ciclo limitado (sem gerar pressão que trave o WSL2).
- **Risco D (regressão NBD):** isolar o modo ublk atrás da flag; o caminho NBD não muda.

## 10. Estratégia de implementação

Fases (cada uma com TDD + smoke root/GPU; commits rastreando RF):
- **F1** Worker DT-3 com residência (sem daemon): `spawn_server_dt3_vram_with_residency` + canário
  no `worker_loop`. Smoke: serve + dispara DEMOTE sintético (latência forçada) → `swapoff` chamado.
- **F2** Wire no `main.rs`: `--transport ublk` monta o device + worker-com-residência + START/STOP.
  Reusa mlockall/oom/zeragem.
- **F3** Validação swap end-to-end pelo daemon (mkswap/swapon/swapoff) + bench p50 do daemon ublk.

## 11. Documentos a atualizar

`docs/ublk-daemon-integration/SPEC.md` (PASSO 2) e `IMPL.md` (PASSO 3); `docs/ublk-backend/IMPL.md`
(link); `MEMORY.md`; `README`/`ARCHITECTURE` se a flag virar suportada de fato.

## 12. Fora de escopo

- `nr_hw_queues > 1` (multi-fila — exige ring/char-region por fila; novo SPEC).
- Reescrever o modelo de lifetimes do `ramshared-cuda` (Opção 2).
- Backoff/admission control no worker (segue como futuro, igual ao NBD).

## 13. Critérios de aceitação

- `ramshared-wsl2d --transport ublk` sobe o device, serve READ/WRITE/FLUSH da VRAM, e no shutdown
  deixa `/dev` e `/proc/swaps` antes==depois.
- DEMOTE sintético dispara `swapoff` do swap-dev (smoke).
- Swap real (ciclo limitado) validado pelo daemon ublk.
- p50 do daemon ublk na faixa do bench (~241µs); NBD sem regressão.
- `unsafe` confinado; clippy lib `-D warnings` limpo.

## 14. Validação

- Smokes root/GPU (RTX 2060): worker-com-residência (DEMOTE sintético), daemon ublk end-to-end,
  swap. Bench p50/p99 do daemon ublk vs o bench de teste. `/dev` + `/proc/swaps` antes==depois.
  `dmesg` sem OOPs. Kahneman: registrar counterfactual do canário (latência da sonda) e o trigger
  de reversão (regressão de p50 ou DEMOTE espúrio sob carga normal).
