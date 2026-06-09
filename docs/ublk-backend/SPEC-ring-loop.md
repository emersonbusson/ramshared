# SPEC — ublk FETCH ring loop (no-wait)

> SSDV3 PASSO 2. Fonte: [`PRD.md`](PRD.md) e [`SPECv2.md`](SPECv2.md) (DT-1/DT-3/DT-5).
> Detalha o **loop de ring io_uring** do servidor ublk: threading, layout de `mmap`,
> submissão `FETCH_REQ` sem espera de CQE e teardown/abort. **Kernel-gated**; kernel custom
> ativo `6.6.123.2-microsoft-standard-WSL2+`.
>
> Escopo: fechar o desenho antes de qualquer código que toque `mmap`/MMIO + barreiras do ring
> (categoria SSDV3 obrigatória: locks/concorrência, DMA/MMIO). **Não** cobre `START_DEV` em
> produção nem `swapon` — esses dependem de bench e de PRD próprio.

## 1. Fatos verificados (base factual)

**Driver** `drivers/block/ublk_drv.c` (kernel 6.6.123.2) — *Confirmado no codebase (lido)*:

- **Layout do mmap de io-desc** (`ublk_ch_mmap`, 1395-1429): cada fila tem um buffer de
  comandos mapeável em `offset = q_id * ublk_max_cmd_buf_size()`; o tamanho mapeável de uma
  fila é `round_up(q_depth * sizeof(ublksrv_io_desc), PAGE_SIZE)` (`__ublk_queue_cmd_buf_size`,
  716-719). O kernel **proíbe `VM_WRITE`** (1413-1414 → `-EPERM`): o mapeamento é
  **somente leitura**. Exige `sz == ublk_queue_cmd_buf_size(ub, q_id)` exato (1425).
- **`sizeof(struct ublksrv_io_desc) = 24`**; descritor de uma tag em `&io_cmd_buf[tag * 24]`
  (`ublk_get_iod`, 704-709). Índice no array = `tag`. Espelhado por `ublk::IoDesc`
  (`repr(C)`, `size_of == 24`, asserido em `tests/ublk_uapi.rs`).
- **`FETCH_REQ`** (`__ublk_ch_uring_cmd`, ~1706-1827): valida `q_id < nr_hw_queues` e
  `tag < q_depth`; exige `addr` do buffer da tag salvo `UBLK_F_NEED_GET_DATA`/`UBLK_F_USER_COPY`;
  retorna **`-EIOCBQUEUED`** (1821) — o comando fica **estacionado** no io_uring, sem CQE
  imediato.
- **Readiness**: `ublk_queue_ready` ⇔ `nr_io_ready == q_depth` (1518-1521). `START_DEV`
  (`ublk_ctrl_start_dev`, 2207-2218) espera em `wait_for_completion_interruptible(&ub->completion)`
  (2217). Submeter `FETCH_REQ` para todas as tags **sem** `START_DEV` é válido: os comandos
  ficam estacionados.
- **Teardown/abort**: `ublk_cancel_queue` (1523-1545) chama
  `io_uring_cmd_done(io->cmd, UBLK_IO_RES_ABORT, 0, IO_URING_F_UNLOCKED)` para cada tag `ACTIVE`;
  `UBLK_IO_RES_ABORT = -ENODEV` (-19). `ublk_cancel_dev` (1548-1553) itera as filas; disparado
  por `DEL_DEV`/`STOP_DEV`. ⇒ os `FETCH` estacionados **recebem CQE** no encerramento.

**Crate `io-uring 0.7.12`** — *Confirmado no codebase (lido, registry cache)*:

- `opcode::UringCmd80::new(types::Fd(fd), cmd_op).cmd([u8; 80]).build()` → `Entry128`;
  `.user_data(u64)`.
- `IoUring::<squeue::Entry128>::builder().build(entries)`.
- `submission().push(&entry)` é **`unsafe`** (validade/lifetime do SQE/buffers).
- `submit()` == `submit_and_wait(0)` → **não bloqueia** (não espera CQE);
  `submit_and_wait(n)` bloqueia até `n` CQEs.
- `completion().next()` → `Option`, `None` quando vazio → **drain não-bloqueante**.

**Ops codificadas** (`cc` vs `include/uapi/linux/ublk_cmd.h`) — *Confirmado via cc*:
`UBLK_U_IO_FETCH_REQ = 0xc0107520`, `UBLK_U_IO_COMMIT_AND_FETCH_REQ = 0xc0107521`,
`UBLK_U_IO_NEED_GET_DATA = 0xc0107522`; `sizeof(ublksrv_io_cmd) = 16`. Já modelados em
`ublk.rs` (`UBLK_U_IO_*`, `IoCmd::fetch`, `IoCmd::to_bytes`).

