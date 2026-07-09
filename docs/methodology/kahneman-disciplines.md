# Kahneman Disciplines — Cognitive Hygiene for AI Decisions

Disciplines derived from **Thinking, Fast and Slow** and **Noise** (Kahneman, Sibony, Sunstein) applied to the AI-assisted development loop in this monorepo.

**Why this exists:** LLMs speak System 1 with extreme fluency — they articulate ideas convincingly even when they are incorrect. The human developer must act as System 2: verifying, measuring, and questioning what seems obvious. This document introduces structural friction (checklists, numerical rubrics, mandatory counterfactuals) that forces System 2 analytical thinking into the loop at critical moments where System 1 errors would be costly.

Kahneman makes it explicit: biases and noise are properties of the system, not removable bugs. What this document provides is **hygiene, not a cure**. The performance gain does not manifest in a single decision — it shows as variance reduction over several months.

---

## The 14 Operational Disciplines

Each entry starts with the bias it combats, details the operational rule, and specifies the measurable signal indicating compliance.

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

---

### 1. WYSIATI — What You See Is All There Is

**Bias:** AI models respond with high confidence based only on immediately visible context, hallucinating by omission.

**Rule:** Before making a critical architectural decision, explicitly list what has NOT been inspected. Request required source files before analyzing a subsystem.

**Signal:** Responses begin with: *"Without having inspected X, I estimate Y with Z% confidence."*

### 2. Mandatory Counterfactuals

**Bias:** Architectural opinions without rollback criteria are System 1 intuitions disguised as reasoning.

**Rule:** Every decision must carry a clear answer to: *"What would make me change my mind?"*. Valid answers are specific: *"If p99 latency > 500ms, revert"* or *"If memory allocation overhead exceeds 10%, rollback"*.

**Signal:** Non-trivial commits contain a `Rollback trigger:` line in the commit body.

### 3. Number, Not Adjective

**Bias:** Question substitution: "Is this system fast?" (hard) becomes "Does it feel fast?" (easy).

**Rule:** Claims must lead with numbers, not adjectives. Bad: *"The driver is much faster"*. Good: *"Latency decreased from 420ns to 98ns across 3 iterations, stddev 4%"*.

**Signal:** Avoid terms like "obviously", "definitely", or "elegant" unless paired with a qualifying metric.

### 4. Estimating via Anchoring

**Bias:** The first number introduced anchors all subsequent estimates.

**Rule:** Project estimates must begin with a **reference class**, not a bottom-up task list. The baseline reference for this monorepo is the `drm/amdgpu` driver Rust porting effort (~3x overrun on initial bottom-up estimates).

**Signal:** Roadmap logs state reference classes explicitly.

### 5. Availability Heuristic

**Bias:** AI models recall frequent events, omitting rare but catastrophic hardware failures.

**Rule:** Explicitly list rare conditions before deciding on a design path: OOM killer during lock contention, PCIe bus resets, IOMMU translation faults, and VM host memory evictions.

**Signal:** The [DEGRADATION-MATRIX.md](../reliability/DEGRADATION-MATRIX.md) is updated alongside critical features.

### 6. Calibrated Confidence

**Bias:** Stating exact values triggers overconfidence. Ranges represent reality.

**Rule:** Numerical estimates must carry error bounds (e.g., *"~300 MB/s ±10%"*). Probabilities must be calibrated (e.g., *"65% chance of passing integration tests"*).

**Signal:** Technical claims lead with ranges rather than point values.

### 7. Hindsight Bias

**Bias:** A good outcome implies a good decision process. A bad outcome implies a bad process. Both are false.

**Rule:** Evaluate decisions based on the **process and information available at the time**, not the final outcome. *"Correct process, failed outcome"* is acceptable; *"fluke success"* is a process alarm.

**Signal:** Postmortems in `/docs/postmortems/` separate process evaluations from final outcomes.

### 8. Planning Fallacy

**Bias:** Inside view estimates (bottom-up task tracking) are systematically optimistic.

**Rule:** Multiply bottom-up estimates by the reference class overrun factor (3x for device driver subsystems).

**Signal:** Roadmaps cite both inside-view and reference-class-adjusted timelines.

### 9. Substitution of Question

**Bias:** Complex questions are substituted for simpler ones without conscious realization.

**Rule:** Translate qualitative questions into objective metrics. "Is this driver safe?" becomes "0 warnings under sparse/checkpatch, no deadlock warnings under lockdep, 0 leaks under kmemleak".

**Signal:** Qualitative claims are followed by measurable verification criteria.

### 10. Technical Debt Hyperbolic Discounting

**Bias:** "I will refactor this later" heavily discounts future maintenance costs.

**Rule:** Refactor debt while it is cheap, not when it fails. Remove dead code immediately rather than leaving a `TODO: clean up later` comment.

**Signal:** `grep -r "TODO.*later\|FIXME"` returns no matches in active code paths.

### 11. Tooling Halo Effect

**Bias:** "This tool worked on project A, so it must be the default for project B."

**Rule:** Every new library, dependency, or framework must reference a written policy or ADR.

**Signal:** Dependency additions reference a written ADR.

### 12. Prompt Priming

**Bias:** Prompt framing alters output. "What bugs are in this code?" yield different results than "Is this code correct?".

**Rule:** Use adversarial framing for reviews: *"What issues do you find in this implementation?"* instead of *"Does this look ready?"*.

**Signal:** Pull request reviews follow adversarial templates.

### 13. Illusion of Validity

**Bias:** Happy path tests provide false confidence if they encode the same assumptions as the implementation.

**Rule:** Every refusal path verification must be paired with a validation of a legitimate input. Destructive or privileged boundaries (sudo, rm, systemd) require integration tests simulating the actual failure mode.

**Signal:** Integration tests validate real failure scenarios, not just mocks.

### 14. Mass-Refactoring Fallacy

**Bias:** The assumption that System 1 can predict all cascading side-effects of a global codebase rewrite.

**Rule:** Restructuring must be fatiated (sliced) orthogonally. Do not request global codebase cleanups; restrict changes to specific crates or modules. Each slice must map to an atomic commit.

**Signal:** Cleanups contain atomic commits segmented by crate.

---

## Noise Reduction

**Noise** is unwanted random variation in professional judgments. The same LLM can produce different outputs under slight variations of the same prompt. We combat noise using **rubrics** — structured guidelines that translate judgment into procedure.

### Active Rubrics

| System | Rubric |
| --- | --- |
| Spec-driven development | [docs/specs/](file:///home/emdev/codespace/ramshared/docs/specs/) templates |
| Code quality | `.claude/rules/coding.md` |
| Driver development | `.claude/rules/kernel.md` |
| Security | `.claude/rules/security.md` |

**Signal of Noise Reduction:** Successive implementations of similar patterns yield uniform structures.
