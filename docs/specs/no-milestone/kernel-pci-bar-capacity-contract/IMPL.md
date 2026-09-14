# IMPL — Exact PCI BAR capacity contract

> SSDV3 Step 3 · SPEC:
> `docs/specs/no-milestone/kernel-pci-bar-capacity-contract/SPEC.md`

## Status

**partial** · static regression ✓ · target-kernel gates env-bound · PCI device
drill env-bound. The source change is complete locally, but the terminal state
remains partial until the target-kernel and PCI-device matrix in the SPEC is
complete.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `tools/ci/kernel-probe-contract.test.mjs` | ITEM-1 / RF-1..4 | RED source-contract reproducer. |
| `drivers/block/ramshared/main.c` | ITEM-2 / RF-1, RF-4 | Checked BAR0 refusal and PCI-enable errno propagation. |
| `drivers/block/ramshared/dma.c` | ITEM-3 / RF-2, RF-3 | Defence-in-depth refusal and exact checked mapping. |
| `drivers/block/ramshared/README.md` | ITEM-4 / RF-1..3 | Documents the exact BAR0 capacity contract. |

## Validation

- RED: `node --test tools/ci/kernel-probe-contract.test.mjs` exited `1` with
  the intended short-BAR and errno-propagation failures before production code.
- GREEN: the same command exited `0`: 2 passed, 0 failed (58.58 ms).
- Diff hygiene: `git diff --check` exited `0`.
- Target-tree platform gates and device drill: environment-bound; not run.

## Gaps

- **Environment-bound:** target-kernel checkpatch/sparse/kselftest/KUnit and
  isolated accepted/refusal PCI BAR drill.
- **Open:** local implementation and its repository checks.

## Rollback trigger

Revert if a target-tree gate fails, a valid BAR/capacity pair no longer probes,
or any target drill observes `capacity_bytes != dma.size`.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1..4 | ITEM-1 | `00abd559`, `83a6b505` |
| RF-1..4 | ITEM-2..4 | Pending commit |
