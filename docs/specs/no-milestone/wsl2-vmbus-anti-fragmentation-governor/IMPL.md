# IMPL — WSL2 VMBus Anti-Fragmentation Governor and Dedicated Ring Pool Resilience

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-vmbus-anti-fragmentation-governor/SPEC.md`

## Status

Implementing · TDD pending · cover gate pending.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-cli/src/stress.rs` | ITEM-1, ITEM-2 / RF-1..4 | Buddyinfo order-7 parser, elevated headroom floor, and anti-fragmentation interlock. |
| `docs/upstream/patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch` | ITEM-3 / RF-5 | Upstream patch formulation for VMBus ring virtual allocation fallback. |

## Validation

- Pending implementation.

## Gaps

- Open.

## Rollback trigger

Revert changes if buddyinfo parsing causes panics on non-standard kernel zone layouts or if false-positive halts occur when order-7 is abundant.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1 | ITEM-1, ITEM-2 | TBD |
| RF-2 | ITEM-2 | TBD |
| RF-3 | ITEM-2 | TBD |
| RF-4 | ITEM-2 | TBD |
| RF-5 | ITEM-3 | TBD |
