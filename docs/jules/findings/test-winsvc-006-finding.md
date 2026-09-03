# FINDING_ONLY: Boundary tests for IPC payload size limits in ipc.rs

## Objective
Test IPC messages at exactly max payload, max+1, zero-length, and with embedded null bytes targeting `crates/ramshared-winsvc/src/ipc.rs`.

## Finding
It is not possible to implement meaningful boundary tests for `MAX_IO` or `MAX_LINE_BYTES` payload size limits inside `crates/ramshared-winsvc/src/ipc.rs`.

## Evidence
1. **Architectural Separation**: The file `crates/ramshared-winsvc/src/ipc.rs` acts purely as an I/O wrapper around the Windows Named Pipe APIs (`CreateFileW`, `ReadFile`, `WriteFile`, `CancelIoEx`, etc).
2. **Missing Limit Constants**: The constants `MAX_LINE_BYTES` (64KiB) and `MAX_IO` (1MiB) do not exist in `ipc.rs`. They are defined and enforced in `crates/ramshared-broker/src/protocol.rs` and the `ramshared-winsvc` configuration parsing / tenant layer.
3. **No Payload Validation Logic**: `ipc.rs` implements `std::io::Read` and `std::io::Write` by directly deferring to the underlying overlapped pipe logic. The only limit logic within `ipc.rs` is `let length = u32::try_from(buf.len()).unwrap_or(u32::MAX);` which is meant to prevent standard library slices larger than 4GB from causing pointer arithmetic issues when cast to `u32` for `ReadFile`/`WriteFile`.
4. **Code Review Rejection**: Previous attempts to test boundaries in `ipc.rs` using a `MockBrokerStream` were rejected in code review because the tests only validated the mock and standard library components. Because the production functions in `ipc.rs` (`NamedPipeBrokerStream::read`/`write`) do not contain payload validation, tests passed to them cannot observe any rejection or specialized handling of payloads over `MAX_IO`.

## Conclusion
Following Immutable Contract Rule 4 ("If safe code modification is not possible, produce FINDING_ONLY with evidence in docs/jules/findings/"), we are recording this finding. The boundary tests for IPC payloads must be written targeting `crates/ramshared-broker/src/protocol.rs` and `crates/ramshared-winsvc/src/broker_tenant.rs` rather than `ipc.rs`.
