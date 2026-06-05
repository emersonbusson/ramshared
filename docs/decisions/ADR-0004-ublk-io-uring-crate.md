# ADR-0004 — ublk (Fase B): usar a crate `io-uring` auditada, não FFI io_uring hand-rolled

**Status:** Proposed (2026-06-05). Requer ratificação do dono (reverte a política zero-dep
**só para io_uring**; ver Consequences). Roteado pelo `docs/ublk-backend/SPECv2.md` DT-1.

## Context

A Fase B propõe trocar o transporte do tier VRAM de **NBD** (socket) por **ublk** (servidor
userspace sobre **io_uring**), por latência menor (sem round-trip de socket). O servidor ublk é
**userspace** (o `ramshared-wsl2d`); o destino Ring-0 bare-metal é um **módulo de kernel separado
e futuro**, que não usa crate userspace nenhuma.

Forças (fatos):
- **Política atual:** zero dependências externas (Cargo.lock confirma); `LIBRARIES.md`: "um LKM
  ideal tem zero deps". O único `unsafe` do projeto vive isolado em `ramshared-cuda` (FFI `dlopen`
  + chamadas **síncronas** de função — sem ordering de memória).
- **io_uring exige barreiras de memória.** As filas SQ/CQ são **memória compartilhada com o
  kernel**; o produtor/consumidor exige `acquire`/`release` corretos nos índices head/tail
  (`smp_load_acquire`/`smp_store_release`). Hand-rollar isso em `unsafe` é uma classe de bug de
  concorrência de baixo nível **difícil de acertar** e **catastrófica num caminho de swap**
  (corrupção silenciosa / panic). É **qualitativamente** mais perigoso que o FFI do CUDA
  (auditoria do SPECv2, M5-1).
- **Precedente clap (issue #3):** clap foi **rejeitado** para preservar zero-dep — mas clap era
  **trivialmente evitável** (~4-9 flags, `std::env::args` basta). io_uring **não** é evitável com
  segurança: mesmo via syscalls cruas (`io_uring_setup`/`enter`), as barreiras das rings continuam
  obrigatórias. A decisão clap **não** generaliza para cá (custo de evitar ≠ trivial).

## Decision

Para o servidor ublk **userspace**, usar a **crate `io-uring` auditada** (madura, amplamente
usada, barreiras corretas) em vez de hand-rollar a FFI num crate `ramshared-uring`. Encapsular o
uso num módulo fino do daemon; o resto do daemon e a lib seguem `#![forbid(unsafe_code)]`.

Critério mensurável (anti-halo #11) para a **adoção do ublk em si** permanece gated em bench:
**latência ublk < NBD por ≥ X%** num kernel custom; sem ganho → **manter NBD** (a crate só entra
se o ublk entrar). Registrar a exceção em `LIBRARIES.md` quando o ublk for adotado.

## Consequences

**+** Correção das barreiras de io_uring fica numa lib auditada (não em `unsafe` hand-rolled no
caminho de swap). Menos superfície de `unsafe` própria. Time foca no que é core (VRAM/CUDA).
**+** ublk é userspace → o módulo de kernel Ring-0 futuro **não herda** essa dep (zero-dep do LKM
preservado no destino).
**−** Quebra o zero-dep **userspace** (1 crate + suas transitivas) — **exceção explícita** à
política, restrita ao io_uring, e **só se** o ublk for adotado (gated em bench).
**−** Acopla a uma crate externa (supply chain): mitigado por `cargo audit`/`cargo deny` na CI e
fixar versão; a crate `io-uring` tem histórico de manutenção e auditoria a verificar no momento da
adoção.

## Alternatives considered

- **FFI io_uring hand-rolled (`ramshared-uring`, zero-dep)** — rejeitado: barreiras de memória em
  `unsafe` num caminho de swap = risco de correção alto demais para o ganho (pureza de política).
- **Manter NBD (sem ublk)** — é o **fallback** se o bench não provar ganho de latência; nesse caso
  a crate nem entra. NBD já é validado (§14/H1).
- **Syscalls cruas io_uring sem crate** — não evita as barreiras; é o hand-rolled sem nem o açúcar.

## Kahneman

- **#11 (halo effect):** dep nova exige critério/alternativas/quando-revisitar — atendido (bench
  de latência + `cargo audit`; revisitar se o ublk não provar ganho → reverter a NBD).
- **#5 (worst-case):** o pior caso (barreira de memória errada → corrupção de swap) é o que motiva
  preferir a lib auditada à `unsafe` própria.
- **#13 (ilusão de validade):** a adoção do ublk só vale com **bench real** (número), não com a
  expectativa de "io_uring é mais rápido".
