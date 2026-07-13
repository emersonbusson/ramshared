# SSDV3 — Spec-Driven Development: Base Prompts

Methodology in 3 steps: **PRD → SPEC → IMPL**

Revised for the RamShared stack: Linux Kernel (C/Rust for Linux) · LKM · HMM · NUMA · DRM · MMU · PCIe Gen5 · CXL 3.0 · userspace (sysfs/ioctl/mmap, Rust daemons)

Goals of this version:

- keep a useful discovery phase before converging
- reduce ambiguity between repo facts and proposals
- produce executable PRD and SPEC in the kernel domain (locks, DMA, uAPI, IRQ)
- improve the handoff PRD → SPEC → code → `IMPL.md`
- add cognitive guardrails (System 2) on critical steps

## How to use

1. Use **Step 1** to generate `docs/specs/no-milestone/{slug}/PRD.md` (or a milestone folder if a formal process exists)
2. Use **Step 2** to turn the PRD into `SPEC.md` **in the same folder**
3. Use **Step 2.5** when there is structural, operational, or security risk — no-go **revises `SPEC.md` in place**; git versions history
4. Use **Step 3** to implement strictly from `SPEC.md` and write `IMPL.md`

If a step finds ambiguity that belongs to the previous step, go back one step.

## File layout

**All SDD artifacts (PRD.md, SPEC.md, IMPL.md) live under `docs/specs/`**, never loose under `docs/` root (except documented legacy).

**Canonical naming:**

```text
docs/specs/
├── no-milestone/
│   └── {slug}/
│       ├── PRD.md
│       ├── SPEC.md    # single SPEC; no-go revises in-place — git is history
│       └── IMPL.md    # Step 3 output
└── M{NN}-{name}/      # optional, only if the project adopts formal milestones
    ├── milestone.md
    └── {slug}/
        ├── PRD.md
        ├── SPEC.md
        └── IMPL.md
```

- `{slug}`: kebab-case, short and descriptive (`<issue>-<description>` when there is an issue, else only `<description>`)
- **One `SPEC.md` only.** Step 2.5 no-go revises it in place (`git` is history). Never `SPECvN.md`.
- Reuse an existing `docs/specs/…/{slug}/` folder when the feature already has one.
- Do not create flat legacy trees `docs/{feature-slug}/` for new work.

## Required frontmatter in PRD.md

Every `PRD.md` starts with YAML frontmatter (when the docs index is active, these fields feed it):

```yaml
---
slug: vram-numa-node
title: Expose VRAM as a NUMA node via HMM
milestone: —
issues: []
---
# PRD — VRAM as NUMA node via HMM
```

- `slug`: same as the folder name
- `title`: human-readable
- `milestone`: `M14`… or `—` if not associated yet
- `issues`: array of GitHub issue numbers (`[]` if none)

Status derived from files: `PRD` → only PRD.md; `SPEC` → SPEC.md present; `DONE` → IMPL.md present.

Regenerate / validate the index:

```bash
node tools/generate-docs-index.mjs          # writes docs/INDEX.md
node tools/generate-docs-index.mjs --check  # fails if out of sync
node tools/check-broken-links.mjs           # broken .md links
./scripts/docs-check.sh                    # index --check + links
```

## Cognitive reference

When a change involves structural, operational, security, rollout, rollback, contract/uAPI migration, page/TLB cache, secrets, Ring 0 vs Ring 3 isolation, DMA/IOMMU, or a hot path, the SPEC must explicitly point to [`docs/methodology/kahneman-disciplines.md`](methodology/kahneman-disciplines.md).

Each critical step answers:

- which bias is being fought
- which mandatory System 2 question must be answered
- what minimum evidence authorizes moving forward
- which objective condition requires abort, step-back, or rollback

## RamShared Day-0 policy

RamShared has no live production with mandatory legacy data. Every change must be the **primary and only** solution, in the final correct Day-0 shape.

By default it is forbidden: workaround, shim, dual-reader, dual-write, compatibility layer for old format/ABI, backfill for non-existent production, dual module path, dead code.

