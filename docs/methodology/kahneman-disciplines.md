# Kahneman Disciplines — Cognitive Hygiene for AI Decisions (RamShared)

From **Thinking, Fast and Slow** and **Noise** (Kahneman, Sibony, Sunstein), applied **only** to AI-assisted work in **RamShared** (cascade, `crates/ramshared-*`, LKM/Windows lab, host-safety). Examples and rubrics are this repo’s paths — do not import foreign product process.

LLMs speak System 1 fluently — including when wrong. This doc is **hygiene, not a cure**: checklists and numeric friction so System 2 enters where System 1 is expensive. Gain is variance reduction over months, not one perfect answer.

Spec pipeline that *uses* these disciplines: [`docs/SSDV3-PROMPTS.md`](../SSDV3-PROMPTS.md). Do not restate PRD/SPEC skeletons here.

---

## Master table (#1–#18)

| # | Discipline | Rule (1 line) | RamShared example | Signal |
| --- | --- | --- | --- | --- |
| 1 | WYSIATI | State what was **not** seen before opining | “Without pageout under >50 concurrent, estimate X @ Y%” | Opens with “without having seen Z…” |
| 2 | Counterfactual | Numeric/observable rollback on non-trivial decisions | `Rollback trigger: interrupt latency >50µs in dmesg → revert` | Commit/ADR/SPEC has real trigger |
| 3 | Number, not adjective | Lead with metrics | “DMA 420ns→98ns, n=3, stddev 4%” not “fast” | Perf claims carry unit + n |
| 4 | Anchoring | Reference class before bottom-up estimates | WSL2/CUDA work anchored on `drm/amdgpu` port (~3× inside view) | Roadmap/ADR cites reference class |
| 5 | Availability | Design worst-case, not only happy path | `DEGRADATION-MATRIX.md`: GPU eviction, OOM under lock, PCIe reset | Matrix updated with critical features |
| 6 | Calibrated confidence | Ranges, not false point precision | “~300 MB/s ±10%”; “65% chance load succeeds” | Claims include bounds |
| 7 | Hindsight | Judge process at decision time, not only outcome | Postmortem: correct process/bad outcome vs fluke success | Template separates process vs result |
| 8 | Planning fallacy | Bottom-up × reference-class multiplier | Inside 1w → adjusted 3w for driver-class work | Both numbers written |
| 9 | Question substitution | Qualitative → measurable criterion | “Safe?” → checkpatch clean, lockdep quiet, kmemleak 0 | Metric paired with claim |
| 10 | Hyperbolic discounting | Pay debt cheap; no `TODO later` | Dead path removed now | No new TODO-later in active paths |
| 11 | Halo (tools) | New dep needs ADR/policy/evidence | Cargo dep cites ADR or this #11 | Dep PR references written justification |
| 12 | Prompt priming | Adversarial/neutral review framing | “What bugs?” not “Looks good?” | Reviews use adversarial ask |
| 13 | Illusion of validity | Effect + refusal **paired** with legitimate pass | ghost `up` fails **and** clean `up` works | Real failure mode tested, not mock-only |
| 14 | Mass-refactor fallacy | Orthogonal slices; never rewrite whole tree | Format one crate; hang audit one subsystem | Atomic commits per slice |
| 15 | Calibrated retry | Retry **only** proven transient | `EAGAIN`/`ETIMEDOUT` reconnect; never `-EINVAL`/checkpatch red | Classify before retry; deterministic fails at attempt 1 |
| 16 | Fail-safe + independent curator | Safe default; cure must not die with resource | Demote works when VRAM full; no unsupervised live WSL2 pressure to “heal” | Exhaustion tests; abort ≠ trigger threshold |
| 17 | Replay idempotency | 2× apply = 1× effect | Second `down`/`swapoff`/`LeaseRelease` no double effect | Test asserts unique state, not “no panic twice” |
| 18 | Right layer + proven sunset | Fix where root lives; remove shim only with class proof | Eviction fixed in demote engine, not sleep-loop; Day-0 sunset with drill | PR cites owning layer; sunset has proof for **this** class |

**#15–#18** are runtime/infra hygiene with kernel / broker / WSL2 examples. Details below.

---

## Details (#1–#14 short · #15–#18 full)

<a id="disc-1"></a>
### 1. WYSIATI

