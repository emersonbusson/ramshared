# FINDING_ONLY: Cold Page Eviction Priority Sorting

## Objective
The task requested to implement logic to "Prioritize oldest unreferenced memory slices for immediate eviction during critical memory pressure" in `crates/ramshared-tier/src/priority.rs`.

## Finding
This is an architectural scope trap. `crates/ramshared-tier/src/priority.rs` only defines and validates static integer priorities (`ZRAM_PRIO = 200`, `VRAM_PRIO = 100`) for the Linux swap cascade (passed to `swapon`). The Linux kernel's virtual memory subsystem manages page tracking (e.g., LRU lists) and eviction routing entirely in kernel space. A user-space Rust library configuring static tier priorities cannot implement page-level eviction sorting or track "oldest unreferenced memory slices." Modifying `priority.rs` to attempt this would violate the system architecture and be functionally impossible.

## Action Taken
No code changes were made to `crates/ramshared-tier/src/priority.rs`. Produced this FINDING_ONLY report to document the architectural impossibility.
