---
name: agent-orchestration
description: Bounded routing, ownership, approvals, and handoffs for RamShared agents.
paths:
  - .claude/rules/**
  - tools/ci/check-agent-orchestration.*
---

# Agent orchestration — RamShared

<!-- agent-orchestration-schema-v1 -->

This rule is the canonical contract for coordinating bounded work in this
repository. It applies to the root agent and every worker in the same session.
The root agent remains accountable for scope, integration, and the final
report. A worker never expands the approved scope by inference.

## Checker-visible representation

The checker reads rendered CommonMark prose for authority and safety statements
and the canonical `yaml` fences below for typed records. Markdown comments,
raw HTML blocks, and indented code do not grant authority or satisfy a required
statement. A fence with another info string is not a typed record.

## Checker-visible safety invariants

- Root Sol is read-only and must not edit, self-approve, commit, push, merge,
  or run host or destructive actions.
- A worker must not spawn agents or workers.
- Every approval is explicit, current, and scoped; a stale or inherited
  approval is invalid.
- The two Sol gates require separate independent verdicts; one Sol verdict
  cannot satisfy both gates.

## R0–R4 routing

Use the lowest route that can safely handle the request. A route is a control
boundary, not a model-quality label.

| Route | Cost-first purpose | Default model/tier | Write authority |
| --- | --- | --- | --- |
| R0 | Read-only deterministic work. Root Sol is orchestration-only and read-only. | gpt-5.6-luna / low | None |
| R1 | Small closed mutation. | gpt-5.6-luna / medium | Assigned files only |
| R2 | Multi-file work with a known contract. | gpt-5.6-luna / high or max | Assigned files only |
| R3 | Structural, security, concurrency, kernel, driver, or host work. | gpt-5.6-terra | Assigned files only |
| R4 | Critical, release, or final audit work. | gpt-5.6-sol / gpt-5.6-terra | Only the explicitly approved action |

Routing requirements:

- R0 is the lowest-cost read-only deterministic route. Root Sol is
  orchestration-only and read-only; it does not edit a worker's files,
  self-approve a Sol gate, commit, push, merge, or perform host/destructive
  actions.
- R1 handles a small closed mutation with an exact file allowlist and a local
  acceptance test. It is not a discovery route.
- R2 handles multi-file work only when the contract, owner, acceptance tests,
  and rollback trigger are already known in the dispatch card.
- R3 is required for structural, security, concurrency, kernel, driver, or
  host work and must be independent of the worker that wrote the slice.
- R4 is reserved for critical work, release boundaries, and final audits.
  Protected or destructive actions still require a fresh, explicit user
  approval naming the action and target before dispatch.

## Luna/Terra/Sol tier matrix

| Model | Tier | Appropriate work |
| --- | --- | --- |
| gpt-5.6-luna | low | Read-only deterministic inspection and bounded evidence. |
| gpt-5.6-luna | medium | Small closed mutation with a local acceptance test. |
| gpt-5.6-luna | high | Multi-file work with a known contract. |
| gpt-5.6-luna | max | Multi-file work with a known contract at the upper Luna budget. |
| gpt-5.6-luna | ultra | Exceptional closed Luna task with critical explicit approval. |
| gpt-5.6-terra | low | Structural or implementation work with bounded risk. |
| gpt-5.6-terra | medium | Security, concurrency, or cross-file implementation work. |
| gpt-5.6-terra | high | Kernel, driver, or host-bound implementation work. |
| gpt-5.6-terra | xhigh | High-risk structural implementation and verification. |
| gpt-5.6-terra | max | High-risk implementation with broad evidence requirements. |
| gpt-5.6-sol | low | Read-only orchestration or independent review. |
| gpt-5.6-sol | medium | Independent review with bounded evidence synthesis. |
| gpt-5.6-sol | high | Protected escalation planning or final audit review. |
| gpt-5.6-sol | xhigh | Critical release-boundary or final audit review. |
| gpt-5.6-sol | max | Critical release and rollback review. |

The model family is selected cost-first: Luna for deterministic and closed
work, Terra for structural implementation, and Sol for orchestration and
critical/final review. Sol root orchestration remains read-only at every tier.

Tier selection does not transfer authority. A higher tier may review a lower
tier's result, but it may not silently widen that result's scope.

## Dispatch card

Every worker dispatch is a complete, immutable card. The parent keeps the card
and the worker receives only the relevant repository context plus this rule.

```yaml
schema: ramshared.dispatch.v1
dispatch_id: current-turn-unique-id
route: R3
model: gpt-5.6-terra
tier: medium
objective: validate-one-bounded-checker-slice
owner: worker-agent-id
parent: root-agent-id
scope:
  include: [tools/ci/check-agent-orchestration.mjs]
  exclude: [scripts/safety/cascade-up.sh]
read_only: false
approval: current-user-request
inputs: [repository-facts]
outputs: [ramshared.handoff.v1]
tests: [node --test tools/ci/check-agent-orchestration.test.mjs]
coverage: lines >= 80, branches >= 80, functions >= 80
rollback_trigger: checker-refusal-is-observable
```

The card is refused when it has no single owner, an absolute or broad path,
an ambiguous approval, an unbounded command, or no named test and rollback
trigger. A worker may report that the card is blocked; it may not rewrite the
card or dispatch another worker. A mutable card uses one permitted route/model/
tier combination: R0 is Luna/low, R1 is Luna/medium, R2 is Luna/high or max,
R3 is Terra, and R4 is Sol or Terra. A mutating card uses a current approval;
`none` is valid only for an explicitly read-only card.

## Ownership, fork, and context rules

- The root agent owns the request, dispatch cards, integration, and final
  handoff. Each file and decision has exactly one active owner at a time.
- Fork only for independent, bounded slices with disjoint file ownership.
  The parent retains integration ownership and must reconcile every handoff.
  Never fork merely to bypass a failing gate or to duplicate an owner.
- Workers receive the minimum relevant context: the card, applicable rules,
  current file state, and explicit acceptance criteria. Do not assume hidden
  conversation state, stale memory, or another worker's untyped conclusions.
- Workers do not spawn agents. Only the root orchestrator may dispatch a
  worker, and a worker must return control to its parent after its card is
  complete or blocked.
- Preserve unrelated working-tree changes. Do not use broad staging, resets,
  generated rewrites, or edits outside the card.

## Current approvals

Approval is explicit, scoped, and current. Never inherit a stale approval from
an earlier turn, another agent, a memory entry, or a similar historical
campaign.

| Action | Approval rule |
| --- | --- |
| Read-only inspection, local parsing, and bounded tests | Covered by the current request; no additional approval. |
| Repository documentation/code edits, issues, commits, PR preparation, and a normal merge | Covered only when named by the current plan and exact scope. |
| Remote, credential, host, reboot, live pressure, device/storage, destructive, release, or external publication action | Requires a separate fresh explicit approval naming that exact action and target. |

## Mandatory typed handoff

Every worker returns exactly one `ramshared.handoff.v1` record to its parent,
even when blocked. The prose summary may follow it, but cannot replace it.

```yaml
schema: ramshared.handoff.v1
dispatch_id: current-turn-unique-id
route: R3
model: gpt-5.6-terra
tier: medium
owner: worker-agent-id
status: PARTIAL
changed_files: [tools/ci/check-agent-orchestration.mjs]
tests: [{command: node --test tools/ci/check-agent-orchestration.test.mjs, result: PASS}]
metrics: {lines: 80, branches: 80, functions: 80}
gates: [agent-orchestration-checker-PASS]
residuals: [env-bound live proof is not claimed]
next_action: none
```

The parent checks that the handoff dispatch identity, route, model, tier,
owner, changed files, tests, metrics, gates, residuals, and next action match
the card. Every changed file is repository-relative and inside the dispatch
include scope. Every test includes an exact command and a `PASS`, `FAIL`, or
`SKIP` result. Missing, malformed, unreconciled, or untyped handoffs are
refused; a `PARTIAL` or `BLOCKED` handoff is not promoted by adjective or
inference.

## Two independent Sol gates

Root Sol dispatches both gates and stays read-only. The gate reviewers are
independent from the worker and from each other; one Sol result cannot satisfy
both gates.

- `SOL-GATE-PRE-COMMIT`: before any commit, an independent Sol performs a
  read-only review of the card, ownership, exact diff, tests, coverage,
  rollback trigger, typed handoff, and residuals. It returns a typed verdict;
  it does not edit, commit, or push.
- `SOL-GATE-PRE-PR`: before a PR is proposed or opened, a second independent
  Sol performs a read-only full-branch review of the diff, synchronized docs,
  docs/governance/hygiene/link checks, test evidence, and unresolved scope.
  It returns a separate typed verdict; it does not edit, push, merge, or
  submit.

Both gates must be `PASS` for the relevant boundary. A failed or missing gate
stops the boundary and leaves the work `PARTIAL` or `NO-GO` with a residual;
rerunning a gate after a material change creates a new verdict.
