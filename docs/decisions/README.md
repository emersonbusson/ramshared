# Architecture Decision Records (ADRs)

Numbered, **append-only** decision records. Every non-trivial architecture,
lock, DMA, or memory decision becomes an ADR.

## Format

`ADR-NNNN-slug.md`, with these sections:

- **Status** — Proposed | Accepted | Superseded by ADR-XXXX (with date).
- **Context** — the problem and forces; facts with numbers.
- **Decision** — what was decided (imperative).
- **Consequences** — trade-offs (+ and −), including what gets worse.
- **Alternatives considered** — what was discarded and why.
- **Kahneman** — discipline(s) applied
  ([`../methodology/kahneman-disciplines.md`](../methodology/kahneman-disciplines.md)).
- **Rollback trigger** — a **numeric/observable** condition that reverses the
  decision (`governance.md` requires this for structural changes).

## Index

| ADR | Title | Status |
| --- | --- | --- |
| [0001](ADR-0001-vram-cascade-tiering.md) | zram→VRAM→VHDX swap cascade (VRAM as a cold tier) | Accepted |
| [0002](ADR-0002-rust-userspace-port.md) | Rust userspace daemon/CLI (port of the nbd-vram design) | Accepted |
| [0003](ADR-0003-page-state-swap-safety.md) | Page states: inherit Linux swap; daemon guarantees durability/DEMOTE/atomicity | Accepted |
| [0004](ADR-0004-ublk-io-uring-crate.md) | ublk/io_uring crate | Accepted |
| [0005](ADR-0005-broker-protocol-jsonl.md) | Broker JSONL protocol | Accepted |
| [0006](ADR-0006-storport-virtual-miniport.md) | StorPort virtual miniport (Windows) | Accepted |
| [0007](ADR-0007-kernel-native-language-c.md) | Kernel-native work in **C**, not app Rust | Accepted |
