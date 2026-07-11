# SPEC — WSL2-native VRAM autotier (Phase 1)

## Scope and traceability

This SPEC covers only the first locally implementable phase. Phase 2/3 must revise this file in place.

| Item | Requirements | Files |
| --- | --- | --- |
| ITEM-1 | RF-1..RF-3, NFR-3 | `crates/ramshared-dxg/**` |
| ITEM-2 | RF-4..RF-8, NFR-1/NFR-4 | `crates/ramshared-wsl2d/src/autotier.rs` |
| ITEM-3 | RF-5..RF-7 | `crates/ramshared-block/src/sparse_vram.rs`, daemon wiring |
| ITEM-4 | RF-8..RF-10, NFR-2 | daemon controller and existing swapoff boundary |
| ITEM-5 | RF-11 | cascade priority/transport regression tests |
| ITEM-6 | all | SSDV3, architecture, degradation, index, validation |

## ITEM-1 — dxg provider

Use `OpenOptions` on `/dev/dxg`; issue `ENUMADAPTERS2` twice (count then bounded array, maximum 64); retain returned handle; query local segment with `process=0`; close every opened adapter on all exits. Reject unknown selector, count growth beyond 64, nonzero reserved fields, and impossible counters (`reservation > budget` is reported but not used for extra capacity). Positive NTSTATUS is not treated as Linux failure.

Selection is automatic only for exactly 1 adapter. With more adapters, an explicit LUID is required. CUDA ordinal matching is not claimed until CUDA LUID evidence exists.

## ITEM-2 — policy

`external_usage = current_usage.saturating_sub(cuda_committed)` and `usable_budget = budget.saturating_sub(external_usage).saturating_sub(safety_margin)`. A sample older than the configured maximum age is invalid. Invalid samples prohibit commit. State transitions require configurable consecutive constrained/recovery samples; a hard commit violation constrains immediately.

## ITEM-3 — allocation gate

Before every sparse first-write allocation, query policy. If `committed + chunk > usable_budget`, mark the tier constrained but **do not return an I/O error for a write already accepted from Linux swap**. Complete at most that in-flight chunk using the safety margin, then trigger bounded swapoff immediately. No subsequent new commit is admitted while demote is pending. Dxg unavailable at startup selects `cuda-fallback`; malformed output, ambiguous topology, or an ioctl failure after open fails closed rather than silently changing authority.

## ITEM-4 — lifecycle

`available → constrained → demoting → parked → recovering → available`. Demote first stops commits, verifies lower-tier absorption, performs bounded swapoff, checks `used_kb == 0`, then frees chunks. A failed/timed-out swapoff or `used_kb > 0` keeps the backend and CUDA chunks alive. Recovery waits for sustained headroom, re-enables empty swap, and performs no eager reads.

## Lock/context matrix

| Path | Context | May sleep | Lock/order |
| --- | --- | --- | --- |
| dxg ioctl | daemon process thread | yes | no RamShared lock held |
| sparse commit | CUDA owner thread | yes | backend exclusive ownership, then dxg query, then CUDA alloc |
| demote | controller process thread | yes | no CUDA lock; swapoff completes before backend release |

## Security and abuse cases

All uAPI arrays are bounded to 64, layouts are fixed-width, pointers reference owned live vectors during ioctl only, and no kernel pointer is logged. Adapter removal maps to a stable error. Unknown adapter selection fails before allocation. Retry is limited to later samples; deterministic ambiguity never retries (#15). Commands/transitions are idempotent (#17). Swapoff is the independent curator and chunks are never reclaimed while referenced (#16). NBD stays until the ublk owning-layer teardown defect is proven fixed (#18).

## Kahneman gates

- **#2 counterfactual:** if CUDA free looked healthy while WDDM budget fell, commit must still stop. Abort if a test permits it.
- **#3 numbers:** every gate uses bytes, sample age, streak count, and duration.
- **#16 reclaim:** abort demote if `used_kb > 0` after swapoff.
- **#17 replay:** 2 identical constraint/recovery commands must equal 1 transition.

## Validation targets

- DT-1: official ioctl numbers and `repr(C)` sizes.
- DT-2: zero/underflow/overflow budget arithmetic.
- DT-3: 1 adapter selects; 0/multiple reject appropriately.
- DT-4: malformed/stale/error sample blocks commit.
- DT-5: state hysteresis and idempotency.
- DT-6: no free with `used_kb > 0`.
- DT-7: zram 200 > VRAM 100 > disk remains green.
- DT-8: constrained commit completes without NBD EIO and schedules demote.
- DT-9: only device-unavailable open errors permit startup CUDA fallback.

## Rollback trigger

Rollback if any path releases chunks with `used_kb > 0`, if one invalid/stale sample permits a commit, or if hardware validation observes corruption, ghost swap, kernel Oops, or forced daemon termination.
