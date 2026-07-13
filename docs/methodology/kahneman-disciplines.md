# Kahneman Disciplines — Cognitive Hygiene for AI Decisions

Disciplines derived from **Thinking, Fast and Slow** and **Noise** (Kahneman, Sibony, Sunstein) applied to the AI-assisted development loop in this monorepo.

**Why this exists:** LLMs speak System 1 with extreme fluency — they articulate ideas convincingly even when they are incorrect. The human developer must act as System 2: verifying, measuring, and questioning what seems obvious. This document introduces structural friction (checklists, numerical rubrics, mandatory counterfactuals) that forces System 2 analytical thinking into the loop at critical moments where System 1 errors would be costly.

Kahneman makes it explicit: biases and noise are properties of the system, not removable bugs. What this document provides is **hygiene, not a cure**. The performance gain does not manifest in a single decision — it shows as variance reduction over several months.

---

## The 18 Operational Disciplines

Each entry starts with the bias it combats, details the operational rule, and specifies the measurable signal indicating compliance.

Disciplines **#15–#18** cover runtime/infra hygiene and are written with **kernel / broker / WSL2** examples.

### Master Table (Consolidated View)

| #   | Discipline               | Operational Rule (1 line)                                                                             | RamShared Example                                                                                                                         | Observable Signal                                                                                                             |
| --- | ------------------------ | ----------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| 1   | WYSIATI                  | Explicitly declare what has not been seen before offering an opinion.                                 | "Without testing pageout under concurrency > 50 req/s, I estimate performance X with Y% confidence."                                      | Responses start with "Without having seen Z..."                                                                               |
| 2   | Counterfactual           | Mandatory numerical rollback trigger for every non-trivial decision.                                  | "If interrupt latency > 50us in dmesg, revert the patch" stated in the commit body.                                                        | Commits/ADRs contain numerical rollback conditions.                                                                           |
| 3   | Number, Not Adjective    | Always lead with raw metrics rather than qualitative adjectives.                                     | "DMA latency reduced from 420ns -> 98ns across 3 runs, stddev 4%" instead of "fast".                                                      | No performance claims allowed without metrics and run counts.                                                                 |
| 4   | Anchoring                | Declare a reference class before building bottom-up estimates.                                        | WSL2 CUDA port anchored against `drm/amdgpu` porting effort (~3x original bottom-up estimate).                                            | Roadmap/ADR references an external reference class repository.                                                               |
| 5   | Availability Heuristic   | Design for the worst-case scenario, not the happy path.                                               | `docs/reliability/DEGRADATION-MATRIX.md` covering host GPU eviction, OOM Killer invocation, and PCIe bus resets.                          | Degradation matrix exists and was updated with the latest feature.                                                           |
| 6   | Calibrated Confidence    | State confidence intervals instead of single-point targets.                                           | "~300 MB/s ±10%" instead of "300 MB/s"; "65% probability of successful load" instead of "it will work".                                  | Numerical claims include standard deviations or confidence intervals.                                                         |
| 7   | Hindsight Bias           | Postmortems must decouple process quality from outcome success.                                       | `docs/postmortems/` distinguishing "correct process, bad outcome" vs. "fluke success".                                                    | Postmortem templates separate process analysis from ultimate outcomes.                                                       |
| 8   | Planning Fallacy         | Multiply bottom-up estimates by reference class overrun ratios.                                       | "Inside view estimate: 1 week; adjusted by reference class multiplier (3x): 3 weeks."                                                     | Roadmap lists both inside-view and adjusted estimates.                                                                       |
| 9   | Substitution of Question | Translate qualitative evaluations into quantifiable metrics.                                           | "Good architecture?" -> 0 warnings in checkpatch, no deadlocks under lockdep, 0 leaks in kmemleak.                                        | Qualitative questions are paired with quantitative criteria.                                                                 |
| 10  | Hyperbolic Discounting   | Refactor debt while it is cheap; reject deferred TODOs.                                               | `grep -r "TODO.*later\|FIXME"` returns no matches in active code paths.                                                                   | Grep checks remain clean in diff reviews.                                                                                    |
| 11  | Halo Effect              | Every new external library must reference a justifying ADR or policy.                                 | New dependency requires referencing an ADR or this specific discipline (#11).                                                             | Dependency updates point to written ADRs.                                                                                     |
| 12  | Prompt Priming           | Frame prompts neutrally or adversarially.                                                             | "What bugs do you find in this code?" instead of "Does this look correct?".                                                               | Pull request reviews use adversarial framing templates.                                                                      |
| 13  | Illusion of Validity     | Test boundaries must validate failures and refusal paths, not just existence or happy paths.          | Paging refusal test paired with "does a legitimate input still pass?" to prevent false-positives under privileged boundaries.               | Destructive tools have integration tests executing the actual failure mode.                                                   |
| 14  | Mass-Refactoring Fallacy | Deconstruct codebase cleanup into orthogonal atomic slices. Never rewrite a repository all at once.    | "Apply formatting rules only to the CUDA wrapper" instead of a repository-wide `refactor: clean codebase`.                                | Refactoring patches contain atomic commits segmented by crate/service.                                                       |
| 15  | Calibrated Retry         | Retry only with a **proven transient** signature; deterministic failures fail-fast on attempt 1.       | NBD reconnect / DMA map retry only on `EAGAIN`/`ETIMEDOUT`; never retry `-EINVAL` ioctl or compile/checkpatch red.                        | Retry loops classify outcome before re-attempt; first deterministic fail logs the real cause.                                 |
| 16  | Fail-safe + Independent Curator | Safe failure mode is the **default**; the cure mechanism must not die with the resource it cures. | Demote/reclaim path must work when VRAM is already exhausted; host-safety scripts must not thrash WSL2 live to "heal" pressure.             | Guards have tests from the **exhausted** state; abort-threshold ≠ trigger-threshold; live measurement at decision time.       |
| 17  | Replay Idempotency       | Every effect behind retry/replay/command re-delivery is idempotent (2× = 1×).                        | Broker `SwapOn`/`DemoteAll`/`LeaseRelease` re-issued after timeout produces one state transition, not double slice.                      | Replayable ops have a 2× apply test asserting **unique** effect (not just "ran twice without panic").                        |
| 18  | Right-Layer Root Cause + Proven Sunset | Fix in the layer that owns the root; remove workarounds only with proof the source covers **this** class. | WDDM eviction fixed in demote engine, not a userspace sleep-loop; dual-path shim removed only with Day-0 proof + multi-lens audit.         | Fix path cites owning layer; every shim sunset carries commit/test proof for **this** failure class.                          |

---

<a id="disc-1"></a>
### 1. WYSIATI — What You See Is All There Is

**Bias:** AI models respond with high confidence based only on immediately visible context, hallucinating by omission.

**Rule:** Before making a critical architectural decision, explicitly list what has NOT been inspected. Request required source files before analyzing a subsystem.

**Signal:** Responses begin with: *"Without having inspected X, I estimate Y with Z% confidence."*

<a id="disc-2"></a>
### 2. Mandatory Counterfactuals

**Bias:** Architectural opinions without rollback criteria are System 1 intuitions disguised as reasoning.

**Rule:** Every decision must carry a clear answer to: *"What would make me change my mind?"*. Valid answers are specific: *"If p99 latency > 500ms, revert"* or *"If memory allocation overhead exceeds 10%, rollback"*.

**Signal:** Non-trivial commits contain a `Rollback trigger:` line in the commit body.

<a id="disc-3"></a>
### 3. Number, Not Adjective

**Bias:** Question substitution: "Is this system fast?" (hard) becomes "Does it feel fast?" (easy).

**Rule:** Claims must lead with numbers, not adjectives. Bad: *"The driver is much faster"*. Good: *"Latency decreased from 420ns to 98ns across 3 iterations, stddev 4%"*.

**Signal:** Avoid terms like "obviously", "definitely", or "elegant" unless paired with a qualifying metric.

<a id="disc-4"></a>
### 4. Estimating via Anchoring

**Bias:** The first number introduced anchors all subsequent estimates.

**Rule:** Project estimates must begin with a **reference class**, not a bottom-up task list. The baseline reference for this monorepo is the `drm/amdgpu` driver Rust porting effort (~3x overrun on initial bottom-up estimates).

**Signal:** Roadmap logs state reference classes explicitly.

<a id="disc-5"></a>
### 5. Availability Heuristic

**Bias:** AI models recall frequent events, omitting rare but catastrophic hardware failures.

**Rule:** Explicitly list rare conditions before deciding on a design path: OOM killer during lock contention, PCIe bus resets, IOMMU translation faults, and VM host memory evictions.

**Signal:** The [DEGRADATION-MATRIX.md](../reliability/DEGRADATION-MATRIX.md) is updated alongside critical features.

<a id="disc-6"></a>
### 6. Calibrated Confidence

**Bias:** Stating exact values triggers overconfidence. Ranges represent reality.

**Rule:** Numerical estimates must carry error bounds (e.g., *"~300 MB/s ±10%"*). Probabilities must be calibrated (e.g., *"65% chance of passing integration tests"*).

**Signal:** Technical claims lead with ranges rather than point values.

<a id="disc-7"></a>
### 7. Hindsight Bias

**Bias:** A good outcome implies a good decision process. A bad outcome implies a bad process. Both are false.

**Rule:** Evaluate decisions based on the **process and information available at the time**, not the final outcome. *"Correct process, failed outcome"* is acceptable; *"fluke success"* is a process alarm.

**Signal:** Postmortems in `/docs/postmortems/` separate process evaluations from final outcomes.

<a id="disc-8"></a>
### 8. Planning Fallacy

**Bias:** Inside view estimates (bottom-up task tracking) are systematically optimistic.

**Rule:** Multiply bottom-up estimates by the reference class overrun factor (3x for device driver subsystems).

**Signal:** Roadmaps cite both inside-view and reference-class-adjusted timelines.

<a id="disc-9"></a>
### 9. Substitution of Question

**Bias:** Complex questions are substituted for simpler ones without conscious realization.

**Rule:** Translate qualitative questions into objective metrics. "Is this driver safe?" becomes "0 warnings under sparse/checkpatch, no deadlock warnings under lockdep, 0 leaks under kmemleak".

**Signal:** Qualitative claims are followed by measurable verification criteria.

<a id="disc-10"></a>
### 10. Technical Debt Hyperbolic Discounting

**Bias:** "I will refactor this later" heavily discounts future maintenance costs.

**Rule:** Refactor debt while it is cheap, not when it fails. Remove dead code immediately rather than leaving a `TODO: clean up later` comment.

**Signal:** `grep -r "TODO.*later\|FIXME"` returns no matches in active code paths.

<a id="disc-11"></a>
### 11. Tooling Halo Effect

**Bias:** "This tool worked on project A, so it must be the default for project B."

**Rule:** Every new library, dependency, or framework must reference a written policy or ADR.

**Signal:** Dependency additions reference a written ADR.

<a id="disc-12"></a>
### 12. Prompt Priming

**Bias:** Prompt framing alters output. "What bugs are in this code?" yield different results than "Is this code correct?".

**Rule:** Use adversarial framing for reviews: *"What issues do you find in this implementation?"* instead of *"Does this look ready?"*.

**Signal:** Pull request reviews follow adversarial templates.

<a id="disc-13"></a>
### 13. Illusion of Validity

**Bias:** Happy path tests provide false confidence if they encode the same assumptions as the implementation.

**Rule:** Every refusal path verification must be paired with a validation of a legitimate input. Destructive or privileged boundaries (sudo, rm, systemd) require integration tests simulating the actual failure mode.

**Signal:** Integration tests validate real failure scenarios, not just mocks.

<a id="disc-14"></a>
### 14. Mass-Refactoring Fallacy

**Bias:** The assumption that System 1 can predict all cascading side-effects of a global codebase rewrite.

**Rule:** Restructuring must be fatiated (sliced) orthogonally. Do not request global codebase cleanups; restrict changes to specific crates or modules. Each slice must map to an atomic commit.

**Signal:** Cleanups contain atomic commits segmented by crate.

<a id="disc-15"></a>
### 15. Calibrated Retry (transient ≠ deterministic)

**Bias:** Availability heuristic (#5) + Illusion of validity (#13). System 1 reaches for the familiar story — "flake, retry" — without classifying the failure. A retry that "sometimes passes" becomes false proof of health and buries the real bug.

**Rule:** A retry loop may re-attempt only with a **proven transient signature**. Deterministic failures fail-fast on attempt 1:

| Transient (retry OK) | Deterministic (fail-fast) |
| --- | --- |
| `EAGAIN`, `ETIMEDOUT`, brief NBD disconnect | `-EINVAL` ioctl, bad uAPI size, compile error |
| temporary DMA map busy under pressure | checkpatch/sparse red, lockdep splat design bug |
| host GPU busy → short backoff before re-probe | wrong GFP in IRQ, capability denied |

Never mask a config/logic bug behind N reconnects of the broker or ublk client.

**Signal:** Retry paths log the classification before re-attempt; deterministic failures appear on attempt 1 with the real cause (dmesg/`dev_err`/test output), not under N retries.

**Pairs with:** #13 (exists ≠ works), #17 (retry is only safe if the effect is idempotent).

<a id="disc-16"></a>
### 16. Fail-safe Default + Independent Curator

**Bias:** Illusion of validity (#13) + Availability (#5). System 1 assumes "the mechanism exists, therefore it protects" and that the error path is too rare to design. Three failures are the same family: the curator that dies with the resource it should heal; blind retry that masks exhaustion; a guard that, when forgotten, leaks silently instead of failing loud.

**Rule — two coupled invariants:**

1. **Safe failure is the default.** Forgetting a protection fails loud, never silent leak. Example: privileged ioctl without `capable()` must refuse; device gone mid-I/O must return `-ENODEV`, not UAF.
2. **The cure must not depend on the resource being healthy.** Reclaim/demote must still run when VRAM is already full; host-safety automation must not thrash the live WSL2 host to "prove" pressure (use qemu/civm). Abort-threshold ≠ trigger-threshold; the measurement at the gate is **live**, not a stale cache.

PR checklist for every resource guard / watchdog / demote path:

1. Test starts from the **exhausted** state (not only happy path).
2. Abort-threshold ≠ trigger-threshold.
3. Decision measurement is live at gate time.
4. "Code exists" ≠ "protection active" on the target under test (`validation.md` / drill).

**Signal:** Guards have exhaustion-path tests; demote/reclaim works under pressure; host-safety scripts refuse live thrash.

**Pairs with:** #5 (DEGRADATION-MATRIX), `validation.md` tag `fail-safe`, `.claude/rules/benchmarks.md` host-safety.

<a id="disc-17"></a>
### 17. Idempotency of Replayable Effects

**Bias:** Availability (#5) + happy-path optimism. System 1 assumes "the op runs once" and ignores that retry (#15), command re-delivery, agent reconnect, and operator double-click are the **common** case.

**Rule:** Every effect behind retry/replay/redelivery must be idempotent: apply twice = apply once. Mechanisms:

- Command id / generation counter on broker ops (`SwapOn`, `DemoteAll`, `LeaseRelease`)
- State machine transitions that are no-ops when already in the target state
- DMA map/unmap balanced so a second cleanup is safe
- `ioctl` that returns success (or stable errno) when the resource is already configured as requested

A non-idempotent side effect behind a "correct" retry is a latent double-slice / double-free / double-swapon bug.

**Signal:** Replayable ops have a test that applies the effect **twice** and asserts a **unique** outcome (not merely "no panic twice").

**Pairs with:** #15 (retry only transient) and #16 (fail-safe).

<a id="disc-18"></a>
### 18. Right-Layer Root Cause + Proven Workaround Sunset

**Bias:** Attribute substitution (#9) in both directions. Under pressure, System 1 swaps "where does the root live?" for "how do I make it pass now?" and glues a band-aid in the wrong layer. The inverse is also System 1: Day-0 cleanup swaps "is it **proven** redundant?" for "it **looks** like a shim" and deletes the only defense of an uncovered class.

**Rule:**

1. **Fix in the layer that owns the root.** Examples:
   - WDDM eviction latency → demote/canary engine (not a userspace `sleep` loop)
   - uAPI layout bug → header + kernel handler together (not a client-only cast)
   - Host thrash risk → safety script / civm policy (not a product dual-path)
2. **Never reconstruct authoritative identity downstream** of a lossy transform (handle/offset must be validated at the boundary that owns the object).
3. **Sunset workarounds only with proof** that the source fix covers **this** failure class. Two distinct failures need two proofs. Multi-lens audit (≥2 perspectives) before deleting "redundant" paths.

**Signal:** Fix PRs cite the owning layer; every shim removal cites test/drill proof for this class; Day-0 exceptions in SPEC list reason + removal deadline + rollback.

**Pairs with:** Day-0 policy in `docs/SSDV3-PROMPTS.md`, #13, #16.

---

## Noise Reduction

**Noise** is unwanted random variation in professional judgments. The same LLM can produce different outputs under slight variations of the same prompt. We combat noise using **rubrics** — structured guidelines that translate judgment into procedure.

### Active Rubrics

| System | Rubric |
| --- | --- |
| Spec-driven development | [`docs/SSDV3-PROMPTS.md`](../SSDV3-PROMPTS.md), [`docs/specs/`](../specs/), [`docs/INDEX.md`](../INDEX.md) |
| Code quality | `.claude/rules/coding.md` |
| Driver development | `.claude/rules/kernel.md` |
| Documentation | `.claude/rules/documentation.md` |
| Benchmarks / host safety | `.claude/rules/benchmarks.md` |
| Empirical validation log | [`validation.md`](../../validation.md) |

**Signal of Noise Reduction:** Successive implementations of similar patterns yield uniform structures.

### How agents should use this doc

1. **Before opining:** declare what was not inspected (#1).
2. **With the opinion:** number, not adjective (#3); calibrated range (#6).
3. **After the opinion:** numerical counterfactual / rollback trigger (#2).
4. **On retry/reconnect/command paths:** #15 + #17.
5. **On reclaim/demote/watchdog/host safety:** #16.
6. **On shims and "quick" dual-path:** #18 + Day-0.

### Auto-application (rollback of this doc)

- **Adoption signal:** non-trivial PRs cite a discipline (#1–#18) in body, review, ADR, or SPEC Kahneman map.
- **Doc rollback trigger:** if after 6 months <30% of non-trivial structural PRs cite any discipline, simplify to Top-5 + 1-pager via superseding ADR (cargo-cult detection).