Exceptions only with an explicit documented requirement (real external integration, published uAPI that cannot break, approved coordinated rollout). The exception records: reason, removal deadline, rollback, and evidence.

## Version 3 principles

1. **Discovery before convergence** — broad investigation; final document with one direction.
2. **Reuse before creation** — before a new ioctl/sysfs, struct, flag, module param, or path, prove the existing one does not suffice.
3. **Separate fact from proposal** — `Confirmed in codebase` · `Confirmed in documentation` · `Inference / proposal`.
4. **Traceability** — every PRD RF/NFR appears in the SPEC; every IMPL block points to SPEC items.
5. **No structural creativity in Step 3** — new decision → back to SPEC before code.
6. **System 2 on critical steps** — Kahneman discipline + minimum evidence + abort trigger.
7. **Number before adjective** — latency, throughput, coverage, and drills with unit, n, and environment (see [`.claude/rules/benchmarks.md`](../.claude/rules/benchmarks.md)).
8. **Host safety** — swap/ublk thrash pressure **never** on the live WSL2 dev host; real load only on an isolated VM (qemu/civm).
9. **Slice cover ≥80% per business-logic file/crate** — workspace monorepo average does **not** close Step 3.
10. **Live E2E + evidence closes Step 3** — unit/cover alone does **not** authorize closing `validation.md` or a real `DONE`.

### Disciplines → test type (Kahneman gate in Step 3)

| # | Required proof type | RamShared example |
| --- | --- | --- |
| **#9** | Numeric criterion (status, used_kb, prio, cover%) | `ramshared status` with priorities 200>100>-2 |
| **#13** | Real effect + paired refusal with legitimate path | ghost swap refuses `up`; clean `up` passes |
| **#15** | Retry only on transient signature | NBD reconnect on `EAGAIN`; `-EINVAL` fail-fast |
| **#16** | Test from **exhaustion** | demote/reclaim with VRAM already full / WDDM commit cap |
| **#17** | Replay 2× = effect 1× | `down`/`up` or re-issued `swapoff` without double free |

Reference: [`docs/methodology/kahneman-disciplines.md`](methodology/kahneman-disciplines.md). Adversarial audit: [`superprompt.md`](../superprompt.md).

### Requirement IDs

- Functional: **`RF-N`** (or domain prefix `RF-B1`, `RF-K2` if the PRD partitions)
- Non-functional: **`NFR-N`**
- Technical decisions in SPEC: **`DT-N`**
- Implementation items in SPEC: **`ITEM-N`**

Commits and `IMPL.md` cite these IDs. (Legacy synonym `FR-N` in old docs = `RF-N`.)

---

## STEP 1 — Generate PRD.md

### Prompt

I need a technical PRD for the following change:

**[DESCRIBE THE FEATURE/CHANGE IN 1–2 SENTENCES]**

Goal:

- [what technical outcome must exist at the end]

Layer(s) involved:

- [ ] Kernel core (mm / sched / pci)
- [ ] Drivers (drm / amd / nouveau / ramshared LKM)
- [ ] Firmware / BIOS / CXL / PCIe
- [ ] LKM module (init/exit, ops, handlers)
- [ ] Userspace (udev / sysfs / ioctl / mmap / daemon)
- [ ] ABI / uAPI headers
- [ ] Documentation / ADR / runbook
- [ ] Benchmark / P0 gate

### Mandatory process

Before writing the final PRD:

#### Phase 1 — Discovery

- Gather real context in the codebase
- Map existing subsystems, helpers, and contracts
- Identify lock order, IRQ context, GFP constraints when relevant
- List abuse cases (ioctl size/TOCTOU, open/close×DMA race, GFP in IRQ, capability bypass, info-leak, UAF on exit)

#### Phase 2 — Convergence

- Choose **one** recommended option
- Mark each item: Confirmed in codebase / Confirmed in docs / Inference
- Close functional requirements as `RF-N`
- Close non-functional requirements as `NFR-N`
- Define out of scope and acceptance criteria

