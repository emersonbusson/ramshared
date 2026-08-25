# AUDIT-2.5 — cascade-vram-ondemand

> **Passo 2.5 SSDV3** (formal). Risk-gated: CUDA lifetime, swap data integrity, WSL host safety, reclaim races.  
> **Date:** 2026-07-11  
> **Auditor role:** adversarial + security + Kahneman #13/#16.  
> **Inputs:** [`PRD.md`](PRD.md), [`SPEC.md`](SPEC.md), codebase (`ramsharedd` NBD path, `VramProvider`, canary), live cascade behaviour, MS WSL reclaim patterns.

**2026-08-22 sunset note:** this audit preserves the 2026-07-11 risk record.
Its full-allocation rollback decision is superseded: the executable selector
and NBD full-VRAM composition were removed, and the SSD-authoritative origin
cache is the only product NBD path. No live result is inferred from that source
closure.

---

## Decision (top)

| Path | Verdict |
| --- | --- |
| MVP: sparse alloc-on-write + free only when nbd `used_kb==0` | **GO** |
| Preflight sparse gate (not full VRAM_MIB free at boot) | **GO** (SPEC revised this pass) |
| Free CUDA while `used_kb > 0` | **NO-GO** (MVP) |
| ITEM-2b mid-flight spill | **NO-GO** (needs new PRD/SPEC/2.5) |
| Product ublk on WSL2 | **NO-GO** (unchanged parent) |
| Historical full-allocation kill-switch | **SUPERSEDED** — no environment selector can restore the removed NBD composition. |

### Overall

The original verdict was **GO** for Passo 3 of the sparse MVP. The current
source-removal verdict is **PASS** for the named static gate only. Live
origin/NBD/GPU qualification remains `PARTIAL`.

---

## 1. Scope under audit

| In | Out |
| --- | --- |
| `SparseVramBackend` + daemon NBD single-worker wire | Kernel/HMM changes |
| Idle / free-floor reclaim when device empty | Page-accurate demote while nbd dirty |
| Telemetry, env flags, tests | MS kernel PR |
| Interaction with cascade boot / orphan recover | Changing priorities zram>nbd>disk |

---

## 2. Threat / abuse / failure model