## 2. Threading (DT-3 concretizado)

- **Ring owner thread**: única dona do `IoUring` (submit **e** drain). Nenhuma outra thread
  toca o ring.
- **Worker H1 (CUDA)**: única a tocar o contexto CUDA. Nunca toca o ring.
- **Canais**: `ring → worker` envia `IoWork` (Job); `worker → ring` envia `IoCompletion`
  (Reply, com a `tag`). Espelha exatamente o writer do H1 (worker manda `Reply`, outra thread
  faz o I/O de saída) — resolve a submissão cross-thread.
- **Fluxo de uma tag**: ring owner drena CQE de `FETCH` → lê `io-desc[tag]` no `mmap`
  (read-only) → `IoDesc::to_block_request` → `IoWork::from_desc` → canal pro worker → worker
  processa na VRAM → `IoCompletion` → canal de volta → ring owner submete
  `UBLK_U_IO_COMMIT_AND_FETCH_REQ` (`IoCmd` com `result` + `addr`), re-armando a tag.

## 3. Memória / mmap

- **io-desc (kernel→user)**: `mmap(fd_ublkc, len = round_up(q_depth*24, PAGE_SIZE),
  PROT_READ, MAP_SHARED, offset = q_id * ublk_max_cmd_buf_size())`. **`PROT_READ` obrigatório**
  (write → `-EPERM`). Para `nr_hw_queues = 1, q_depth = 1`: `offset = 0`, `len = 4096`.
- **buffer de dados (por tag)**: alocado em **userspace** (heap), endereço vai no `addr` do
  `FETCH`/`COMMIT`. **Não** é o mmap do io-desc — não confundir. `io_buffer_position`
  (`UBLKSRV_IO_BUF_OFFSET`) é para o caminho zero-copy/`mmap` de dados, fora deste recorte.

## 4. Fronteira `unsafe` e barreiras

- Barreiras `acquire`/`release` do ring ficam na crate `io-uring` auditada (ADR-0004).
- **Todo `unsafe` novo fica em `ramshared-uring`**: `submission().push` (SQE) **+**
  `mmap`/`munmap` (RAII) **+** leitura do io-desc mapeado. Daemon e lib seguem
  `#![forbid(unsafe_code)]`. Cada bloco com `// SAFETY:`.

## 5. Teardown/abort (ordem inversa, idiom `goto out_err`)

1. Parar de submeter novos `FETCH`/`COMMIT`.
2. `DEL_DEV` (control) → `ublk_cancel_dev` chama `io_uring_cmd_done(ABORT = -ENODEV)` em cada
   `FETCH` estacionado, e então o `DEL_DEV` **bloqueia** em `wait_event(idr_freed)`
   (ublk_drv.c:2523) até o char device ser liberado.
3. Ring owner **drena** os CQEs (`res == UBLK_IO_RES_ABORT`); ao soltar o ring e fechar o char,
   a referência do device zera e o `DEL_DEV` retorna.
4. `munmap` → fechar fds (`ublkc`, ring) → liberar buffers de dados. Ordem inversa da alocação.
- **Invariante anti-deadlock (1):** o ring owner **nunca** usa `submit_and_wait` esperando um
  CQE de `FETCH` (que só chega com I/O **ou** abort). Usa `submit()` + drain não-bloqueante.
- **Invariante de concorrência (2) — verificado no M2:** o `DEL_DEV` é **bloqueante** e espera o
  char fechar; o char só fecha quando os `FETCH` (que seguram `fget` do char via io_uring) são
  completados. Logo o `DEL_DEV` **não pode** rodar na mesma thread que ainda precisa drenar o
  ring: exige a thread dona do ring (DT-3) drenando **em paralelo**. Teardown single-thread
  (DEL_DEV bloqueante + drain depois) **deadlocka**.

## 6. Decisões pequenas (sem nova ADR; ADR-0004 cobre a exceção userspace)

- **DT-R1 — `mmap` via `libc`**: usar `libc::mmap`/`munmap` (a crate `libc` já está no
  `Cargo.lock` como transitiva de `io-uring`) encapsulado num wrapper RAII em
  `ramshared-uring`, em vez de adicionar `memmap2`. *Counterfactual*: se a auditoria exigir
  abstração mais forte, trocar por `memmap2` (nova dep) é localizado.
  Ver [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) #2 (counterfactual).
