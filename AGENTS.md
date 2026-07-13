# AGENTS.md — RamShared

Terse summary for Codex/aider/Jules-style CLIs. For full guidance, read `CLAUDE.md` and `README.md`.

## Repo purpose

`ramshared` is the main R&D repository for hardware acceleration, vRAM-as-RAM (NUMA), and low-level kernel drivers.

## For external agents (Jules, Codex, aider)

**Keep root `AGENTS.md` and `CLAUDE.md` small.**
The source of truth for architecture and coding rules is:

- [`.claude/rules/kernel.md`](.claude/rules/kernel.md)
- [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md)
- [`.claude/rules/coding.md`](.claude/rules/coding.md)
- [`.claude/rules/governance.md`](.claude/rules/governance.md)
- [`.claude/rules/benchmarks.md`](.claude/rules/benchmarks.md)

### Before planning, editing, or opening a patch/PR

1. Read `README.md`.
2. Read relevant [`.claude/rules/*.md`](.claude/rules/*.md).
3. Read `MEMORY.md` bottom-up (append-only temporal context). **`MEMORY.md` is local-only** (listed in `.gitignore`) — absent on a clean clone; proceed without it if missing.
4. Read `conversa.md` if present (active context).

### Language

- **English** across the project: source (`.rs`, `.h`, `.c`), comments, structural docs (`README.md`, `ARCHITECTURE.md`, `docs/**` except locale-specific marketing posts), commit titles, and pull requests.

## Commits and patches

Conventional Commits in **English**, imperative title, ≤72 chars. Body in **English**.
Non-trivial commits (locks, DMA, or atomic allocation) **MUST** include `Rollback trigger: ...` in the body.

## Methodologies (SSDV3 and Kahneman)

- **SSDV3**: Spec-Driven Development. See [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md) and [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md). Artifacts under `docs/specs/no-milestone/{slug}/{PRD,SPEC,IMPL,AUDIT-2.5}.md` (single SPEC, in-place revision; no `SPECv2` for new features). Index: [`docs/INDEX.md`](docs/INDEX.md) (`node tools/generate-docs-index.mjs`). Mandatory for locks/DMA/mm/uAPI/hardware/MMU/DRM — **not** for CI/host-safety scripts alone (#15–#18 + `benchmarks.md`). **Step 3:** cover ≥80% per slice crate/file + live E2E with deployed binary before closing `validation.md` (unit alone does not close).
- **Kahneman Disciplines**: 18 operational disciplines. Source: [`docs/methodology/kahneman-disciplines.md`](docs/methodology/kahneman-disciplines.md). Ring 0 and structural PRs: counterfactual (#2), number before adjective (#3); retry/reconnect (#15), demote/reclaim (#16), replayable commands (#17), shim sunset (#18).
- **Adversarial superprompt (hang/freeze):** [`superprompt.md`](superprompt.md) — ghost swap, free with used_kb≠0, BINARY_MATCH, honest postmortem.
- **Docs hygiene**: `.claude/rules/documentation.md`, `.claude/rules/security.md` · `./scripts/docs-check.sh` (index + broken links).

## Cognitive profiles

### 1. Kernel Hacker (`kernel-coder`)
**Purpose:** Write `C` or `Rust for Linux` that manipulates memory management, PCIe, and DRM drivers.
**Rules:** Read [`.claude/rules/kernel.md`](.claude/rules/kernel.md).

### 2. Hardware Architect (`hardware-researcher`)
**Purpose:** Research CXL, NUMA, and VRAM-as-memory topology decisions.
**Rules:** Prefer evidence, ADRs, and SSDV3 when structural.

### 3. Reliability / hang auditor
**Purpose:** Ghost swap, swapoff-first, BINARY_MATCH, postmortem validity.
**Rules:** Use [`superprompt.md`](superprompt.md) and Kahneman #13/#16.

## Anti-skynet

- No auto-commit/auto-merge without supervision/approval.
- No persisting secrets.
- No undocumented dependencies.
- No thrash pressure on the live WSL2 daily host.
