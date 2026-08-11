# Runbook — Periodic ADR and discipline review (anti-cargo-cult)

A **quarterly** review (or one performed when closing a milestone) that checks
whether Kahneman disciplines and ADRs remain alive rather than becoming ritual.
It is the self-enforcement mechanism for
[`../methodology/kahneman-disciplines.md`](../methodology/kahneman-disciplines.md).

## Checklist

1. **Active rollback triggers** — for each ADR in
   [`../decisions/`](../decisions/) with `Rollback trigger:`: has the condition
   already fired? If so, was it acted on? A fired condition with no action =
   cargo cult → open a postmortem.
2. **Discipline adoption** — among non-trivial commits
   (`feat|fix|refactor|perf`) in the period, do ≥30% cite a discipline (#1–14)
   in the body/ADR/review? If <30% for 6 months → simplify to Top-5 (the
   Kahneman document's own trigger).
3. **Anchors exist** — does every file cited by the disciplines/`ssdv3.md`
   exist (`docs/postmortems/`, `docs/reliability/`, `docs/decisions/`,
   `docs/LIBRARIES.md`, `methodology/SUPERPROMPT.md`)? Verify with a path grep.
4. **Hygiene** — mark superseded ADRs; update `DEGRADATION-MATRIX.md` for the
   latest critical feature.

## Output

An entry in [`../postmortems/`](../postmortems/) (including “no deviation”),
recording rollback-trigger activations and the measured adoption percentage.