| ID | Scenario | Sev | Control in SPEC (post-audit) |
| --- | --- | --- | --- |
| T1 | Free Live chunk while guest still has swap pages on nbd → **corruption / hang** | **CRITICAL** | Free only if `used_kb==0` or `down`; never free when used>0 |
| T2 | Reclaim on side thread races with write → free under I/O | **CRITICAL** | Reclaim **only** on CUDA worker thread between jobs |
| T3 | TOCTOU: `used_kb==0` then page-in before free | **HIGH** | Same thread as I/O; re-read after free; no concurrent writers |
| T4 | Alloc-on-write fails mid pressure → silent data loss | **HIGH** | Return `IoError` to NBD; no retry storm (#15) |
| T5 | Preflight requires full 3 GiB free → defeats sparse product | **HIGH** | **FIXED:** sparse gate = headroom + canary + 1 chunk |
| T6 | Historical full-allocation rollback becomes a permanent second path | **MED** | The selector was removed; rollback keeps the product off and repairs the origin path. |
| T7 | Host thrash “to prove reclaim” | **MED** | `cascade-pressure-probe.sh` cgroup only |
| T8 | Cross-chunk I/O bug → wrong data | **HIGH** | Unit tests cross-chunk; split I/O mandatory |
| T9 | Zero-fill path skips alloc but returns garbage | **HIGH** | Empty read → explicit zeros |
| T10 | Info-leak kernel pointers in telemetry | **LOW** | Counters/sizes only |
| T11 | Privilege: unprivileged sparse free | **LOW** | Daemon already root for nbd/swap |

---

## 3. Security checklist (project `security.md` adapted)

| Check | Result |
| --- | --- |
| Privileged surface documented | **OK** — daemon root; no new ioctl |
| User buffer / TOCTOU | **OK** — no new uAPI; NBD path kernel↔daemon |
| Bounds on offsets | **OK** if ITEM-1 splits chunks with range checks |
| IRQ/atomic | **N/A** — userspace process context |
| Lifetime / free balance | **OK** — Drop on chunk; full free on down |
| Hot-unplug / terminate | **OK** — orphan recover parent; sparse does not worsen if used=0 free only |
| Host safety | **OK** — no thrash plan |
| Secrets | **N/A** |

---

## 4. Findings (this Passo 2.5) → SPEC changes

| Sev | Finding | Disposition |
| --- | --- | --- |
| **HIGH** | Preflight `free >= VRAM_MIB + headroom` forced full free at boot and **contradicted** RF-L1 / user “3 GiB capacity without holding 3 GiB” | The 2026-07-11 sparse gate fixed that issue; the later origin-cache design removed the full-allocation mode entirely. |
| **HIGH** | ITEM-2 “demote content” ambiguous; reclaim thread not specified → race with I/O | **SPEC ITEM-2 revised:** worker-thread only; algorithm steps; demote telemetry without free when used>0 |
| **MED** | Canary size not numeric in SPEC | **SPEC:** `CANARY_BYTES = 4096` (confirmed `canary_probe.rs`) |
| **MED** | Live free-delta budget vague vs full size | **SPEC:** idle `up` Δ free ≤ 64 MiB slack |
| **LOW** | GAT ownership of `VramProvider` awkward | IMPL note: same single-thread model as `VramBackend` today; no new Send |
| **LOW** | Early dual reclaim narratives | Already cleaned to single MVP table |

No remaining **CRITICAL** open after SPEC edit.

---

## 5. Codebase confirmation (pre-IMPL)

| Fact | Evidence | Class |
| --- | --- | --- |
| Full allocation at audit time | Historical `main.rs`: `provider.alloc(size)?; mem.zero()?`; removed from current NBD composition | Confirmed historical codebase |
| Canary separate alloc | `CANARY_BYTES = 4096` | Confirmed codebase |
| Single CUDA worker | `jobs_rx` loop owns backend | Confirmed codebase |
| DEMOTE sampler exists | `residency.rs` / canary path | Confirmed codebase |
| Pressure harness exists | `scripts/safety/cascade-pressure-probe.sh` | Confirmed repo |
| Live free returns on `down` | nvidia-smi ~+2 GiB after down (2026-07-11) | Confirmed environment |

---

## 6. Microsoft / WSL alignment (audit)

| MS behaviour | Class | SPEC mapping |
| --- | --- | --- |
| `autoMemoryReclaim` returns unused guest RAM | Confirmed docs | Idle free when `used_kb==0` |
| `sparseVhd` logical ≠ provisioned | Unsafe-lab-only hypothesis; production omits it because WSL 2.7.12 requires a data-corruption-risk override | Capacity vs committed chunks without live sparse conversion |
| `hv_balloon` under host pressure | Confirmed MS kernel tree | Free chunks when free-floor + empty device |
| GPU = GPU-PV / dxgkrnl, not system RAM | Confirmed docs/tree | Userspace only; no MS kernel PR |
| Features: experimental → default | Confirmed docs | Historical sparse rollout only; current product selection is origin-cache without a full-allocation switch |

**Inference (labelled):** MS would not merge CUDA-NBD into stock `config-wsl`; they would ship **sparse commit + reclaim** style policy in userspace/service if at all — matches our layering.

---

## 7. Kahneman map (Passo 2.5)

| # | Question | Evidence required at IMPL | Abort |
| --- | --- | --- | --- |
| #2 | What invalidates the current source removal? | Named scan finds a selector/composition or a retained consumer breaks | Keep product off; repair origin path without restoring the selector |
| #13 | Did we test refuse free when used>0? | Unit/integration must force used>0 mock or live | Missing test → no DONE |
| #15 | Alloc fail retry? | Code review: single fail path | Loop found → no-go merge |
| #16 | Safe default when unsure? | used>0 → no free | Free anyway → CRITICAL |
| #17 | Double free Empty chunk? | Idempotent drop | Panic → fix |
| #18 | Right layer? | Daemon backend, not kernel patch | Kernel-only “fix” → reject |

---

## 8. Atomicity / rollback

| Kind | Behaviour |
| --- | --- |
| Code | Restore only an exact reviewed origin-capable source snapshot; no removed environment selector is a rollback. |
| Contract | NBD size still `VRAM_MIB` (capacity stable) |
| State | Partial chunk set OK; Empty reads zeros; down frees all |
| Live host | No thrash drills; pressure via cgroup probe only |

**Rollback trigger (numeric):**

1. Any active-source scan hit for a selector, profile chooser, or full-VRAM NBD
   composition invalidates the removal gate.
2. Any origin-backed NBD, broker, ublk, Windows, or existing-test regression
   keeps activation disabled and requires a corrected source change; it does
   not justify restoring the removed path.

---

## 9. Validation plan (executable)

| # | Command / check | Pass criteria |
| --- | --- | --- |
| V1 | `cargo test -p ramshared-block` (sparse unit) | all green |
| V2 | `cargo test -p ramshared-wsl2d` | all green |
| V3 | Historical isolated-lab idle observation | Recorded idle delta only; current execution is **BLOCKED** |
| V4 | Historical isolated-lab pressure observation | Recorded order only; current execution is **BLOCKED** |
| V5 | Historical isolated-lab reclaim observation | Recorded release only; current execution is **BLOCKED** |
| V6 | `node --test --test-name-pattern=legacy_preallocation_removed_before_day0_deadline tools/ci/check-legacy-preallocation-removal.test.mjs` plus candidate scan | Named test and scan pass; no host action |

Harness **`scripts/safety/cascade-pressure-probe.sh` is real** (not fictional).

---

## 10. Open questions (non-blocking for GO)

1. Chunk default 128 vs 64 MiB on 6 GB GPUs — tune after V3 numbers.  
2. Whether canary cadence alone is enough idle free without timer msg — IMPL may add lightweight timer `WMsg` if needed (no SPEC change if same free rules).

---

## 11. SPEC revisions applied in this Passo 2.5

1. ITEM-2: worker-thread reclaim algorithm + demote/free split.  
2. ITEM-4: sparse preflight gate (not full VRAM_MIB).  
3. ITEM-5: numeric canary / free-delta budgets; sparse preflight live test.

---

## 12. Final go / no-go

| | |
| --- | --- |
| **Historical verdict** | **GO (superseded)** |
| **May new product IMPL start?** | **No** — product NBD follows the origin-only source boundary |
| **Must not IMPL** | ITEM-2b, free-when-used>0, ublk product, MS kernel PR, or a restored full-allocation selector |
| **Current status** | Source sunset is **CLOSED**; live qualification, release, and activation remain **BLOCKED** pending separate attended approval. |
