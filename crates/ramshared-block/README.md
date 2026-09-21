# ramshared-block

Authoritative SSD storage origin, revocable VRAM block cache, and NBD protocol engine.

## Scope & Responsibility

`ramshared-block` implements the storage model and network block device protocols for RamShared:
- **Authoritative SSD Origin:** Ensures all writes are persisted to an authoritative backing store before cache acknowledgement.
- **Revocable VRAM Cache:** Provides clean, dynamically demountable 128 MiB block chunks in GPU memory.
- **NBD Fixed-Newstyle Wire Protocol:** Safe parser and encoder for NBD protocol negotiation without root privileges.
- **Inflight Range Model:** A small, mutable range-conflict model for tests and
  prospective callers. It is not lock-free, is not wired into the daemon I/O
  path, and does not itself provide request idempotence or teardown safety.

## Workspace Dependencies

- Internal crates: `ramshared-vram` for the reusable VRAM-backed block models.

## Safety Invariants

- **Safe Code Only:** `#![forbid(unsafe_code)]` enforced across the entire crate.
- **Write-Through Invariant:** Acknowledged writes reach the origin storage before cache mutation.
- **Typed Errors:** Uses [`HandshakeError`](src/handshake.rs) and [`ProtocolError`](src/protocol.rs) with zero unchecked panics.

## Testing

```bash
cargo test -p ramshared-block
```
