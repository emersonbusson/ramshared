# Passo 0 — Lab inventory (kernel-vram-as-memory)

> Gate A from `PRD.md` §14. Updated **2026-07-10**.

## Environment

| Check | Result |
| --- | --- |
| Host | Windows + Hyper-V (`EMEDEV`) |
| WSL guest | **YES** — GPU-PV only (PCI vendor **0x1414**, no `/dev/dri`) |
| GPU (host) | NVIDIA GeForce RTX 2060 6 GB |
| Dual-boot free space (R: RUSSIA) | **FAIL** — free ~170 GB but shrinkable only ~**2.68 GB** (NTFS end-of-volume / immovable layout) |
| Dual-boot free space (E: ESPANHA) | **PASS** — shrunk 2026-07-10; **~32 GB unallocated** on disk 0 (SAMSUNG HD154UI) |
| Ubuntu ISO for installer | `R:\Hyper-V\iso\ubuntu-24.04.2-live-server-amd64.iso` |
| Hyper-V Linux lab | `linux-kernel-lab` Running (cloudimg) — **not** bare-metal GPU |

## Gate A verdict (updated)

| Gate | Pass? | Notes |
| --- | --- | --- |
| A1 bare-metal or real passthrough | **PARTIAL** | Space for dual-boot **prepared** on E:; OS install still needs USB boot once. WSL alone remains FAIL for GPU-true. |
| A2 inventory recorded | **PASS** | this file + `docs/labs/DUALBOOT-KERNEL-TRUE.md` |
| A3 pressure not on daily host blindly | **PASS** | dual-boot on data HDD E:, not C: |

## Gate B

**Not run** — needs boot into bare-metal Linux first, then ≥3 latency runs.

## Decision

| Track | Status |
| --- | --- |
| Kernel-true on **WSL GPU-PV only** | Still **NO-GO** |
| Dual-boot space | **UNBLOCKED on E:** (32 GB) |
| Next | USB install into unallocated; then `lspci`/`/dev/dri` proof → Gate B |
| DDA alternative | Still optional; host loses 2060 while assigned |

## Why R: was not enough

Windows shrink requires free **extents at the end of the partition**.  
R: free space was large but **not shrinkable** past ~2.7 GB (media layout + immovable).  
E: allowed ~33 GB shrink → used ~32 GB. See `docs/labs/DUALBOOT-KERNEL-TRUE.md`.


## Re-check 2026-07-14 (post host policy + power cycle)

| Check | Result |
| --- | --- |
| `uname -r` | `6.18.33.2-microsoft-standard-WSL2` (inbox; custom kernel path armed in `.wslconfig` with forward-slash encoding) |
| `/dev/dri` | absent (expected GPU-PV) |
| `lspci` NVIDIA native | GPU-PV path only (product cascade NBD+CUDA) |
| `/dev/ublk-control` | **absent** on this boot (ublk not product Day-1) |
| Gate A1 | still **FAIL for kernel-true** on this WSL guest |
| Product cascade | **GO** — zram 2G + nbd/VRAM 4G + disk (see validation host policy entries) |

### Go / no-go (issue #32 acceptance)

| Question | Answer |
| --- | --- |
| Inventory recorded? | **YES** (this file) |
| SPEC-level HMM/NUMA on **this** WSL2 GPU-PV host? | **NO-GO** (ADR-0001 / Gate A1) |
| Bare-metal dual-boot path? | Space on E: prepared; OS install + Gate B still **OPEN** (USB install) — out of scope for daily WSL product |

**Recommendation:** close research issue #32 as **inventory complete / WSL NO-GO**; keep bare-metal dual-boot as a separate future issue when USB install is scheduled.
