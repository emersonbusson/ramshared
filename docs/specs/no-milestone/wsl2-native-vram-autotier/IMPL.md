# IMPL — WSL2-native VRAM autotier

## Status

**IN PROGRESS.** Phase 1 code and automated validation are recorded here as gates complete. Phase 2 and Phase 3 are intentionally unresolved.

## Traceability

| Requirement/item | Implementation | Status |
| --- | --- | --- |
| RF-1..RF-3 / ITEM-1 | dxg provider | pending |
| RF-4..RF-8 / ITEM-2 | pure budget/state policy | pending |
| RF-5..RF-7 / ITEM-3 | sparse commit gate | pending |
| RF-8..RF-10 / ITEM-4 | live demote/recovery | environment-bound |
| RF-11 / ITEM-5 | priority/NBD regression | pending |

## Validation

No new performance numbers are claimed. Existing 2026-07-11 baseline before implementation: 146 relevant tests passed; 2 GPU and 17 privileged ublk tests were explicitly ignored. Rust 1.93 clippy reported 2 pre-existing sparse allocator style lints, to be cleared in the implementation series.

## External gaps

Multi-vendor hardware, another Windows GPU workload, reset/removal, and WDDM cross-view validation require the WSL hardware lab. Zram writeback requires the custom kernel. Host-pressure notifications and a VMBus block service require Microsoft/Windows changes.

## Rollback trigger

Same as SPEC: invalid sample allowing commit, release with `used_kb > 0`, corruption, ghost swap, Oops, or forced daemon termination.
