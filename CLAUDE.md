# CLAUDE.md — RamShared

> **ATTENTION:** Keep this filename lowercase. All project-specific rules are located in [`.claude/rules/*.md`](.claude/rules/*.md). Do not copy long dossiers here.

## Agent Source Of Truth

[`.claude/rules/*.md`](.claude/rules/*.md) are the authoritative code rules. `AGENTS.md` mirrors these guidelines.

Before changing code:

1. Read this file and `MEMORY.md` (local-only / gitignored; if missing, proceed).
2. For kernel modules (LKM), HMM, Rust for Linux, and CXL, read [`.claude/rules/kernel.md`](.claude/rules/kernel.md).
3. If structural changes, lock manipulation, chronic allocation, or new hardware is involved, follow the **SSDV3** methodology ([`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md) and [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md)).
4. Follow [`.claude/rules/coding.md`](.claude/rules/coding.md) for formatting, checkpatch, and tests.
5. In Pull Requests, follow the commit table format defined in [`.claude/rules/governance.md`](.claude/rules/governance.md).
6. For benchmarks/measurements backing decisions, follow [`.claude/rules/benchmarks.md`](.claude/rules/benchmarks.md) (auto context + ≥3 rounds + append-only log in [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md)).

## Core Methodologies

- **Kahneman Disciplines**: Every architectural or lock/DMA decision must follow the 18 Kahneman disciplines ([`docs/methodology/kahneman-disciplines.md`](docs/methodology/kahneman-disciplines.md)). Avoid System 1; use counterfactuals and numerical rollbacks; follow #15–#18 for retry, fail-safe, idempotency, and shim sunsetting.
- **SSDV3**: Spec-Driven Development. Pipeline: PRD → SPEC → (2.5 + `AUDIT-2.5.md`) → IMPL in `docs/specs/…`. Index: [`docs/INDEX.md`](docs/INDEX.md). See [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md) and [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md). Documentation guidelines are in [`.claude/rules/documentation.md`](.claude/rules/documentation.md).

## Day-0 Policy

RamShared requires that all code sent to Ring 0 is the definitive version for Day-0. The following are forbidden:
- Compatibility shims that introduce latency.
- Temporary workarounds to bypass hardware flaws or cache coherence issues.
- Modules that ignore warnings from `checkpatch.pl`.

## Commits & Patches

- **English** is mandatory across the entire project: source code, comments, commits, PRs, issues, and root/`/docs/` documents.
- Structural commits or those affecting the MMU/DRM require a `Rollback trigger:` in the body.

## Tech Stack Overview

- **Linux Kernel**: Development of LKM (Loadable Kernel Modules) focusing on CXL, PCIe Gen5.
- **Languages**: C11 (Kernel standards) and Rust for Linux.
- **Subsystems**: HMM (Heterogeneous Memory Management), DRM (Direct Rendering Manager), MMU.
- **Validation**: kselftest, checkpatch.pl, sparse, lockdep, kmemleak.

Refer to files in `.claude/rules/` for deep guidelines on each topic.
