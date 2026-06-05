# IMPL — H1 — Daemon NBD multi-conexão / leitor dedicado

SPEC ativo: [`SPECv3.md`](SPECv3.md) (go no Passo 2.5 após 2 no-go: SPEC → SPECv2 → SPECv3).
Implementa **estritamente** o SPECv3, com **uma revisão no Passo 3** (DT-16, ver abaixo).

## Escopo entregue

`main` virou o **worker CUDA único** (afinidade preservada por tipo — `Context`/`DeviceMem`
são `!Send`); uma thread **acceptor** + **leitor/escritor por conexão** alimentam o worker via
canais. NBD multi-conexão (`nbd-client -C N`) via `NBD_FLAG_CAN_MULTI_CONN`. Sem head-of-line
blocking. Latência por-request preservada como gatilho de DEMOTE.

## Arquivos

| Ação | Arquivo | Conteúdo | SPEC |
|---|---|---|---|
| CRIAR | `crates/ramshared-wsl2d/src/conn.rs` | `WMsg`/`Job`/`Reply`/`LiveCount` + `spawn_reader`/`spawn_writer`/`spawn_acceptor` + 5 testes | DT-7/8/15/18 |
| MOD | `crates/ramshared-block/src/protocol.rs` | `+ NBD_FLAG_CAN_MULTI_CONN = 1<<8` | DT-10 |
| MOD | `crates/ramshared-wsl2d/src/main.rs` | `run()` reescrito: canal `WMsg` + worker loop (`LiveCount`) + serve-only latency + teardown bounded; flag CAN_MULTI_CONN; N-agnóstico | DT-1/15/16/17 |
| MOD | `crates/ramshared-wsl2d/src/lib.rs` | `pub mod conn` + re-exports | — |
| MOD | `crates/ramshared-cli/src/cascade.rs` | `up --connections N` → `nbd-client -C N` (N>1); daemon N-agnóstico | DT-15 |
| MOD | `crates/ramshared-wsl2d/src/backend.rs` | comentário F-7 (flush no-op depende de write síncrono p/ CAN_MULTI_CONN) | F-7 |

## Rastreabilidade RF → entrega

- **RF-1** (multi-conexão): `spawn_acceptor` (N-agnóstico) + `up --connections N` → `-C N`. Validado: `-C 2`, 2 sockets.
- **RF-2** (worker CUDA único): worker loop em `main` (thread do `ctx`); `!Send` garante afinidade.
- **RF-3** (réplicas corretas/fora de ordem): `Reply` por conexão (canal ilimitado), `handle` NBD. Validado: `-C 2` 0 corrupção.
- **RF-4** (sem HOL): reader↔worker↔writer desacoplados; canal `WMsg` único backpressure.
- **RF-5** (canário/DEMOTE): canário §9 (serve-only) + §9.4 (sonda) no worker; `spawn_swapoff`.
- **RF-6** (teardown gracioso): `LiveCount` → break em live==0; `recv_timeout(5s)` + zera ambas as regiões.

## Revisão no Passo 3 (disciplina IMPL→SPEC)

**DT-16 revisado por evidência ao vivo (Kahneman #13).** O SPECv3 (auditado `go`) previa medir a
**latência total** (espera na fila + serve) para fechar a válvula de backpressure (F-8). O `§14.3`
ao vivo mostrou que isso causa **falso-positivo de DEMOTE** sob carga normal: baseline 85µs (idle)
→ 1.1ms (sob fila) = 13× → a VRAM era demovida com só ~10-72 MiB absorvidos (vs 511 do baseline),
inutilizando o tier (daemon.log: `DEMOTE (Latency) lat=1101us`). **Corrigido para serve-only**;
SPECv3 DT-16 atualizado com a justificativa. A auditoria teórica não pegou isto — só o teste ao vivo.

## Disciplina Kahneman (itens críticos)

- **DT-15 (lifecycle, #5):** término determinístico por `LiveCount` (Opened do acceptor / Closed do
  reader). Evidência: `live_count_*` (3 testes) verdes; `-C 2` sobe e desce limpo.
- **DT-16 (serve-only, #5/#13):** mede o sinal certo (op de VRAM), não a fila. Evidência: §14.3 nbd0
  511 MiB **sem** falso-positivo. Abort trigger (DEMOTE indevido) **disparou** no IMPL → revertido.
- **DT-17 (teardown, #5/#2):** `recv_timeout` bounded + zera safe pós-disconnect. Evidência: §14.4 0 corrupção.

## Validação

- `cargo fmt --all -- --check` · `cargo clippy --workspace --all-targets -- -D warnings` — limpos.
- `cargo test --workspace` — verde. `ramshared-wsl2d`: 21 (LiveCount×3, slow_writer_does_not_deadlock,
  job_reply_roundtrip, chan_cap_is_bounded + canário/sampler/state) + 1 GPU ignorado.
- **Ao vivo (RTX 2060, sudo):** §14.3 spill **511 MiB** / 332.800 páginas (sem falso-DEMOTE);
  §14.4 DEMOTE **479 MiB** migrados / 384.000 páginas / **0 corrupção**; `-C 2` **2 conexões** +
  hog íntegro (worker serializa).

## Atomicidade & rollback

Worker único serializa todo acesso à VRAM (coerência multi-conexão por construção, sem lock no hot
path). Rollback **app-only** (revert dos commits); sem migração/dados; protocolo NBD on-wire
compatível (+1 flag).
