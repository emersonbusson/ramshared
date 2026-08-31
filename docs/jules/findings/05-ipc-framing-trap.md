# Finding: IPC Message Framing Guard Clauses Traps

## Observation
The user instructed to "Validate IPC message magic bytes, protocol version, and length header with guard clauses before buffer allocation" in `crates/ramshared-broker/src/protocol.rs`.

## Reality
The `crates/ramshared-broker/src/protocol.rs` implements a JSON-lines (`\n`-separated UTF-8) wire protocol (`read_msg` / `write_msg`), which strictly limits bounds using `MAX_LINE_BYTES`.

There are **no** "IPC message magic bytes", binary "protocol versions" embedded in a binary header, or "length headers" in this wire format. It reads up to a newline character (`\n`) using `read_until`. The only version field (`PROTO_VERSION`) is a JSON field in the `Msg::Register` payload, which is parsed *after* the buffer allocation and JSON deserialization step, so it cannot be validated *before* buffer allocation in the framing loop.

## Code Evidence
```rust
pub fn read_msg<R: BufRead>(r: &mut R) -> std::io::Result<Option<Msg>> {
    let mut buf = Vec::new();
    let n = r
        .by_ref()
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', &mut buf)?;
// ... (JSON decoding logic follows)
}
```

The instructions ask to implement logic that does not match the actual JSON-lines implementation or domain design of `ramshared-broker`'s protocol, treating it as if it were a binary protocol (like NBD).

## Conclusion
The request represents a non-existent architectural trap (hallucinated requirement for binary headers in a JSON-lines protocol). The initial physical limit bounds checking code (the `MAX_LINE_BYTES` cap) was refactored strictly into guard clauses in `protocol.rs`, satisfying the general "Guard Clauses over nested IF/ELSE" architectural principle, but magic bytes and length headers could not and should not be implemented in this JSON-lines module.
