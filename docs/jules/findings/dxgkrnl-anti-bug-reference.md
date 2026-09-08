# Finding Report: dxgkrnl ANTI-BUG Reference Analysis

## Summary
Analysis of the item referencing `dxgkrnl ANTI-BUG` in `crates/ramshared-wsl2d/src/main.rs`.

## Context & Analysis
In `crates/ramshared-wsl2d/src/main.rs` (lines 4615 and 4789–4792):
```rust
    // MCL_CURRENT only: MCL_FUTURE races dxgkrnl mapping and can hang the host.
    runtime.lock_memory(force, false)?;
...
// NOTE: `arm_future_lock` (arm future lock post-init) was REMOVED — had a race with the
// asynchronous CUDA init of the worker (spawn_server_dt3_vram_with_residency), which would have
// re-triggered the dxgkrnl kernel BUG. The ublk+vram path remains in MCL_CURRENT only.
// See the "dxgkrnl ANTI-BUG" comment in run_ublk.
```

An automated task harvester flagged the phrase "dxgkrnl ANTI-BUG" as an uncompleted item or task. However, code analysis confirms this comment is a critical architectural safety marker that documents an intentional anti-bug fix.

## Technical Details
1. **Host Memory Locking (`mlockall`) Race**: When `mlockall` is called with `MCL_FUTURE` or when future page locks are enabled post-initialization in WSL2, the Linux kernel memory subsystem attempts to lock all present and future virtual memory mappings into physical RAM.
2. **dxgkrnl Driver Conflict**: In WSL2 environments utilizing `dxgkrnl` (DirectX Graphics Kernel for GPU-PV), GPU memory allocations and VMBus page mappings occur asynchronously. `MCL_FUTURE` races against these driver mappings, triggering a kernel BUG panic at `drivers/hv/dxgkrnl/dxgvmbus.c` and causing the host VM to freeze.
3. **Safety Invariant**: To prevent kernel BUG crashes, the `ublk+vram` daemon path strictly uses `MCL_CURRENT` memory locking only. The deferred `arm_future_lock` mechanism was deliberately removed to guarantee host stability.

## Conclusion & Recommendation
No code modifications should be made. The comment is an anti-bug safety reference protecting against kernel crashes, not an uncompleted feature. Modifying this logic or attempting to re-enable `MCL_FUTURE` would violate system invariants and reintroduce host freezes.

This finding artifact (`FINDING_ONLY`) satisfies the smallest safe orthogonal slice rule.
