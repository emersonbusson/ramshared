---
slug: daemon-multiconn
title: Daemon NBD multi-conexão / leitor dedicado (sem head-of-line blocking)
milestone: —
issues: [3]
---
# PRD — H1 — Daemon NBD multi-conexão / leitor dedicado

> **Nota (pós-auditoria, F-2):** o SPEC ativo é o [`SPECv3.md`](SPECv3.md). Em relação aos
> esboços deste PRD, o SPECv3 fixou: `Reply.reply: [u8; SIMPLE_REPLY_LEN]` (não `Vec`),
> canal de réplica **ilimitado** (`Sender`, não `SyncSender` — evita deadlock DT-7), e a
> latência do canário passou a medir o **total** (enfileiramento→réplica, DT-16). O resto
> do PRD permanece válido.

## Resumo

O daemon `ramshared-wsl2d` serve **uma** conexão NBD num laço **serial**: lê 1 request,
processa (`serve()` síncrono, I/O CUDA), responde, repete. Um request lento (op de VRAM
sob eviction, ou a sonda-canário §9.4) **bloqueia a leitura do próximo** (head-of-line
blocking) e **alonga a janela do DEMOTE** — o kernel não consegue pipeline de I/O e o
read-back do `swapoff` compete com o tráfego normal no mesmo fluxo serial.

Esta mudança desacopla **leitura do socket** de **processamento CUDA**: leitor(es)
dedicado(s) drenam o socket para uma fila limitada; um **único worker CUDA** (preservando
a afinidade de thread obrigatória) processa e responde. Resultado: sem HOL blocking, NBD
multi-conexão (kernel multi-queue) e janela de DEMOTE menor — sem aumentar a vazão real de
VRAM (limitada pela thread CUDA única, por design).

Valor: reduz latência percebida e o tempo de DEMOTE sob pressão — o gargalo operacional do
tier VRAM (Fase A, ROADMAP "Agora").

## Contexto técnico

- **Módulo:** `crates/ramshared-wsl2d` (daemon) — `src/main.rs` (laço de serve);
  `crates/ramshared-block` (protocolo NBD, `serve()`/`parse_request`); `crates/ramshared-cuda`
  (`Context`/`DeviceMem`, I/O de VRAM).
- **Estado atual — Confirmado no codebase:**
  - `main.rs:133` — `listener.accept()` **uma vez**; serve exatamente 1 conexão = a vida do swap.
  - `main.rs:147` — `loop` serial: `read_exact(hdr)` → `serve(&req,…,&mut backend)` (síncrono)
    → `write_all(reply)` → poll de `demote_rx` → canário. **Tudo na mesma thread.**
  - `crates/ramshared-cli/src/cascade.rs:190` — `nbd-client -unix <sock> <nbd>` **sem `-C`**
    (1 conexão); teardown `nbd-client -d`.
  - **Restrição dura (`crates/ramshared-cuda/src/driver.rs:176-181`):** a corrente do
    contexto CUDA é **thread-local**; todo I/O de VRAM tem de rodar **na mesma thread** que
    criou o contexto. `cuCtxSetCurrent` não é usado. → **não dá para paralelizar ops de VRAM
    entre threads.**
  - DEMOTE já roda em thread separada (`spawn_swapoff`, só `swapoff`, sem CUDA) e o laço
    continua servindo o read-back via poll de `demote_rx`.
- **Confirmado na documentação oficial:** NBD permite **réplicas fora de ordem** (cada request
  carrega um `handle` de 64 bits) e **multi-conexão** (`nbd-client -C N` abre N sockets para o
  mesmo device; multi-queue do bloco). FLUSH coordena durabilidade.
- **Proposto:** arquitetura leitor(es) → fila limitada → **worker CUDA único** → writer(s) de
  réplica por conexão.

## Opção recomendada

**Reader/Worker/Writer com worker CUDA único.**

- **N leitores** (1 por conexão NBD aceita): cada um faz só `read_exact` do header+payload e
  enfileira `(request, payload, reply_tx_da_conexão)` num **canal limitado** (`sync_channel`,
  backpressure).
- **1 worker CUDA** (a thread que criou o `Context`): consome a fila, chama `serve(&req,…,&mut
  backend)`, devolve a réplica ao `reply_tx` da conexão de origem. Roda também o canário §9.4
  (sonda/free-floor) e dispara DEMOTE — **toda interação CUDA fica nesta thread** (afinidade
  preservada).
- **N writers** (1 por conexão): drenam o `reply_rx` da conexão e escrevem no socket. Réplicas
  podem sair fora de ordem (NBD `handle`).

Por quê: é a **única** forma de remover o HOL blocking **respeitando a afinidade de thread do
CUDA**. O leitor nunca fica preso atrás de uma op de VRAM lenta; o read-back do DEMOTE é só
mais um item na fila, servido com prioridade natural; o kernel pode manter vários requests em
voo (multi-queue).

**Alternativas descartadas:**

- **Múltiplos workers CUDA** — **impossível**: afinidade de thread do contexto (driver.rs:176).
- **async/tokio** — overhead e zero ganho: a FFI CUDA é **bloqueante/síncrona**; não há await real
  no caminho quente. Viola também o "zero deps externas" atual sem benefício.
