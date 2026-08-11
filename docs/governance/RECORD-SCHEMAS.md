# Temporal record schemas

This document defines RamShared's prospective temporal records. It does not
reinterpret or rewrite historical evidence.

## Task record v1

`TASK.md` is the live coordination surface. Every record after
`<!-- task-schema-v1 -->` has a stable `TASK-NNNN` identifier and requires:

- `Schema: ramshared.task.v1`;
- a bounded status and an owner role;
- `Registered time` and `Updated time`, plus one shared `Date` when both
  moments are on the same calendar day; otherwise separate `Registered date`
  and `Updated date` fields;
- the observed Git source revision;
- destinations, scope, and evidence or blockers.

The log is mutable only to represent current work. When an existing task
changes, its updated date/time must advance. A task cannot be silently removed
in a diff-aware review.

Run:

```bash
node tools/ci/check-task-log.mjs --all
node tools/ci/check-task-log.mjs --diff <base-revision>
```

## Validation evidence v2

Historical `validation.md` entries retain their existing schema. Entries after
`<!-- validation-schema-v2 -->` require `ramshared.validation.v2` and carry:

- a stable `EVD-NNNN` identifier and owner role;
- RFC3339 `Observed at` and `Verified at` timestamps;
- the source revision examined;
- lifecycle state: `reviewable` or `immutable`;
- retention plus freshness for reviewable evidence, or a concrete immutability
  reason (retention may also be recorded) for immutable evidence.

The evidence body still requires the established `What`, measured data where
applicable, re-execution pointer for effect categories, and verdict fields.
This keeps empirical claims tied to a reproducible command and a known time
window rather than treating a historical success as current state.

Run:

```bash
node tools/ci/check-validation-schema.mjs --all
node tools/ci/check-validation-schema.mjs --diff <base-revision>
```
