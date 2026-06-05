# SPECv2 — H1 — Daemon NBD multi-conexão / leitor dedicado

> Versão melhorada após auditoria do Passo 2.5.
> Baseline preservado: [`SPEC.md`](SPEC.md).
> Motivo: o `SPEC.md` tinha **deadlock worker↔reply** (réplica limitada, CRITICAL-1),
> **type-mismatch que não compila** (`Reply.reply` vs `ServeOutcome.reply: [u8;N]`,
> CRITICAL-2), **hang de teardown** por N parcial (acceptor segurando o canal, HIGH-1),
> **falta `NBD_FLAG_CAN_MULTI_CONN`** (quebra `-C N` + coerência não fundamentada, HIGH-2),
> **backpressure×timeout não analisado** (risco de I/O-error→panic em swap, HIGH-3),
> contradição **RF-4×DT-6** no read-back do DEMOTE (MEDIUM-1), `Inflight` não declarado
> (MEDIUM-2) e testes sem prova de não-deadlock (MEDIUM-3).

## 0. Proveniência da auditoria (regra de saída do Passo 2.5)

- **Auditado:** `docs/daemon-multiconn/SPEC.md`. **Resultado:** `no-go`.
- **Findings bloqueantes endereçados:** CRITICAL-1, CRITICAL-2, HIGH-1, HIGH-2, HIGH-3,
  MEDIUM-1, MEDIUM-2, MEDIUM-3 (ver Decisões técnicas DT-7..DT-14).
- **Este `SPECv2.md` é o candidato ativo** para nova auditoria / Passo 3.

## Escopo fechado

**Igual ao [`SPEC.md`](SPEC.md)** (worker CUDA único = `main`; acceptor + reader/writer por
conexão; flag `--connections N`; teardown que espera o swapoff). Os deltas abaixo (DT-7..DT-14)
**substituem** as decisões com falha do baseline.

## Matriz de rastreabilidade PRD → SPECv2

Igual ao SPEC; reforços: RF-3/RF-4 → DT-7/DT-8; RF-1 → DT-9/DT-10; RF-6 → DT-12; RNF resiliência → DT-11/DT-13/DT-14.

## Decisões técnicas (delta sobre o SPEC.md)

