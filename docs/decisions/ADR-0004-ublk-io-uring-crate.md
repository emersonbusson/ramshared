# ADR-0004 — ublk (Phase B): use the audited `io-uring` crate, not hand-rolled io_uring FFI

**Status:** Accepted (2026-06-07). A userspace exception to the zero-dependency
policy **only for `io-uring`** and **only on the Phase B/ublk path**; see
Consequences. Routed by DT-1 of this ADR (former path
`docs/ublk-backend/SPECv2.md` retired).

## Context

Phase B proposes replacing the VRAM tier transport from **NBD** (socket) with
**ublk** (a userspace server over **io_uring**) for lower latency (without a
socket round trip). The ublk server is **userspace** (`ramshared-wsl2d`); the
future bare-metal Ring-0 target is a **separate kernel module** that does not
use any userspace crate.

Forces (facts):

- **Current policy:** zero external dependencies (`Cargo.lock` confirms);
  `LIBRARIES.md`: "an ideal LKM has zero dependencies." The project's only
  `unsafe` is isolated in `ramshared-cuda` (FFI `dlopen` + **synchronous**
  function calls — no memory ordering).
- **io_uring requires memory barriers.** SQ/CQ queues are **shared memory with
  the kernel**; the producer/consumer requires correct `acquire`/`release` on
  head/tail indices (`smp_load_acquire`/`smp_store_release`). Hand-rolling this
  in `unsafe` is a low-level concurrency bug class that is **hard to get
  right** and **catastrophic on a swap path** (silent corruption / panic). It
  is **qualitatively** more dangerous than CUDA FFI (SPECv2 audit, M5-1).
- **clap precedent (issue #3):** clap was **rejected** to preserve zero
  dependencies — but clap was **trivially avoidable** (~4–9 flags,
  `std::env::args` is sufficient). io_uring is **not** safely avoidable: even
  through raw syscalls (`io_uring_setup`/`enter`), ring barriers remain
  required. The clap decision does **not** generalize here (avoidance cost ≠
  trivial).

## Decision

For the **userspace** ublk server, use the audited **`io-uring` crate**
(mature, widely used, correct barriers) instead of hand-rolling the FFI.
Encapsulate use in the `ramshared-uring` wrapper crate: it depends on
`io-uring` and isolates any `unsafe` required by SQE/lifetimes; the
`ramshared-wsl2d` daemon and core library retain `#![forbid(unsafe_code)]`.

Version used in the first smoke: **`io-uring 0.7.12`**, MIT OR Apache-2.0
license, `tokio-rs/io-uring` repository, `rust-version = 1.63`
(`cargo info io-uring`, 2026-06-07). `Cargo.lock` also records `libc`,
`bitflags`, and `cfg-if`.

The measurable criterion (anti-halo #11) for **adopting ublk itself** remains
benchmark-gated: **ublk latency < NBD by ≥ X%** on a custom kernel; without a
gain → **keep NBD** and remove the dependency if it has already entered the
smoke test. The exception is recorded in `LIBRARIES.md`.

## Consequences

**+** io_uring barrier correctness resides in an audited library (not in
hand-rolled FFI on the swap path). The project's `unsafe` surface remains
restricted to the `ramshared-uring` wrapper when real operations require
`SubmissionQueue::push`.
**+** ublk is userspace → the future Ring-0 kernel module **does not inherit**
this dependency (the LKM's zero-dependency target remains preserved).
**−** Breaks the **userspace** zero-dependency policy (one crate + transitive
dependencies) — an **explicit exception** restricted to io_uring, and **only
if** ublk is adopted (benchmark-gated).
**−** Couples to an external crate (supply chain): mitigated by a fixed lockfile
version, review of the `Cargo.lock` diff, `cargo audit`/`cargo deny` when
available, and automatic rollback if the benchmark does not justify ublk.

## Alternatives considered

- **Hand-rolled io_uring FFI (`ramshared-uring`, zero dependencies)** —
  rejected: memory barriers in `unsafe` on a swap path create too much
  correctness risk for the gain (policy purity).
- **Keep NBD (without ublk)** — the **fallback** if the benchmark does not
  prove a latency gain; in that case, the crate never enters. NBD is already
  validated (§14/H1).
- **Raw io_uring syscalls without a crate** — does not avoid the barriers; it
  is hand-rolling without even the convenience layer.

## Kahneman

- **#11 (halo effect):** a new dependency requires a criterion, alternatives,
  and a revisit condition — satisfied (latency benchmark + `cargo audit`;
  revisit if ublk does not prove a gain → revert to NBD).
- **#5 (worst-case):** the worst case (incorrect memory barrier → swap
  corruption) motivates preferring the audited library to project-owned
  `unsafe`.
- **#13 (illusion of validity):** ublk adoption is valid only with a **real
  benchmark** (number), not with the expectation that "io_uring is faster."
