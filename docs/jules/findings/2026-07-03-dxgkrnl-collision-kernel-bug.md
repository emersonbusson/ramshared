# FINDING_ONLY: Analysis of `dxgkrnl` Collision Kernel BUG Incident and Mitigation Invariants

## Overview

This report documents the architectural safety analysis regarding the item:
- **Target File**: `crates/ramshared-wsl2d/src/main.rs`
- **Context**: `/// comment there (incident 2026-07-03: kernel BUG due to collision with dxgkrnl).`
- **Classification**: `FINDING_ONLY` (Architectural Observation & Safety Invariant Audit)

The task rationale notes that the code comment describes a historical incident where a kernel BUG occurred due to memory locking collisions with `dxgkrnl`, rather than an unhandled code bug requiring structural refactoring.

---

## Technical Context & Root Cause Analysis

On **2026-07-03**, a guest Linux kernel `kernel BUG` occurred in WSL2 environments running the `ramshared` daemon (`ramsharedd`) when paired with GPU virtual memory allocations via `dxgkrnl`.

### Mechanism of the Collision
1. **`mlockall(MCL_FUTURE)` Interaction**:
   When `mlockall` is called with the `MCL_FUTURE` flag, the Linux kernel attempts to lock all present and future virtual memory mappings into physical RAM.
2. **`dxgkrnl` Dynamic Page Pinning**:
   Microsoft's virtualized DirectX Graphics Kernel driver (`/dev/dxg` / `dxgkrnl.sys` guest driver) dynamically maps and unmaps VRAM allocation pages during GPU operations.
3. **Race Condition & Kernel BUG**:
   When `MCL_FUTURE` was active or when `arm_future_lock` attempted to lock memory post-initialization asynchronously during worker initialization (`spawn_server_dt3_vram_with_residency`), it raced with `dxgkrnl` page table manipulation, resulting in memory subsystem corruption and a guest `kernel BUG`.

---

## Codebase Safety Invariant Verification

Audit of `crates/ramshared-wsl2d/src/main.rs` and the broader codebase confirms that all required safety mitigations are currently implemented and strictly enforced:

1. **`MCL_CURRENT`-Only Memory Locking**:
   In `crates/ramshared-wsl2d/src/main.rs` (lines 5061 and 5237-5239), `lock_memory` is invoked with `MCL_CURRENT` exclusively (`lock_memory(force, false)`). `MCL_FUTURE` is explicitly disabled for the `ublk + vram` path.
2. **Removal of Post-Init Future Locking**:
   The `arm_future_lock` function was permanently removed to prevent asynchronous race conditions with CUDA/dxgkrnl worker initialization.
3. **Safety Guard & Preflight Enforcement**:
   The safety preflight script (`scripts/safety/preflight.sh`) explicitly checks for the anti-bug marker `MCL_CURRENT-only no caminho ublk+vram` to ensure no binary or build regression introduces `MCL_FUTURE`.

---

## Conclusion & Action Taken

The code comment at `crates/ramshared-wsl2d/src/main.rs` is an intentional architectural documentation anchor explaining historical incident context and safeguarding against future regressions.

Because the production code already completely complies with the safety invariants and no code changes are required or safe to perform, this report is submitted under the **`FINDING_ONLY`** workflow to document the audit and analysis without introducing unnecessary modifications to `crates/ramshared-wsl2d/src/main.rs`.
