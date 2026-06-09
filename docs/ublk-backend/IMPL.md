# IMPL — Fase B ublk (prep seguro)

Status: **prep em andamento** (2026-06-07). Kernel custom ativo:
`6.6.123.2-microsoft-standard-WSL2+`, `CONFIG_BLK_DEV_UBLK=m`,
`CONFIG_IO_URING=y`, `kernel.io_uring_disabled=0`, `/dev/ublk-control` presente.

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
3. **SSDV3-gated:** o loop ublk exige `mmap` de `/dev/ublkcN` (nova superfície `unsafe` em
   `ramshared-uring`) e ring persistente que submete `FETCH_REQ` **sem** esperar CQE (o driver
   deixa o comando pendente em `-EIOCBQUEUED` até I/O ou abort). Por tocar `mmap`/MMIO +
   barreiras/threading do ring, fechar SPECv2/IMPL antes do código. Thread io_uring continua dona
   única do ring (DT-3); worker CUDA único; integrar com `IoWork`/worker H1 antes de `START_DEV`.
4. Bench ublk vs NBD com número p50/p99. Sem ganho medível: manter NBD e remover a dependência.

## Rollback trigger

- `io_uring_setup` retorna `EPERM`/`ENOSYS` no smoke mesmo com `check` ready.
- `grep unsafe` encontra `unsafe` novo em `ramshared-wsl2d`.
- Smoke `ADD_DEV`/`DEL_DEV` deixa `/dev/ublkcN` persistente ou cria `/dev/ublkbN` antes de
  `START_DEV`.
- Bench ublk não melhora a latência p99 contra NBD por margem definida no bench.
