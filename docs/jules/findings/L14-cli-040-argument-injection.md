# FINDING_ONLY: Prevent Command Argument Injection in CLI Dispatch

## Objective
The task instructed to strictly validate and escape all command arguments passed from CLI to child processes in `crates/ramshared-cli/src/bounded_process.rs` and its related test module to prevent command argument injection.

## Evidence

In Rust, `std::process::Command` (which is what `crates/ramshared-cli/src/bounded_process.rs` builds upon) does not invoke a shell on Unix systems. It uses the `execve` system call under the hood. The `execve` system call takes an array of string arguments, which are passed directly to the new program as its `argv`.

Because there is no shell intermediate layer interpreting the arguments, there is no command injection vulnerability possible when passing arguments via `Command::arg()` or `Command::args()`. Spaces, quotes, semicolons, pipes, and other shell metacharacters are treated simply as literal string characters. They are not evaluated as commands or structural delimiters.

In `crates/ramshared-cli/src/bounded_process.rs`, the `run_capture_command` function takes an already constructed `&mut Command` object:

```rust
pub(crate) fn run_capture_command<F>(
    command: &mut Command,
    label: &str,
    timeout: Duration,
    output_limit: usize,
    on_spawn: F,
) -> Result<BoundedOutput, BoundedProcessError>
where
    F: FnOnce(u32),
```

The arguments are already securely encapsulated within the `Command` object by the callers (e.g., in `cascade_io.rs`, `monitor.rs`, `supervisor.rs`). If we were to attempt to "escape" these arguments within `bounded_process.rs`, we would actually corrupt the literal values of the arguments, breaking functionality, without providing any security benefit, as no shell is involved to unescape them.

Where the codebase *does* invoke a shell explicitly (e.g., in tests for fixtures like `Command::new("/bin/sh").args(["-c", "exec sleep 10"])`), the arguments passed to `-c` are static, hardcoded strings used exclusively for controlled testing purposes, not dynamically constructed from user input.

Therefore, the requested fix is architecturally invalid and unnecessary, as the vulnerability does not exist due to the inherent safety of Rust's `std::process::Command` on Unix platforms.

## Conclusion
No code changes were made to `crates/ramshared-cli/src/bounded_process.rs` because the vulnerability (command argument injection via shell interpolation) is architecturally impossible in this context. Rust's `std::process::Command` directly invokes `execve` with an argument array, bypassing shell interpretation entirely. Attempting to "escape" arguments would be functionally incorrect and actively harmful.
