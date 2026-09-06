# FINDING ONLY: Architectural Mismatch

## Analysis
The task instructed to defensively limit the named pipe transfer buffer to 64 MiB in `crates/ramshared-winbroker/src/lib.rs` to prevent memory exhaustion. This represents an architectural mismatch trap for two reasons:

1. **Separation of Concerns**: The target file `crates/ramshared-winbroker/src/lib.rs` manages the high-level session lifecycle and state machine via `BrokerSessionCore::on_authenticated_msg`. It natively consumes completely deserialized and structured `Msg` enums rather than raw byte buffers.
2. **Pre-existing Stricter Bounds**: The actual pipe stream reading, buffering, and message framing are physically handled in `crates/ramshared-winbroker/src/service.rs` via `serve_session`. The transfer buffer memory is strictly capped by the `MAX_LINE_BYTES` limit (64 KiB) imported from `ramshared_broker::protocol`.

### Concrete Evidence
In `crates/ramshared-winbroker/src/service.rs`, the transfer buffer is bounded defensively, rejecting any frame exceeding the 64 KiB physical limit:

```rust
    let mut frame = initial.to_vec();
    let mut chunk = [0u8; 4096];
    while !stop.load(Ordering::Acquire) {
        if frame.len() > MAX_LINE_BYTES {
            write_message(
                pipe,
                &Msg::Error {
                    reason: "frame_too_large".into(),
                },
            )?;
            break;
        }
```

In `crates/ramshared-winbroker/src/lib.rs`, there is no byte-level buffering logic; only domain models are processed directly:

```rust
    pub fn on_authenticated_msg(&mut self, session_id: usize, message: Msg) -> Vec<BrokerEffect> {
        if self.live_session != Some(session_id) {
            return self.on_unregistered_msg(session_id, message);
        }
```

Since the architecture effectively separates I/O framing from logical processing, and the current framing limit (64 KiB) inherently provides an infinitely tighter and safer boundary than the suggested 64 MiB, no code modifications are necessary or appropriate.
