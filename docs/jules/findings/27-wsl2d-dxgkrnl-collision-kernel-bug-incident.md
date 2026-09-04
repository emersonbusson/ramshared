# Finding 27: WSL2 dxgkrnl Collision Kernel BUG Analysis

- **Crate:** `ramshared-wsl2d`
- **Module:** `main.rs`
- **Classification:** `FINDING_ONLY`

## Observation

The item refers to an inline context note regarding incident 2026-07-03, where a host `kernel BUG` occurred due to memory locking collisions with `dxgkrnl` during `MCL_FUTURE` / asynchronous CUDA worker initialization.

Analysis of `crates/ramshared-wsl2d/src/main.rs` confirms:
1. `run_ublk_with_runtime` enforces `runtime.lock_memory(force, false)?` (`MCL_CURRENT` only).
2. The `arm_future_lock` logic was previously removed to prevent racing asynchronous CUDA initialization.
3. The codebase already implements the necessary safeguards, and all unit tests in `ramshared-wsl2d` pass cleanly.

## Verdict

Accepted as documented historical incident note and architectural verification. No code modification required.
