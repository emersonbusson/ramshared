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
