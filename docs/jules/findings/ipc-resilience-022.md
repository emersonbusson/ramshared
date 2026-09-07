# Finding: IPC Message Sequence Framing Not Applicable to JSON-Lines (ADR-0005)

## Executive Summary
The task requests tracking monotonically increasing sequence numbers and rejecting duplicated or replayed IPC frames in `crates/ramshared-broker/src/protocol.rs`.

However, according to architectural decisions and documentation, the broker protocol uses a JSON-lines wire format via `serde_json` and cannot safely or effectively support binary headers or framing like sequence numbers without fundamentally altering the wire format contract.

## Evidence

From `docs/decisions/ADR-0005-broker-protocol-jsonl.md`:
> The broker protocol is **JSON Lines** (one JSON object per line, `\n`, UTF-8),
> serialized with **`serde` + `serde_json`**. It is encapsulated **only in the
> `ramshared-broker` crate** (the `ramshared-agent` inherits it transitively);
> the daemon and library retain `#![forbid(unsafe_code)]` (serde `derive`
> generates safe code). No `tokio` — `std` threads, the workspace pattern.
>
> Versions (registry, 2026-06-13): **`serde 1.0.228`** (MIT OR Apache-2.0,
> `rust-version` 1.56, `serde-rs/serde` repository) with the `derive` feature;
> **`serde_json 1.0.150`** (MIT OR Apache-2.0, `rust-version` 1.71,
> `serde-rs/json` repository). The exact pin + transitives (`serde_derive`,
> `proc-macro2`, `quote`, `syn`, `itoa`, `ryu`, `memchr`) enter `Cargo.lock` in
> ITEM-3 (review the lockfile diff).

The protocol relies on `MAX_LINE_BYTES` limit and `serde_json::from_slice` which expects valid JSON text up to the newline. Injecting a binary sequence number header or adding fields to every message struct for sequence tracking breaks the simplicity of the control-plane JSON protocol (which must remain debuggable via `nc`/`jq` as explicitly stated in ADR-0005).

Additionally, the protocol already correctly limits lines to avoid DoS and rejects bad JSON. Adding framing sequence guard clauses is an architectural mismatch for this specific text-based protocol.

Therefore, this request is an architectural scope trap.
