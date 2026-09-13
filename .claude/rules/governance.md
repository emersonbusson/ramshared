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

1. `## Summary` — English, sufficient for someone outside the conversation.
2. `## Commits` — table with `Commit | What was done | Why it was done | Details`. Each line has hash + clickable `<details>` with context, impact, files, validation, and risk/rollback (`**Files:** ...<br>**Validation:** ...<br>**Risk/rollback:** ...` or local draft equivalent). **Every commit line is visible in the table**, even in PRs with 20+ commits — forbidden to wrap multiple lines inside a grouping `<details>` that hides commits from the initial preview. Per-row `<details>` in the `Details` field remains mandatory and serves the role of hiding deep context. Grouping by editorial category goes in the line's `summary` or in short text in `Details`, never in a `<details>` that hides commits.
3. `## Issue` — `Closes #NNN`, `Fixes #NNN`, or `Resolves #NNN`.
4. `## Assignee` — `@user`. PR and linked issue share assignee.
5. `## Labels` — at least one `type:*` and one `area:*` (e.g.: `area:mm`, `area:drm`).
6. `## Validation` — checklist with relevant gates (`checkpatch.pl`, `make modules`, `dmesg` clean of OOPs, `kselftest`).
7. `## Rollback trigger` — numerical/observable condition that justifies reverting the kernel patch (e.g.: stall > 1ms, kernel panic).
8. **Empirical Hardware Evidence Table & Mandatory Tier 3 Gate** — For PRs touching performance, memory management, or stress benchmarks, provide a self-contained side-by-side comparison table directly reflecting the 4 canonical evaluation categories from benchmark JSON:
   - `1. Workload & Capacity` (Requested RAM, Total Swap, Tier 1 ZRAM, Tier 2 VRAM, Tier 3 SSD)
   - `2. Speed & Transfer Latency` (ZRAM speed, VRAM DMA speed, Speedup factor, Latencies, Reclaim throughput)
   - `3. Pressure & Stalls` (PSI memory index, PSI stall time %, Major faults)
   - `4. Integrity & Stability` (Bit-exact integrity %, Restored RAM, OOM kills, `PASS_ZERO_PANIC`)
   Include physical hardware delta, directionality (`[Higher is better 🔺]` / `[Lower is better 🔻]`), and status icons (`🟢 GAIN`, `🟡 NEUTRAL`, `🔴 ALARM`). **Tier 3 (Host SSD) qualification metrics are strictly mandatory**: the table MUST contain an explicit Tier 3 row with verified capacity/usage in MB, % capacity, and operational status — merges are strictly blocked by CI (`pr-body`) if Tier 3 data is missing. All evidence must be directly readable and humanly explained in the PR body without excessive vertical scrolling. Every single commit in the branch must be listed in the `## Commits` table (CI fails closed if any branch commit is missing). If any metric triggers a 🔴 ALARM (>3% throughput loss, >5% latency spike, unintended SSD spill), the PR is blocked until triaged via [`docs/reliability/HARDWARE-METRICS-TRIAGE.md`](../../docs/reliability/HARDWARE-METRICS-TRIAGE.md). Do NOT point reviewers to raw JSON telemetry files or external links for core performance claims. Do NOT use internal methodology buzzwords (e.g. "cognitive hygiene", "Kahneman disciplines") in the PR description or README.
9. **Two-Stage Language Governance** — PR descriptions may be drafted and reviewed in Portuguese (PT-BR) during active development and review cycles with the maintainer, but MUST be transitioned to English prior to final merge into `main` (enforced by CI upon `stage:ready` or merge qualification).

## Zero-Sum README Policy & Public Hygiene

The root `README.md` and its localized counterpart `README.pt-BR.md` are public-facing technical architectural documents, not internal scratchpads or historical changelogs.

