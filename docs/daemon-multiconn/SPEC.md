# SPEC — H1 — Daemon NBD multi-conexão / leitor dedicado

Fonte: [`PRD.md`](PRD.md). Implementa a Fase A "Agora" do ROADMAP (issue #3, H1).
Decisões fechadas para o Passo 3.

## Escopo fechado desta implementação

**Entra agora:**
- `main` (run) vira o **worker CUDA único** (dono de `ctx`/`backend`/`probe`/`sampler` —
  afinidade preservada). Uma thread **acceptor** aceita até `N` conexões; cada conexão tem um
  **reader** e um **writer** dedicados. Comunicação por **canais limitados** (`sync_channel`).
- Flag `--connections N` (default 1) no daemon + propagação `-C N` no `ramshared up`.
- Teardown drena in-flight, **espera o `swapoff` em voo** (resolve finding #2a) e zera VRAM+canário.

**Fica fora:** múltiplos exports; vazão paralela de VRAM (1 thread CUDA); async; writeback/ublk.

**Dependências prontas:** `ramshared-block` (`serve`/`parse_request`/`server_handshake`),
`ramshared-cuda`, `ramshared-wsl2d` (canário §9.4 já mergeado).

## Matriz de rastreabilidade PRD → SPEC

| PRD  | Implementação no SPEC |
| ---- | --------------------- |
| RF-1 (multi-conexão) | ITEM-4 (acceptor N), ITEM-6 (CLI -C), DT-5 |
| RF-2 (worker CUDA único) | ITEM-2 (worker loop em `main`), DT-1, DT-2 |
| RF-3 (réplicas fora de ordem) | ITEM-3 (writer por conexão + `reply_tx` no Job), DT-3 |
| RF-4 (sem HOL) | ITEM-2+ITEM-3 (reader↔worker↔writer desacoplados), DT-2 |
| RF-5 (canário/DEMOTE) | ITEM-2 (canário roda no worker), DT-4 |
| RF-6 (teardown gracioso) | ITEM-5, DT-6 |

## Decisões técnicas

| #    | Decisão | Justificativa |
| ---- | ------- | ------------- |
| DT-1 | **`main` é o worker CUDA.** O setup CUDA (load/ctx/alloc/zero/canário) fica em `run()` como hoje; `run()` então **consome o canal de Jobs**. Não move CUDA para thread spawnada. | Afinidade de thread (driver.rs:176) + mínima perturbação do caminho CUDA validado §14. |
| DT-2 | **Canal de Jobs limitado:** `std::sync::mpsc::sync_channel::<Job>(CHAN_CAP)`, `CHAN_CAP=64`. Readers clonam o `SyncSender`; worker tem o `Receiver`. Fim por drop de todos os senders. | Backpressure (sem OOM sob burst); fim natural quando todas as conexões caem. |
| DT-3 | **Réplica por conexão:** cada conexão cria `sync_channel::<Reply>(CHAN_CAP)`; o `Job` carrega o `SyncSender<Reply>`. Writer da conexão drena o `Receiver<Reply>`. | Réplicas fora de ordem por conexão; nenhum interleaving entre conexões; worker nunca bloqueia num socket. |
| DT-4 | **Canário/DEMOTE no worker.** `Canary`/`ResidencySampler`/`Cadence`/`probe`/`demote_rx` viram estado local do worker loop (como hoje, só que dirigido por Jobs em vez do laço serial). `spawn_swapoff` inalterado. | Mantém §9/§9.4 e a afinidade (sonda usa CUDA). |
| DT-5 | **Acceptor thread** faz `listener.accept()` em laço até `N`; por conexão spawna reader+writer e clona o `SyncSender<Job>`. Após `N` aceitas, encerra. `N` default 1. | Desacopla o accept do worker; `N=1` ≡ comportamento atual. |
| DT-6 | **Teardown ordenado:** worker sai do loop quando o `Receiver<Job>` fecha (todas as conexões caíram) → **espera `demote_rx` em voo** (join lógico do swapoff) → `backend.zero()` + `probe.zero()` (incondicional, fix #6a já mergeado) → remove socket. | Resolve #2a (não zerar VRAM com swapoff migrando); §11. |

## Fronteira de atomicidade e política de rollback

- **Atomicidade:** o **worker único serializa** todo acesso à VRAM — não há 2 writes concorrentes
  (coerência multi-conexão por construção, sem lock no hot path). Cada Job é processado
  atomicamente (read→serve→reply). DEMOTE (`swapoff`) atômico no kernel (caminho existente).
- **Rollback:** **app-only** (revert dos commits). Sem migração/dados; protocolo NBD on-wire inalterado.

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-2/ITEM-3 (modelo de threads/canais) | #5 Worst-case | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "que sequência de canal-cheio/desconexão trava o worker ou perde réplica?" | unit: `Job→serve→Reply` round-trip; teste de canal cheio (backpressure não deadlocka); `§14.3` ao vivo sem regressão | worker bloqueia >T ou réplica perdida → reverter |
| ITEM-4 (multi-conexão) | #5 Worst-case + #13 ilusão de validade | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "2 conexões escrevendo a mesma página corrompem?" | integração: `nbd-client -C 2` + hash íntegro; revisão: worker único serializa | corrupção/divergência de hash sob `-C N` → reverter p/ N=1 |
| ITEM-5 (teardown sob DEMOTE) | #5 Worst-case + #2 Counterfactual | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "disconnect no meio do swapoff zera a VRAM migrando?" | `§14.4` ao vivo: DEMOTE + disconnect, 0 corrupção; código espera `demote_rx` antes de `zero()` | corrupção no read-back durante teardown → reverter |

## Checklist de segurança (pré-implementação)

- [x] Isolamento: todas as conexões servem o **mesmo** export; `serve()`/`DeviceMem` bounds-check inalterado; canário separado.
- [x] OOB: cap anti-DoS de WRITE (`len>device`) movido para o **reader** (por conexão).
- [x] Permissões: daemon root (subido pelo `up`).
- [x] Input validation: só protocolo NBD (`parse_request`); sentinela do canário sintética.
- [x] Sem `unsafe` novo; `#![forbid(unsafe_code)]` mantido (threads/canais são `std` safe).
- [x] Ponteiros: nenhum endereço de VRAM logado.

## Arquivos a CRIAR

### `crates/ramshared-wsl2d/src/conn.rs`
- **Propósito:** tipos e threads de conexão (Job/Reply, acceptor, reader, writer). Mantém `main.rs` pequeno (<800 linhas, regra `coding.md`).
- **Requisitos:** RF-1, RF-3, RF-4, DT-2, DT-3, DT-5.
- **Types/funcs (assinaturas exatas):**
  ```rust
  use std::os::unix::net::{UnixListener, UnixStream};
  use std::sync::mpsc::{Receiver, SyncSender};
  use ramshared_block::Request;

  pub const CHAN_CAP: usize = 64; // DT-2/DT-3

  /// Um request a processar pelo worker CUDA, com a rota de réplica da conexão.
  pub struct Job {
      pub req: Request,
      pub payload: Vec<u8>,
      pub reply: SyncSender<Reply>, // canal de réplica da conexão de origem (DT-3)
  }

  /// Resultado do `serve()` a escrever no socket da conexão.
  pub struct Reply {
      pub reply: Vec<u8>,     // header NBD (out.reply)
      pub data: Vec<u8>,      // read-back (out.read_data, pode ser vazio)
      pub disconnect: bool,   // out.disconnect
  }

  /// Thread leitora: lê headers/payload do socket, valida cap anti-DoS, enfileira Jobs.
  /// Encerra em EOF/erro; ao encerrar, dropa seu SyncSender<Job> (fim natural do worker).
  pub fn spawn_reader(
      reader: UnixStream,
      device_size: u64,
      jobs: SyncSender<Job>,
      replies: SyncSender<Reply>,
  ) -> std::thread::JoinHandle<()>;

  /// Thread escritora: drena Reply e escreve no socket (réplicas fora de ordem OK).
  pub fn spawn_writer(
      writer: UnixStream,
      replies: Receiver<Reply>,
  ) -> std::thread::JoinHandle<()>;

  /// Thread acceptor: aceita até `n` conexões; por conexão faz o handshake NBD,
  /// cria o canal de réplica e spawna reader+writer. Clona `jobs` por conexão.
  pub fn spawn_acceptor(
      listener: UnixListener,
      n: u32,
      device_size: u64,
      tx_flags: u16,
      jobs: SyncSender<Job>,
  ) -> std::thread::JoinHandle<()>;
  ```
- **Lógica resumida:**
  - `spawn_reader`: loop `read_exact(hdr)` → `parse_request` → se `Write` e `len>device_size` → log + break; lê payload se Write; `jobs.send(Job{req,payload,reply: replies.clone()})` (bloqueia se cheio = backpressure). EOF/Err → break (dropa `jobs`/`replies`).
  - `spawn_writer`: `for r in replies { write_all(r.reply); if !r.data.is_empty() {write_all(r.data)}; flush(); if r.disconnect {break} }`.
  - `spawn_acceptor`: `for _ in 0..n { let (s,_)=accept()?; let w=s.try_clone()?; server_handshake(&mut BufReader::new(s),&mut w,device_size,tx_flags)?; let (rtx,rrx)=sync_channel(CHAN_CAP); spawn_writer(w,rrx); spawn_reader(s,device_size,jobs.clone(),rtx); }` (handshake por conexão).
- **Dependências internas:** `ramshared-block`. **Externas:** nenhuma.
- **Padrão de referência:** o laço serial atual de `main.rs` (read→serve→write) refatorado.
- **Testes requeridos (`#[cfg(test)]`, sem GPU):** `job_reply_roundtrip` (constrói Job/Reply, checa campos); `chan_cap_is_bounded` (sync_channel(CAP) bloqueia no CAP+1 — backpressure, via try_send). Round-trip real multi-conexão = integração `--ignored`/§14.
- **Disciplina Kahneman:** ITEM-2/3/4 (#5/#13) — ver Mapa.

## Arquivos a MODIFICAR

### `crates/ramshared-wsl2d/src/main.rs`
- **O que muda:** `run()` deixa de fazer `accept()`+laço serial; passa a: (a) parse `--connections N`;
  (b) setup CUDA + mlockall + canário **inalterados**; (c) bind socket; (d) criar `sync_channel::<Job>(CHAN_CAP)`;
  (e) `spawn_acceptor(listener, n, size, tx_flags, jobs_tx)`; (f) **worker loop**: `for job in jobs_rx { serve + canário/DEMOTE + job.reply.send(Reply) }`; (g) teardown (DT-6).
- **Requisitos:** RF-2, RF-5, RF-6, DT-1, DT-4, DT-6.
- **Função/bloco afetado:** `run()` (linhas ~119-234: do `accept()` ao teardown).
- **Antes:** `let (stream,_)=listener.accept()?; … loop { read_exact; serve; write; poll demote; canário } … backend.zero()`.
- **Depois (shape):**
  ```rust
  let n: u32 = connections; // --connections, default 1
  let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel::<Job>(CHAN_CAP);
  let acceptor = spawn_acceptor(listener, n, size, tx_flags, jobs_tx); // dropa o template ao fim
  // worker loop (thread atual = dona do CUDA):
  for job in jobs_rx.iter() {
      let out = serve(&job.req, &job.payload, &mut backend);
      // poll demote_rx + canário §9 (latência por-request) + cadência §9.4 — IDÊNTICOS a hoje,
      // só que `touches_vram`/`lat_us` derivam de `job.req` e do tempo do `serve()`.
      let _ = job.reply.send(Reply { reply: out.reply, data: out.read_data, disconnect: out.disconnect });
  }
  let _ = acceptor.join();
  // DT-6: espera o swapoff em voo antes de zerar (fix #2a)
  if let Some(rx) = demote_rx.take() { let _ = rx.recv(); }
  backend.zero().and(probe.zero()).ok(); // ambos (fix #6a já mergeado)
  ```
- **Impacto:** sem uAPI/ABI; protocolo NBD inalterado; latência por-request preservada; remove HOL.
  `serve()`/handshake reusados. **Day-0:** o laço serial é **substituído** (sem dual-path).
- **Testes:** `cargo test -p ramshared-wsl2d`; clippy `-D warnings`; **§14.3/§14.4 ao vivo** + smoke `-C 2`.
- **Disciplina Kahneman:** ITEM-2/5 (#5/#2) — ver Mapa.

### `crates/ramshared-wsl2d/src/lib.rs`
- **O que muda:** `pub mod conn;` + `pub use conn::{Job, Reply, CHAN_CAP};` (acceptor/reader/writer são fns de bin; manter `pub` p/ teste).
- **Impacto:** API de lib cresce (interna); `forbid(unsafe_code)` mantido.

### `crates/ramshared-wsl2d/src/main.rs` — parse de args
- **O que muda:** adicionar `--connections N` (u32, default 1; valida `N>=1`).
- **Por quê:** RF-1. **Impacto:** novo flag opcional; sem mudança se omitido.

### `crates/ramshared-cli/src/cascade.rs`
- **O que muda:** `ramshared up` ganha `--connections N` (default 1); passa `--connections N` ao daemon e `-C N` ao `nbd-client` (linha `sh("nbd-client", &["-unix", SOCK, NBD])` → inclui `-C` quando N>1).
- **Requisitos:** RF-1, ITEM-6. **Impacto:** default 1 = sem mudança; teardown `nbd-client -d` inalterado.
- **Testes:** smoke `ramshared up --connections 2` + `nbd-client -C 2` (manual/§14).

## Ordem de implementação

1. `conn.rs`: `Job`/`Reply`/`CHAN_CAP` + testes puros (`job_reply_roundtrip`, `chan_cap_is_bounded`).
2. `main.rs`: trocar o laço serial pelo **canal + worker loop** com **acceptor de 1 conexão** (N=1); canário/DEMOTE no worker. Validar §14 (não-regressão).
3. `conn.rs`: `spawn_reader`/`spawn_writer` desacoplados (remove HOL); validar §14.
4. `conn.rs`: `spawn_acceptor` até N + flag `--connections`; smoke `-C 2`.
5. `main.rs`: teardown DT-6 (espera swapoff + zero ambos).
6. `cascade.rs`: propagar `--connections`/`-C`.

## Plano de testes

- **Unitários (sem GPU):** `job_reply_roundtrip`, `chan_cap_is_bounded` (conn.rs); os 15 testes da lib seguem verdes.
- **Integração/ao vivo (GPU+root):** `§14.3` (spill, sem regressão), `§14.4` (DEMOTE + disconnect, 0 corrupção), **smoke `nbd-client -C 2`** (multi-conexão, hash íntegro).
- **Concorrência:** canal cheio não deadlocka (backpressure); 2 conexões → worker serializa (sem corrupção).

## Contratos e documentação viva

| Documento | Atualização | Motivo |
| --- | --- | --- |
| `ARCHITECTURE.md` | Alterar | daemon deixa de ser "serve loop serial"; H1 resolvido |
| `ROADMAP.md` | Alterar | mover H1 de "Agora" para "Feito" |
| `docs/daemon-multiconn/IMPL.md` | Criar | Passo 3 |
| `MEMORY.md` | Alterar | entrada da sessão |

## Checklist de validação

- [ ] `cargo fmt --all -- --check` · `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (+ `conn.rs` novos)
- [ ] §14.3 / §14.4 ao vivo (sudo bash, sandbox off) — sem regressão
- [ ] smoke `nbd-client -C 2` — multi-conexão íntegra
- [ ] cada etapa crítica: pergunta/evidência/abort do Mapa Kahneman cumpridos