- **Thread por request** — churn de threads e ainda serializado no worker CUDA único; pior.
- **Manter serial + só um leitor "à frente"** — alivia parcialmente mas não suporta multi-conexão
  nem desacopla o writer; meia-solução (não Day-0).

**Trade-offs aceitos:** a **vazão real de VRAM não aumenta** (1 thread CUDA) — o ganho é
latência/HOL/janela-de-DEMOTE e capacidade de absorver bursts via a fila. Mais threads (N
leitores + 1 worker + N writers) e um canal limitado (memória previsível).

## Requisitos funcionais

- **RF-1 — Multi-conexão.** O daemon aceita `--connections N` (default 1) conexões NBD para o
  mesmo export; cada conexão tem um leitor e um writer dedicados.
  - *Critério:* `nbd-client -C N -unix <sock> <nbd>` conecta; `N` sockets ativos; I/O íntegro.
  - *Isolamento:* todas as conexões servem o **mesmo** `VramBackend` (mesmo export); nenhum
    acesso fora dos limites do device (bounds-check de `serve()`/`DeviceMem` inalterado).
- **RF-2 — Worker CUDA único.** Todo I/O de VRAM ocorre numa única thread worker; requests
  chegam por canal limitado.
  - *Critério:* nenhuma chamada `DeviceMem`/`Context`/`serve()` fora da thread worker (revisão +
    teste de fumaça); afinidade preservada.
- **RF-3 — Réplicas corretas e fora de ordem.** Cada réplica volta para a conexão de origem com o
  `handle` correto; sem interleaving corrompido entre conexões.
  - *Critério:* round-trip multi-conexão íntegro (hash); réplicas fora de ordem aceitas pelo kernel.
- **RF-4 — Sem head-of-line blocking.** Uma op de VRAM lenta **não** impede o leitor de drenar o
  socket nem o writer de responder outras conexões; o read-back do DEMOTE é servido enquanto o
  `swapoff` está em curso.
  - *Critério:* sob sonda/op lenta, o leitor continua enfileirando; §14.4 (DEMOTE) sem regressão.
- **RF-5 — Canário/DEMOTE preservados.** O canário §9 (latência por-request) e §9.4
  (sonda/free-floor) e o DEMOTE (`spawn_swapoff`) continuam funcionando, agora ancorados na thread
  worker.
  - *Critério:* os testes de `ResidencySampler`/`Canary` seguem verdes; §14.3/§14.4 ao vivo OK.
- **RF-6 — Teardown gracioso.** Ao EOF/disconnect de todas as conexões, drena os in-flight, **espera
  o DEMOTE em voo**, zera a VRAM (§11) e sai.
  - *Critério:* sem páginas perdidas no disconnect durante DEMOTE; VRAM zerada no fim (inclui canário).

## Requisitos não-funcionais

- **Performance:** latência por-request **não regride** vs. serial atual (p50/p99); HOL eliminado.
  Vazão de VRAM = 1 thread (inalterada). Canal limitado (ex.: 64 itens) para backpressure.
- **Segurança:** sem novo `unsafe` (`#![forbid(unsafe_code)]` mantido na lib); daemon root; sem
  input externo além do protocolo NBD já validado (`parse_request`, bounds). Sem endereços logados.
- **Observabilidade:** logs `[wsl2d]` por conexão (conectou/saiu), profundidade de fila sob pressão,
  DEMOTE distinguindo gatilho real de erro (M4, já existente).
- **Escalabilidade:** N conexões com 1 worker; canal limitado evita memória ilimitada sob burst.
- **Resiliência:** canal cheio → backpressure no leitor (não OOM); writer lento de uma conexão não
  trava as outras; pânico de uma thread não corrompe a VRAM (worker único é o dono).
- **LGPD:** N/A (sem dados pessoais; sentinelas sintéticas).

## Fluxos

**Happy path (multi-conexão):**
1. `ramshared up --connections N` sobe o daemon; CLI chama `nbd-client -C N -unix <sock> <nbd>`.
2. Daemon faz `accept()` N vezes; para cada conexão: spawna **leitor** + **writer** e cria o
   `reply_tx/reply_rx` da conexão.
3. Leitor lê header (+payload se WRITE), valida cap anti-DoS, enfileira `(req,payload,reply_tx)` no
   canal do worker (bloqueia se cheio — backpressure).
4. **Worker CUDA** desenfileira, `serve()` na VRAM, envia a réplica ao `reply_tx`; roda canário e,
   em gatilho, dispara DEMOTE (`spawn_swapoff`) e segue servindo.
5. Writer da conexão drena `reply_rx` e escreve no socket (réplicas podem sair fora de ordem).
6. EOF numa conexão → leitor encerra, sinaliza; quando todas encerram e o worker drena, teardown.

**Fluxos alternativos:** N=1 (comportamento equivalente ao atual, mas com leitor/worker/writer
desacoplados). DEMOTE em curso: read-back flui pela fila normalmente.