### PRD output sections (fixed)

1. Summary
2. Technical context
3. Recommended option
4. Functional requirements (`RF-N`)
5. Non-functional requirements (`NFR-N`)
6. Flows
7. Data model
8. API / Interfaces (ioctl/sysfs/uAPI)
9. Dependencies and risks
10. Implementation strategy
11. Documents to update
12. Out of scope
13. Acceptance criteria
14. Validation

Inferences must be scarce. Prefer facts from code and docs.

---

## STEP 2 — Generate SPEC.md

### Prompt

Turn the PRD into an executable `SPEC.md` in the **same** folder.

The SPEC translates the PRD into:

- files to create/modify (paths from repo root)
- handler validations (`copy_{from,to}_user`, bounds, alignment, `capable`)
- **lock order table** and **context matrix** (process/softirq/hardirq × may-sleep × GFP) when applicable
- Kahneman blocks on critical steps (question + minimum evidence + abort trigger)
- numeric rollback trigger for non-trivial mm/DMA/lock work
- traceability: `RF-N` / `NFR-N` → `ITEM-N` / `DT-N`
- **required tests matrix** per business-logic production file (named `#[test]` + cover target ≥80%)

### Hard rules for SPEC

- Day-0 clean by default
- Critical steps must link Kahneman disciplines
- Test matrix must be specific (not “add unit tests”)

---

## STEP 2.5 — SPEC audit (risk-gated)

Use when locks, DMA, uAPI, privilege, Ring 0/3 isolation, or high oops/deadlock risk apply.

Output: `go` / `no-go`. On `no-go`, fix `SPEC.md` **in the same turn** (in-place). Persist as `AUDIT-2.5.md` in the same folder (or `docs/reviews/YYYY-MM-DD-{slug}.md`): severity findings, open questions, verdict, blockers fixed.

Exit: new decision → Step 2; missing Kahneman/evidence/abort or Day-0 violation → `no-go`; `go` → Step 3.

---

## STEP 3 — Implementation + IMPL.md

> Read `SPEC.md` and execute it step by step.
> Step 3 **does not** close architectural gaps; it implements what was already decided.
> When finished (or when a reviewable slice closes), write/update `IMPL.md` in the same folder.

### Prompt

Implement the feature described in `SPEC.md` under `docs/specs/no-milestone/{slug}/` (or milestone path).

At the end, write `IMPL.md` with what was done, numeric validation, and remaining gaps.

### Execution rules

1. Follow the SPEC implementation order
2. Use SPEC signatures and contracts as the base
3. Do not add functionality outside the closed scope
4. Structural gap → back to Step 2 before continuing
5. Contract change → docs/headers in the same cycle
6. Data, capability, isolation, or page-cache change → test
7. Do not refactor adjacent code without functional need
8. Critical item → run the Kahneman block before the next slice
9. Day-0 clean: no shims, fallbacks, dual-path, or dead code
10. If two versions seem necessary → stop and return to SPEC for a Day-0 exception
11. When the SPEC consolidates Day-0 structure, rewrite/remove the old path instead of preserving dead code
12. Commits cite `RF-N` / `NFR-N` / `ITEM-N` when non-trivial; body has `Rollback trigger:` if it touches locks/DMA/mm
13. **Slice coverage gate ≥80%** on business logic **per file** (or per crate when the slice is crate-scoped): `cargo llvm-cov -p <crate> --summary-only` / line-level report. Workspace monorepo average does **not** count. Pure wiring boilerplate may be `N/A — boilerplate` in IMPL.
14. Every SPEC “Required tests” item must exist as a named `#[test]` / `#[tokio::test]`; hang/swapoff/ghost path without paired refusal+legitimate test = incomplete slice
15. Kahneman #13/#15/#16/#17 evidence requires the _type_ in the table above (effect, refusal+legitimate, exhaustion, replay 2×) — smoke without assert does not close
16. **Live E2E + evidence closes Step 3** (not an optional follow-up). Fixed order: (a) unit/cover/docs green; (b) slice binary **deployed** (cascade/daemon with current `target/release` inode — `BINARY_MATCH=OK`, no deleted exe); (c) real journey: `ramshared status` / cascade-health / SPEC drill with ≥1 legitimate scenario + refusals (ghost swap, used_kb>0, preflight fail); (d) artifacts under `docs/specs/.../evidence/` or paths in `validation.md` (health JSON, `cat /proc/swaps`, drill screenshot if UI); (e) only then `validation.md` + `IMPL.md` with numbers and verdict. Unit/cover alone does **not** close Step 3.

