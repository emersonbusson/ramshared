# Finding Report: dxgkrnl Anti-Bug Reference Audit

## 1. Context & Background
In `crates/ramshared-wsl2d/src/main.rs`, lines 5236-5238 contain the following comment block:
```rust
// NOTE: `arm_future_lock` (arm future lock post-init) was REMOVED — had a race with the
// asynchronous CUDA init of the worker (spawn_server_dt3_vram_with_residency), which would have
// re-triggered the dxgkrnl kernel BUG. The ublk+vram path remains in MCL_CURRENT only.
// See the "dxgkrnl ANTI-BUG" comment in run_ublk.
```
This comment refers to the historical kernel bug incident (Incident #1, 2026-07-03) where calling `mlockall` with `MCL_FUTURE` raced against asynchronous `dxgkrnl` host GPU mapping during CUDA init, triggering a kernel BUG in the guest kernel.

## 2. Architectural Analysis
- **Current Safeguard**: The `ublk+vram` path enforces `MCL_CURRENT` memory locking only via `runtime.lock_memory(force, false)`.
- **Validation**:
  - `run_ublk` calls `lock_memory(force, false)` which invokes `mlockall(MCL_CURRENT)`.
  - Future memory lock (`MCL_FUTURE`) and `arm_future_lock` post-init arming logic have been permanently removed to prevent host/guest deadlock or kernel BUG in `dxgkrnl`.
- **Conclusion**: The codebase already correctly implements the anti-bug safeguard. No code changes are required.

## 3. Compliance Verification
- Code in `crates/ramshared-wsl2d/src/main.rs` respects `MCL_CURRENT`-only policy.
- Guard markers in preflight checks (`scripts/safety/preflight.sh`) confirm `MCL_CURRENT-only no caminho ublk+vram`.
