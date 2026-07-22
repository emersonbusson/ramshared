---
name: documentation
description: When code changes require same-commit documentation updates.
paths:
  - docs/**
  - "**/*.md"
  - .claude/rules/**
---

# Documentation rules — RamShared

## Core principle

Structural, architectural, or pattern-changing work is **incomplete** without the matching documentation in the **same commit or PR**. `CLAUDE.md` / `AGENTS.md` stay short entrypoints — deep detail lives in ADRs, SPECs, runbooks, and `.claude/rules/*`.

## Scope (this repo only)

Documentation under this tree describes **RamShared only**: cascade, kernel/userspace crates, Windows lab drivers, safety/lab scripts, and local methodology.

- Do **not** narrate other products, services, or monorepos as if they were part of this codebase
- Do **not** paste foreign SSDV3/API/tenant/web conventions “because another project has them”
- Cross-links stay inside this repo (`docs/`, `.claude/rules/`, root entrypoints) unless citing an external standard (kernel docs, RFCs, vendor manuals)
- Agent prompts and rules must be readable without context from any other repository

## Day-0 documentation

PRDs, SPECs, ADRs, and agent rules describe the **final** primary path, not compatibility scaffolding for a production that does not exist.

- Default: one primary implementation path
- Mark backfill / dual-path as exception with reason, removal deadline, rollback, evidence
- Do not document shims as first-class implementation tasks without that exception

## Change → document map

| Change | Update in the same commit/PR |
| --- | --- |
| Architecture / topology / crates layout | `ARCHITECTURE.md`; short pointer in `CLAUDE.md` / `AGENTS.md` if the root map changes |
| Locks, DMA, uAPI, mm, new driver surface | SSDV3 under `docs/specs/…` (PRD/SPEC/IMPL); regenerate `docs/INDEX.md` via `node tools/generate-docs-index.mjs` |
| Architectural decision | `docs/decisions/ADR-NNN-*.md` |
| New failure mode / degradation | `docs/reliability/DEGRADATION-MATRIX.md` |
| Numerical gate / perf claim | `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl` (see `benchmarks.md` rule) |
| Empirical “does it work now?” | Append to `validation.md` (append-only) |
| Coding / agent convention | `.claude/rules/*` + sync `CLAUDE.md` / `AGENTS.md` when the root map changes (see `governance.md`) |
| Build / test / drill command | Root `README.md` or relevant runbook; agent entrypoints if agents must run it |
| Kahneman discipline / cognitive gate | `docs/methodology/kahneman-disciplines.md` + SPEC Kahneman map when critical |
| Host safety / supervised pressure | `benchmarks.md` rule + runbook; never document direct live WSL2 pressure as a happy path |

## Spec index hygiene

After adding/removing a `docs/specs/**` (or legacy flat) PRD/SPEC/IMPL folder:

```bash
node tools/generate-docs-index.mjs
node tools/generate-docs-index.mjs --check
```

Broken relative markdown links:

```bash
node tools/check-broken-links.mjs
# or
./scripts/docs-check.sh
```

## ADR shape (minimum)

```markdown
# ADR-NNN: Title

## Status
Accepted | Proposed | Deprecated | Superseded by ADR-XXX

## Context
## Decision
## Consequences
## Alternatives considered
## Rollback trigger
(numerical/observable when non-trivial)
```

## Don't

- ❌ Ship structural code with stale SPECs or missing IMPL when the work claims DONE
- ❌ Leave `docs/INDEX.md` out of sync after adding a feature folder
- ❌ Put long feature dossiers only in `CLAUDE.md`
- ❌ Document dual-path/shims as default without Day-0 exception fields