**Fluxos de erro:**
- WRITE com `len > device` → leitor desconecta aquela conexão (anti-DoS; já existe), sem derrubar as outras.
- Socket de uma conexão quebra (`write_all`/`read_exact` Err) → encerra **só** aquela conexão.
- Canal worker desconectado (worker morreu) → leitores encerram; teardown com erro logado.
- `swapoff` falha → re-arma o canário (comportamento atual), worker segue.

## Modelo de dados

- **`struct Job`** (item do canal worker): `req: Request`, `payload: Vec<u8>`,
  `reply: SyncSender<Reply>` (o canal de réplica da conexão de origem). Movido por valor.
- **`struct Reply`**: `reply: Vec<u8>` (header NBD), `data: Vec<u8>` (read-back, possível vazio),
  `disconnect: bool`. (Espelha o `out` atual de `serve()`.)
- **Canais:** `sync_channel::<Job>(CAP)` (worker, **limitado** → backpressure); por conexão,
  `sync_channel::<Reply>(CAP)` (writer). Ciclo de vida: leitores/worker/writers spawnados no `up`,
  encerram por EOF/Drop dos sends.
- **Posse:** o **worker** é o único dono de `&mut VramBackend` + `CanaryProbe`/`Cadence`/
  `ResidencySampler` + `ctx` (afinidade). Leitores/writers só tocam sockets e canais. Sem
  `Arc<Mutex<backend>>` (o worker serializa por construção — sem lock no hot path).
- **uAPI/ABI:** **nenhuma mudança** — protocolo NBD on-wire inalterado; sem ioctl/sysfs novo.

## API / Interfaces

- **Sem ioctl/sysfs/debugfs novo.** Protocolo NBD on-wire inalterado (handshake + requests).
- **CLI/daemon flag nova:** `--connections N` no `ramshared-wsl2d` e propagação no
  `ramshared up` (CLI `cascade.rs`) → `nbd-client -C N`. Default `N=1` (sem mudança de comportamento).
- **Impacto em ABI/headers:** nenhum. **Module params:** N/A (userspace).
- **Erros (mantém os atuais):** WRITE `len>device` → desconecta a conexão; sem novos códigos.

## Dependências e riscos

- **Pré-requisitos:** nenhum externo novo (std `thread`/`sync_channel`). `nbd-client` suporta `-C`
  (confirmar versão no SPEC).
- **Riscos + mitigação:**
  - *Réplica fora de ordem / interleaving entre conexões* → cada conexão tem seu próprio writer e
    `reply_rx`; o `handle` NBD identifica a réplica. **Mitig.:** teste multi-conexão de integridade.
  - *Coerência multi-conexão (mesma página por 2 conexões)* → o **worker único serializa** todos os
    acessos; não há 2 escritas concorrentes na VRAM. **Mitig.:** invariante "1 worker" + teste.
  - *Deadlock por canal cheio* (worker bloqueado escrevendo réplica enquanto leitor bloqueado
    enfileirando) → réplicas vão para canais **por conexão** com writer próprio; o worker nunca
    bloqueia indefinidamente num socket. **Mitig.:** disciplina #5 worst-case no SPEC.
  - *Perda no disconnect durante DEMOTE* (finding #2a já conhecido) → teardown espera o `swapoff` em
    voo antes de zerar (resolvido junto do item 2/hardening). **Mitig.:** §14.4 ao vivo.
  - *Regressão de latência* → medir p50/p99 vs. serial; abort trigger no SPEC.
- **Breaking changes:** nenhum on-wire. Internamente **substitui** o laço serial (Day-0: reescreve,
  não empilha).
- **Rollout:** app-only (binário). **Rollback:** reverter os commits (sem migração/dados).
- **Hipóteses que exigirão disciplina no SPEC:** modelo de threads/canais (concorrência, #5/#2);
  teardown sob DEMOTE (#5).

## Estratégia de implementação

Ordem das fatias (cada uma compila + testa):
1. Extrair `serve()`-por-request num formato `Job→Reply` puro (sem mudar `serve()`), preparando o
   canal. Validável cedo (unit do empacotamento Job/Reply).
2. Worker CUDA único + canal limitado: mover o I/O de VRAM + canário para a thread worker; `main`
   vira orquestrador. (N=1 ainda.)
3. Leitor + writer desacoplados (1 conexão): remover HOL entre socket e CUDA.
4. Multi-conexão: `accept()` em laço até N; 1 leitor + 1 writer por conexão; flag `--connections`.
5. Teardown: drenar in-flight + esperar DEMOTE + zerar VRAM (§11).
6. Propagar `--connections`/`-C` no CLI `up`.

O que valida cedo: fatias 1-2 (canal + worker) com N=1 já provam não-regressão (`§14` + testes).
O que exige cuidado: fatias 3-5 (concorrência) — disciplina #5/#2.

## Fora de escopo

- **Múltiplos exports/devices** num daemon (só 1 export, N conexões a ele).
- **Vazão paralela de VRAM** (impossível: 1 thread CUDA).
- **async/tokio** (sem ganho; FFI síncrona).
- **Writeback / ublk** (itens 4-5, Fase B).
- **Detecção em idle do canário** (H1 do canário, separado).