- **DT-R2 — submit não-bloqueante**: o ring owner faz `submit()` + drain; nunca
  `submit_and_wait` sobre `FETCH`. Ver KAHNEMAN #5 (worst-case): `-EIOCBQUEUED` deadlock.

## 7. Validações (antes de cada submissão)

- `q_id < nr_hw_queues`; `tag < q_depth`; `addr != 0` (não usamos `NEED_GET_DATA`/`USER_COPY`
  neste recorte).
- `mmap`: `len` exatamente `round_up(q_depth*24, PAGE_SIZE)`; `PROT_READ`; checar
  `ptr != MAP_FAILED`.

## 8. Marcos de IMPL (recortes TDD; cada um RED→GREEN→docs)

- **M1 — `mmap` read-only do io-desc** (smoke root): `ADD_DEV` → `mmap(PROT_READ)` da fila 0 →
  ler `io-desc[0]` (zerado, sem I/O) → `munmap` → `DEL_DEV`. Sem `START_DEV`, sem `/dev/ublkbN`,
  sem `swapon`. Introduz o wrapper `mmap`/`munmap` RAII em `ramshared-uring`. *(RNF: `grep
  unsafe` confinado a `ramshared-uring`.)*
- **M2 — `FETCH_REQ` sem esperar CQE** (smoke root) — **feito**: `ADD_DEV` → submeter
  `FETCH_REQ` para todas as tags (`q_depth`) com `addr` = buffer alocado → `submit()` (não
  bloqueia) → confirmar **nenhum CQE imediato** → drenar os aborts numa **thread dona do ring**
  enquanto o `DEL_DEV` roda → cada CQE `res == UBLK_IO_RES_ABORT (-ENODEV)` → cleanup. Sem
  `START_DEV`, sem `swapon`. (`mmap` do io-desc não é exigido para o FETCH — fica no M1.)
- **M3 — `START_DEV` + loop ring↔worker H1** *(gated)*: só após M1/M2, com PRD/bench de
  latência ublk vs NBD provando ganho. **Fora deste SPEC de prep.**

## 9. Critérios de aceitação

- **M1**: teste root cria/mapeia/lê/`munmap`/`DEL_DEV` sem resíduo em `/dev` (antes == depois,
  só `ublk-control`); `cargo test`/`clippy -D warnings` verdes; `grep unsafe` só em
  `ramshared-uring`.
- **M2**: teste root submete `FETCH` para todas as tags, confirma estacionamento (sem CQE
  imediato), `DEL_DEV` entrega CQEs `ABORT`, cleanup completo; `/dev` limpo; sem `swap`.

## 10. Rollback triggers

- `mmap` exige `PROT_WRITE` (→ `-EPERM`): uso errado, deve ser read-only.
- Qualquer `submit_and_wait` sobre `FETCH` trava o teste (`-EIOCBQUEUED`) → voltar a
  `submit()` + drain.
- `grep unsafe` acha `unsafe` fora de `ramshared-uring`.
- Teardown não entrega CQEs `ABORT` (loop travaria) → revisar ordem (`DEL_DEV` antes do drain).

## 11. Fluxo de serviço de I/O (M3) — fatos verificados

*Confirmado lendo `ublk_drv.c` (2026-06-07).* Modo plano (sem `USER_COPY`/`NEED_GET_DATA`).

- **START_DEV** (`ublk_ctrl_start_dev`, 2207): exige `ublksrv_pid>0` (`data[0]`); **exige
  SET_PARAMS** (`ublk_apply_params` → `-EINVAL` sem BASIC; `set_capacity(dev_sectors)` em 553);
  espera `ub->completion` (todas as filas ready = FETCH submetido); então `add_disk` cria
  `/dev/ublkbN` (state `LIVE` em 2250).
- **Partition scan obrigatório com daemon privilegiado:** se `nr_privileged_daemon ==
  nr_queues_ready` (todos com `CAP_SYS_ADMIN` — root, nosso caso), `GD_SUPPRESS_PART_SCAN` **não**
  é setado (2246-2247) e `add_disk` lê o **setor 0**. Logo a **thread servidora deve estar
  drenando/servindo durante o START_DEV**: `add_disk` bloqueia até esse read ser servido.
  Single-thread deadlocka (mesmo padrão do teardown M2).
- **Notificação:** ao chegar um request, `ublk_setup_iod` (1000) preenche `io-desc[tag]`
  (op_flags/nr_sectors/start_sector/addr) e o FETCH completa com `res = UBLK_IO_RES_OK (0)`
  (1244); o **tag vai no `user_data`** da SQE (responsabilidade do servidor).
