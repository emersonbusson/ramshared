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

### Scope

- Docs and agent rules describe **RamShared only**. No foreign product narratives, service names, or imported process templates from other codebases.

## Commits and patches

Conventional Commits in **English**, imperative title, ≤72 chars. Body in **English**.
Non-trivial commits (locks, DMA, or atomic allocation) **MUST** include `Rollback trigger: ...` in the body.

## Methodologies (SSDV3 and Kahneman)

- **SSDV3**: [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md) (RamShared-only skeletons, matrix, platform gates) + thin [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md). Specs: `docs/specs/no-milestone/{slug}/`. Step 3: named SPEC tests + cover gate `node tools/ci/check-rust-slice-coverage.mjs -p … --files … --min 80` + live E2E on **this** surface (`before→action→after`; cascade/LKM/Windows as SPEC; `BINARY_MATCH` when daemon). Env-bound → partial, not DONE.
- **Kahneman**: [`docs/methodology/kahneman-disciplines.md`](docs/methodology/kahneman-disciplines.md) — #2/#3/#15–#18 structural/hang; test *types* #9/#13/#15–#17 for SPEC evidence.
- **Hang audit**: [`superprompt.md`](superprompt.md).
- **Docs check**: `./scripts/docs-check.sh`.

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
