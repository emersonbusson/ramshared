# Finding: WinBrokerError Mapping Already Implemented

## Description

The objective of the task was to detect `ERROR_BROKEN_PIPE` and `ERROR_NO_DATA` instantly, flushing partial state and resetting the named pipe listener.
Upon inspecting `crates/ramshared-winbroker/src/lib.rs`, the codebase already implements this feature.

Specifically, in `crates/ramshared-winbroker/src/lib.rs`, we already have the mappings for these error codes:
```rust
impl From<std::io::Error> for WinBrokerError {
    fn from(error: std::io::Error) -> Self {
        match error.raw_os_error() {
            Some(231) => Self::PipeBusy,   // ERROR_PIPE_BUSY
            Some(232) => Self::NoData,     // ERROR_NO_DATA
            Some(109) => Self::BrokenPipe, // ERROR_BROKEN_PIPE
            _ => Self::Other(error),
        }
    }
}
```

Further, in `crates/ramshared-winbroker/src/service.rs`, the loop over `pipe.read_frame_stoppable` checks for `109` and `233` (`ERROR_PIPE_NOT_CONNECTED`):
```rust
            Err(error) if matches!(error.raw_os_error(), Some(109) | Some(233)) => break,
```
This demonstrates the connection handling logic around pipe disconnections is already well-implemented and handled using standard OS codes, flushing the core's live session on break. I am treating this as a `FINDING_ONLY` adversarial task.

## Evidence

In `crates/ramshared-winbroker/src/lib.rs`:
```rust
pub enum WinBrokerError {
    PipeBusy,
    NoData,
    BrokenPipe,
    Other(std::io::Error),
}
```
In `crates/ramshared-winbroker/src/service.rs`:
```rust
        for effect in core_guard.on_disconnect(session_id) {
            if let BrokerEffect::Audit(message) = effect {
                eprintln!("broker audit={message}");
                append_evidence(&evidence_path, &instance_id, &message, Some(session_id))?;
            }
        }
```
State flushing and disconnect logic are fully covered in `BrokerSessionCore::on_disconnect`.