### Per-slice execution ritual

1. Implement only the current item
2. Validate compilation
3. Validate related tests (**write them if the SPEC lists them** — TDD)
4. Run cover gate on slice crates/files (`--min 80` / line ≥80%)
5. Validate Kahneman question / evidence / abort if present
6. Compare with the SPEC (test matrix)
7. Only then advance

### Implementation checklist

- [ ] Code compiles without errors
- [ ] checkpatch / lint / clippy for the scope pass
- [ ] Existing tests still pass
- [ ] New slice tests added (all SPEC “Required tests”)
- [ ] Slice coverage gate ≥80% on touched business-logic files/crates
- [ ] Isolation (address space / capabilities) preserved
- [ ] uAPI/ABI updated when required
- [ ] Docs updated when the item requires it
- [ ] Critical steps coherent with Kahneman
- [ ] #13/#15/#16/#17 proofs present when the discipline applies

### When to return to SPEC

- Index/field/lock not anticipated
- Handler needs an extra field
- Uncovered edge case
- Implementation order does not close
- uAPI struct layout or ioctl/sysfs contract changed
- Rollout/rollback not described
- Critical step requires a decision the Kahneman map did not close
- Need for shim/dual-path/compat not documented as Day-0 exception

> **Absolute rule:** if code needs to decide something the SPEC did not decide, stop and update `SPEC.md` first (in-place).

### Final validation

Run the SPEC checklist. Typical RamShared skeleton:

```bash
# C / LKM (adjust paths to the real module)
./scripts/checkpatch.pl -f path/to/file.c
make modules
# make W=1 C=1 M=...   # if sparse/W=1 is in the repo flow
# make kselftest       # relevant targets

# Userspace Rust
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
# Slice cover (example; list crates from SPEC):
cargo llvm-cov -p ramshared-cli -p ramshared-tier -p ramshared-dxg --summary-only
# or per file: cargo llvm-cov report --json | jq ...

# Isolated drill (never thrash live WSL2)
# scripts/kernel/qemu-*.sh  or  civm job documented in the SPEC
```