**Bias:** Confident answers from visible context only.  
**Rule:** Before critical architecture, list uninspected surfaces; fetch sources first.  
**Signal:** “Without having inspected X, estimate Y @ Z% confidence.”

<a id="disc-2"></a>
### 2. Mandatory counterfactuals

**Bias:** Opinion without “what would change my mind?” is System 1.  
**Rule:** Non-trivial decisions carry a specific reverse condition (unit + window).  
**Signal:** `Rollback trigger:` in commit body / ADR / SPEC.  
**Invalid:** “if it goes wrong, revert.”

<a id="disc-3"></a>
### 3. Number, not adjective

**Bias:** “Is it fast?” becomes “does it feel fast?”  
**Rule:** Lead with numbers; ban bare “obviously / elegant / faster.”  
**Signal:** Metrics + run count + environment when comparative.

<a id="disc-4"></a>
### 4. Anchoring

**Bias:** First number freezes later estimates.  
**Rule:** Start from a reference class (driver/port effort ~3× inside view for similar work).  
**Signal:** Roadmap/ADR names the class.

<a id="disc-5"></a>
### 5. Availability heuristic

**Bias:** Frequent cases crowd out rare catastrophic ones.  
**Rule:** List rares before design: OOM under lock, PCIe reset, IOMMU fault, host GPU eviction.  
**Signal:** [`DEGRADATION-MATRIX.md`](../reliability/DEGRADATION-MATRIX.md) updated with the feature.

<a id="disc-6"></a>
### 6. Calibrated confidence

**Bias:** Point estimates overstate certainty.  
**Rule:** Ranges and calibrated probabilities.  
**Signal:** Claims carry ± / intervals; stddev ≫ noise band → investigate, don’t accept.

<a id="disc-7"></a>
### 7. Hindsight bias

**Bias:** Outcome quality ≠ process quality.  
**Rule:** Evaluate with information available **then**.  
**Signal:** Postmortems separate process vs outcome (`docs/postmortems/` when used).

<a id="disc-8"></a>
### 8. Planning fallacy

