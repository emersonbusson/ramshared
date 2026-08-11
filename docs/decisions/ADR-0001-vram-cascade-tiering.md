# ADR-0001 — zram → VRAM → VHDX swap cascade (VRAM as a cold tier)

**Status:** Accepted (2026-06-05). Supersedes the MVP of "VRAM as raw,
high-priority swap" (SPEC-WSL2 v1).

## Context

Objective: use idle VRAM as system memory in WSL2/GPU-PV (RTX 2060). Phase 0
([`../reliability/wsl2-fase0-final.md`](../reliability/wsl2-fase0-final.md)) measured on a
real GPU:

- WDDM eviction: data **survives** (integrity hash), but a 4 KiB read under
  full VRAM cost **1,183,094 µs (~1.18 s)** versus the normal ~3–4 ms.
- A priority `swapon` cascade works: 1 GiB of zram filled, VRAM absorbed
  **983 MiB** of overflow, and VHDX remained untouched.

Conclusion: VRAM is **data-safe but latency-unsafe** under host GPU
contention.

## Decision

VRAM is **not** hot swap. It is a **cold tier** in a priority cascade:
`zram (200) → VRAM (100) → VHDX (−2)`. zram (compressed RAM) absorbs the hot
working set; VRAM receives only cold spill. A latency canary **demotes** VRAM
(swapoff only that tier; pages fall to VHDX) under eviction, without killing
processes.

## Consequences

- (+) Uses VRAM's strength (bandwidth/capacity) and hides its weakness
  (latency under pressure).
- (+) Day-0: native priority cascade, without a custom kernel.
- (−) Requires zram and invariant A1 (DEMOTE is safe only with a tier below
  VRAM).

## Alternatives considered

- **VRAM as hot swap (highest priority):** rejected — latency-unsafe (1.18 s
  freezes the process).
- **zram with writeback to VRAM:** requires `CONFIG_ZRAM_WRITEBACK` (not set)
  → custom kernel; remains Phase B.
- **NUMA hotplug / HMM `DEVICE_PRIVATE`:** impossible on consumer GeForce
  WSL2 (`nvidia_p2p_*` → `EINVAL`; no DRM control in the guest).

## Kahneman

- #5 worst-case (WDDM eviction measured, not assumed) · #3 number (1.18 s;
  983 MiB) · #2 counterfactual (rollback below).

## Rollback trigger

Revert to VHDX-only swap if, in a three-round retest, the VRAM tier's p99 read
latency under real pressure exceeds VHDX's p99 **and** the canary (§9) fails to
detect eviction before any hash divergence.

Links: [`../specs/no-milestone/wsl2-cascade-swap/SPEC.md`](../specs/no-milestone/wsl2-cascade-swap/SPEC.md) §1, §9 ·
[`../reliability/DEGRADATION-MATRIX.md`](../reliability/DEGRADATION-MATRIX.md).