**IMPL.md (close-out)** — record with numbers (Kahneman #3/#9):

- cover command(s) and exit code
- crate/file → % table (or gate output)
- list “SPEC Required tests → `test_name` implemented”
- residual &lt;80% only with `N/A — boilerplate` or explicit env-bound gap

### End-to-end validation and evidence (Step 3 blocker)

> **Mandatory order:** unit/cover/docs → **live E2E + artifacts** → `validation.md`
> / `IMPL.md` → commits. Partial entry “unit green; E2E pending” does **not** close Step 3.

With the slice code **in the running binary** (not only in git):

1. initial state: `ramshared status`, `/proc/swaps`, `cascade-health.sh`, PID + `readlink /proc/$pid/exe` (= `BINARY_MATCH`);
2. real SPEC action (`up`/`down`/`demote`/lab drill — **no** thrash on daily WSL if destructive pressure);
3. result: priorities, used_kb, flags.ghost/order_ok, exit codes;
4. ≥1 legitimate + SPEC refusals (ghost, used_kb>0, preflight without binary, WDDM fail-closed);
5. artifact paths in `validation.md` (JSON, swaps dump, drill log).

**Hang/freeze class** (ghost ublk, free with used_kb≠0, kill -9 daemon with active swap, false postmortem CRASH) requires #13/#16 proof in E2E or integration — “code exists” ≠ “protection is active”.

### Required output — `IMPL.md`

Generate/update `docs/specs/no-milestone/{slug}/IMPL.md` with **exactly** this structure:

```markdown
# IMPL — {title}

> SSDV3 STEP 3. Implements `SPEC.md` at `docs/specs/.../{slug}/`.
> Branch: `{branch}`. PR: {link or "not yet"}.

## Status

{implemented | partial} · gates: {list with ✓/✗}

## Files (RF/ITEM → change)

| File | ITEM / RF | What was done |
| --- | --- | --- |
| `path` | ITEM-1 (RF-1) | … |

## Small decisions (no new ADR)

- …

## Validation (numbers)

- tests: {n pass / n fail}
- checkpatch / clippy: {clean | findings}
- cover: {crate/file → % or gate output}
- drill / E2E: {PASS/FAIL + command + BINARY_MATCH}
- benchmark (if any): median / p99 / n / tag idle|loaded · run-id

## Gaps

- closed this session: …
- env-bound (needs hardware/civm/GPU): …
- open: …

## Rollback trigger

{numeric/observable condition; align with SPEC and commit body}

## Traceability

| PRD | SPEC ITEM | Commit(s) |
| --- | --- | --- |
| RF-1 | ITEM-3 | `abc1234` |
```

`DONE` in the specs index = presence of `IMPL.md` coherent with the SPEC **and** documented gates.

---

## Exit criteria between steps

### PRD → SPEC

Only advance if:

- there is a clear recommended option
- functional requirements are closed
- structural risks are explicit
- out of scope is defined
- abuse cases are at least listed when there is uAPI/locks/DMA

### SPEC → Implementation

Only advance if:

- every PRD RF is traced
- implementation order is closed
- files to create/modify are explicit
- test and docs plan are defined
- critical steps have Kahneman + question + evidence + abort
- lock order / context matrix if applicable
- if high risk, Step 2.5 with `go`

### Implementation → Commit / DONE

Only advance if:

- code, tests, and docs are consistent with the SPEC
- final validations executed (or env-bound gaps explicit in IMPL)
- **slice cover ≥80%** on touched business-logic files/crates
- **live E2E** with deployed binary and evidence in `validation.md` / `evidence/`
- no drift between uAPI/headers, implementation, and tests
- `IMPL.md` written/updated (with cover + E2E, not narrative only)
- non-trivial commits have rollback trigger when required

> **Note on the index (`DONE`):** presence of `IMPL.md` in `docs/INDEX.md` is an artifact, **not** quality. Closing SSDV3 without cover/E2E/Kahneman test types violates Step 3 even with DONE in the index.

---

## Quick reference — RamShared stack

| Layer | Technology |
| --- | --- |
| Languages | C11 (Linux kernel style) + Rust for Linux / Rust userspace |
| Subsystems | mm (HMM/NUMA), DRM, MMU, PCIe Gen5, CXL |
| Build | Kbuild / Makefiles; Cargo (tooling and daemons) |
| Validation | checkpatch.pl, sparse, lockdep, kmemleak, KASAN, kselftest/KUnit, cargo test |
| Observability | ftrace, perf, dmesg, `dev_*` / `pr_*`, debugfs |
| Userspace (MVP) | Rust (libcuda via FFI, NBD/ublk paths, broker/agent) |
| Lab | qemu drills, civm — **not** thrash on live WSL2 |

---

## Golden rule

> The **PRD** decides what and why.
> The **SPEC** closes how, where, in what order, and with which guardrails.
> **Implementation** executes without reinventing the decision.
> **`IMPL.md`** records what was done, with numbers and honest gaps.

## When to iterate

- If Step 3 finds a real gap, return to Step 2
- If Step 2 finds insoluble ambiguity, return to Step 1
- Never resolve a structural gap only in code
- Never create `PRD.md` / `SPEC.md` / `IMPL.md` outside `docs/specs/…`

## Language

Structural docs and agent rules: **English** (`CLAUDE.md`). Marketing under `docs/marketing/` may be locale-specific.