**Bias:** Inside-view task sums are optimistic.  
**Rule:** Adjusted = inside view × reference-class multiplier (see #4).  
**Signal:** Both numbers in plan/roadmap.

<a id="disc-9"></a>
### 9. Substitution of question

**Bias:** Hard question silently replaced by easy one.  
**Rule:** Force a metric: checkpatch/sparse, lockdep, cover %, p99, exit codes.  
**Signal:** Qualitative claim paired with numeric gate (SSDV3 cover/E2E counts).

<a id="disc-10"></a>
### 10. Hyperbolic discounting

**Bias:** “Refactor later” discounts future pain.  
**Rule:** Remove dead path now; no `TODO: later` on active paths.  
**Signal:** Diff free of new TODO-later/FIXME debt dumps.

<a id="disc-11"></a>
### 11. Tooling halo

**Bias:** Worked elsewhere → default here.  
**Rule:** New dependency needs ADR, written policy, or measurable evidence.  
**Signal:** Dep PR cites justification.

<a id="disc-12"></a>
### 12. Prompt priming

**Bias:** Framing changes answers.  
**Rule:** Reviews: “What issues?” not “Ready?”  
**Signal:** Adversarial framing in PR/SSDV3 audit (pair programming can stay softer).

<a id="disc-13"></a>
### 13. Illusion of validity

**Bias:** Green tests encode the same wrong assumption as the code; “file exists” ≠ “protects.”  
**Rule:** Validate purpose and failure mode. Every **refusal** test pairs with **legitimate still passes**. Privileged/destructive paths need real-mode proof (drill/integration), not hermetic mock only.  
**Signal:** Failure-mode tests + paired legitimate path.

<a id="disc-14"></a>
### 14. Mass-refactoring fallacy

**Bias:** System 1 cannot foresee whole-tree rewrite side effects.  
**Rule:** Orthogonal slices (crate/subsystem/pattern); atomic commits per slice.  
**Signal:** No mega `refactor: clean codebase` blobs.

---

<a id="disc-15"></a>
### 15. Calibrated retry (transient ≠ deterministic)

**Bias:** #5 + #13 — “flake, retry” without classification.  
**Rule:** Retry only on **proven transient** signature; deterministic fails at attempt 1.

| Transient (retry OK) | Deterministic (fail-fast) |
| --- | --- |
| `EAGAIN`, `ETIMEDOUT`, brief NBD drop | `-EINVAL` ioctl, bad uAPI size, compile error |
| temporary DMA map busy | checkpatch/sparse red, lockdep design bug |
| short GPU busy backoff | wrong GFP in IRQ, capability denied |

**Signal:** Log classification before re-attempt; real cause on attempt 1.  
**Pairs:** #13, #17 (retry only safe if effect idempotent).

<a id="disc-16"></a>
### 16. Fail-safe default + independent curator

**Bias:** “Mechanism exists ⇒ protects”; error path rare ⇒ undesigned.  
**Rule:**

1. **Safe failure is default** — missing protection fails loud (no silent leak/UAF).  
2. **Cure must not depend on the resource being healthy** — demote/reclaim under full VRAM; host-safety must not use unsupervised live WSL2 pressure (qemu/civm preferred, shared-host watchdog harness when explicit). Abort-threshold ≠ trigger-threshold; measure **live** at the gate.

PR checklist for guards / watchdogs / demote:

1. Test from **exhausted** state  
2. Abort ≠ trigger threshold  
3. Live measurement at decision  
4. Deployed artifact == repo (`BINARY_MATCH` / validation) — existence ≠ active  

**Signal:** Exhaustion-path tests; drills prove effect.  
**Pairs:** #5, `validation.md`, [`.claude/rules/benchmarks.md`](../../.claude/rules/benchmarks.md).

<a id="disc-17"></a>
### 17. Idempotency of replayable effects

**Bias:** Assumes “op runs once”; retry/reconnect/agent re-issue is the common case.  
**Rule:** Behind retry/replay/redelivery: apply twice = apply once (generation counters, target-state no-ops, balanced map/unmap, ioctl success when already configured).  
**Signal:** Test applies **2×** and asserts **unique** state effect.  
**Pairs:** #15, #16.

<a id="disc-18"></a>
### 18. Right-layer root cause + proven sunset

**Bias:** #9 both ways — “make it pass now” glues wrong layer; Day-0 cleanup deletes only defense of an uncovered class.  
**Rule:**

1. Fix where the root lives (eviction → demote engine; uAPI layout → header+handler; thrash → safety/civm policy).  
2. Never reconstruct authoritative identity downstream of a lossy transform.  
3. Sunset workarounds only with proof the source covers **this** failure class (two failures → two proofs; multi-lens before deleting “redundant” paths).

**Signal:** PR cites owning layer; shim removal cites test/drill for this class; Day-0 exceptions list reason + deadline + rollback.  
**Pairs:** Day-0 in SSDV3, #13, #16.

---

## Disciplines → test evidence (SSDV3)

Kahneman does **not** replace cover ≥80%. It defines **what kind of proof** a test must carry. Use in SPEC Kahneman blocks / IMPL close. Cover gates: [`docs/SSDV3-PROMPTS.md`](../SSDV3-PROMPTS.md) Step 3.

| # | When SPEC cites it | Minimum proof (beyond cover) | Abort if… |
| --- | --- | --- | --- |
| **#9** | “quality / ready / validated” qualitative | Number: exit code, cover %, priority order, `used_kb`, p99 | Adjective-only / smoke without assert |
| **#13** | privilege, uAPI refusal, preflight/`capable`, “check exists” | Real effect; refusal **and** legitimate pass | File-exists only; refusal without pair |
| **#15** | NBD/ublk reconnect, DMA map busy, broker retry | Retry only transient class; deterministic fails once | Blind retry |
| **#16** | reclaim, demote, VRAM budget, host-safety, gone device | Starts from **exhaustion** / deny / `-ENODEV` | Happy-path-only guard |
| **#17** | cascade `up`/`down`, swapoff, lease, replayable ioctl | Apply **2×** → unique effect | “Ran twice without error” only |

**Anti-pattern:** high cover on pure parsers in a crate, zero proof on cascade refusal, lock order, or live drill — still fails #13 and SSDV3 Step 3.

---

## Discipline → RamShared rubric

| # | Where the signal is checked |
| --- | --- |
| 1 | PRD fact tags; SPEC “without inspecting…” |
| 2 | Commit `Rollback trigger:`; ADR; SPEC abort |
| 3–6, 9 | SSDV3 validation + [`benchmarks.md`](../../.claude/rules/benchmarks.md) for P0 |
| 5 | `docs/reliability/DEGRADATION-MATRIX.md` |
| 7 | `docs/postmortems/` when used |
| 10–12 | Review / coding rules |
| 13–17 | SPEC test matrix + live drills / `validation.md` |
| 15–16 | Host-safety scripts; no unsupervised pressure on live WSL2 |
| 18 | Day-0 in SSDV3; multi-lens before shim delete |
| Hang-class | [`superprompt.md`](../../superprompt.md) (audit, not second SSDV3) |

Shared rails: `.claude/rules/{coding,kernel,security,benchmarks,ssdv3,governance}.md`, `validation.md`, `docs/decisions/`.

---

## Counterfactual as gatekeeper

If you cannot answer **“what would make me change my mind?”** with a specific unit/window, stop — System 1.

| Valid | Invalid |
| --- | --- |
| “p99 +>5% over 3 benches → revert” | “if broken, revert” |
| “lockdep splat in drill → revert” | “depends” |
| “BINARY_MATCH fails after deploy → do not close IMPL” | “when things change” |

---

## Counterexamples (discipline as cargo cult)

| Pattern | # | Symptom | Mitigation |
| --- | --- | --- | --- |
| Form without content | 2, 3, 6 | Empty rollback; metric without env/n | Refuse non-numeric triggers; record env |
| Over-engineering | 5, 8 | Designing for fantasy rares at huge cost | Prioritize probability × impact in matrix |
| Unjustified friction | 10–12 | Scope creep / NIH / hostile pair tone | Diff-only TODO; light ADR for deps; framing by context |
| Nuance killed | 1, 4, 7, 9 | Paralysis listing all ignorance; metric kills valid qualitative NFR | Cap inference ~30%; reference class from driver/port work (e.g. `drm/amdgpu`), not unrelated products |
| Drill fetish | 13 | Full live cascade drill for pure `plan_*` helpers with no host effect | Real-mode drill for privileged/destructive boundaries; unit for pure logic |
| Slice paralysis | 14 | 1-file PRs forever | Slice = orthogonal crate/subsystem (e.g. only `ramshared-cli` cascade), tracked |
| Retry theater | 15 | N NBD reconnects hide `-EINVAL` ioctl | Classify first |
| Guard theater | 16 | “demote code exists” never drilled from full VRAM | Exhaustion test + BINARY_MATCH / health |
| Double-apply | 17 | Retry “saved” run double-swapon | 2× unique-effect test |
| Wrong-layer glue / reckless sunset | 18 | userspace sleep-loop for WDDM eviction; Day-0 delete of only defense | Fix in demote/owning layer + class proof |

**Meta:** triggers written but never executed when condition fires = cargo cult. Capture firings (or justified non-fire) in postmortem/`validation.md`.

---

## Noise reduction

Unwanted variance in judgments. Same model + slightly different prompt → different structure. Combat with **rubrics** (fixed procedure), not freeform opinion.

| Procedure | Rubric |
| --- | --- |
| Spec-driven work | [`docs/SSDV3-PROMPTS.md`](../SSDV3-PROMPTS.md), `docs/specs/`, `docs/INDEX.md` |
| Code quality | `.claude/rules/coding.md` |
| Kernel | `.claude/rules/kernel.md` |
| Security | `.claude/rules/security.md` |
| Docs | `.claude/rules/documentation.md` |
| Benchmarks / host safety | `.claude/rules/benchmarks.md` |
| Empirical log | [`validation.md`](../../validation.md) |

**Signal:** successive similar features produce similarly shaped PRs/SPECs.

---

## How agents use this doc

1. **Before opinion:** uninspected surfaces (#1).  
2. **With opinion:** number (#3), range (#6).  
3. **After:** numeric counterfactual (#2).  
4. **Retry/reconnect:** #15 + #17.  
5. **Reclaim/demote/watchdog/host safety:** #16.  
6. **Shim / dual-path:** #18 + Day-0.  
7. **SSDV3 critical ITEM:** Kahneman block with **executable** evidence (see test table above).

### Auto-application (rollback of this doc)

- **Adoption:** non-trivial PRs cite a discipline in body, review, ADR, or SPEC map.  
- **Doc rollback:** after 6 months if &lt;30% of non-trivial structural PRs cite any discipline → simplify to Top-5 + 1-pager via superseding ADR (cargo-cult detection).

### Non-scope

Does not guarantee quality; does not replace human review; not philosophy — each rule has an observable signal.
