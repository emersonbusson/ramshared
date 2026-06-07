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

## Decisão de dependência

ADR-0004 está **Accepted**: usar `io-uring 0.7.12` (MIT/Apache-2.0) no userspace para evitar
hand-roll de barreiras acquire/release no caminho de swap. A dependência entrou em
`crates/ramshared-uring/Cargo.toml` no recorte de smoke mínimo do ring; `Cargo.lock` registra
`io-uring 0.7.12`, `libc 0.2.186`, `bitflags 2.13.0` e `cfg-if 1.0.4`.

## Sequência segura

1. **Feito:** adicionar `ramshared-uring` + `io-uring 0.7.12` e rodar smoke mínimo de ring sem
   ublk device e sem swap (`io_uring_setup` + `io_uring_enter` sem SQEs).
2. Abrir `/dev/ublk-control`/`/dev/ublkcN` só em smoke ublk explícito, ainda sem `swapon`.
3. Integrar loop ublk com `IoWork`/worker H1; worker CUDA continua único.
4. Bench ublk vs NBD com número p50/p99. Sem ganho medível: manter NBD e remover a dependência.

## Rollback trigger

- `io_uring_setup` retorna `EPERM`/`ENOSYS` no smoke mesmo com `check` ready.
- `grep unsafe` encontra `unsafe` novo em `ramshared-wsl2d`.
- Bench ublk não melhora a latência p99 contra NBD por margem definida no bench.