1. **Strict Fixed Ceiling & Zero-Sum Policy**:
   - The README maintains a fixed maximum scope: Architectural Overview, Hardware Rationale/FAQ link, Single Latest Verified Hardware Benchmark, Real-Time Observability (`ramshared top`), Build/Install instructions, and Upstream roadmap (`trovaldo.md`).
   - When a new benchmark qualification or major milestone is added to the README, older historical runs or intermediate test stages MUST be pruned from the README and archived in `docs/benchmarks/history/` or `docs/reliability/`. Never accumulate historical comparison rows or audit tallies in the README.
2. **Anti-Jargon & Internal Agent Shielding**:
   - The public READMEs must NEVER contain internal development tool/bot names (e.g. "Jules", "Codex", "Aider"), automated PR batch censuses (e.g. "162 Jules PRs", "383 PRs"), or methodology buzzwords.
   - All internal audit records and PR census tallies live exclusively in canonical documentation under `docs/reliability/` (e.g. `docs/reliability/JULES-PR-AUDIT-20260905.md`).

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

## Repository Boundary & Out-of-Tree Isolation

A strict, machine-enforced boundary protects the external host and ensures RamShared remains a completely self-contained open-source project.

1. **Confined Workspace Execution (Zero Out-of-Tree Mutation)**:
   - All agent operations, file edits, builds, tests, and cleanup scripts must operate exclusively within the repository tree (`$REPO_ROOT`).
   - Automated scripts or agents are strictly forbidden from inspecting, modifying, or executing destructive operations (`rm`, `del`, `Remove-Item`) against any host system paths, hypervisor instances, or external drives outside the declared repository directory.
2. **Zero Foreign Product / Repository Cross-Contamination**:
   - RamShared is an independent open-source project.
   - Never import narratives, references, private host paths, or service names from external systems or other repositories into tracked source code, documentation, or commit messages.

## Release, Packaging & Reliability Gap Parity

1. **Stable Release Alignment**:
   - Production posture is strictly stable (`v0.11.0`). No beta or prerelease flags remain on public releases.
   - Package build scripts (`scripts/package/build-deb-package.sh`, `build-rpm-package.sh`), documentation badges (`README.md`, `README.pt-BR.md`), and manifests (`docs/localization/manifest.json`) must stay synchronized with the active release tag.
2. **Semantic Gap Register Governance**:
   - `docs/reliability/GAP-REGISTER.md` must accurately reflect real CI and repository state.
   - Never retain phantom blockers (e.g. "await external Guard repair") when CI gates for that capability are passing.
   - Verified milestones (Tier 3 SSD qualification, Rust slice coverage restoration, release publication) must be recorded in `Closed In This Session`.

## Don't

- ❌ Opening a PR without filling out the 7 sections.
- ❌ Commit table without per-row `<details>` and without hash.
- ❌ Grouping `<details>` hiding multiple commit lines from the PR preview.
- ❌ Labels without `type:*` and `area:*`.
- ❌ PR without assignee shared with the issue.
- ❌ Rollback trigger in the form of "if it goes wrong, revert" — needs a numerical/observable window in the Kernel.
- ❌ Changing `CLAUDE.md` without synchronizing `AGENTS.md` in the same commit.
- ❌ Putting internal methodology buzzwords ("cognitive hygiene", "Kahneman disciplines") in PR bodies or READMEs; present observable hardware metrics directly.
- ❌ Directing reviewers to raw JSON files or external pages for core performance/stress claims instead of self-contained explanatory tables.
- ❌ Mentioning internal agent/tool names ("Jules", "Codex", "Aider") or intermediate bot PR batch censuses in `README.md` or `README.pt-BR.md`.
- ❌ Accumulating historical benchmark rows in the README without pruning superseded runs (violating the Zero-Sum README policy).
- ❌ Touching, modifying, or executing destructive cleanup commands outside the repository workspace.
- ❌ Cross-referencing foreign repositories, private services, or external environments in RamShared code or documentation.
- ❌ Retaining phantom blockers or stale incident narratives in `GAP-REGISTER.md` when CI suites are passing.
- ❌ Allowing package scripts or README badges to lag behind the current stable release tag.
