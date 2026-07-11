# IMPL — WSL2-native VRAM autotier

## Status

**PHASE 1 CODE GREEN; HARDWARE PRESSURE GATE OPEN.** The WDDM provider, lossless constrained-write handling, bounded demote polling, fail-safe teardown, and empty-tier recovery are implemented and deployed. A real host-budget reduction with live swap pages still requires an isolated GPU-PV lab. Phase 2 and Phase 3 remain unresolved.

## Traceability

| Requirement/item | Implementation | Status |
| --- | --- | --- |
| RF-1..RF-3 / ITEM-1 | `ramshared-dxg`: bounded enum/query/close and ambiguity refusal | GREEN |
| RF-4..RF-8 / ITEM-2 | `autotier.rs`: saturating arithmetic, stale gate, five states | GREEN (pure policy) |
| RF-5..RF-7 / ITEM-3 | `CommitBudgetGate` before CUDA allocation; startup-only fallback | GREEN |
| RF-8..RF-10 / ITEM-4 | idle polling, bounded swapoff, parked state, 3-sample empty-tier recovery | CODE GREEN; pressure drill environment-bound |
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
- Safety audit RED/GREEN: constrained NBD writes no longer return EIO; malformed dxg never falls back; teardown requires confirmed swapoff plus `used_kb == 0`.
- Workspace default suite: 273 tests passed after the final changes; 22 privileged/GPU tests remained gated.
- Safe GPU suite: 5 additional tests passed (CUDA 256 MiB, Vulkan 2, VRAM backend 2).
- Coverage: `ramshared-dxg` 92/92 lines and `autotier.rs` 68/68 lines (**100% each**, no exclusions).
- Workspace aggregate coverage remains below 100% because legacy Windows/ublk binaries require other environments; no 100% workspace claim is made.
- `cargo fmt --check`, workspace clippy `-D warnings`, `cargo audit`, `cargo deny`, and docs-check: GREEN (duplicate `windows-sys` warning only).
- Live deployment: active daemon inode matches the final release binary, holds `/dev/dxg`, and serves NBD priority 100 with `used=0` at deployment.

## External gaps

Multi-vendor hardware, another Windows GPU workload, reset/removal, and WDDM cross-view validation require the WSL hardware lab. Zram writeback requires the custom kernel. Host-pressure notifications and a VMBus block service require Microsoft/Windows changes.

Umbrella tracking: [issue #21](https://github.com/emersonbusson/ramshared/issues/21).

## Rollback trigger

Same as SPEC: invalid sample allowing commit, release with `used_kb > 0`, corruption, ghost swap, Oops, or forced daemon termination.
