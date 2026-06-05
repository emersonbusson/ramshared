# SPECv3 — H1 — Daemon NBD multi-conexão / leitor dedicado

> Versão melhorada após 2ª auditoria do Passo 2.5.
> Baselines preservados: [`SPEC.md`](SPEC.md), [`SPECv2.md`](SPECv2.md).
> Motivo: o `SPECv2.md` consertou CRITICAL-1/2 + HIGH-2 (DT-7/DT-8/DT-10 **verificados OK**),
> mas 3 fixes falharam: DT-9 (acceptor com janela temporal não-determinística — F-3/F-4/F-5/F-6),
> DT-11 (**a válvula backpressure→DEMOTE não existia** — F-8), DT-12 (teardown trocava hang por
> corrupção no edge #2a — F-9). O SPECv3 substitui essas 3 por desenhos determinísticos
> (DT-15/DT-16/DT-17) + testes que de fato as exercem (DT-18).

## 0. Proveniência da auditoria

- **Auditado:** `docs/daemon-multiconn/SPECv2.md`. **Resultado:** `no-go`.
- **Verificados OK (mantidos):** DT-7 (réplica ilimitada → sem deadlock), DT-8 (`Reply.reply:[u8;SIMPLE_REPLY_LEN]`), DT-10 (`CAN_MULTI_CONN` + coerência via write síncrono), DT-13 (`Inflight` fora de escopo).
- **Bloqueantes endereçados:** F-3/F-4/F-5/F-6 → **DT-15**; F-8 → **DT-16**; F-9 → **DT-17**;
  F-1/F-10 → **DT-16/DT-18**; F-2/F-7 → docs.
- **Este `SPECv3.md` é o candidato ativo.**
- **Revisão no Passo 3 (IMPL):** o `§14.3` ao vivo revelou que **DT-16 (latência total)
  causava falso-positivo de DEMOTE** sob carga normal (baseline 85µs idle → 1.1ms sob fila
  = 13× → VRAM demovida cedo, tier inutilizado). DT-16 foi **revisado para serve-only** (ver
  tabela). Evidência empírica acima da auditoria teórica (Kahneman #13: validar contra a realidade).

## Escopo fechado

Igual ao [`SPECv2.md`](SPECv2.md). Deltas: ciclo de vida acceptor/worker **por mensagens**
(determinístico, sem timer); latência do canário passa a medir **latência total** (enfileiramento
→ réplica), fechando a válvula de backpressure; teardown honesto e bounded. **Daemon vira
N-agnóstico** (aceita qualquer nº de conexões; `N` é só do `nbd-client -C N`).

## Decisões técnicas (delta sobre o SPECv2)

| #    | Decisão | Corrige |
| ---- | ------- | ------- |
| DT-7, DT-8, DT-10, DT-13 | **Mantidas do SPECv2** (verificadas OK). | — |
| DT-1..DT-6 | Herdadas, exceto onde DT-15/16/17 sobrescrevem. | — |
| **DT-9** | **SUPERSEDED por DT-15.** | F-3/4/5/6 |
| **DT-11** | **SUPERSEDED por DT-16.** | F-8 |
| **DT-12** | **SUPERSEDED por DT-17.** | F-9 |
| **DT-15** | **Ciclo de vida determinístico por mensagens (sem janela temporal).** O canal worker carrega `enum WMsg { Opened, Job(Job), Closed }`. **Acceptor:** loop `accept()` **bloqueante infinito** (sem N, sem timer); por conexão aceita, envia `WMsg::Opened` e spawna **reader** (que faz o **handshake dentro da própria thread** — não no acceptor) + **writer**. **Reader:** se o handshake falhar → loga e sai enviando `WMsg::Closed` (erro confinado à conexão, **não** derruba o acceptor — F-6); senão entra no read-loop e, ao EOF/erro, sai enviando `WMsg::Closed`. **Worker:** `live=0; opened=false;` `for m in rx { Opened⇒{live+=1;opened=true} Closed⇒{live-=1; if live==0 && opened {break}} Job(j)⇒{processa} }`. Término = **todas as conexões abertas fecharam** (determinístico, sem race de template nem janela de 250ms). O acceptor segue bloqueado em `accept()`; o worker quebra explícito → `main` zera e retorna → o processo sai (mata o acceptor). **Daemon N-agnóstico:** sem flag `--connections` no daemon; aceita o que vier. Handshake no reader resolve F-4 (acceptor nunca bloqueia em handshake lento). | **F-3/4/5/6** |
| **DT-16** | **REVISADO no Passo 3 (§14.3 ao vivo):** a latência do canário mede **serve-only** (tempo da op de VRAM em volta do `serve()`), **NÃO** a latência total. A tentativa de medir a espera na fila (`enqueued.elapsed()`) deu **falso-positivo de DEMOTE sob carga normal** — baseline 85µs (idle) vs ~1.1ms (sob fila) = 13× → VRAM demovida com só ~10-72 MiB absorvidos (vs 511 MiB do baseline), inutilizando o tier. **F-8 mitigado sem trigger por fila:** (a) a falha REAL (eviction WDDM) spike o serve ~330× (Fase 0) → o canário serve-only dispara nela; (b) timeout NBD generoso (sem `-t`) → backpressure degrada a swap-lento, não I/O-error (§14.4: swapoff 6 s sem panic); (c) o regime "moderadamente lento que enche a fila sem spike de serve" não é observado no WDDM (eviction é binário: ok ou 330×) → trigger por profundidade-de-fila é YAGNI; adicionar só se observado. | **F-8** (revisado) |
| **DT-17** | **Teardown honesto e bounded (liveness + safety):** o read-back do swapoff é servido **no loop** (Jobs READ, enquanto a conexão vive); poll de `demote_rx` **no loop** (desarma o canário). Ao sair do loop (todas as conexões caíram): `match demote_rx.take()` → se `Some(rx)`, `rx.recv_timeout(5s)` (**bounded**, sem hang). **Loga o resultado com honestidade**: `Ok(true)`=DEMOTE limpo; `Ok(false)`/`Err`/timeout=swapoff **não confirmado** (loga `WARN`, não finge sucesso). Zera as duas regiões (`backend.zero()`+`probe.zero()`, ambos): no caminho de teardown **a conexão NBD já caiu** → ninguém lê a VRAM por NBD → zerar é **safe** (o cenário "page-in lê zeros" de F-9 exige conexão viva, que não existe aqui). Edge #2a documentado: disconnect não-ordenado ⇒ swap já quebrado pelo transporte; o daemon loga e zera (lesser evil), não corrompe um swap vivo. | **F-9** |
| **DT-18** | **Testes que exercem os findings (sem GPU):** `live_count_terminates_on_all_closed` + `live_count_balanced_open_then_close` + `live_count_never_stops_before_any_open` (DT-15: lifecycle determinístico via `LiveCount`); `slow_writer_does_not_deadlock` (DT-7: réplica ilimitada, sem deadlock); `job_reply_roundtrip`/`chan_cap_is_bounded`. O gatilho serve-latency→DEMOTE (DT-16 revisado) é coberto por `latency_demote_needs_consecutive` (residency). A não-regressão do DEMOTE indevido é provada **ao vivo** (§14.3: nbd0 volta a absorver ~500 MiB sem falso-positivo). | **F-10** |
| DT-19 | **Teto de memória do canal de réplica (corrige F-1):** backlog por conexão ≤ `nr_requests` (profundidade da fila NBD do kernel; o cliente não submete além sem ler réplicas) × `(SIMPLE_REPLY_LEN + max_read_len)`. Para READs grandes são dezenas de MiB/conexão — **limitado**, não ilimitado; declarado explicitamente (não é a conta subdimensionada da DT-7). | F-1 |

## Fronteira de atomicidade e política de rollback

Igual ao SPECv2. Reforço (DT-15): término determinístico por contagem `live` no worker — não
depende de drop de sender nem de timer. Rollback **app-only**.

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| DT-15 (lifecycle) | #5 Worst-case | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "N parcial / handshake falho / última conexão cai cedo travam o término?" | `acceptor_terminates_on_all_closed` + `handshake_error_isolated` verdes; smoke `-C 2` | worker não termina ou termina cedo → reverter |
| DT-16 (canário serve-only) | #5 Worst-case + #13 ilusão de validade | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "medir latência total causa DEMOTE indevido sob carga? a falha real é pega?" | §14.3 ao vivo: nbd0 absorve ~500 MiB SEM falso-positivo (serve-only); falha real = serve 330× (Fase 0) | DEMOTE indevido sob carga normal (nbd0 mal usado) → reverter p/ serve-only |
| DT-17 (teardown) | #5 + #2 | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "disconnect no meio do swapoff trava ou corrompe?" | §14.4 ao vivo: DEMOTE 0 corrupção; `recv_timeout` bounded; log honesto | hang no teardown ou corrupção de swap vivo → reverter |

## Checklist de segurança

Igual ao SPECv2. `Context`/`DeviceMem` `!Send` → worker preso à thread do `ctx` por tipo (garantia do compilador).

## Arquivos a CRIAR

### `crates/ramshared-wsl2d/src/conn.rs`
- **Igual ao SPECv2**, com deltas DT-15/DT-16:
  - `enum WMsg { Opened, Job(Job), Closed }`; `Job { req: Request, payload: Vec<u8>, reply: std::sync::mpsc::Sender<Reply>, enqueued: std::time::Instant }` (DT-16).
  - `Reply { reply: [u8; SIMPLE_REPLY_LEN], data: Vec<u8>, disconnect: bool }` (DT-8).
  - `spawn_reader`: faz `server_handshake` **na thread**; em erro → loga + envia `WMsg::Closed` + retorna. Loop: `read_exact`→`parse_request`→cap anti-DoS→`jobs.send(WMsg::Job(Job{...,enqueued:Instant::now()}))`. EOF/erro→`jobs.send(WMsg::Closed)`.
  - `spawn_writer`: `for r in replies { write_all(&r.reply); if !r.data.is_empty(){write_all(&r.data)}; flush(); if r.disconnect {break} }`.
  - `spawn_acceptor`: loop `accept()` bloqueante; por conexão `jobs.send(WMsg::Opened)` + cria canal de réplica ilimitado + spawna writer + reader. (Sem janela, sem N, sem handshake inline.)
  - Consts: `CHAN_CAP=64` (canal worker de `WMsg`, bounded). Sem `ACCEPT_WINDOW_MS`.
- **Testes (DT-18, sem GPU):** `job_reply_roundtrip`, `chan_cap_is_bounded`, `slow_writer_does_not_deadlock`, `acceptor_terminates_on_all_closed`, `handshake_error_isolated`.

### `crates/ramshared-block/src/protocol.rs`
- Igual ao SPECv2: `+ pub const NBD_FLAG_CAN_MULTI_CONN: u16 = 1 << 8;` (DT-10).

## Arquivos a MODIFICAR

### `crates/ramshared-wsl2d/src/main.rs`
- **Igual ao SPECv2**, com deltas:
  - `tx_flags |= NBD_FLAG_CAN_MULTI_CONN` (DT-10).
  - Canal worker: `sync_channel::<WMsg>(CHAN_CAP)`. Worker loop = `match m { Opened/Closed/Job }` (DT-15) com `live`/`opened`.
  - Em `Job`: `let lat_us = job.enqueued.elapsed().as_micros() as u64;` (DT-16, latência TOTAL); alimenta o `Canary` existente; poll de `demote_rx` + cadência §9.4 **no loop**; `job.reply.send(Reply{ reply: out.reply, data: out.read_data, disconnect: out.disconnect })` (DT-8).
  - Sem flag `--connections` no daemon (N-agnóstico, DT-15).
  - Teardown DT-17: `recv_timeout(5s)` + log honesto + zera ambos.
  - Doc-comment do módulo reescrito (multi-conexão + worker único; sem "futuro").
- **Impacto:** sem uAPI/ABI on-wire além do flag compatível; latência por-request preservada; HOL removido; válvula de backpressure ativa.

### `crates/ramshared-wsl2d/src/lib.rs`
- `pub mod conn;` + `pub use conn::{Job, Reply, WMsg, CHAN_CAP};`.

### `crates/ramshared-cli/src/cascade.rs`
- `up --connections N` (default 1) → `-C N` ao `nbd-client` quando N>1. **Sem `-t` agressivo** (DT-16). Daemon spawnado **sem** `--connections` (N-agnóstico).

### `crates/ramshared-cuda/src/driver.rs` e `crates/ramshared-wsl2d/src/backend.rs`
- **F-7 (LOW):** comentário no `write_at`/`flush`: "NÃO trocar `cuMemcpyHtoD` para Async sem revisar `CAN_MULTI_CONN` (a coerência cross-conn depende do write síncrono)". Sem mudança de código.

### `docs/daemon-multiconn/PRD.md`
- **F-2 (LOW):** nota de que o SPEC ativo usa `Reply.reply:[u8;N]` + canal de réplica ilimitado + latência total (PRD descrevia `Vec`/`SyncSender`/serve-latency).

## Ordem de implementação

1. `conn.rs`: `WMsg`/`Job`(+enqueued)/`Reply`([u8;N]) + canal de réplica ilimitado + testes puros (DT-7/DT-18: `slow_writer_does_not_deadlock`, `job_reply_roundtrip`, `chan_cap_is_bounded`).
2. `protocol.rs`: `NBD_FLAG_CAN_MULTI_CONN`.
3. `main.rs`: canal `WMsg` + worker loop (`live`/`opened`) + latência total (DT-16) + canário/DEMOTE no loop. Validar §14 (não-regressão).
4. `conn.rs`: `spawn_reader`(handshake na thread)/`spawn_writer`/`spawn_acceptor` + testes `acceptor_terminates_on_all_closed`, `handshake_error_isolated`. Remove HOL.
5. `main.rs`: teardown DT-17 (recv_timeout + log honesto + zera ambos).
6. `cascade.rs`: `up --connections N` → `-C N`. Smoke `-C 2`.
7. `residency.rs` teste `backpressure_inflates_latency_demotes` (DT-18, prova a válvula).

## Plano de testes

- **Unit (sem GPU):** os 5 de `conn.rs` (DT-18) + `backpressure_inflates_latency_demotes` (residency) + os 15 da lib seguem verdes.
- **Ao vivo (GPU+root):** §14.3 (spill, sem regressão + sem reset de conexão sob pressão, DT-16), §14.4 (DEMOTE + 0 corrupção + teardown bounded DT-17), **smoke `nbd-client -C 2`** (DT-10/DT-15).

## Contratos e documentação viva

| Documento | Atualização | Motivo |
| --- | --- | --- |
| `ARCHITECTURE.md` | Alterar | daemon = worker único + N conexões (não serial) |
| `ROADMAP.md` | Alterar | H1 → Feito |
| `docs/daemon-multiconn/IMPL.md` | Criar | Passo 3 |
| `MEMORY.md` | Alterar | entrada da sessão |

## Checklist de validação

- [ ] `cargo fmt --all -- --check` · `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (+ `conn.rs` + `backpressure_inflates_latency_demotes`)
- [ ] §14.3 / §14.4 ao vivo (sudo bash, sandbox off) — sem regressão, sem reset de conexão
- [ ] smoke `nbd-client -C 2` — multi-conexão íntegra
- [ ] cada etapa crítica do Mapa Kahneman: pergunta/evidência/abort cumpridos
