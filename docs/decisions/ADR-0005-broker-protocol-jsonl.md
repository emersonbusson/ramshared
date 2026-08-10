# ADR-0005 — Broker protocol (P1): JSON Lines via `serde`/`serde_json`, not length-prefixed

**Status:** Accepted (2026-06-13). A userspace exception to the zero-dependency
policy **only for `serde`/`serde_json`** and **only in the broker control
plane** (the `ramshared-broker` crate, Phase P1); see Consequences. Routed by
[`docs/specs/no-milestone/memory-broker/SPEC.md`](../specs/no-milestone/memory-broker/SPEC.md)
DT-1.

## Context

The Memory Broker (P1) needs an agent↔broker protocol (RF-B1): `Register`,
`Psi`, `SwapOn/Off`, `LeaseRequest/Release`, `DemoteAll`, `Status`, and
responses. It is a **control plane**: a low rate (~**1 msg/s/tenant** — `Psi`
at 1 Hz), with few tenants. The **data plane** (copying swap pages) is **NBD**,
not this protocol.

Forces (facts):

- **Current policy:** zero external dependencies on the NBD path (`Cargo.lock`
  confirms); the only exception is `io-uring` (Phase B/ublk, benchmark-gated —
  [ADR-0004](ADR-0004-ublk-io-uring-crate.md)). The project's only `unsafe`
  lives in `ramshared-cuda`/`ramshared-uring`.
- **`io-uring` precedent (ADR-0004):** a userspace exception is accepted when
  the cost of avoidance is high and hand-rolling risk is catastrophic. **`clap`
  precedent (issue #3):** rejected because it was **trivially avoidable**
  (`std::env::args` is sufficient for ~4–9 flags).
- **Where this case fits:** serializing/validating ~15 message variants with
  field evolution is **not trivial to hand-roll safely** (fragile manual
  parsing, without shape checking → protocol bug on a path that commands
  `swapon`/`swapoff`). It does **not** require memory barriers like io_uring —
  the risk is *parsing correctness*, not concurrency.
- **JSON Lines is operationally superior here:** one JSON object per line
  (`\n`, UTF-8) is **debuggable with `nc`/`jq`** in a low-frequency control
  plane; optional field evolution provides forward compatibility. Binary
  length-prefixed encoding would only gain data-plane throughput — which is
  NBD here.

## Decision

The broker protocol is **JSON Lines** (one JSON object per line, `\n`, UTF-8),
serialized with **`serde` + `serde_json`**. It is encapsulated **only in the
`ramshared-broker` crate** (the `ramshared-agent` inherits it transitively);
the daemon and library retain `#![forbid(unsafe_code)]` (serde `derive`
generates safe code). No `tokio` — `std` threads, the workspace pattern.

Versions (registry, 2026-06-13): **`serde 1.0.228`** (MIT OR Apache-2.0,
`rust-version` 1.56, `serde-rs/serde` repository) with the `derive` feature;
**`serde_json 1.0.150`** (MIT OR Apache-2.0, `rust-version` 1.71,
`serde-rs/json` repository). The exact pin + transitives (`serde_derive`,
`proc-macro2`, `quote`, `syn`, `itoa`, `ryu`, `memchr`) enter `Cargo.lock` in
ITEM-3 (review the lockfile diff).

**Anti-DoS:** the codec imposes the `MAX_LINE_BYTES = 64 KiB` line limit
**before** allocation (mirroring `MAX_OPT_LEN` in the NBD handshake); an invalid
shape → `Err` (serde rejects it), never corrupted state.

**Rollback trigger (#2):** if the protocol needs to carry a **data payload
(>64 KiB/msg)** or exceeds **>100 msg/s/tenant**, migrate to
**length-prefixed (for example, `bincode`)** through a superseding ADR — JSON
no longer pays for itself when it becomes the data plane. The exception remains
in `LIBRARIES.md`.

## Consequences

**+** Debuggable messages (`nc`/`jq`), optional field evolution, and shape
validation **for free** (serde rejects malformed/unknown JSON — RF-B1 input
validation) rather than a manual parser.
**+** Free of `unsafe`: `derive` is safe; new crates retain
`#![forbid(unsafe_code)]`.
**+** The broker is userspace → the future Ring-0 LKM **does not inherit** the
dependency (the destination's zero-dependency target remains preserved).
**−** Breaks the **userspace** zero-dependency policy on the broker path
(serde + serde_json + proc-macro transitives) — an **explicit exception**
restricted to the broker control plane.
**−** Supply chain: mitigated by a lockfile pin, review of the `Cargo.lock`
diff, and `cargo audit`/`cargo deny` when available. (serde is one of the
ecosystem's most-used libraries — low risk.)

## Alternatives considered

- **Length-prefixed + `bincode`** — rejected for P1: binary is not debuggable
  in a low-frequency control plane; the throughput gain matters only in the
  data plane, which is NBD here. It is the **rollback-trigger target** if the
  usage profile changes.
- **Hand-rolled zero-dependency parser** (a simple custom format) —
  rejected: reimplementing serialization + robust shape validation for ~15
  evolving variants is fragile and riskier than the mature library; **unlike
  `clap`** (which was trivially avoidable). The risk here is parsing
  correctness on a path that commands swap.
- **`serde` with a binary format (`postcard`/`bincode`) instead of JSON** —
  loses `nc`/`jq` debuggability without a relevant control-plane gain;
  reconsider alongside the rollback trigger.

## Kahneman

- **#11 (halo effect):** a new dependency has a measurable criterion (message
  rate/size), alternatives, and a revisit condition (numeric rollback trigger)
  — satisfied; record the exception in `LIBRARIES.md` in the same commit.
- **#2 (counterfactual):** explicit rollback trigger (>64 KiB/msg or >100
  msg/s/tenant → bincode).
- **#5 / #13 (worst-case / illusion of validity):** an invalid shape **fails**
  (serde `Err`), does not corrupt state; the 64 KiB line limit before allocation
  (anti-DoS) is tested in ITEM-3.
