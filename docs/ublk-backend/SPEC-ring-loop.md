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