- **WRITE (host→device):** kernel copia bio→`addr` **antes** do CQE (`ublk_map_io`, dispatch). O
  servidor lê o próprio buffer e grava no backend.
- **READ (host←device):** o servidor preenche `addr`; no COMMIT o kernel copia `addr`→bio
  **exatamente `result` bytes** (`ublk_unmap_io` copia `io->res`, 965). **`result` load-bearing:**
  READ com `result=0` é forçado a `-EIO` (1068). Sucesso ⇒ `result = nr_sectors*512`.
- **`result` no COMMIT:** `>=0` = bytes transferidos (sucesso), `<0` = `-errno`. FLUSH/DISCARD não
  copiam; `result>=0` basta.
- **COMMIT_AND_FETCH_REQ:** um ioctl **completa o tag e re-arma o FETCH**; re-fornecer `addr`
  não-NULL a cada round é obrigatório (1792-1799). É a op de regime do loop.
- **Teardown de device LIVE:** `STOP_DEV` (`del_gendisk`→DEAD→cancel) e depois `DEL_DEV`; ou
  `DEL_DEV` direto (`ublk_remove` chama `ublk_stop_dev`, 2183). Como no M2, exige a thread
  servidora ativa durante o controle.

### Sub-marcos do M3 (gated por bench)
- **M3a — feito:** SET_PARAMS/GET_PARAMS.
- **M3b — feito:** `UblkServer` (ring + mmap io-desc + buffers) + thread servidora; backend de
  **RAM (`Vec<u8>`)**; `START_DEV` + I/O READ/WRITE no `/dev/ublkbN` + `STOP`/`DEL_DEV`. Sem `swapon`.
- **M3c:** ligar ao `VramBackend`/worker H1; bench ublk vs NBD; só então `swapon`.

## 12. Plano do M3c (ligar VRAM + bench + swap) — gated

Pré-requisito fechado: o loop serve READ/WRITE/FLUSH com um backend trocável (`serve_request`
despacha; o backend faz o storage). Passos:

1. **Trait de backend:** extrair `BlockBackend { read_into, write_from }` de `RamBackend` e tornar
   o loop/`spawn_server` genéricos. `RamBackend` e o futuro adapter de VRAM o implementam.
   Refactor seguro e testável (sem device).
2. **Adapter VRAM:** investigar a API do `VramBackend`/worker H1 (`ramshared-cuda`/`ramshared-block`)
   e adaptá-la a `BlockBackend` (offset/len ↔ `Request`). O worker CUDA continua a **única** thread
   a tocar o ctx (DT-3): o loop ublk **não** chama CUDA direto — envia `IoWork` ao worker e recebe
   `IoCompletion` (canais já modelados em `ublk.rs`). A thread do ring drena; o worker serve.
3. **Smoke I/O contra VRAM (sem swap):** read/write no `/dev/ublkbN` servido pela VRAM, conferindo
   dados — antes de qualquer `swapon`.
