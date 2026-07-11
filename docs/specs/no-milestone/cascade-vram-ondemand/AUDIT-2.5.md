# AUDIT-2.5 — cascade-vram-ondemand

> Passo 2.5 SSDV3 + security/fail-safe review of SPEC MVP.  
> **Date:** 2026-07-11  
> **Scope:** sparse CUDA commit for NBD VRAM tier; idle free; kill-switch prealloc.

## Decision

| Path | Verdict |
| --- | --- |
| MVP sparse alloc-on-write + free when `nbd used_kb==0` | **GO** |
| Mid-flight free while `used_kb>0` (ITEM-2b) | **NO-GO** for this IMPL |
| Change default transport / ublk product | **NO-GO** (unchanged) |
| Full prealloc remains kill-switch | **GO** (`RAMSHARED_VRAM_PREALLOC=1`) |

**Overall: GO** for IMPL of SPEC MVP (ITEM-1..5 as revised).  
**Do not** start ITEM-2b without a new AUDIT.

---

## 1. Spec quality findings

| Sev | Finding | Disposition |
| --- | --- | --- |
| MED | Early draft listed competing reclaim “choice A/B” | **Fixed** in SPEC — single MVP table |
| MED | True demote-while-full is hard on NBD | Explicitly phase 2; honest gap |
| LOW | GAT/`VramProvider` ownership in daemon | IMPL must keep CUDA single-thread affinity |
| LOW | Preflight still checks full VRAM_MIB free | Intentional capacity gate; log “capacity not commit” |

## 2. Security / privileged surface

| ID | Risk | Control |
| --- | --- | --- |
| S1 | Free Live chunk while swap pages still on nbd → **corruption / hang** | SPEC: free only if `used_kb==0` or `down` |
| S2 | Alloc storm on write fail | No retry loop (#15); I/O error |
| S3 | Kill -9 with live nbd | Unchanged cascade contract |
| S4 | Info-leak of kernel addresses | Telemetry: sizes/counters only |
| S5 | Host thrash in validation | Use existing `cascade-pressure-probe.sh` (cgroup), not full RAM bomb |

## 3. Microsoft / WSL alignment (audit of “would they ship this?”)

| MS pattern | SPEC alignment |
| --- | --- |
| `autoMemoryReclaim` — return unused resources | Idle free when used=0 |
| `sparseVhd` — logical size ≠ full provision | NBD capacity vs committed chunks |
| `hv_balloon` — give back under pressure | Free chunks when GPU free-floor + idle |
| Experimental → default | Kill-switch prealloc; sparse becomes default after live gates |
| No CUDA-in-stock-kernel | Userspace daemon only |

**Confirmed:** MS does **not** productize “VRAM as system RAM” in stock WSL; GPU is GPU-PV/dxgkrnl. Our feature stays **userspace policy**, MS-compatible.

## 4. Kahneman

| # | Audit note |
| --- | --- |
| #2 | Rollback: idle `up` still drops GPU free ≈ VRAM_MIB → PREALLOC=1 |
| #13 | Must test free-blocked when used&gt;0, not only happy idle free |
| #16 | Safe default: never free Live when used&gt;0 |
| #18 | Fix in `ramsharedd` / `SparseVramBackend`, not kernel patch |

## 5. Validation harness existence

| Artifact | Status |
| --- | --- |
| `scripts/safety/cascade-pressure-probe.sh` | **Exists** (git `06957fe`); cgroup MemoryMax; proves zram→nbd→disk |
| `nvidia-smi` before/after | Manual / shell in validation.md |
| Journal demote | Existing canary path; extend counters per ITEM-3 |

## 6. Open questions (non-blocking)

1. Exact canary size vs slack budget (64 MiB) — measure on first IMPL live.  
2. Chunk default 128 MiB vs 64 MiB on 6 GB cards — tune after first free-delta numbers.

## 7. Go / no-go

**GO** — implement SPEC MVP.  
Blockers: none.  
**NO-GO** expansion: ITEM-2b, ublk product, kernel tree PR to Microsoft.
