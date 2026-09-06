# Finding 28: Audit of dxgkrnl Anti-Bug Safeguard Reference in ramshared-wsl2d

## Overview
- **Target File**: `crates/ramshared-wsl2d/src/main.rs`
- **Topic**: `dxgkrnl ANTI-BUG` reference and memory lock mode (`MCL_CURRENT`)

## Context and Findings
In `crates/ramshared-wsl2d/src/main.rs`, comments reference the "dxgkrnl ANTI-BUG" safeguard:
```rust
// NOTE: `arm_future_lock` (arm future lock post-init) was REMOVED — had a race with the
// asynchronous CUDA init of the worker (spawn_server_dt3_vram_with_residency), which would have
// re-triggered the dxgkrnl kernel BUG. The ublk+vram path remains in MCL_CURRENT only.
// See the "dxgkrnl ANTI-BUG" comment in run_ublk.
```
and:
```rust
// MCL_CURRENT only: MCL_FUTURE races dxgkrnl mapping and can hang the host.
runtime.lock_memory(force, false)?;
```

The task details pointed to a comment referencing the dxgkrnl anti-bug fix. This is an intentional architectural safeguard rather than an uncompleted item or bug:
1. `mlockall` is strictly restricted to `MCL_CURRENT` (`lock_memory(force, false)`).
2. `MCL_FUTURE` is prohibited because asynchronous CUDA initialization and GPU-PV `dxgkrnl` driver page mappings race with future memory locks, triggering host/kernel panics.
3. Post-init future locking (`arm_future_lock`) was intentionally removed from the design.

## Conclusion
The codebase already correctly implements the `dxgkrnl` anti-bug safeguard. No code modifications in `crates/ramshared-wsl2d/src/main.rs` are required.
