# Finding: WSL2D dxgkrnl Collision Kernel BUG Incident Safeguards Audit

## Context
In `crates/ramshared-wsl2d/src/main.rs:2688`, a comment references an incident on 2026-07-03 where a kernel BUG occurred due to collision with `dxgkrnl`.

## Technical Analysis
- **Root Cause**: Memory locking with `MCL_FUTURE` races with asynchronous CUDA driver initialization and hypervisor memory mapping in `dxgkrnl`, causing host kernel BUG / hangs.
- **Safeguards Verified**:
  1. In `crates/ramshared-wsl2d/src/main.rs:4617`, `run_ublk` strictly invokes `runtime.lock_memory(force, false)?`, passing `future = false` to enforce `MCL_CURRENT`-only memory locking.
  2. Post-init `arm_future_lock` was explicitly removed to avoid re-triggering asynchronous CUDA init races with `dxgkrnl`.
  3. Preflight scripts (`scripts/safety/preflight.sh`) explicitly check for `FIX_MARKER='MCL_CURRENT-only no caminho ublk+vram'`.

## Conclusion
The codebase already enforces the necessary safeguards in `run_ublk` and preflight verification to prevent `dxgkrnl` kernel BUG collisions. No additional code changes are required in `crates/ramshared-wsl2d/src/main.rs`.
