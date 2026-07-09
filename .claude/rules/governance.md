---
name: governance
description: PR template, sync rule, and commit visibility rule.
paths:
  - .github/**
  - CLAUDE.md
  - AGENTS.md
  - .claude/rules/**
---

# Governance rules — RamShared

These rules exist so that every PR (Patch/Pull Request) carries reviewable context and so that changes in agent rules live synchronized between `CLAUDE.md`, `AGENTS.md`, and `.claude/rules/*`.

## PR Template (canonical format)

Every PR uses `.github/pull_request_template.md`. Mandatory sections:

1. `## Summary` — PT-BR, sufficient for someone outside the conversation.
2. `## Commits` — table with `Commit | What was done | Why it was done | Details`. Each line has hash + clickable `<details>` with context, impact, files, validation, and risk/rollback. **Every commit line is visible in the table**, even in PRs with 20+ commits — forbidden to wrap multiple lines inside a grouping `<details>` that hides commits from the initial preview. Per-row `<details>` in the `Details` field remains mandatory and serves the role of hiding deep context. Grouping by editorial category goes in the line's `summary` or in short text in `Details`, never in a `<details>` that hides commits.
3. `## Issue` — `Closes #NNN`, `Fixes #NNN`, or `Resolves #NNN`.
4. `## Assignee` — `@user`. PR and linked issue share assignee.
5. `## Labels` — at least one `type:*` and one `area:*` (e.g.: `area:mm`, `area:drm`).
6. `## Validation` — checklist with relevant gates (`checkpatch.pl`, `make modules`, `dmesg` clean of OOPs, `kselftest`).
7. `## Rollback trigger` — numerical/observable condition that justifies reverting the kernel patch (e.g.: stall > 1ms, kernel panic).

## Commit visibility rule

**Why it exists:** a PR with 16 commits collapsed into a grouping `<details>` showed only 5 lines in the preview; the human reviewer did not see the others and asked where they were. The rule guarantees that this does not happen.

## Sync rule

Every rule that changes here must change in at least 2 of these places in the same commit:

- `CLAUDE.md`
- `AGENTS.md`
- `.claude/rules/<topic>.md`
- `.github/pull_request_template.md`

Skip via `[sync-skip-justified]` in the commit body with explanation.

## Resource guards & fail-safe frontiers (cross-cutting)

Universal principle (not only kernel): any PR that adds/changes a **guard, watchdog, demote/reclaim path, retry loop, or host-safety script** must apply Kahneman:

- **#15** — retry only with proven transient signature
- **#16** — safe default + curator independent of the resource; test from exhaustion
- **#17** — replayable effects are idempotent (2× = 1×)
- **#18** — fix in the owning layer; sunset workarounds only with proof for **this** class

Source: [`docs/methodology/kahneman-disciplines.md`](../../docs/methodology/kahneman-disciplines.md). Domain rules (`kernel.md`, `benchmarks.md`) **reference** these numbers; they do not re-scope them.

CI / scripts / lab harnesses are **not** SSDV3 by default (see `ssdv3.md` § Out of scope); they still obey #15–#18 and host-safety in `benchmarks.md`.

## Don't

- ❌ Opening a PR without filling out the 7 sections.
- ❌ Commit table without per-row `<details>` and without hash.
- ❌ Grouping `<details>` hiding multiple commit lines from the PR preview.
- ❌ Labels without `type:*` and `area:*`.
- ❌ PR without assignee shared with the issue.
- ❌ Rollback trigger in the form of "if it goes wrong, revert" — needs a numerical/observable window in the Kernel.
- ❌ Changing `CLAUDE.md` without synchronizing `AGENTS.md` in the same commit.
