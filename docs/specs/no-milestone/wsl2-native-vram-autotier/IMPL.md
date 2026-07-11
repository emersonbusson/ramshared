# IMPL — WSL2-native VRAM autotier

## Status

**PARTIAL GREEN.** The Phase 1 WDDM provider, pure state policy, and physical commit gate are implemented. Automatic live demote/re-enable (ITEM-4) remains blocked by the audit's isolated absorption/timeout gate. Phase 2 and Phase 3 are intentionally unresolved.

## Traceability

| Requirement/item | Implementation | Status |
| --- | --- | --- |
| RF-1..RF-3 / ITEM-1 | `ramshared-dxg`: bounded enum/query/close and ambiguity refusal | GREEN |
| RF-4..RF-8 / ITEM-2 | `autotier.rs`: saturating arithmetic, stale gate, five states | GREEN (pure policy) |
| RF-5..RF-7 / ITEM-3 | `CommitBudgetGate` before CUDA allocation; startup-only fallback | GREEN |
| RF-8..RF-10 / ITEM-4 | live demote/recovery | environment-bound |
| RF-11 / ITEM-5 | existing priority and NBD paths preserved | GREEN (regression) |

## Validation

No new performance numbers are claimed.

- RED 1: missing dxg/policy interfaces produced 12 intended compile errors.
- GREEN 1: 70 library tests passed; 2 GPU tests ignored by environment gate.
- RED 2: missing `CommitBudgetGate` produced the intended compile error.
- GREEN 2: 105 library tests passed; 2 GPU tests ignored.
- Final scoped suite: 157 tests passed; 19 GPU/root/ublk tests remained explicitly ignored.
- `cargo clippy -p ramshared-block -p ramshared-dxg -p ramshared-wsl2d --all-targets -- -D warnings`: GREEN, 0 warnings.
- Live `/dev/dxg`, CUDA allocation, swapoff, reset, and pressure tests: **not run in this agent environment**.

## External gaps

Multi-vendor hardware, another Windows GPU workload, reset/removal, and WDDM cross-view validation require the WSL hardware lab. Zram writeback requires the custom kernel. Host-pressure notifications and a VMBus block service require Microsoft/Windows changes.

Umbrella tracking: [issue #21](https://github.com/emersonbusson/ramshared/issues/21).

## Rollback trigger

Same as SPEC: invalid sample allowing commit, release with `used_kb > 0`, corruption, ghost swap, Oops, or forced daemon termination.
