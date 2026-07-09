# ADR-0002 — Daemon/CLI userspace em Rust (port do design da referência)

**Status:** Accepted (2026-06-05).

## Context

O tier VRAM (ADR-0001) é um daemon userspace (Ring 3) que aloca VRAM via CUDA
Driver API e serve como block device NBD. Existe uma referência provada
(`c0deJedi/nbd-vram`, MIT, C) com esse design exato, validada em bare-metal. O
repo usa C (kernel) + Rust for Linux; `coding.md` proíbe `.unwrap()/.expect()` em
produção e exige `goto out_err`/RAII e `// SAFETY` em `unsafe`.

## Decision

Implementar o daemon e a CLI em **Rust (std, userspace)**, **portando** o design
da referência (CUDA via FFI sobre `libcuda`, protocolo NBD fixed-newstyle) —
**não** forkando o C. `unsafe` de FFI isolado em `ramshared-cuda`, com RAII
garantindo a ordem de teardown (`free → ctx destroy → dlclose`).

## Consequences

- (+) Memory safety, `Result<T,E>`, RAII de recursos GPU; alinhado a `coding.md`.
- (+) Roundtrip já validado em GPU real (RTX 2060) — `ramshared-cuda`.
- (−) Custo de port (traduzir + reorganizar em crates) vs. copiar o C.

## Alternatives considered

- **Forkar o C da referência:** rejeitado — Day-0 prefere reescrita limpa; perde safety.
- **Reescrever do zero ignorando a referência:** rejeitado — a referência é blueprint provado (anti-NIH).

## Kahneman

- #4 anchoring (referência como reference class) · #11 halo (adoção justificada por evidência: roundtrip GPU verde).

## Rollback trigger

Reavaliar fork em C se o FFI Rust↔`libcuda` no WSL2/GPU-PV apresentar falhas de
`cuInit`/`cuMemcpy` que a referência C não tem, em ≥ 2 ambientes distintos.

Links: [`../specs/no-milestone/wsl2-cascade-swap/SPEC.md`](../specs/no-milestone/wsl2-cascade-swap/SPEC.md) §4 ·
`crates/ramshared-cuda`.
