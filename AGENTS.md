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
- Agent orchestration and dispatch: [`.claude/rules/agent-orchestration.md`](.claude/rules/agent-orchestration.md).
- Its rendered policy and canonical typed records are the machine-checked source.

### Before planning, editing, or opening a patch/PR

1. Read `README.md`.
2. Read relevant [`.claude/rules/*.md`](.claude/rules/*.md).
3. Read `MEMORY.md` bottom-up (append-only temporal context). **`MEMORY.md` is local-only** (listed in `.gitignore`) — absent on a clean clone; proceed without it if missing.
4. Read `conversa.md` if present (active context).
5. Read [`trovaldo.md`](trovaldo.md) when working on Linux upstreaming, distro packaging, or kernel drivers, and append progress upon qualification.

### Language

- **English** across the project: source (`.rs`, `.h`, `.c`), comments,
  structural docs (`README.md`, `ARCHITECTURE.md`, `docs/**` except
  locale-specific marketing posts), commits, pull requests, and issues.

### Scope

- Docs and agent rules describe **RamShared only**. No foreign product narratives, service names, or imported process templates from other codebases.

### Zero-Sum README Policy & Public Hygiene

- `README.md` and `README.pt-BR.md` have a fixed scope ceiling. When a new benchmark qualification is added, superseded historical benchmarks must be pruned from the README and archived in `docs/benchmarks/history/`.
- The public READMEs must never mention internal agent/bot names ("Jules", "Codex", "Aider") or intermediate bot PR batch censuses. All audit census records belong exclusively in `docs/reliability/`.

## Commits and patches

Conventional Commits in **English**, imperative title, ≤72 chars. Body in **English**.
Non-trivial commits (locks, DMA, or atomic allocation) **MUST** include `Rollback trigger: ...` in the body.
PR descriptions must follow `.github/pull_request_template.md` strictly: canonical 4-column commits table (`| Commit | What was done | Why it was done | Details |`) with per-row `<details>` block (`**Arquivos:** ...<br>**Validacao:** ...<br>**Risco/rollback:** ...`). Every branch commit must be visible. Performance/hardware PRs must include the full 4-category hardware benchmark comparison table (1. Workload & Capacity, 2. Speed & Transfer Latency, 3. Pressure & Stalls, 4. Integrity & Stability), with mandatory Tier 3 (SSD) qualification metrics and `PASS_ZERO_PANIC` verdict (merges are strictly blocked by CI if missing). No internal methodology buzzwords or external links to raw JSON. PRs may be reviewed in PT-BR during draft/collaboration, but must transition to English before merge.

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
- No unsupervised thrash pressure on the live WSL2 daily host. Shared-host
  pressure requires the Windows watchdog harness, explicit approval, telemetry,
  bounded cgroup pressure, and cleanup artifacts.
- **Repository Boundary & Host Isolation**: RamShared is strictly an open-source,
  self-contained project. Agents and automated scripts must never touch, alter,
  or delete files, system paths, or virtual machines outside this repository's
  workspace. Never reference or cross-contaminate with foreign repositories or
  private host environments.
- **Reliability Gap Register & Release Parity**: Keep `docs/reliability/GAP-REGISTER.md`
  semantically synchronized with active CI status and releases (`v0.11.0`). Phantom
  blockers (such as resolved Guard repairs) are strictly forbidden when CI gates pass.
