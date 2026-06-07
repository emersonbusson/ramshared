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

## Decisão de dependência

ADR-0004 está **Accepted**: usar `io-uring 0.7.12` (MIT/Apache-2.0) no userspace para evitar
hand-roll de barreiras acquire/release no caminho de swap. A dependência ainda não entra no
`Cargo.toml` neste recorte documental; ela entra no primeiro smoke de ring.

## Sequência segura

1. Adicionar `io-uring 0.7.12` ao daemon em recorte dedicado, com `Cargo.lock` revisado.
2. Smoke mínimo de ring: criar/submeter/completar uma operação local que valide `io_uring_setup`
   no kernel custom, sem ublk device e sem swap.
3. Abrir `/dev/ublk-control`/`/dev/ublkcN` só em smoke ublk explícito, ainda sem `swapon`.
4. Integrar loop ublk com `IoWork`/worker H1; worker CUDA continua único.
5. Bench ublk vs NBD com número p50/p99. Sem ganho medível: manter NBD e remover a dependência.

## Rollback trigger

- `io_uring_setup` retorna `EPERM`/`ENOSYS` no smoke mesmo com `check` ready.
- `grep unsafe` encontra `unsafe` novo em `ramshared-wsl2d`.
- Bench ublk não melhora a latência p99 contra NBD por margem definida no bench.
