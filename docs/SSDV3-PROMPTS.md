# SSDV3 — Spec-Driven Development (RamShared)

**PRD → SPEC → (2.5 when risky) → IMPL**

**Exclusive scope — RamShared only.** Surfaces in this tree:

| Surface | Paths |
| --- | --- |
| Cascade CLI/daemon | `ramshared` / `ramsharedd`, `crates/ramshared-cli`, related |
| Workspace crates | `crates/ramshared-*` (broker, wsl2d, block, dxg, tier, vram, cuda, agent, …) |
| Kernel / mm | LKM, DRM-MMU, CXL, HMM as in-repo or documented kernel-tree work |
| Windows lab | `crates/ramshared-winsvc`, `scripts/windows/`, WDK drivers when in SPEC |
| Safety / P0 | `scripts/safety/*`, `scripts/p0/*` |
| Specs | `docs/specs/no-milestone/{slug}/` |

Do **not** import process, service names, or API shapes from other repositories. Proof = evidence for **this** tree (cargo, drills, dmesg, `/proc/swaps`, BINARY_MATCH, WDK/Verifier when Windows) — not foreign app-layer smoke.

**When mandatory / not SSDV3:** [`.claude/rules/ssdv3.md`](../.claude/rules/ssdv3.md).  
**Hang-class audit only:** [`superprompt.md`](../superprompt.md).  
**Cognitive rules + test *types* #9/#13/#15–#17:** [`methodology/kahneman-disciplines.md`](methodology/kahneman-disciplines.md) (do not restate full discipline text here).

---

## Layout

```text
docs/specs/no-milestone/{slug}/
  PRD.md
  SPEC.md            # one file; revise in place — never SPECvN.md
  IMPL.md
  AUDIT-2.5.md       # optional; or docs/reviews/YYYY-MM-DD-{slug}.md
  evidence/          # optional live artifacts for this slug
```

- `{slug}`: English kebab-case  
- After add/remove under `docs/specs/`: `node tools/generate-docs-index.mjs` (+ `--check` / `./scripts/docs-check.sh`)  
- Index `PRD` | `SPEC` | `DONE` = **file presence only** — not cover/E2E quality  

### PRD frontmatter

```yaml
---
slug: wsl2-cascade-orphan-recover
title: Zero-used orphan swap recovery
milestone: —
issues: []
---
```

### IDs

| ID | Role |
| --- | --- |
| `RF-N` | Functional (PRD) |
| `NFR-N` | Non-functional (PRD). Legacy SPECs may say `RNF-N` — treat as synonym |
| `DT-N` | Decision closed in SPEC |
| `ITEM-N` | Ordered implementation step |

Commits/IMPL cite IDs. On non-obvious production code:  
`// SPEC: docs/specs/no-milestone/{slug}/SPEC.md §RF-N`

---

## Day-0

One primary path. No shim, dual-path, dual-reader/writer, or dead code unless SPEC records exception: **reason · removal date · rollback · evidence**.

---

## Principles

1. **Discovery before convergence** — investigate wide; document one option.  
2. **Reuse before create** — `crates/ramshared-*`, existing SPECs, kernel APIs first.  
3. **Fact vs inference** — `Confirmed in codebase` | `Confirmed in docs` | `Inference`. Inferences ≲30% of PRD facts.  
4. **Traceability** — RF → ITEM/DT → commit/test.  
5. **No structural creativity in IMPL** — new decision → update SPEC first.  
6. **Number before adjective** — P0: [`.claude/rules/benchmarks.md`](../.claude/rules/benchmarks.md).  
7. **Host safety** — never thrash swap/ublk on live WSL2; pressure only qemu/civm.  
8. **English** structural docs and code comments.  
9. **Cover + live E2E close Step 3** — `IMPL.md` / index `DONE` is not proof.  
10. **Platform-native gates** — Linux LKM ≠ Windows WDK ≠ pure userspace cascade; pick the row in Cover vs E2E.

