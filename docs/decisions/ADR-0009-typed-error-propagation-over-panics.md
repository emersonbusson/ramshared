# ADR-0009 — Typed error propagation over panics across workspace crates

## Status

Accepted

## Context

RamShared operates at the boundary of virtualization, operating system kernels, and physical GPU memory. Unchecked panics (`unwrap()`, `expect()`, `panic!()`) inside daemons (`ramshared-wsl2d`), user-mode Windows services (`ramshared-winsvc`), or the CLI stress harness (`ramshared-cli`) can abruptly terminate background block devices while active in `/proc/swaps` or the Windows swap table. Such abrupt crashes trigger catastrophic operating system deadlocks, ghost swap handles, or kernel BugChecks (e.g., BugCheck `0x7A` `KERNEL_DATA_INPAGE_ERROR`).

Previous iterations contained defensive unwrap calls in handshake logic, configuration parsing, volume management, and test helpers that risked runtime panics under malformed inputs or transient I/O failures.

## Decision

1. **Eliminate All Unchecked Panics:** Prohibit `unwrap()` and `expect()` across all production crates, enforcing `#![deny(clippy::unwrap_used, clippy::expect_used)]` or `#![forbid(unsafe_code)]` wherever applicable.
2. **Domain-Specific Typed Errors:** Introduce explicit, strongly typed error enums across all crates:
   - `CascadeError` in `ramshared-cli::cascade`
   - `WinSvcError` and `LockedVolumeError` in `ramshared-winsvc`
   - `HandshakeError` and `ProtocolError` in `ramshared-block`
   - `ConfigError` in `ramshared-config`
   - `UringError` in `ramshared-uring`
   - `IntegrityError` in `ramshared-integrity`
3. **Fail-Closed Result Propagation:** All public functions must return `Result<T, E>`. In the event of an internal invariant violation, processes must execute bounded, ordered teardown (e.g. `swapoff` first) rather than immediately aborting.

## Consequences

### Positive

- Daemons and services never crash abruptly during active block device operations, eliminating ghost swap corruptions.
- Error states are actionable, machine-parsable, and provide diagnostic context without leaking sensitive host memory addresses.
- Test suites can deterministically verify error branches and failure recovery behaviors.

### Costs and limitations

- Requires verbose error enum definitions and explicit `From` implementations across crate boundaries.
- Developers must handle all potential error paths explicitly rather than relying on rapid prototyping shortcuts.

## Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Retain `unwrap()` in non-critical CLI commands | CLI tools run with elevated privileges; panicking during cascade operations leaves kernel devices in half-initialized states. |
| Use `anyhow::Error` uniformly across crates | Type-erased errors prevent granular, automated error recovery and make IPC protocol decoding ambiguous. |
| Catch panics at thread boundaries (`catch_unwind`) | Unwinding across FFI boundaries or while holding raw device locks causes resource leaks and undefined behavior. |

## Kahneman

- **#15 — structural-before-tactical:** Replace the panic mechanism structurally at the crate type-system level rather than adding isolated nil checks.
- **#16 — fail-safe default:** Fall back to safe error returns that preserve ordered swapoff rather than terminating the process abruptly.
- **#18 — fix in the owning layer:** Each crate defines and owns its semantic error domain rather than relying on foreign wrappers.

## Rollback trigger

Roll back this policy via an ADR amendment if typed error propagation causes measurable latency degradation (>5% on hot block read/write paths) or if error conversions introduce cyclic dependencies between workspace crates that cannot be decoupled.
