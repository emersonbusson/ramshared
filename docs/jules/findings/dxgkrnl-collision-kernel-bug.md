# Finding Report: dxgkrnl Collision Kernel BUG Safeguards Audit

## Executive Summary
This report analyzes the task item regarding line 2688 (or context around line 4615/4791) in `crates/ramshared-wsl2d/src/main.rs` mentioning an incident on 2026-07-03 where a `kernel BUG` occurred due to a collision between `mlockall(MCL_FUTURE)` and `dxgkrnl` mappings.

## Analysis & Findings
1. **Nature of the Incident**: On 2026-07-03, locking future memory allocations (`mlockall(MCL_FUTURE)`) caused a race condition when `dxgkrnl` mapped dynamic VRAM/D3D12 memory pages, resulting in a host/kernel crash (`kernel BUG`).
2. **Current Implementation**:
   - `runtime.lock_memory(force, false)?` is explicitly called with `MCL_CURRENT` only (passing `false` for `MCL_FUTURE`).
   - The code contains comments explicitly enforcing `MCL_CURRENT`-only locking on `ublk+vram` paths to avoid racing `dxgkrnl` mappings.
   - Preflight checks (`scripts/safety/preflight.sh`) verify the existence of the anti-dxgkrnl BUG fix marker string (`MCL_CURRENT-only no caminho ublk+vram`).
3. **Architectural Invariant**:
   - The incident comment is intentional documentation of historical hardware/kernel constraints.
   - Code changes that re-enable `MCL_FUTURE` or alter memory locking for `dxgkrnl` driver interaction would violate memory safety invariants and destabilize the kernel.
   - Per memory rules, when adherence to the IMMUTABLE CONTRACT rule requiring 'smallest safe orthogonal slice' or avoiding unsafe modification applies, a `FINDING_ONLY` artifact in `docs/jules/findings/` must be generated.

## Conclusion
No code modification to `crates/ramshared-wsl2d/src/main.rs` is required or safe, as the existing memory locking strategy strictly prevents the `dxgkrnl` kernel BUG collision. The safety mechanism remains fully operational and verified.
