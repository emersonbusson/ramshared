# Workstation space recovery

## Purpose and boundary

This is a documentation-only decision aid for an operator recovering local
workstation space. It is not a cleanup script, a command catalogue, or an
authorization to remove data. It does not operate on Docker, cargo, a VM, a
VHD, a host filesystem, or a volume.

The machine-readable historical ledger is
[`../governance/space-cleanup-receipts.jsonl`](../governance/space-cleanup-receipts.jsonl).
Validate its structure without changing the workstation:

```bash
node tools/ci/check-space-cleanup-receipts.mjs --check
```

The checker is intentionally read-only. A passing result means only that the
receipt is bounded, sanitized, and cannot be promoted into an authorization;
it does not show current free space or make a deletion safe.

## Before any human-approved recovery

1. Measure the affected volume and record its exact timestamp, free bytes, and
   total bytes. For `I:`, do not infer headroom from a percentage or an earlier
   session.
2. Identify an exact, rebuildable target and its owner. Record before and after
   bytes for each target when they are available.
3. Preserve source code, product data, and evidence. Stop if target identity,
   ownership, or the postcondition is ambiguous.
4. Keep a receipt with an explicit owner, source, target disposition,
   postcondition, retention class, and repository-relative sanitized evidence
   path. A historical or unqualified receipt is never permission for a new
   action.

## Target classification

The only target classes that may be considered for a separately approved,
operator-owned recovery are rebuildable build caches, rebuildable container
caches, rebuildable tool caches, and temporary test output. They still require
current inspection and a measured receipt.

These classes are retain-only in this runbook: toolchains, repository source,
and persistent product data. The following are outside this runbook and must
not be represented as removable targets: disks, partitions, pagefiles, swap,
VHD/VHDX files, virtual machines, checkpoints, container volumes, databases,
credentials, and host services.

For virtual-lab storage, use the safety constraints in
[`../labs/LAB-DISK-GUARD.md`](../labs/LAB-DISK-GUARD.md). In particular, this
runbook does not authorize broad deletion, VHD conversion, merge, compaction,
or filesystem trimming.

## Recorded cleanup history

The most recently documented RamShared cleanup is
`2026-07-15T15:30:00-03:00`: `docker builder prune -f` reported approximately
12.74 GB reclaimed from rebuildable container cache. Its original validation
entry has no preserved source revision or exact per-target byte counters, so
the ledger records `revision_status: not-recorded` rather than inventing one.

An earlier record at `2026-07-13T14:42:26-03:00`, tied to revision
`1be585ed700dccc6ece825e25ccc48b1650099c4`, reports rebuildable container and
build caches removed while persistent data was retained. It too has no
before/after byte receipt.

Both entries keep `I:` as `not-qualified`. Neither is a success claim,
baseline, or instruction to repeat a cleanup. The full, append-only history is
the JSONL ledger; new receipts must preserve an exact source revision when it
exists, or explicitly state that the original source did not retain one.

## Receipt contract

Each JSONL line must be a `ramshared-space-cleanup-receipt/v1` historical
receipt. The validator requires:

- an exact recorded timestamp, repository-local source document, source
  revision when preserved (otherwise an explicit `not-recorded` state), and a
  non-personal owner role;
- an `I:` gate observation, including byte values when measured and an explicit
  `not-recorded` state when they were not preserved;
- one or more target classes with an explicit disposition and internally
  consistent before/after byte state;
- append-only retention, a non-promotable postcondition, and sanitized
  repository-relative documentation paths; and
- no private profile paths, credentials, email addresses, path traversal, or
  unsafe target classes.

Only historical, unqualified, non-promotable outcomes are accepted. This is
deliberate: an archival ledger makes uncertainty visible rather than turning a
past cleanup into a general-purpose deletion mechanism.