| #    | Decisão | Corrige |
| ---- | ------- | ------- |
| DT-1..DT-6 | **Herdadas do SPEC.md**, exceto onde DT-7..DT-14 sobrescrevem. | — |
| **DT-7** | **Canal de réplica ILIMITADO** (`std::sync::mpsc::channel::<Reply>()`, não `sync_channel`). O **único** ponto de backpressure é o canal de **Jobs** (`sync_channel(CHAN_CAP)`). O worker **nunca** bloqueia em `reply.send()` → sempre progride consumindo Jobs. **Limite de memória:** o backlog de réplicas por conexão é limitado pela **profundidade da fila NBD do kernel** por conexão (o cliente não submete mais requests sem receber réplicas; se o writer trava, o reader da mesma conexão para de receber requests do kernel → para de gerar Jobs → o backlog de réplicas para de crescer). Teto ≈ `nr_requests × max_reply` por conexão. | **CRITICAL-1** (deadlock) |
| **DT-8** | **`Reply.reply: [u8; ramshared_block::protocol::SIMPLE_REPLY_LEN]`** (array fixo `Copy`, sem alocação no hot path), espelhando `ServeOutcome.reply`. O writer faz `write_all(&r.reply)`. `data: Vec<u8>` e `disconnect: bool` inalterados. | **CRITICAL-2** (não compila) |
| **DT-9** | **Término determinístico do acceptor:** aceita a 1ª conexão (bloqueante), depois `set_nonblocking(true)` e **drena o backlog** aceitando até `N` no total dentro de uma janela curta (`ACCEPT_WINDOW_MS = 250`, re-tentando em `WouldBlock`); ao fim **dropa seu `SyncSender<Job>` template** e fecha o `listener`. `nbd-client -C N` abre os N sockets juntos no `up`, então caem no backlog inicial. Assim o canal de Jobs fecha quando **todas as conexões aceitas caem** (não depende de "N exato atingido") → sem hang com N parcial. `run()` move sua única cópia de `jobs_tx` para o acceptor (não retém nenhuma). | **HIGH-1** (hang teardown) |
| **DT-10** | **Anuncia `NBD_FLAG_CAN_MULTI_CONN = 1 << 8`** (novo em `protocol.rs`) em `tx_flags`. **Justificativa de coerência (cadeia explícita):** `cuMemcpyHtoD` é **síncrono** (driver.rs) ⇒ toda WRITE é durável na VRAM **no instante do ack** ⇒ `VramBackend::flush` é no-op **correto** ⇒ um FLUSH em qualquer conexão cobre trivialmente todas as WRITEs já ackadas (de todas as conexões) ⇒ a promessa do `CAN_MULTI_CONN` é satisfeita **por construção** (write síncrono + worker único serializando). Sem o flag, o kernel recusa/degrada `-C N`. | **HIGH-2** (protocolo) |
| **DT-11** | **Backpressure×timeout (worst-case #5):** `CHAN_CAP = 64` é da ordem do `nr_requests` default do block layer (~128); o worker drena FIFO. O `ramshared up`/`nbd-client` **não** seta `-t` curto (mantém o default tolerante — validado no §14.4, onde o swapoff levou 6 s sem panic). **Sob lentidão sustentada da VRAM** (eviction), o backpressure faz o canário §9.4/latência **disparar o DEMOTE** — que é a **válvula de alívio projetada** (migra para fora da VRAM), não um caminho de panic. Declarado: `CHAN_CAP` ≥ profundidade típica; nenhum `-t` agressivo; backpressure→DEMOTE é intencional. | **HIGH-3** (panic em swap) |
| **DT-12** | **Teardown sem deadlock (corrige RF-4×DT-6):** o read-back do swapoff é servido **dentro** do worker loop (chega como Jobs READ enquanto a conexão vive); o poll de `demote_rx` continua **no loop** (desarma o canário ao confirmar). Quando `jobs_rx` fecha (todas as conexões caíram — durante DEMOTE normal isso ocorre **após** o swapoff terminar o read-back), faz-se um **`recv_timeout(5s)`** em qualquer swapoff ainda em voo (edge #2a: disconnect no meio) — **bounded**, nunca `recv()` infinito; depois zera as duas regiões (`backend.zero()` + `probe.zero()`, ambos). | **MEDIUM-1** (deadlock/contradição) |
| **DT-13** | **`Inflight` explicitamente fora de escopo (Day-0):** o worker único serializa todos os acessos à VRAM; sob `CAN_MULTI_CONN`, **o cliente garante** não criar dependências de ordem cross-conexão, então a ordem de entrega cross-reader (mpsc não-determinística entre senders) é irrelevante para corretude. `Inflight` (§8.1) **não** é usado nem criado. Declarado para remover o drift com a doc do `ramshared-block`. | **MEDIUM-2** (drift/ordem) |
| **DT-14** | **Evidência de não-deadlock reproduzível (sem GPU):** teste `slow_writer_does_not_deadlock` (monta reader→jobs(bounded)→worker-stub→reply(unbounded DT-7)→writer-que-não-drena; assere que o worker-stub continua consumindo Jobs e o sistema não trava) e `acceptor_terminates_on_partial_n` (N=2, 1 conexão que cai → o canal de Jobs fecha). | **MEDIUM-3** (evidência) |

## Fronteira de atomicidade e política de rollback

Igual ao SPEC.md. Reforço (DT-7): o worker nunca bloqueia em réplica → progresso garantido;
backpressure só no canal de Jobs. Rollback **app-only** (revert dos commits).

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-2/3 (canais, DT-7/DT-8) | #5 Worst-case | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "writer travado de 1 conexão trava o daemon?" | `slow_writer_does_not_deadlock` (DT-14) verde; §14.3 ao vivo sem regressão | worker bloqueia / réplica perdida → reverter |
| ITEM-4 (multi-conn, DT-9/DT-10) | #5 + #13 | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "`-C 2` é recusado (sem flag) ou corrompe?" | smoke `nbd-client -C 2` conecta + hash íntegro; `acceptor_terminates_on_partial_n` verde | recusa de `-C N` ou divergência de hash → reverter p/ N=1 |
| ITEM-5 (teardown sob DEMOTE, DT-12) | #5 + #2 | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "disconnect no meio do swapoff zera VRAM migrando / trava o teardown?" | §14.4 ao vivo: DEMOTE + 0 corrupção; `recv_timeout` bounded (sem hang) | corrupção no read-back ou hang no teardown → reverter |
| ITEM-6 backpressure (DT-11) | #5 Worst-case | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "burst + VRAM lenta estoura timeout NBD → I/O error em swap?" | §14.3 sob pressão sem reset de conexão; sem `-t` agressivo | reset de conexão/I/O-error em swap sob carga → reverter |

## Checklist de segurança (pré-implementação)

Igual ao SPEC.md (isolamento, OOB cap anti-DoS no reader, root, sem `unsafe` novo,
`forbid(unsafe_code)`, sem endereços logados). **`Context`/`DeviceMem` são `!Send`** → o
compilador **força** o worker a ficar na thread do `ctx` (afinidade garantida por tipo, LOW-2).

## Arquivos a CRIAR

### `crates/ramshared-wsl2d/src/conn.rs`
- **Igual ao SPEC.md**, com os deltas:
  - `Reply.reply: [u8; SIMPLE_REPLY_LEN]` (DT-8); `import ramshared_block::protocol::SIMPLE_REPLY_LEN`.
  - O `Job.reply` é `std::sync::mpsc::Sender<Reply>` (canal **ilimitado**, DT-7), não `SyncSender`.
  - `spawn_writer(writer, replies: Receiver<Reply>)`: `for r in replies { write_all(&r.reply); if !r.data.is_empty() {write_all(&r.data)}; flush(); if r.disconnect {break} }`.
  - `spawn_acceptor(...)`: DT-9 (1ª bloqueante → `set_nonblocking` → drena até N em `ACCEPT_WINDOW_MS=250` → dropa template + fecha listener); anuncia `tx_flags` com `CAN_MULTI_CONN` (DT-10).
  - Consts: `CHAN_CAP=64` (Jobs, bounded), `ACCEPT_WINDOW_MS=250`.
- **Testes (sem GPU):** `job_reply_roundtrip`, `chan_cap_is_bounded`, **`slow_writer_does_not_deadlock`** (DT-14), **`acceptor_terminates_on_partial_n`** (DT-14, usando `UnixListener` em socketpair/tempdir, sem GPU).

### `crates/ramshared-block/src/protocol.rs`
- **O que muda:** `+ pub const NBD_FLAG_CAN_MULTI_CONN: u16 = 1 << 8;` (DT-10). Sem outra mudança no protocolo.
- **Impacto:** novo flag exportado; `server_handshake` inalterado (recebe `tx_flags` pronto).
- **Teste:** o handshake já é testado; adicionar asserção de que o flag pode compor `tx_flags`.

## Arquivos a MODIFICAR

### `crates/ramshared-wsl2d/src/main.rs`
- **Igual ao SPEC.md** (run = setup CUDA + mlockall + canário inalterados; cria canal de Jobs;
  `spawn_acceptor`; worker loop; teardown), com os deltas:
  - `tx_flags = NBD_FLAG_HAS_FLAGS | NBD_FLAG_SEND_FLUSH | NBD_FLAG_CAN_MULTI_CONN` (DT-10).
  - Worker loop: `let out = serve(...); /* poll demote_rx + canário §9/§9.4 (no loop, DT-12) */ let _ = job.reply.send(Reply { reply: out.reply, data: out.read_data, disconnect: out.disconnect });` — `out.reply` é `[u8;N]`, casa com `Reply.reply` (DT-8), **sem `.to_vec()`**.
  - Teardown (DT-12): após `jobs_rx` fechar e `acceptor.join()`, `if let Some(rx)=demote_rx.take() { let _ = rx.recv_timeout(std::time::Duration::from_secs(5)); }`; depois `let r1=backend.zero(); let _=probe.zero(); r1?;`.
  - **Doc-comment do módulo** (LOW-1): reescrever "serve **uma** conexão... multi-conexão futuro" → reflete N conexões + worker único.
- **Impacto:** sem uAPI/ABI on-wire (só um flag a mais, compatível); latência por-request preservada; HOL removido.

### `crates/ramshared-wsl2d/src/lib.rs`
- Igual ao SPEC.md: `pub mod conn;` + `pub use conn::{Job, Reply, CHAN_CAP};`.

### `crates/ramshared-cli/src/cascade.rs`
- Igual ao SPEC.md: `--connections N` no `up` → `--connections N` ao daemon e `-C N` ao `nbd-client` (só quando N>1). **Não** adiciona `-t` curto (DT-11). Default N=1.

## Ordem de implementação

Igual ao SPEC.md (1→6), com: na fatia 1, `Reply.reply` array fixo + canal de réplica ilimitado;
fatia 4 inclui `CAN_MULTI_CONN` + término DT-9; fatia 5 = teardown DT-12. Os 2 testes de
concorrência (DT-14) entram nas fatias 1 e 4.

## Plano de testes

- **Unit (sem GPU):** `job_reply_roundtrip`, `chan_cap_is_bounded`, `slow_writer_does_not_deadlock`,
  `acceptor_terminates_on_partial_n` (conn.rs) + os 15 da lib seguem verdes.
- **Ao vivo (GPU+root):** §14.3 (spill, sem regressão + sem reset de conexão sob pressão, DT-11),
  §14.4 (DEMOTE + disconnect, 0 corrupção, teardown bounded DT-12), **smoke `nbd-client -C 2`** (DT-9/DT-10).

## Contratos e documentação viva

Igual ao SPEC.md + `crates/ramshared-block` (novo flag `CAN_MULTI_CONN` — comentário no `protocol.rs`).

## Checklist de validação

Igual ao SPEC.md.
