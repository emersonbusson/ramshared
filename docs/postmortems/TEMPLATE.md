# Postmortem — Short incident title

> Blameless review: assess the process, not only the outcome (Kahneman #7).
> Copy to `docs/postmortems/YYYY-MM-DD-<slug>.md`.

**Governance schema:** 1

## Metadata

- **Incident ID:** `<stable-id>`
- **UTC interval:** `<start> → <end>`
- **Severity:** P0 (panic/data loss) · P1 (OOPS/severe stall) · P2 (regression) · P3 (minor)
- **Component:** `mm` | `drm` | `cxl` | `pci` | `dma` | `core` | `windows` | `wsl2`
- **Owner role:** `<role, never a username>`
- **Affected claim IDs:** `<claims>`
- **Related ADR/SPEC:** `docs/decisions/ADR-NNN.md` / `docs/specs/no-milestone/<slug>/SPEC.md`
- **Evidence manifest:** `<repository-relative path>`

## Summary

One paragraph sufficient for a reader outside the incident.

## Timeline

| UTC time | Event and evidence signal |
| --- | --- |
| 00:00 | ... |

## Measured blast radius

- Tasks/CPUs/NUMA nodes affected: `<n>`
- Exceeded budget: `<observed value and unit>` versus `<limit>`
- Detection and mitigation durations: `<values and units>`
- Data loss or corruption: `<state plus count>`

## Detection

State how the incident appeared: lockdep, kmemleak, KASAN, OOPS/panic,
kselftest/KUnit, `/proc/vmstat`, SCM/Event Log, watchdog, or operator report.
If an earlier green test missed it, explain the missing failure mode.

## Root cause

Give the precise owning-layer cause: lock inversion, unbalanced DMA mapping,
wrong allocation context, lifetime race, storage teardown ordering, or another
specific mechanism. Do not close with “luck” or “be more careful”.

## Process analysis

- Was the decision reasonable with the evidence available at the time?
- Did it succeed or fail accidentally?
- Did the existing rollback trigger fire, and was it acted on?
- Which defense worked and limited the blast radius?

## Counterfactual rollback trigger

Use a numeric or observable condition:
`if <metric/state> <operator> <limit> for <window>, restore <known-good state>`.

## Corrective actions

| Action ID | Type | Owner role | Due date | Numeric/observable done criterion | PR/commit |
| --- | --- | --- | --- | --- | --- |
| ... | prevent\|detect\|respond | ... | YYYY-MM-DD | ... | ... |

At least one `detect` action must reproduce the real failure mode through a
named regression test or platform drill.

## Effectiveness closure

Repeat this block for every corrective action. `effective` requires measured
post-fix evidence for the regression, the legitimate path, and the critical
refusal path.

**Action ID:** `<stable-id>`

**Type:** `prevent` | `detect` | `respond`

**Owner role:** `<role>`

**Due date:** `YYYY-MM-DD`

**Regression command:** `<repository-relative runner/test path>`

**Threshold:** `<number and unit or observable state>`

**Revalidation:** `<legitimate path result>; <critical refusal path result>`

**Observed result:** `<measured result>`

**Evidence:** `<repository-relative artifact or validation path>`

**Closure state:** `open` | `effective` | `blocked`

## Residual limitations

List environment-bound or externally owned gaps. Do not convert them to DONE.

## References

- [`kahneman-disciplines.md`](../methodology/kahneman-disciplines.md)
- [`DEGRADATION-MATRIX.md`](../reliability/DEGRADATION-MATRIX.md)