---

## Kahneman in SPEC/IMPL

Critical steps: block **or** one row in the Kahneman map.

| Field | Content |
| --- | --- |
| Discipline | `#N` + name |
| Question | System-2 question before advancing |
| Min evidence | **Executable** (named test, drill, metric) — not “code exists” |
| Abort | Numeric/observable stop/rollback |

Test *type* (shape of proof, not cover %): see Kahneman § *Disciplines → test evidence*. Quick map: **#9** number · **#13** refusal+legitimate · **#15** transient retry only · **#16** from exhaustion · **#17** 2× = 1×.

---

## STEP 1 — PRD

### Input

```
Change: [1–2 sentences]
Outcome: [what must exist when done]
Layers: [ ] userspace crates  [ ] ramsharedd / cascade CLI
        [ ] LKM / Linux kernel  [ ] uAPI/sysfs/ioctl
        [ ] Windows lab / WDK / winsvc  [ ] safety or p0 script
        [ ] ADR/runbook  [ ] P0 benchmark gate
```

### Discovery (this tree only)

1. Touched crates under `crates/ramshared-*`, `scripts/safety/`, `scripts/p0/`, `scripts/windows/`, LKM paths if any.  
2. SPECs in `docs/specs/no-milestone/`, ADRs in `docs/decisions/`.  
3. `docs/reliability/DEGRADATION-MATRIX.md`; hang-class → `superprompt.md`.  
4. Exists vs extend vs create-from-zero.  
5. Abuse cases if uAPI / locks / DMA / privilege / hot-unplug / ghost-swap / thrash.  
6. Tag material facts: codebase | docs | inference.

### Convergence

One option + discarded alternatives · Day-0 (no fake production compat) · open gaps → risks/out-of-scope.

### Output — `PRD.md` (fixed order)

1. **Summary**  
2. **Technical context** (paths, crates, state; facts tagged)  
3. **Recommended option** (why, discarded, trade-offs)  
4. **RF-N** — description + verifiable acceptance (+ abuse note if boundary)  
5. **NFR-N** — perf (units if claimed), safety, observability (dmesg/metrics/logs), resilience, host-safety bounds  
6. **Flows** — happy (numbered + component); alternate; errors (trigger → errno/CLI exit → log → state)  
7. **Data / state model** — Rust/C structs, sysfs/ioctl, swap/lease/VRAM machines  
8. **Interfaces** — CLI, uAPI, sysfs, daemon frames (`capable`/device policy, bounds, idempotency)  
9. **Dependencies and risks** — prereqs, mitigations, rollout, **numeric rollback trigger**  
10. **Implementation strategy** — slice order, early validation  
11. **Documents to update**  
12. **Out of scope**  
13. **Acceptance criteria**  
14. **Validation plan** — unit · live path for **this** layer · env-bound gaps  

Path: `docs/specs/no-milestone/{slug}/PRD.md`

---

## STEP 2 — SPEC

Surgical `SPEC.md` from the folder PRD — closes decisions; does not restate the PRD.  
After 2.5 `no-go`: revise **this** file in place (optional changelog line under H1). Never `SPECvN.md`.

### Rules

1. Only this slice.  
2. Full repo-root paths.  
3. Each change: **what / how / why**; exact existing symbols.  
4. `ITEM-N` order = implement order.  
5. PRD ambiguity → `DT-N`.  
6. Every RF (and NFR that needs code) traced.  
7. **Linux kernel:** lock order; IRQ/sleep/GFP; `copy_*_user` + size/align/max; `capable`; no KASLR leaks.  
8. **Windows driver (when in scope):** IOCTL validation, IRQL rules, SDV/Verifier plan — not checkpatch-as-primary.  
9. **Userspace:** validate at boundary; no TOCTOU on user/host buffers.  
10. Critical steps: Kahneman question + **executable** evidence + abort.  
11. **Atomicity frontier** + rollback split: **userspace/daemon** · **kernel/module (or Windows driver)** · **host/persistent** (`/proc/swaps`, lease, VRAM, pagefile). Mark **forward-only** when unsafe.  
12. Day-0 clean; exceptions full.  
13. Living-docs row filled for structural change.  
14. Test matrix names real tests — never “add unit tests”.  
15. Business logic: cover ≥80% **or** `N/A — boilerplate` / `N/A — E2E-only` with reason.  
16. Live validation commands must match the **layer** (cascade vs LKM vs Windows) — do not force `cascade-health` on a Windows-only or pure-crate SPEC.