4. **Bench ublk vs NBD:** p50/p99 de latência sob a mesma carga, nos dois transportes, no kernel
   custom. Critério (anti-halo #11): ublk só é adotado se latência < NBD por ≥ X%.
5. **`swapon` (passo final, separado):** só com ganho provado; `--transport ublk` deixa de ser
   gated. DEMOTE segue `swapoff <swap_dev>` (a VRAM continua em swap; SPECv2 DT-6).

Rollback: sem ganho no bench → manter NBD e remover a dependência `io-uring`/`ramshared-uring`.

### Design DT-3 (decisões verificadas, fechado para IMPL)

*Base: leitura de `backend.rs`/`conn.rs`/`request.rs`/`driver.rs` (2026-06-07).*

- **Reuso fechado:** o trait `ramshared_block::BlockBackend` (`size_bytes`/`block_size`/`read_at`/
  `write_at`/`flush`) já existe e o `VramBackend` o implementa (`backend.rs:24`). `serve_request`
  já é genérico sobre ele — o loop ublk serve VRAM sem mudança. **Não** criar backend novo.
- **Quem toca CUDA:** `DeviceMem::read_at`/`write_at` fazem `cuMemcpyDtoH/HtoD` **síncronos**
  (`driver.rs:248`/`263`) na thread que chama. O `DeviceMem<'c,'a>` **borrows** `Context` e **não é
  `Send`**. Logo o **worker** deve **criar e possuir** o stack `Cuda`+`Context`+`DeviceMem`+
  `VramBackend` **dentro da própria thread** — `spawn_ublk_worker` recebe parâmetros (tamanho,
  block size), não o backend pronto. (Espelha `main.rs::run()`, que é o próprio worker H1.)
- **Canais (mpsc):** ring→worker `SyncSender<IoWork>` (bounded `CHAN_CAP`, backpressure); worker→
  ring `Sender<WorkerReply>` (unbounded, DT-7, o worker nunca bloqueia). Canais **únicos** (um ring
  owner), diferente do NBD (um reply channel por conexão).
- **Gap do READ resolvido:** `IoCompletion` só tem `result` (`ublk.rs:204`); os dados do READ
  precisam chegar ao buffer da tag. **Decisão:** novo `WorkerReply { qid, tag, result,
  read_data: Vec<u8> }` no canal worker→ring; o **ring owner** copia `read_data` no
  `server.buffer_mut(tag)` antes do `commit_and_fetch`. Mantém o worker como única thread CUDA.
- **Buffers cross-thread:** nunca passar o ponteiro cru de `UblkServer.buffers` ao worker; o WRITE
  vai como `IoWork.payload: Vec<u8>` (owned, já é a forma de `from_desc`). O ring owner copia o
  payload do buffer da tag para o `IoWork` no envio.
- **Loop atual (M3b) ≠ alvo:** `run_server_loop` single-thread (serve inline) valida a **mecânica
  do ring** sem CUDA; é correto para RAM mas **não** é DT-3. O M3c separa ring owner / worker.
- **Entry point:** o worker H1 está inlined em `main.rs::run()` (NBD). Criar um
  `spawn_ublk_worker` dedicado (loop `Receiver<IoWork>` → `serve_request` → `Sender<WorkerReply>`)
  — `serve_request`/`VramBackend` reusados verbatim; só o wrapper de loop é novo.

### Status DT-3 (IMPL com RAM — falta só VRAM)

- **Feito (sem GPU):** `spawn_ublk_worker` (worker) + `spawn_server_dt3` (ring owner) +
  `WorkerReply` em `ublk_server.rs`; `serve_request` unificado em `Request`. Smoke DT-3 root passa
  (READ via block device, teardown sem deadlock) + worker testado puro. A **arquitetura alvo está
  validada com RamBackend** — ring owner e worker em threads separadas, do jeito que o VRAM exige.
- **Feito (GPU):** `spawn_server_dt3_vram` — o worker cria o stack `Cuda`/`Context`/`DeviceMem`/
  `VramBackend` **na própria thread** (resolve o `!Send`/`!'static` do `DeviceMem<'c,'a>`) e roda o
  loop ali. Smoke root+GPU (RTX 2060): WRITE→`cuMemcpyHtoD`, `drop_caches`, READ→`cuMemcpyDtoH`
  confere. **O ublk serve a VRAM end-to-end.**
- **Swap validado (capstone):** `mkswap`/`swapon`/`/proc/swaps`/`swapoff` sobre o `/dev/ublkbN`
  VRAM — a VRAM funciona como **área de swap via ublk** (ciclo limitado, sem pressão; 9.6 GiB RAM
  livre; `swapon` sem `-p`; `SwapGuard`). É o objetivo central da Fase B.
- **Bench (gate anti-halo #11) — passado:** `fio` 4KB randread `O_DIRECT` iodepth=1 nos dois
  transportes servindo VRAM: ublk p50=241µs/IOPS=3911 vs NBD p50=326µs/IOPS=2900 → **ublk ~26%
  mais rápido**. Adoção do ublk justificada por bench.
- **No-alloc DT-8 — feito:** ring owner mantém pool de buffers pré-aquecido (`queue_depth` ×
  `buf_size`), ciclado ring owner↔worker (`dispatch` pega/dimensiona → worker serve in-place →
  `commit` copia READ e recicla). Zero malloc/free no hot path em regime (`pool.len()+in_flight
  ==queue_depth`, pool nunca esvazia). Remove o hazard de alocar no caminho de I/O sob pressão de
  swap. `WorkerReply`: `read_data` → `buf`+`is_read`. `mlockall` é do daemon (`main.rs`).
- **queue_depth > 1 — validado:** device com `queue_depth=4` (fila única) + 4 leitores `O_DIRECT`
  concorrentes mantêm múltiplos tags em voo; integridade por bloco confere o pool no-alloc com
  `in_flight > 1` e o endereçamento por-tag (`self.buffers[tag]` no FETCH/COMMIT). Sem corrupção
  nem deadlock (RTX 2060). Só servimos a fila 0; multi-`nr_hw_queues` fica fora de escopo (exigiria
  um ring/char-region por fila — novo SPEC).
- **Fase B completa:** VRAM + swap + bench (ublk vence NBD) + no-alloc + qd>1, validado em hardware.
