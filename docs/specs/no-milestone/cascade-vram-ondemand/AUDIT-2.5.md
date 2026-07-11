# AUDIT-2.5 — cascade-vram-ondemand

> **Passo 2.5 SSDV3** (formal). Risk-gated: CUDA lifetime, swap data integrity, WSL host safety, reclaim races.  
> **Date:** 2026-07-11  
> **Auditor role:** adversarial + security + Kahneman #13/#16.  
> **Inputs:** [`PRD.md`](PRD.md), [`SPEC.md`](SPEC.md), codebase (`ramsharedd` NBD path, `VramProvider`, canary), live cascade behaviour, MS WSL reclaim patterns.

---

## Decision (top)

| Path | Verdict |
| --- | --- |
| MVP: sparse alloc-on-write + free only when nbd `used_kb==0` | **GO** |
| Preflight sparse gate (not full VRAM_MIB free at boot) | **GO** (SPEC revised this pass) |
| Free CUDA while `used_kb > 0` | **NO-GO** (MVP) |
| ITEM-2b mid-flight spill | **NO-GO** (needs new PRD/SPEC/2.5) |
| Product ublk on WSL2 | **NO-GO** (unchanged parent) |
| Kill-switch full prealloc | **GO** (`RAMSHARED_VRAM_PREALLOC=1`) |

### Overall

**GO** for Passo 3 (IMPL) of SPEC MVP **as revised in this audit** (preflight + worker-thread reclaim).  
**Blockers:** none remaining after SPEC in-place edits below.

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
| T6 | Prealloc kill-switch missing → no rollback | **MED** | `RAMSHARED_VRAM_PREALLOC=1` |
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
| **HIGH** | Preflight `free >= VRAM_MIB + headroom` forces full free at boot and **contradicts** RF-L1 / user “3 GiB capacity without holding 3 GiB” | **SPEC ITEM-4 revised:** sparse gate = headroom + canary + one chunk; prealloc keeps legacy gate |
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
| Full prealloc today | `main.rs`: `provider.alloc(size)?; mem.zero()?` | Confirmed codebase |
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
| `sparseVhd` logical ≠ provisioned | Confirmed docs | Capacity vs committed chunks |
| `hv_balloon` under host pressure | Confirmed MS kernel tree | Free chunks when free-floor + empty device |
| GPU = GPU-PV / dxgkrnl, not system RAM | Confirmed docs/tree | Userspace only; no MS kernel PR |
| Features: experimental → default | Confirmed docs | Kill-switch prealloc; sparse default after live green |

**Inference (labelled):** MS would not merge CUDA-NBD into stock `config-wsl`; they would ship **sparse commit + reclaim** style policy in userspace/service if at all — matches our layering.

---

## 7. Kahneman map (Passo 2.5)

| # | Question | Evidence required at IMPL | Abort |
| --- | --- | --- | --- |
| #2 | What makes us revert sparse default? | Idle `up` Δ free ≈ VRAM_MIB | `PREALLOC=1` + revert default |
| #13 | Did we test refuse free when used>0? | Unit/integration must force used>0 mock or live | Missing test → no DONE |
| #15 | Alloc fail retry? | Code review: single fail path | Loop found → no-go merge |
| #16 | Safe default when unsure? | used>0 → no free | Free anyway → CRITICAL |
| #17 | Double free Empty chunk? | Idempotent drop | Panic → fix |
| #18 | Right layer? | Daemon backend, not kernel patch | Kernel-only “fix” → reject |

---

## 8. Atomicity / rollback

| Kind | Behaviour |
| --- | --- |
| Code | Revert commit; or env `RAMSHARED_VRAM_PREALLOC=1` without revert |
| Contract | NBD size still `VRAM_MIB` (capacity stable) |
| State | Partial chunk set OK; Empty reads zeros; down frees all |
| Live host | No thrash drills; pressure via cgroup probe only |

**Rollback trigger (numeric):**

1. After idle sparse `up`, `nvidia-smi` free drop **> 64 MiB** attributable to daemon (excluding other apps) → treat as fail; enable PREALLOC or revert.  
2. Any ghost nbd/ublk or WSL freeze after sparse reclaim → PREALLOC + validation entry; stop sparse default.

---

## 9. Validation plan (executable)

| # | Command / check | Pass criteria |
| --- | --- | --- |
| V1 | `cargo test -p ramshared-block` (sparse unit) | all green |
| V2 | `cargo test -p ramshared-wsl2d` | all green |
| V3 | Idle `up` VRAM_MIB=3072; `nvidia-smi` Δ | Δ free ≤ 64 MiB (not ~3072) |
| V4 | `sudo bash scripts/safety/cascade-pressure-probe.sh --prove-disk` | order zram→nbd→disk |
| V5 | After V4 release + idle free | committed↓; free_GPU↑ |
| V6 | PREALLOC=1 `up` | Δ free ≈ size (legacy) |

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
| **Verdict** | **GO** |
| **May IMPL start?** | **Yes** — Passo 3 against revised SPEC only |
| **Must not IMPL** | ITEM-2b, free-when-used>0, ublk product, MS kernel PR |
| **Next** | Passo 3: tests first → `SparseVramBackend` → wire `ramsharedd` → live V3–V6 → `IMPL.md` |
