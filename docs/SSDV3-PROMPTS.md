# SSDV3 — Spec-Driven Development (RamShared)

**PRD → SPEC → (2.5 audit when risky) → IMPL**

**Scope:** this repository only — WSL2/Linux cascade (`ramshared` / `ramsharedd`), workspace crates, Windows lab drivers, `scripts/safety/*`, specs under `docs/specs/`. Do not import process, service names, or API shapes from other codebases.

**When mandatory:** locks/DMA/uAPI/mm/new hardware/structural DRM-MMU — see [`.claude/rules/ssdv3.md`](../.claude/rules/ssdv3.md).  
**Not SSDV3:** pure CI, host-safety script tweaks, doc-only, dependency bumps (still use Kahneman + `validation.md` when empirical).

---

## Layout

```text
docs/specs/no-milestone/{slug}/
  PRD.md
  SPEC.md          # single file; revise in place — never SPECvN.md
  IMPL.md          # Step 3
  AUDIT-2.5.md     # optional
```

- `{slug}`: English kebab-case
- After add/remove folders: `node tools/generate-docs-index.mjs` (+ `--check` / `./scripts/docs-check.sh`)
- Index status: `PRD` | `SPEC` | `DONE` from file presence — `DONE` ≠ quality (cover + E2E required)

### PRD frontmatter

```yaml
---
slug: wsl2-cascade-orphan-recover
title: Zero-used orphan swap recovery
milestone: —
issues: []
---
```

---

## IDs and Kahneman

| ID | Use |
| --- | --- |
| `RF-N` | Functional |
| `NFR-N` | Non-functional |
| `DT-N` | Technical decision in SPEC |
| `ITEM-N` | Implementation step in SPEC |

Commits/IMPL cite IDs. Critical SPEC steps link [`docs/methodology/kahneman-disciplines.md`](methodology/kahneman-disciplines.md).

### Test-type table (Step 3)

| # | Proof | RamShared example |
| --- | --- | --- |
| **#9** | Number not adjective | priorities 200>100>-2; cover %; used_kb |
| **#13** | Effect + paired refusal | ghost `up` fails; clean `up` works |
| **#15** | Retry only transient | `EAGAIN` reconnect; not `-EINVAL` |
| **#16** | From exhaustion | reclaim with VRAM/budget already tight |
| **#17** | Replay 2× = 1× effect | second `down`/`swapoff` safe |

Hang-class adversarial review: [`superprompt.md`](../superprompt.md).

---

## Day-0

One primary path. No shim, dual-path, dual-reader/writer, or dead code unless SPEC records exception: reason, removal date, rollback, evidence.

---

## STEP 1 — PRD (copy-paste prompt)

```
Write docs/specs/no-milestone/{slug}/PRD.md for ramshared only.

Change: [1–2 sentences]
Outcome: [what must exist when done]

Layers (check all that apply):
[ ] userspace Rust crate(s)  [ ] ramsharedd / cascade CLI
[ ] LKM / kernel  [ ] uAPI/sysfs/ioctl  [ ] Windows lab driver
[ ] safety script  [ ] ADR/runbook  [ ] P0 benchmark gate

Process:
1) Discovery in THIS repo (paths, crates, existing SPECs). List abuse cases if uAPI/locks/DMA.
2) One recommended option. Tag each fact: Confirmed in codebase | Confirmed in docs | Inference.
3) RF-N / NFR-N, out of scope, acceptance criteria, validation plan.

Sections (fixed): Summary; Technical context; Recommended option; RF; NFR; Flows;
Data model; Interfaces; Dependencies and risks; Implementation strategy;
Documents to update; Out of scope; Acceptance criteria; Validation.
```

---

## STEP 2 — SPEC (copy-paste prompt)

```
Write docs/specs/no-milestone/{slug}/SPEC.md from the PRD in the same folder.
RamShared-only. No SPECvN.md.

Include:
- files to create/modify (repo-root paths)
- validations (userspace and/or copy_*_user, bounds, capable, GFP/context)
- lock order + IRQ/sleep/GFP matrix if kernel
- ITEM-N ordered implementation list
- RF/NFR → ITEM/DT traceability
- Kahneman block on each critical step (question, min evidence, abort)
- numeric Rollback trigger
- Required tests matrix: production file/crate → named #[test] → type (#9/#13/…) → cover target ≥80% for business logic

Day-0 clean. Test matrix must name tests, not “add unit tests”.
```

---

## STEP 2.5 — Audit (when risky)

Triggers: locks, DMA, uAPI, privilege, Ring0/3, hang-class cascade, oops risk.

