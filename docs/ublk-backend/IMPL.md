# IMPL — Fase B ublk (prep seguro)

Status: **VRAM-as-RAM via swap por ublk — completo e validado** (2026-06-09). Kernel custom ativo:
`6.6.123.2-microsoft-standard-WSL2+`, `CONFIG_BLK_DEV_UBLK=m`,
`CONFIG_IO_URING=y`, `kernel.io_uring_disabled=0`, `/dev/ublk-control` presente.
`/dev/ublkbN` serve READ/WRITE/FLUSH a partir da **VRAM** (RTX 2060, `cuMemcpy`) via loop DT-3
(ring owner + worker dono do ctx CUDA), foi **validado como área de swap** (`mkswap`/`swapon`/
`swapoff`, `/proc/swaps`), e o **bench fio mostra ublk ~26% mais rápido que NBD** (p50 241µs vs
326µs; gate anti-halo #11 satisfeito). O **no-alloc do worker** (DT-8) foi implementado — pool de
buffers ciclado ring owner↔worker, zero malloc/free no hot path. **Fase B funcionalmente completa
e pronta para PR.**

## Fechado sem tocar swap

- `ramshared check` valida DT-5 completo: Kconfig ublk, io_uring runtime e abertura de
  `/dev/ublk-control` como root.
- CLI aceita `--transport {nbd,ublk}` e `--swap-dev`, mas `ublk` real segue gated antes de efeitos
  colaterais.
- `crates/ramshared-wsl2d/src/ublk.rs` espelha UAPI, layouts `repr(C)`, helpers de posição,
  `IoDesc -> Request`, `IoWork` e `IoCompletion`. Tudo sem `unsafe`, sem FD, sem `io_uring`.
- `crates/ramshared-uring` encapsula a crate externa `io-uring`; o daemon continua
  `#![forbid(unsafe_code)]`.
- `ublk_control::get_features` consulta `/dev/ublk-control` via `UringCmd80`/SQE 128 sem criar
  device. O smoke root confirmou `UBLK_F_CMD_IOCTL_ENCODE` presente e zero-copy ausente.
- `ublk_control::add_device` + `delete_device` cobrem `ADD_DEV`/`DEL_DEV` com `dev_id` automático.
  O smoke root cria e remove somente `/dev/ublkcN`; `START_DEV` ainda não foi chamado e
  `/dev/ublkbN` não aparece.
- `ublk.rs` modela as ops de IO **codificadas** (`UBLK_U_IO_FETCH_REQ`/`COMMIT_AND_FETCH_REQ`/
  `NEED_GET_DATA`) e `IoCmd::fetch`/`IoCmd::to_bytes` (layout `ublksrv_io_cmd` de 16 B). Encoding
  puro, pronto para a SQE; sem ring, sem `mmap`, sem `unsafe`.
- **M1:** `ublk_queue::read_io_desc` mapeia o buffer de io-desc de `/dev/ublkcN` **read-only**
  (`MmapRo` RAII + `page_size`/`round_up_to_page` em `ramshared-uring`) e decodifica por tag
  (`IoDesc::from_ne_bytes`). Smoke root: `mmap` da fila 0 sem `START_DEV`, io-desc zerado, `/dev`
  intacto. Todo `unsafe` novo fica em `ramshared-uring`; o daemon-lib segue `#![forbid(unsafe_code)]`.
- **M2:** `UblkFetchRing` (em `ramshared-uring`) submete `FETCH_REQ` para as tags da fila 0 sem
  esperar CQE (`submit()`/want=0; `unsafe push` isolado; `fetch_cmd80` monta o `ublksrv_io_cmd`).
  `ublk_queue::FetchSession` segura char device + ring. Smoke root: FETCH estacionado (drain
  vazio); `DEL_DEV` aborta (`-ENODEV`) com o ring drenado numa **thread dona do ring** (DT-3) —
  necessário porque o `DEL_DEV` bloqueia esperando o char fechar. `/dev` intacto, sem `START_DEV`.
- **M3c DT-3 (sem GPU) — arquitetura completa:** `spawn_ublk_worker` (thread dona do backend, a
  única a tocar VRAM/CUDA; serve via `serve_request` unificado em `Request`, reusando
  `BlockBackend`; devolve `WorkerReply{result, read_data}`) + `spawn_server_dt3` (ring owner:
  drena CQE → `IoWork` → worker → `WorkerReply` → copia `read_data` na tag → `COMMIT_AND_FETCH`).
  Validado com `RamBackend`: worker puro (sem root) + smoke DT-3 root (READ via block device,
  teardown **sem deadlock**); 3/3 smokes I/O.
- **M3c VRAM — feito (GPU):** `spawn_server_dt3_vram` — o worker cria o stack
  `Cuda`/`Context`/`DeviceMem`/`VramBackend` **na própria thread** (resolve o `!Send`/`!'static`
  do `DeviceMem<'c,'a>`) e roda o loop ali. Smoke root+GPU (RTX 2060): WRITE→`cuMemcpyHtoD`,
  `drop_caches`, READ→`cuMemcpyDtoH` confere o bloco. 4/4 smokes I/O.
- **Swap — validado (capstone):** `vram_ublk_round_trips_as_swap_device` faz `mkswap`→`swapon`→
  confere `/proc/swaps`→`swapoff` sobre o `/dev/ublkbN` servido pela VRAM (ciclo limitado, sem
  pressão; 9.6 GiB RAM livre; `SwapGuard` com `swapoff` no teardown). A **VRAM funciona como swap
  via ublk** — o objetivo central da Fase B. 5/5 smokes I/O, `/proc/swaps` antes==depois.
- **Bench + perf:** `bench_vram_ublk_read_latency` mede 4KB `O_DIRECT` no ublk-VRAM. O ring owner
  passou a **bloquear** (CQE/reply) em vez de poll de 200µs → p50 **628µs → 231µs** (2.7×, RTX
  2060); o residual é o custo do DT-3 (2 saltos de thread) + WSL2.
- **Bench fio ublk vs NBD (gate anti-halo #11):** mesmo `fio` (4KB randread `O_DIRECT` iodepth=1)
  nos dois transportes servindo VRAM. **ublk** p50=241µs p99=461µs IOPS=3911 vs **NBD** p50=326µs
  p99=635µs IOPS=2900 → **ublk ~26% mais rápido, ~35% mais IOPS**. Gate satisfeito.
- **No-alloc DT-8 (feito):** o ring owner mantém um **pool de buffers pré-aquecido** (`queue_depth`
  buffers de `buf_size`); `dispatch_request` pega um e dimensiona a `len`, o worker serve in-place
  e o devolve em `WorkerReply.buf`, `commit_reply` copia (READ) e recicla (clear preserva
  capacidade). Em regime: zero malloc/free no hot path (invariante `pool.len()+in_flight==qd`,
  pool nunca esvazia). Remove o hazard de deadlock de alocar no caminho de I/O sob pressão de swap.
  `WorkerReply` passou de `read_data` para `buf`+`is_read`. Latência inalterada (p50 ~250µs);
  validado pelos smokes DT-3 RAM/VRAM/swap + worker unit. `mlockall` é do daemon (`main.rs`).
- **SET_PARAMS** (pré-requisito do `START_DEV`): `ublk_control::set_params`/`get_params`
  (control-only) aplicam/leem `ublk_params` (112 B); `Params::basic_disk`/`to_bytes`/`from_bytes`
  espelham o layout (offsets via `cc`). Smoke root: round-trip de `dev_sectors`/bs-shifts sem
  `START_DEV`.
- **M3b — ublk funcional:** `UblkServer` (ring + mmap io-desc + buffers por tag) + `spawn_server`
  rodam o loop numa **thread dona do ring** (DT-3); `RamBackend`/`serve_request` servem
  READ/WRITE/FLUSH; `start_dev`/`stop_dev` no control. Smokes root: `START_DEV` cria `/dev/ublkbN`;
  **READ** (setor pré-gravado no backend volta correto) e **WRITE** (escrita via block device +
  `fsync` chega ao backend, conferido pelo backend devolvido no `join`) end-to-end; `STOP`+`DEL_DEV`
  limpam. **Sem swap.** A thread servidora é obrigatória durante START/STOP_DEV (servem o partition
  scan / os aborts).

## Decisão de dependência

ADR-0004 está **Accepted**: usar `io-uring 0.7.12` (MIT/Apache-2.0) no userspace para evitar
hand-roll de barreiras acquire/release no caminho de swap. A dependência entrou em
`crates/ramshared-uring/Cargo.toml` no recorte de smoke mínimo do ring; `Cargo.lock` registra
`io-uring 0.7.12`, `libc 0.2.186`, `bitflags 2.13.0` e `cfg-if 1.0.4`.

## Sequência segura

1. **Feito:** adicionar `ramshared-uring` + `io-uring 0.7.12` e rodar smoke mínimo de ring sem
   ublk device e sem swap (`io_uring_setup` + `io_uring_enter` sem SQEs).
2. **Feito:** consultar `/dev/ublk-control` (`GET_FEATURES`) e exercitar `ADD_DEV` + `DEL_DEV`
   em smoke explícito. Limite validado: `/dev/ublkcN` temporário, sem `/dev/ublkbN`, sem
   `START_DEV`, sem `swapon`.
3. **SSDV3 — SPEC fechado:** o loop ublk exige `mmap` read-only de `/dev/ublkcN` (nova superfície
   `unsafe` em `ramshared-uring`) e ring que submete `FETCH_REQ` **sem** esperar CQE (driver para
   em `-EIOCBQUEUED` até I/O ou abort). Desenho verificado em
   [`SPEC-ring-loop.md`](SPEC-ring-loop.md): threading DT-3, layout de `mmap`, barreiras na crate
   `io-uring`, teardown/abort (`UBLK_IO_RES_ABORT = -ENODEV`). **M1** (`mmap`), **M2** (`FETCH`
   no-wait), **M3a** (SET_PARAMS) e **M3b** (`START_DEV` + loop servidor + I/O com backend de RAM)
   **feitos** — `/dev/ublkbN` serve READ/WRITE end-to-end. Próximo **M3c**: ligar ao
   `VramBackend`/worker H1 + bench ublk vs NBD; só então `swapon`.
4. Bench ublk vs NBD com número p50/p99. Sem ganho medível: manter NBD e remover a dependência.

## Rollback trigger

- `io_uring_setup` retorna `EPERM`/`ENOSYS` no smoke mesmo com `check` ready.
- `grep unsafe` encontra `unsafe` novo em `ramshared-wsl2d`.
- Smoke `ADD_DEV`/`DEL_DEV` deixa `/dev/ublkcN` persistente ou cria `/dev/ublkbN` antes de
  `START_DEV`.
- Bench ublk não melhora a latência p99 contra NBD por margem definida no bench.
