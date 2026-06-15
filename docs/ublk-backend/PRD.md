---
slug: ublk-backend
title: ublk no lugar do NBD para o tier VRAM (Fase B)
milestone: —
issues: [3]
---
# PRD — Fase B — ublk no lugar do NBD para o tier VRAM

> **Status: IMPL prep (Fase B, kernel custom ativo em 2026-06-07).** O kernel WSL2 custom
> `6.6.123.2-microsoft-standard-WSL2+` tem `CONFIG_BLK_DEV_UBLK=m`, `io_uring_disabled=0` e
> `/dev/ublk-control`. A tensão de política foi fechada pela ADR-0004: usar `io-uring 0.7.12`
> no userspace, gated em bench ublk vs NBD.

## Resumo

A VRAM é exposta como block device via **NBD** (socket Unix + protocolo on-wire; o
`ramshared-wsl2d` serve cada request com round-trips de socket). O **ublk** (`ublk_drv` +
servidor userspace sobre **io_uring**) elimina o round-trip de socket e cópias: o kernel e o
servidor compartilham buffers via io_uring, com menos context-switches. Objetivo: **latência
menor** no caminho quente do tier VRAM (Confirmado em docs: ROADMAP Fase B; LIBRARIES.md "ublk:
latência menor (io_uring), sem round-trip socket").

## Contexto técnico

- **Confirmado no codebase:**
  - Transporte atual = NBD: `crates/ramshared-block` (protocolo) + `crates/ramshared-wsl2d`
    (daemon, worker CUDA único H1). `LIBRARIES.md`: "Block backend: ublk (Fase B) — exige
    kernel custom; só após Fase B".
  - O daemon é **`#![forbid(unsafe_code)]`**; o único `unsafe` do projeto vive isolado em
    `ramshared-cuda` (FFI da libcuda). **Zero deps externas** (Cargo.lock confirma).
- **Confirmado na documentação oficial (kernel `Documentation/block/ublk.rst`):**
  - `ublk_drv` cria `/dev/ublkbN`; o servidor userspace usa **io_uring** + a uAPI ublk
    (`UBLK_IO_FETCH_REQ`/`COMMIT_AND_FETCH`) para receber/completar I/O. Buffers compartilhados.
  - Requer `CONFIG_BLK_DEV_UBLK`. Implementação de referência: `ubdsrv` (C, liburing).
- **Proposto / Inferência:**
  - Servir o tier VRAM via ublk: o **worker CUDA único** (H1) processa os requests vindos do
    io_uring em vez do socket NBD. **Inferência:** o reader/acceptor do H1 (sockets) é
    substituído por um loop io_uring; o worker CUDA e o canário §9/§9.4 permanecem.

## Opção recomendada

**Servidor ublk reusando o worker CUDA único do H1; transporte NBD→io_uring; crate `io-uring`
auditada encapsulada pelo wrapper `ramshared-uring`.**

- O daemon ganha um modo ublk: loop io_uring (submit/complete) alimenta o **mesmo** canal
  `WMsg`/worker do H1; o worker serve a VRAM e completa via io_uring.
- **Tensão de política resolvida:** io_uring hand-rolled foi rejeitado no SPECv2/ADR-0004 porque
  barreiras erradas no ring compartilhado são risco de corrupção no caminho de swap. A exceção
  userspace é a crate externa `io-uring 0.7.12`, isolada em `ramshared-uring`; o daemon e a lib
  continuam sem `unsafe` próprio.
- **Alternativas descartadas:**
  - **`ramshared-uring` hand-rolled** — preserva zero-dep, mas põe barreiras acquire/release sob
    `unsafe` próprio no caminho de swap.
  - **manter NBD (Day-0)** — funciona (validado §14/H1); ublk é otimização Fase B, não correção.
  - **NBD + ublk dual-path permanente** — viola Day-0 (dois transportes); ublk **substitui** o
    NBD quando maduro (ou fica atrás de feature-flag de build até a paridade).
- **Trade-offs:** ganho de latência (Inferência — medir) ao custo de uma área `unsafe` nova
  (io_uring FFI) e complexidade da uAPI ublk; depende de kernel custom.

## Requisitos funcionais

- **RF-1 — Device ublk servido pela VRAM.** O daemon cria/serve `/dev/ublkbN` respaldado pela
  VRAM. *Aceite:* `mkswap`/`swapon` do ublk device; round-trip íntegro. **(kernel-gated)**
- **RF-2 — Worker CUDA único preservado.** O loop io_uring alimenta o worker H1 (afinidade
  CUDA); a VRAM segue serializada por 1 thread. *Aceite:* sem corrupção; 0 `unsafe` no worker.
- **RF-3 — Canário/DEMOTE preservados.** §9 (latência serve-only) e §9.4 + DEMOTE funcionam
  sobre ublk. *Aceite:* §14 adaptado verde. **(kernel-gated)**
- **RF-4 — Isolamento do `unsafe`.** A FFI io_uring fica num crate dedicado; daemon/lib seguem
  `forbid(unsafe_code)`. *Aceite:* `grep unsafe` só no crate novo + `ramshared-cuda`.

## Requisitos não-funcionais

- **Performance:** latência < NBD (Inferência — medir p50/p99 no kernel).
- **Segurança:** sem `unsafe` novo no daemon; `unsafe` de ring fica em `ramshared-uring` e na
  crate externa `io-uring`.
- **Resiliência:** falha de io_uring → DEMOTE (como hoje); worst-case #5 no SPEC.

## Fluxos

**Happy path (Fase B):** daemon modo ublk → io_uring fetch req → enfileira no worker → CUDA
serve → completa via io_uring. Canário/DEMOTE iguais ao H1. **Erro:** io_uring/CUDA falha →
DEMOTE; device removal → teardown zera a VRAM.

## Modelo de dados

- `ramshared-uring`: wrappers finos sobre `io-uring`; qualquer `unsafe` de SQE/lifetime fica aqui.
- Módulo ublk do daemon: uAPI ublk espelhada em `crates/ramshared-wsl2d/src/ublk.rs`.

## API / Interfaces

- **Kconfig:** `CONFIG_BLK_DEV_UBLK=y` (kernel custom). **CLI:** `up --transport ublk` (default
  nbd até paridade). Sem ioctl próprio (usa a uAPI ublk do kernel).

## Dependências e riscos

- **Pré-requisito duro:** kernel com `CONFIG_BLK_DEV_UBLK` + `ublk_drv`; atendido no kernel custom
  `6.6.123.2-microsoft-standard-WSL2+`, mas o caminho ublk real ainda depende de smoke/bench.
- **Riscos:** (a) **io_uring + afinidade CUDA** — o thread que faz `io_uring_enter` vs o worker
  CUDA: a completion não pode rodar CUDA fora da thread do ctx (worst-case #5, fechar no SPEC);
  (b) **supply chain** da crate externa — mitigado por lockfile/audit/rollback; (c) complexidade
  da uAPI ublk (FETCH/COMMIT) — validar contra `ublk.rst`/driver.
- **Rollout/rollback:** app-only + `--transport` (default nbd). ublk substitui NBD só na paridade.

## Estratégia de implementação (quando houver kernel)

1. ADR-0004 + LIBRARIES.md: aceitos para `ramshared-uring` + `io-uring 0.7.12`. 2. Smoke mínimo de ring sem swap.
3. Daemon modo ublk reusando worker H1. 4. `--transport ublk` remove o gate só após paridade.
5. §14 adaptado + bench de latência vs NBD.

## Fora de escopo

- IMPL/validação agora (kernel-gated). Trocar o modelo de worker único. Manter NBD+ublk dual
  permanente (Day-0). zram-writeback (item 4, separado, mas combinável depois).