### Kahneman block

```markdown
- **Discipline:** #N — name
- **Question:** …
- **Min evidence:** `cargo test -p ramshared-… test_name` | drill | metric
- **Abort:** …
```

### Output — `SPEC.md` (fixed order)

#### Closed scope

In now · out now · assumed-ready dependencies (with codebase anchors when possible).

#### Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-3, ITEM-4 |

#### Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | … | … |

#### Atomicity and rollback

- Atomicity frontier  
- Rollback: userspace/daemon · kernel/module or Windows driver · host/persistent · forward-only?

#### Kahneman map (critical only)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |

#### Security checklist (pre-impl)

Fill items that apply; mark **N/A** with one-word reason when surface absent.

- [ ] Privilege: `capable` / device node / Windows access rights  
- [ ] User/host copy: bounds + align + max; operate on owned copy  
- [ ] Flags/IOCTL codes: reject unknown  
- [ ] Info-leak: no kernel addresses in default logs/sysfs/uAPI errors  
- [ ] IRQ/atomic or IRQL: no illegal sleep; correct alloc context; lock order written  
- [ ] Lifetime: get/put map/unmap balanced; remove reverse of probe  
- [ ] Hot-unplug / device-gone: stable errno, not UAF  
- [ ] Host safety: no live WSL2 thrash; Windows pressure only in lab VM when required  
- [ ] Replayable ops: idempotent (#17)  

#### Files to CREATE / MODIFY / DELETE

**CREATE** — per file:

```markdown
**`path/from/repo/root.ext`**
- Purpose:
- RF / DT:
- Types / fns (signatures) — or WDK entrypoints:
- Reference pattern in this repo:
- Required tests: `path` :: `test_name` (+ #9/#13/… if critical)
- Cover target: ≥80% | N/A — …
- Kahneman: (if critical)
```

**MODIFY** — what · RF/DT · symbol · before→after · callers/docs · tests · cover · Kahneman if critical.  
**DELETE** — path · why.

#### Observability (if any new signal)

| Signal | Where | Level / type |
| --- | --- | --- |
| e.g. demote count | `status` JSON / dmesg / metric | … |

#### Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Alter / N/A |
| `docs/decisions/ADR-…` | Create / N/A |
| `docs/reliability/DEGRADATION-MATRIX.md` | Alter / N/A |
| `validation.md` | Append on close |
| `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl` | If P0 claim |
| `.claude/rules/*` · `CLAUDE.md` · `AGENTS.md` | If convention changes |

#### Implementation order

`ITEM-1…N` — no gaps; types/protocol before hot path; tests with the code they prove.

#### Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-cli/…/foo.rs` | `…` :: `plan_orphan_…` | unit | #13 | ≥80% |

Kinds: unit · integration · kselftest · WDK/SDV/Verifier · drill/E2E.

#### Validation checklist

Layer-specific (delete N/A rows in the written SPEC):

- [ ] `cargo fmt` / `clippy -D warnings` / `cargo test -p <slice>`  
- [ ] Cover gate: `node tools/ci/check-rust-slice-coverage.mjs -p … --files … --min 80` (paths = test matrix business logic)  
- [ ] Linux LKM: checkpatch/sparse/kselftest from **target kernel tree** (tree+commit in IMPL)  
- [ ] Windows: InfVerif / SDV / Driver Verifier / lab VM drill as SPEC lists  
- [ ] Live path for this product surface (not a generic foreign checklist)  
- [ ] Every matrix row has a real test name  
- [ ] Kahneman critical rows have executable evidence  

---

## STEP 2.5 — Audit

**Use when:** locks, DMA, uAPI, privilege, Ring0/3, hang-class cascade, oops risk, new hardware, structural mm/DRM, Windows kernel driver, non-trivial rollback.  
**Skip when:** small local change, no contract/privilege/concurrency surface.

### Review for

- Ambiguity; atomicity frontier missing or incompatible with real code  
- Rollback without layer split / numbers  
- Evidence without executable path  
- Security: flow subversion (ghost/skip preflight), TOCTOU, race, sleep-in-atomic / bad IRQL, wrong GFP, info-leak, thrash-on-host  
- Day-0 shim without exception  
- Vague verbs without observable criterion  
- Critical step without Kahneman question/evidence/abort  
- Test matrix missing/generic; boundary refusal without #13 pair  
- Evidence = “check exists” only  
- Wrong platform gate (e.g. cascade-only commands on Windows-only SPEC, or checkpatch as primary for WDK)  
- Paths/process from another product/repo  

### Output

1. Findings by severity (cite SPEC §)  
2. Open questions  
3. **`go`** | **`no-go`**  

Write `AUDIT-2.5.md` (or reviews path). On **`no-go`**: fix `SPEC.md` **same turn**; optional H1 changelog. Never `SPECvN.md`.

**Hard no-go:** missing Kahneman on critical · Day-0 violation · incomplete test matrix · privilege/uAPI/driver boundary without refusal+legitimate when applicable · foreign process/API shapes · platform gate mismatch.

### `AUDIT-2.5.md` skeleton

```markdown
# AUDIT-2.5 — {slug}
## Findings
| Sev | SPEC § | Issue | Required fix |
## Open questions
## Verdict
go | no-go
```

---

## STEP 3 — IMPL

Implement `SPEC.md` only. Numbers in `IMPL.md`. Decision missing from SPEC → stop → update SPEC → continue.

### Hard order

1. Code + tests named in SPEC.  
2. `cargo fmt` / `clippy` / `test` for touched crates (and platform tools from SPEC).  
3. **Cover gate** on slice business logic — `node tools/ci/check-rust-slice-coverage.mjs` (see Cover vs E2E).  
4. Deploy when daemon involved — **BINARY_MATCH**:  
   `readlink /proc/$(pgrep -n -x ramsharedd)/exe` equals built `target/release/ramsharedd` (or path SPEC names).  
5. Live E2E for **this** surface (evidence protocol).  
6. Append root `validation.md` (What · Measured data · Verdict ✅/🔴/🟡).  
7. `IMPL.md` then commits (`Rollback trigger:` on non-trivial).  

**Partial vs DONE:** `env-bound` gaps (no GPU, no lab VM, no EV signing) → IMPL **partial**, verdict 🟡, **not** index-quality DONE. Do not invent live proof offline.

### Ritual per ITEM

Implement → compile → tests for ITEM → cover on touched business logic → Kahneman if mapped → confront SPEC matrix → advance.

### Cover vs E2E

| Kind | Gate |
| --- | --- |
| Pure policy/logic in crates (`cascade` plan, tier, broker arbiter, dxg helpers, …) | **Canonical gate:** `node tools/ci/check-rust-slice-coverage.mjs -p <pkgs> --files <matrix paths> --min 80` (line % per production file; workspace average does **not** count). Optional `--report-json tmp/slice-cov.json` for IMPL evidence. |
| Shell orchestration (`scripts/safety` cascade_io, install) | Live E2E + BINARY_MATCH when daemon + refusals; unit optional if SPEC says `N/A — E2E-only` |
| Linux LKM | checkpatch/sparse/kselftest from **target kernel tree** + drill; not a fake in-repo `scripts/checkpatch.pl` |
| Windows driver / winsvc | WDK build · InfVerif · SDV/Verifier as SPEC · lab VM drill; zero thrash on daily host |

### Live E2E evidence (blocks DONE)

Order: unit/cover → **live path** → `validation.md` / `IMPL.md` → commits.  
“Unit green; E2E later” does **not** close DONE.

**before → action → after** on the real operator surface for this slug:

1. **Before** — relevant baseline (`ramshared status`, `/proc/swaps`, dmesg, Windows service state, …).  
2. **Action** — CLI / daemon / sysfs / uAPI / lab script — not a private unit fn as “E2E”. Prefer `scripts/safety/*`, `scripts/p0/*`, `scripts/windows/*`, or SPEC drill.  
3. **After** — numbers + state; ≥1 **legitimate** + SPEC **refusals**.  
4. Artifacts: `docs/specs/no-milestone/{slug}/evidence/` or `tmp/{slug}-e2e/` — paths in `validation.md`.  
5. Promote stable drills into `scripts/safety/` or `scripts/p0/` same cycle.  
6. No secrets; no KASLR addresses in committed logs.  

**Userspace default commands** (packages/files from SPEC matrix):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <slice-crates>

# Canonical cover gate (records per-file %; fails if any business-logic file < 80%)
node tools/ci/check-rust-slice-coverage.mjs \
  -p <slice-crates> \
  --files crates/.../foo.rs,crates/.../bar.rs \
  --min 80 \
  --report-json tmp/<slug>-cov.json

# Cascade live (only if cascade is in scope):
./target/release/ramshared status
# BINARY_MATCH when ramsharedd involved
sudo ./scripts/safety/cascade-health.sh
# + SPEC preflight/drill
```

### `IMPL.md` skeleton

```markdown
# IMPL — {title}

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/{slug}/SPEC.md

## Status
{implemented|partial} · cover ✓/✗ · E2E ✓/✗/env-bound · BINARY_MATCH ✓/✗/N/A

## Files
| Path | ITEM/RF | Change |

## Validation (numbers)
- tests: cmd → exit
- fmt/clippy (and platform tools): exit
- cover: `check-rust-slice-coverage.mjs` table (path → %); N/A rows justified; optional report JSON path
- SPEC matrix → each `TestName` present
- E2E: before/action/after · cmds · state · evidence paths

## Gaps
closed | env-bound (blocker) | open

## Rollback trigger
{numeric/observable}

## Traceability
| RF | ITEM | commit |
```

---

## Exit criteria

| Gate | Advance only if |
| --- | --- |
| PRD → SPEC | one option; RF+acceptance; risks/out-of-scope; abuse cases if boundary; facts tagged |
| SPEC → IMPL | RF matrix; ITEM order; paths; named tests; Kahneman on critical; atomicity/rollback; security N/A-or-checked; platform gates correct; 2.5 **`go`** if risk-triggered |
| IMPL → **DONE** | cover gate; all SPEC tests exist; live E2E before/action/after on **this** surface; IMPL numbers; Day-0 intact; no SPECvN |
| IMPL → **partial** | env-bound recorded with blocker; no false DONE |

### SPEC ↔ code (long-lived)

Every ITEM still maps to path + named test. Tests in code but not in SPEC → update SPEC in place. Example: `docs/reliability/SPEC-CODE-CONFRONT-cascade-2026-07-13.md`.

```bash
rg "fn (canonicalize_swap_path|plan_orphan_action|cascade_already_healthy)" crates/ramshared-cli
cargo test -p ramshared-cli -- --test-threads=1
sudo ./scripts/safety/cascade-preflight.sh
sudo ./scripts/safety/cascade-health.sh
```

---

## Golden rule

> **PRD** = what and why.  
> **SPEC** = how, where, order, guards, named tests, **platform-correct** gates.  
> **IMPL** = execute without reinventing decisions.

Gap in Step 3 → Step 2. Ambiguous SPEC → Step 1. Never invent structure only in code. Never write SDD files outside `docs/specs/…`.
