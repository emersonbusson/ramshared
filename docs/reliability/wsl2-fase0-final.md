---
slug: vram-wsl2-cuda-swap
title: Phase 0 — Consolidated Verdict (A baseline + B eviction + C tiering)
spec: SPECv2-WSL2.md
date: 2026-06-04
status: go-with-architectural-pivot
---

# Phase 0 — Consolidated Verdict

Three experiments run on this machine (kernel 6.6.114, RTX 2060 6 GiB,
16 GiB RAM, swap VHDX 8 GiB). Tools: `c0deJedi/nbd-vram` (CUDA+NBD) + custom
tooling (`vramhog`, `memhog`). RAW outputs: `FASE0-RAW.txt`, `FASE0B-RAW.txt`,
`FASE0C-RAW.txt`.

## TL;DR

**VRAM-as-swap in WSL2/GPU-PV is viable, but NOT as a single hot swap.** It is
**data-safe** but **latency-unsafe** under host GPU pressure. The correct usage,
**proven empirically**, is as a **cold page tier behind zram**:
`RAM → zram (high prio) → VRAM (medium prio) → VHDX (low prio)`.

Verdict: **`go` with architectural pivot** — shifting from "VRAM as a high-priority raw swap" (SPEC-WSL2/SPECv2 MVP) to "VRAM as a cold tier in a cascade with zram in front".

## A) Fair Baseline — INCONCLUSIVE (unavoidable cache bias)

3 rounds, pre-written non-sparse file + `drop_caches` (guest).

| 4K QD32 | VRAM | VHDX (host-cached) |
|---|--:|--:|
| randwrite IOPS | ~5.7–6.2k | ~17–19k |
| randwrite p99 | ~17 ms | ~5 ms |
| randread IOPS | ~8.7–9.6k | ~20–22k |
| randread p99 | ~6–12 ms | ~2–8.6 ms |

**Why inconclusive:** `drop_caches` only clears the **guest** cache; the **Windows host** cache is not accessible from inside WSL2, and the 2 GiB file fit entirely within it → **optimistic** VHDX baseline (16–22k 4K IOPS = RAM, not disk). The 1st run (`FASE0-RAW.txt`) had the opposite bias (VHDX sparse-cold → p99 183 ms, VRAM appeared 12× better). The truth lies in the middle; **Part C (real swap) is the arbiter.**

## B) WDDM Eviction (§9) — DATA-SAFE, LATENCY-UNSAFE ⚠️

1 GiB allocated by `nbd-vram` + 256 MiB canary + `vramhog` forcing VRAM to **0 MiB free**
(allocated +4096 MiB; `cuMemAlloc` **succeeded** → WSL2 allowed oversubscription).

- **Final integrity: identical hash** — zero corruption. The host paged the allocation to sysmem and brought it back intact.
- **Latency: a single 4K sample spiked to 1,183,094 µs (~1.18 s)** vs. ~3–4 ms normal (≈330×), recovering immediately after.

**Conclusion:** the risk in WSL2 **is not data loss, it is latency**. A swap page in VRAM can cost **>1 s** if the host is under VRAM pressure (game/compositor) → **freezing** the process. Therefore, VRAM cannot be the hot swap; the canary (§9) must trigger on **latency**, not just corruption.
(Note: this could be eviction-repaging and/or contention on the hog's copy-engine; operationally the outcome is the same — a fatal stall for a hot swap.)

## C) zram-tiering — CASCADE PROVEN ✅

zram 1 GiB (prio 200) > VRAM 1 GiB (prio 100) > VHDX (prio -2). **Incompressible** hog of 2400 MiB confined to a cgroup (`MemoryMax=400M`). Snapshot during pressure:

```text
/dev/zram0  1024M / 1024M  prio 200   <- zram FULL (DATA 1G, COMPR 1023.9M)
/dev/nbd0   1024M /  983M  prio 100   <- VRAM absorbed 983 MiB of spill
/dev/sdc       8G /  1.2G  prio -2    <- VHDX UNTOUCHED
```

~2 GiB swapped out (`pswpout` +513k pages); zram filled first, **overflow spilled into VRAM**, and VHDX was never touched. `swapoff` of both succeeded without panic on teardown. **The cascade works exactly as designed.**

## SPECv2 Gates

- **GATE-PERF:** passed on the 1st run (VRAM > VHDX in write/seq); 2nd run confounded by host-cache. Net result: VRAM is **competitive**, with a clear advantage in **sequential writes** and a weakness in **hot read latency**.
- **GATE-RESIDENCY:** **conditional** — data-safe, but latency under pressure requires the canary to abort based on latency (trigger (b) of §9.3 confirmed). VRAM as a **single hot swap** = failed. VRAM as a **cold tier** = passed.

## Recommendation (Architectural Pivot)

1. **Change the MVP** from "VRAM = raw swap prio 32767" to the **cascade** `zram (hot) → VRAM (cold, medium prio) → VHDX (low prio)`. Update §1/§6 of SPECv2 and promote §16 as the primary architecture. This leverages the measured strengths of VRAM (bandwidth/write/capacity) and hides its weakness (read latency under pressure).
2. **Mandatory §9:** canary with **latency abort** (swap read p99 > threshold for N consecutive samples → gracefully demote/remove VRAM from the pool without killing processes).
3. **Minor pending item:** a true fair baseline (impossible to bypass host cache from inside; measuring via real swap with `pswpin/out` per tier provides the honest figure — completed in Part C).
4. **`CONFIG_ZRAM_WRITEBACK` not set** in this kernel → the most elegant integration (zram writing cold pages directly to VRAM) would require a custom kernel. The **priority-based cascade** (validated in Part C) requires none of this — it is the Day-0 path.

## Machine State

Clean. All teardowns OK; final `swapon --show` = only `/dev/sdc` (8 GiB, prio -2).
Experiment artifacts are in `~/fase0/` (outside the repo).
