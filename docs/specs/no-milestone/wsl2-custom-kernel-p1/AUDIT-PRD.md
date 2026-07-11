# AUDIT-PRD — wsl2-custom-kernel-p1 (Kahneman + SSDV3)

> **Date:** 2026-07-10  
> **Object:** `PRD.md` (UX-revised)  
> **Question:** Ready to write `SPEC.md`?  
> **Verdict:** **CONDITIONAL GO → GO after PRD patch** (gaps below must land in PRD or be explicitly deferred as SPEC freezes with no product ambiguity).

---

## 1. SSDV3 shape check (14 sections + discovery)

| Check | Result |
| --- | --- |
| Frontmatter slug/title/issues | OK |
| Summary + GO | OK |
| Fact vs inference labeled | OK (§2) |
| UX law binding (§0) | OK — critical |
| RF / NFR IDs | OK (RF-K1..17, NFR-K1..9) |
| Flows | OK (6.0–6.4) |
| Out of scope | OK |
| Acceptance + validation | OK |
| Rollback numerical | **Partial** — apply timeout not pinned to a number; enable &lt;30s OK |
| Abuse cases (TOCTOU, privilege, fail-open) | **Weak** — SPEC must expand; PRD needs one abuse row |
| Inference ratio | Low enough (&lt;30% of binding decisions) |

---

## 2. Kahneman #1–#18 map

| # | Discipline | PRD status | Finding |
| --- | --- | --- | --- |
| 1 | WYSIATI | **PASS** | Declares unseen: bzImage not ready, interop flaky, modules VHDX path inference |
| 2 | Counterfactual | **PASS+** | QEMU fail → no apply; apply fail → restore `.wslconfig`; enable hang &gt;30s = bug |
| 3 | Number not adjective | **PARTIAL** | 30s enable bound good; READY criteria not fully quantitative (uname pattern TBD); no apply timeout seconds |
| 4 | Anchoring | **PASS** | Anchored on MS tree + existing boot-kernel-safe / FASE-B, not greenfield |
| 5 | Availability / worst case | **PARTIAL** | Boot brick + shutdown covered; **missing explicit**: modules path empty after boot; half-armed `.wslconfig`; concurrent arm from two shells |
| 6 | Calibrated confidence | **PASS** | MS stock enablement explicitly non-gate; no false certainty on merge |
| 7 | Hindsight | N/A (pre-impl) | — |
| 8 | Planning fallacy | **WEAK** | No inside-view vs 3× estimate for build/CLI; not blocking SPEC if SPEC phases work |
| 9 | Substitution | **PASS** | “Safe” → no shutdown, &lt;30s, exit codes, qemu PASS |
| 10 | Debt discounting | **PASS** | No “TODO later” dual-path as product; NBD Day-1 kept explicit |
| 11 | Halo | **PASS** | Custom kernel ≠ VRAM-as-RAM; enable ≠ cascade rewrite (#13 style) |
| 12 | Prompt priming | N/A | Audit used adversarial frame |
| 13 | Illusion of validity | **PASS** | enable no-op tested twice; grep enable must not call shutdown; apply failure path |
| 14 | Mass refactor | **PASS** | Min config only; no mega config-wsl rewrite |
| 15 | Calibrated retry | **PARTIAL** | Fail-fast interop stated; no “retry only on X” for modprobe/apply — SPEC |
| 16 | Fail-safe curator | **PASS** | enable default safe; apply auto-revert; curator (revert) independent of custom kernel health |
| 17 | Idempotency | **PASS** | RF-K16 arm/enable 2× = 1× |
| 18 | Right layer | **PASS** | Kconfig not fake LKM; shutdown only at WSL lifecycle layer |

### Score (binding disciplines for this PRD)

| Band | Count |
| --- | --- |
| PASS | 12 |
| PARTIAL / WEAK | 4 |
| FAIL | 0 |

**No FAIL.** PARTIALs are closable in PRD patch + SPEC freezes.

---

## 3. Product / UX consistency check

| Claim | Consistent? |
| --- | --- |
| enable never shutdown | Yes RF-K14 / NFR-K9 |
| Permanent custom kernel after arm + restart | Yes (user understanding) |
| Cannot switch kernel with WSL already on other kernel without restart | Yes §0.2 / §2.3 |
| “Do nothing when ready” | Yes RF-K14 / A6 |

**No internal contradiction.**

---

## 4. Gaps that block a clean SPEC (must fix in PRD)

| ID | Gap | Severity | Action |
| --- | --- | --- | --- |
| G1 | **READY / NEED_*** not formally defined as a state machine | HIGH | Add §3.2.1 state table (probe → state) |
| G2 | **arm without artifact** could write dead path | HIGH | RF: arm requires bzImage exists (size &gt; 0); apply still requires qemu PASS |
| G3 | **apply timeout** not numerical | MED | Pin default (e.g. boot-kernel-safe existing timeout or 120s) in PRD validation |
| G4 | **Abuse / race** (double arm, partial write `.wslconfig`) | MED | One NFR or RF: atomic write + single backup path |
| G5 | **Who runs CLI** (product Ubuntu vs lab) | LOW | Clarify: daily enable on **product** distro; build on lab |

Gaps G1–G4 patched into PRD in the same turn as this audit.

---

## 5. Explicitly deferred to SPEC (not PRD defects)

- Exact CLI path/name freeze  
- Exact uname/release match string  
- How to read `.wslconfig` from WSL (interop vs `/mnt/c/Users/...`)  
- modules_install vs modules.vhdx procedure  
- Full context matrix for scripts (process only)  
- Unit test list for shell  

---

## 6. Abuse cases (minimum for PRD; expand in SPEC)

| Abuse | Required behavior |
| --- | --- |
| `enable` while stock | exit 2, no shutdown, no thrash |
| `apply` without flag | refuse |
| `arm` with missing bzImage | refuse |
| Interop down | exit 3 + copy-paste Windows command, &lt;5s |
| apply boot fail | restore clean config; stock returns |

---

## 7. Verdict

| Question | Answer |
| --- | --- |
| PRD solid enough for SPEC after G1–G4 patch? | **YES — GO to SPEC** |
| Write SPEC before patching G1–G4? | **NO** |
| AUDIT-2.5 now? | **NO** — 2.5 is on **SPEC** (apply path), not on PRD |
| IMPL now? | **NO** |

**Pipeline:** PRD (patched) → **SPEC** → **AUDIT-2.5** (before first apply) → IMPL.