```
Audit SPEC.md (adversarial). Output go|no-go.
Write AUDIT-2.5.md same folder: findings by severity, open questions, verdict.
no-go → revise SPEC.md in place same turn (no SPECvN).
Missing Kahneman/evidence/abort or Day-0 violation → no-go.
go → Step 3.
```

Optional path: `docs/reviews/YYYY-MM-DD-{slug}.md` if not colocated.

---

## STEP 3 — IMPL (copy-paste prompt)

```
Implement docs/specs/no-milestone/{slug}/SPEC.md only. Zero scope creep.
Write/update IMPL.md in the same folder with numbers (not adjectives).

Order (hard):
1) implement + unit tests named in SPEC
2) cargo fmt/clippy/test for touched crates
3) cover gate on slice business logic ≥80% per crate or file:
   cargo llvm-cov -p <crates-from-SPEC> --summary-only
   Workspace average does NOT count.
   N/A — boilerplate: CLI dispatch main, pure shell wiring proven only by E2E
   (e.g. cascade_io up/down) when SPEC marks E2E-only + live proof recorded
4) deploy binary if cascade/daemon: BINARY_MATCH
   (readlink /proc/$(pgrep -n -x ramsharedd)/exe == target/release/ramsharedd)
5) live E2E: status + scripts/safety/cascade-health.sh + SPEC drill
   ≥1 legitimate + SPEC refusals (ghost, used_kb>0, preflight)
6) append validation.md (English labels: What, Measured data, Verdict ✅/🔴/🟡)
7) IMPL.md then commits (Rollback trigger on non-trivial)

If code needs a decision SPEC did not make → stop, update SPEC, then continue.
```

### Cover vs E2E (do not fake 80%)

| Kind | Gate |
| --- | --- |
| Policy/pure logic (`cascade/mod.rs`, tier, dxg, sparse, broker logic) | llvm-cov ≥80% on those files/crates |
| Shell orchestration (`cascade_io`, install scripts) | E2E health + BINARY_MATCH + refusal cases; unit optional |
| Kernel LKM | kselftest/checkpatch from **kernel tree** + drill; not a fake `scripts/checkpatch.pl` in this repo |

### Final validation commands (userspace)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace   # or -p <crates>; --ignored needs GPU/root/lab
cargo llvm-cov -p <slice-crates> --summary-only

# cascade slice live (no destructive pressure on daily WSL)
./target/release/ramshared status
sudo ./scripts/safety/cascade-health.sh
# BINARY_MATCH as above
```

Kernel LKM (when in scope): use checkpatch/sparse from the **kernel source tree** the module targets; document the exact tree/commit in IMPL.

### IMPL.md skeleton

```markdown
# IMPL — {title}

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/{slug}/SPEC.md

## Status
{implemented|partial} · gates: {list ✓/✗}

## Files
| Path | ITEM/RF | Change |
| --- | --- | --- |

## Validation (numbers)
- tests: …
- clippy/fmt: …
- cover: crate/file → % (command + exit)
- E2E: command · BINARY_MATCH · health ok · swaps priorities

## Gaps
- closed / env-bound / open

## Rollback trigger
{numeric/observable}

## Traceability
| RF | ITEM | commit |
```

---

## Exit criteria (short)

| Gate | Advance only if |
| --- | --- |
| PRD → SPEC | one option; RFs closed; risks/out-of-scope; abuse cases if uAPI/locks/DMA |
| SPEC → IMPL | every RF traced; ITEM order; test matrix; Kahneman on critical; 2.5 `go` if risky |
| IMPL → DONE | cover + live E2E (or env-bound gap explicit); IMPL has numbers; no SPECvN |

`docs/INDEX.md` `DONE` is file presence only — not a substitute for cover/E2E.

---

## Principles (once)

1. Discovery before convergence  
2. Reuse before create (search crates/ + kernel APIs first)  
3. Fact vs inference  
4. Traceability RF→ITEM→commit  
5. No creativity in IMPL  
6. Number before adjective ([`benchmarks.md`](../.claude/rules/benchmarks.md) for P0 claims)  
7. Host safety: no thrash on live WSL2  
8. English structural docs  

---

## Quick stack map

| Layer | Here |
| --- | --- |
| Userspace | Cargo workspace `crates/*` (`ramshared-cli`, `wsl2d`, `block`, `dxg`, …) |
| Cascade product | `ramshared` / `ramsharedd`, `scripts/safety/cascade-*.sh` |
| Kernel / lab | LKM paths + Windows lab under `scripts/windows/` (host-safety rules apply) |
| Proof log | root `validation.md` (append-only, English labels) |

---

## Iterate

Step 3 gap → Step 2. Ambiguous SPEC → Step 1. Never invent structure only in code. Never write SDD files outside `docs/specs/…`.
