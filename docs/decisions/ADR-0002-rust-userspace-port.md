# ADR-0002 — Rust userspace daemon/CLI (port of the reference design)

**Status:** Accepted (2026-06-05).

## Context

The VRAM tier (ADR-0001) is a userspace daemon (Ring 3) that allocates VRAM
through the CUDA Driver API and serves an NBD block device. A proven reference
(`c0deJedi/nbd-vram`, MIT, C) uses this exact design and has been validated on
bare metal. The repository uses C (kernel) + Rust for Linux; `coding.md`
forbids `.unwrap()/.expect()` in production and requires `goto out_err`/RAII
and `// SAFETY` in `unsafe`.

## Decision

Implement the daemon and CLI in **Rust (std, userspace)**, **porting** the
reference design (CUDA through FFI over `libcuda`, fixed-newstyle NBD
protocol) — **not** forking the C implementation. Isolate FFI `unsafe` in
`ramshared-cuda`, with RAII guaranteeing teardown order
(`free → ctx destroy → dlclose`).

## Consequences

- (+) Memory safety, `Result<T,E>`, and GPU-resource RAII; aligned with
  `coding.md`.
- (+) Round trip already validated on a real GPU (RTX 2060) —
  `ramshared-cuda`.
- (−) Porting cost (translation + reorganization into crates) versus copying
  the C implementation.

## Alternatives considered

- **Fork the reference C implementation:** rejected — Day-0 prefers a clean
  rewrite; it loses safety.
- **Rewrite from scratch while ignoring the reference:** rejected — the
  reference is a proven blueprint (anti-NIH).

## Kahneman

- #4 anchoring (reference as the reference class) · #11 halo (adoption
  justified by evidence: green GPU round trip).

## Rollback trigger

Re-evaluate a C fork if Rust↔`libcuda` FFI on WSL2/GPU-PV has `cuInit`/
`cuMemcpy` failures that the C reference does not have in ≥2 distinct
environments.

Links: [`../specs/no-milestone/wsl2-cascade-swap/SPEC.md`](../specs/no-milestone/wsl2-cascade-swap/SPEC.md) §4 ·
`crates/ramshared-cuda`.
