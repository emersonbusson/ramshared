# FINDING_ONLY: validate Windows named pipe security attributes and explicit ACLs

## Finding
The task requests ensuring that named pipe creation sets explicit restrictive security descriptors rather than default permissive ACLs. However, the scope is strictly confined to `crates/ramshared-winbroker/src/lib.rs` and its related test module.

## Evidence
After thoroughly reading the entirety of `crates/ramshared-winbroker/src/lib.rs` (all 457 lines verified), it is evident that no named pipes are created or managed within this file. The `lib.rs` file acts strictly as a domain logic and configuration module, which imports the `pipe` sub-module but does not invoke `CreateNamedPipeW` or any related security attribute settings directly.

The actual implementation of named pipes and explicit ACL assignment via `SECURITY_ATTRIBUTES` is located in `crates/ramshared-winbroker/src/pipe.rs` (e.g., `fn bind` at line 181, which calls `security_descriptor()` and `CreateNamedPipeW`).

Because the strict scope limits modifications exclusively to `crates/ramshared-winbroker/src/lib.rs`, and modifying `pipe.rs` would violate this instruction, it is impossible to implement a safe code change for this task within the provided constraints.

Therefore, this finding reports the architectural separation as proof that no changes can be made inside `lib.rs` to satisfy the goal.