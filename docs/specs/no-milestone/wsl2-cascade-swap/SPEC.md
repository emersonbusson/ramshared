---
slug: wsl2-cascade-swap
title: VRAM as a Cold Tier in a WSL2 Swap Cascade
source_prd: PRD.md
variant: WSL2/GPU-PV/CUDA
milestone: M01
status: go
phase0: completed (docs/reliability/wsl2-fase0-final.md)
impl_language: rust
reference_impl: c0deJedi/nbd-vram (MIT) — blueprint/benchmark only, NOT included in the product
---

# SPEC — VRAM as a Cold Tier in a Swap Cascade (zram → VRAM → VHDX)

## 0. Audit Provenance (SSDV3 Step 2.5 — single-file model)

> **Arquivo único:** este `SPEC.md`. Revisões do Passo 2.5 são **in-place**; arqueologia = `git log` — **não** criar `SPECvN.md` (política alinhada ao Advoq).

- **Rodada 1 (pré-Phase-0 architecture):** `no-go` — MVP “VRAM as hot swap” contradizia Phase 0.
- **Rodada 2 (pivot cascade, 2026-06):** blockers V3-F1…F8 incorporados **in-place** neste arquivo:
  - **V3-F1** → **§1**: VRAM is a **COLD** tier, never the highest priority swap.
  - **V3-F2** → **§1/§4**: cascade `zram` → `VRAM` → `VHDX` is the **MVP architecture**.
  - **V3-F3** → **§9**: graceful **DEMOTE** (swapoff only VRAM tier).
  - **V3-F4** → **§5/§6/§10**: zram setup + `CONFIG_ZRAM` + `up` mounts cascade.
  - **V3-F5** → **§6.2**: priorities `zram=200 > VRAM=100 > VHDX=−2`.
  - **V3-F6** → **§3**: acceptance by real swap activity, not unfair fio baseline.
  - **V3-F7/F8** → Phase 0 complete; §14 cgroup-confined pressure.
- **Self-audit (2026-06-04):** A1–A4 fixed in-place → **`go`** → Step 3.
- **Candidato ativo:** este arquivo. Nova rejeição → revisar in-place de novo.

## 0.2 Research & Reuse (Summary)

`c0deJedi/nbd-vram` (MIT) is the reference CUDA+NBD daemon and **confirms** that on consumer GeForce GPUs only the NBD path works (`nvidia_p2p_*` → `EINVAL`, BAR1 only maps ~16 MiB). The reference itself uses the cascade `RAM` → `VRAM` → `zram` → `SSD` (prioritized); we invert this to `zram` → `VRAM` because §9.5 proved VRAM is latency-unsafe (zram, being compressed RAM, must be the hot tier). NVIDIA docs (limited pinned, absent UVM, WDDM) support §9. `.wslconfig` allows custom kernel (ublk Phase B).

## 1. Architectural Decision (PIVOT — resolves V3-F1, V3-F2)

**VRAM is not swap. VRAM is a COLD tier in a priority-based swap cascade:**

```text
memory pressure ──►  zram   (compressed RAM, lzo-rle)   Priority 200   HOT
                   └─►  VRAM   (nbd-vram, CUDA+NBD)         Priority 100   COLD
                   └─►  VHDX   (/dev/sdc, WSL2 swap disk)   Priority −2    LAST
```

**Why (Phase 0 evidence, `wsl2-fase0-final.md`):**
- VRAM is **data-safe** (integrity hash OK after eviction) but **latency-unsafe**: a 4K read under full VRAM cost **1.18 s** (§9.5). As a hot swap, it freezes the system.
- zram (low latency, in RAM) absorbs the **hot working set**; VRAM takes only the **cold spill** (rare access) — hiding its latency weakness and leveraging its strengths (bandwidth/capacity). **Proven (Part C):** zram filled 1024 MiB, VRAM absorbed **983 MiB** of overflow, VHDX remained untouched.
- The cascade is established via **swapon priority** — Day-0, without custom kernel. `CONFIG_ZRAM_WRITEBACK` is not set in this kernel, so integration via writeback (zram writing cold pages directly to VRAM) is deferred to Phase B (custom kernel).

**What RamShared delivers:** orchestrates the cascade and **manages the VRAM tier** (CUDA+NBD daemon) with a **canary that demotes VRAM under latency pressure** (§9), without killing processes. zram and VHDX are kernel mechanisms that it merely configures.

**Out of scope for WSL2:** PRD-4 (DAMON), PRD-6 (HMM `DEVICE_PRIVATE`), BAR1/P2P.

## 2. Local Platform Evidence (2026-06-04, confirmed in Phase 0)

```text
kernel:  6.6.114.1-microsoft-standard-WSL2
GPU:     RTX 2060, 6144 MiB (cuInit/cuMemAlloc OK — confirmed by nbd-vram)
RAM:     15.6 GiB | swap VHDX /dev/sdc 8 GiB prio −2
zram:    CONFIG_ZRAM=m, CONFIG_ZSMALLOC=m, zramctl present, lzo-rle default
         CONFIG_ZRAM_WRITEBACK not set
nbd:     CONFIG_BLK_DEV_NBD=m (requires modprobe; /dev/nbd* on demand)
ublk:    unavailable (CONFIG_BLK_DEV_UBLK not set) — Phase B
io_uring: enabled | systemd: active (degraded) | cgroup v2 memory: ok
/dev/dxg, libcuda: present | nvidia-smi: OK
```

Everything required by the cascade exists **today** without a custom kernel.

## 3. Phase 0 — COMPLETED (resolves V3-F6, V3-F7)

Original gate (perf vs VHDX) **bypassed**: Phase 0 ran (3 experiments, see `wsl2-fase0-final.md`, `FASE0-RESULTS.md`, `FASE0[B,C]-RAW.txt`). Results establishing this SPEC:

| Experiment | Result | Consequence in this SPEC |
|---|---|---|
| A) Fair baseline | Inconclusive (host-cache not bypassable from inside) | Acceptance moves to real swap, not fio vs VHDX |
| B) WDDM eviction | Data-safe; 4K latency → **1.18 s** under pressure | §9 DEMOTE by latency; VRAM = cold tier |
| C) zram-tiering | Cascade OK: zram 1G full, VRAM +983M, VHDX untouched | §1 MVP architecture |

**Acceptance criterion (redefined, real swap):** under pressure **confined to a cgroup** (§14), the `pswpout` counters per tier must show `zram` filling before `VRAM`, and `VRAM` before `VHDX`; no confined process should be killed by OOM while capacity remains in the cascade.

## 4. Implementation Objectives

Two binaries (same as v2 §4, adjusted roles):

- `ramshared`: CLI that **orchestrates the cascade** (`check`, `up`, `down`, `status`, `recover`, `test`).
- `ramshared-wsl2d`: daemon for the **VRAM tier** (CUDA Driver API + NBD), with the residency canary (§9). I/O core = v2 §8 (unchanged).

**Language: Rust (decision CLOSED — Phase 0 is over).** All production implementation is Rust (crates in §5): CUDA via FFI over `libcuda.so`, NBD protocol in Rust, `Result<T, Error>` without `.unwrap()/.expect()` (coding.md rule), unsafe isolated in `ramshared-cuda` with documented invariants. `c0deJedi/nbd-vram` (C, MIT) served only as (a) the measurement yardstick for Phase 0 and (b) the architectural and NBD fixed-newstyle protocol blueprint — it is not forked nor included in the binary. Day-0: clean rewrite in Rust, without C shims/forks.

Manual usage (no auto-start):

```sh
sudo ramshared check
sudo ramshared up            # mounts zram + VRAM + (VHDX already exists), in this priority
sudo ramshared status        # shows the cascade and canary state
sudo ramshared down          # unmounts in reverse order, safely
```

## 5. Code Tree (resolves V3-F4)

Same as v2 §5, with **additions for the cascade**:

```text
crates/ramshared-cli/src/commands/
    check.rs  up.rs  down.rs  status.rs  recover.rs  test.rs
crates/ramshared-tier/             # NEW — cascade orchestration
    Cargo.toml
    src/lib.rs
    src/zram.rs        # create/size/mkswap/swapon zram (prio 200)
    src/cascade.rs     # priority scheme, up/down order, verification
    src/priority.rs    # constants and priority validation between tiers
crates/ramshared-wsl2d/src/
    residency.rs       # canary with graceful DEMOTE (§9)  [changed vs v2]
# (ramshared-cuda, ramshared-block, ramshared-integrity = v2 §5, unchanged)
docs/specs/no-milestone/wsl2-cascade-swap/SPEC.md
```

## 6. CLI Contract

### 6.1 `ramshared check` (v2 §6.1 + zram)

Adds to the check from v2:
- **zram:** `CONFIG_ZRAM=y|m` and `zramctl` present → `zram=ok`; else `zram=fail`. Report default algorithm (`lzo-rle`).
- **cgroup v2 memory** (for testing): presence of `memory` in `/sys/fs/cgroup/cgroup.controllers`.

Output adds line:
```text
Tiers: zram=<ok|fail>, vram=<ok|needs-modprobe|fail>, vhdx=<device,prio>
```
`Decision: ready` requires zram **or** vram to be usable (the cascade degrades to whatever is available). `blocked` only if no extra tier is possible.

### 6.2 `ramshared up` (resolves V3-F4, V3-F5 — replaces start+swapon)

Fixed and validated priority scheme (**resolves V3-F5**):
```text
ZRAM_PRIO = 200    VRAM_PRIO = 100    VHDX = maintain existing (-2)
```
Flags: `--zram-size` (default `25%` of RAM, **OQ-zram**), `--vram-size` (default `1G`, backoff `512 MiB`, same as v2 §6.2), `--no-zram`, `--no-vram`, `--vram-min` (default `256M`), `--force-large`.

Sequence (each step is idempotent; abort does not leave a partial cascade — rolls back what was mounted):
1. Preflight (`check`). Abort if `blocked`.
2. **zram Tier (HOT):** `modprobe zram`; `zramctl --find --size <N> --algorithm lzo-rle`; `mkswap -L RAMSHARED_ZRAM`; `swapon -p 200`.
3. **VRAM Tier (COLD):** start `ramshared-wsl2d` (CUDA alloc with backoff §6.2-v2; `mlockall`+`oom_score_adj=-1000`; staging; **residency canary armed** §9); connect `nbd` (§10); `mkswap -L RAMSHARED_VRAM`; `swapon -p 100`.
4. **VHDX Tier (DEMOTE safety net — A1):** **do not touch** (already at -2). **Safety Invariant:** the VRAM tier is only armed if there is a target of priority LOWER than VRAM for the DEMOTE (§9.2) to drain pages into — that is, VHDX swap present **OR** `MemAvailable ≥ vram_size`. If `.wslconfig swap=0` (no VHDX) **and** RAM is insufficient, `up` **rejects** the VRAM tier (exit != 0) unless `--force-no-safety-net` is specified. Warn if VHDX priority ≥ 100 (collides with VRAM).
5. Publish `/run/ramshared/cascade.json` (tiers, priorities, devices, PID, sizes).
6. Print resulting cascade (same as `status`).

### 6.3 `ramshared status`
Reads `cascade.json` + `/proc/swaps` + `zramctl`; prints cascade, `Used` per tier, canary status (`armed|demoted`), and current `pswpin/pswpout`.

### 6.4 `ramshared down` (reverse order, safe — resolves inherited V3-F3)
1. **VRAM:** `swapoff <vram_dev>` (kernel migrates pages to zram/VHDX). If it fails (ENOMEM), **do not disconnect** nbd (panic) → `recover` (§13).
2. Graceful daemon shutdown (drain, stop canary, zero VRAM, `nbd -d`, free `CUdeviceptr`).
3. **zram:** `swapoff <zram_dev>`; `zramctl -r`.
4. VHDX remains. Remove `/run/ramshared/*`.

### 6.5 `ramshared recover` — v2 §13 (escalated, `wsl --terminate` as last resort).

## 7. Daemon — State Machine (v2 §7 + demote)

```text
Init → PreflightOk → MemoryLocked → CudaReady → VramAllocated
     → ResidencyArmed → BlockReady → SwapActive
     → Demoted        (canary triggered latency; VRAM removed from pool, system alive)
     → Stopping → end
     → Failed         (hard error)
```
`Demoted` is **new** and is **not** `Failed`: the VRAM tier has left the cascade, zram/VHDX continue. From `Demoted`, we can re-promote (OQ-demote) or run `down`.

## 8. CUDA I/O and Atomicity — unchanged from v2 §8

Ordered stream, in-flight blocks map (no torn reads), VRAM-durability before completion via `cuEvent`, CUDA errors → I/O error (never partial success). Complete details in this SPEC §8 (and git history of earlier revisions).

## 9. Residency with Graceful DEMOTE (resolves V3-F1, V3-F3) — evidence in §9.5

Premise and mechanics of canary = v2 §9.1/§9.2 (canary region, sampler every `T_sample`, latency baseline). **The ACTION changes:**

### 9.1 Trigger (calibrated by Phase 0)
```text
DEMOTE-VRAM if:
  (a) canary p99 latency > K × baseline for ≥ M consecutive samples
      (default K=8, M=3; Phase 0 measured a 330× spike — huge margin)   [DOMINANT risk]
  (b) canary content != pattern                          [not observed, but serves as guard]
  (c) cuMemGetInfo free < floor                              [host reclaiming VRAM]
```

### 9.2 Action: DEMOTE, not abort (the key difference in v3)
```text
PRECONDITION (A1): lower tier below VRAM (VHDX) exists OR MemAvailable >= vram_size.
                  Guaranteed in 'up' (§6.2 step 4); without it, VRAM is not armed.
1. swapoff <vram_dev>  (timeout T_demote=30s default; kernel migrates VRAM-resident
   pages to the lower priority target — VHDX — or RAM. Bounded by VRAM size:
   worst-case ~vram_size/4KiB pages, each potentially slow under eviction, hence the timeout.)
2. nbd -d; free CUdeviceptr; state → Demoted.
3. Processes are NOT killed: zram (hot) and VHDX (cold) continue serving swap.
4. Log with numbers (observed latency, pages migrated). No fluff (#3).
If swapoff exceeds T_demote (eviction blocking readback) → escalate to recover (§13),
as I/O is stuck in the kernel.
```

This is only safe because VRAM is an intermediate tier: a lower tier (VHDX) exists to receive the pages. This was impossible in raw-swap v2 (where VRAM was the end). **Discipline #2 (counterfactual):** the numeric latency trigger IS the reversion condition; **#5 (worst-case):** the worst-case scenario (host reclaiming VRAM) has an exit path without data loss.

### 9.3 Empirical Evidence (Phase 0) — see §9.5 of v2 and `wsl2-fase0-final.md`
Data survives (hash OK); 4K latency → 1.18 s under full VRAM. Confirms (a) as the relevant trigger and justifies DEMOTE instead of trusting VRAM under pressure.

### 9.4 Dedicated Content/Free-Floor Canary — implemented (issue #8)
Triggers (b)/(c) from §9.1 are probed by a dedicated canary region (separate from swap, not NBD-addressable) + sampler with hysteresis (`ResidencySampler`): content corruption triggers immediate demotion; free-floor and transient errors require `consecutive` samples (anti-false-positive, DT-9/DT-11). Per-request latency (a) remains the primary trigger. Spec/Impl: `docs/008-vram-residency-canary/`.

## 10. Tiers and Backends

- **zram:** `ramshared-tier/zram.rs` (create, size, mkswap, swapon 200, teardown). lzo-rle. No writeback (current kernel).
- **VRAM/nbd:** v2 §10.1 (modprobe nbd, `NBD_SET_*` ioctls, flush only if implemented, `NBD_DISCONNECT` in teardown). swapon 100.
- **ublk (Phase B):** v2 §10.2 + custom kernel recipe (unchanged).
- **VHDX:** unmanaged; only read/validated (priority < VRAM).

## 11. Safety Limits (updated)

- No auto-start. VRAM **never** as the highest priority swap (V3-F1). Fixed scheme: `200 > 100 > −2`.
- VRAM: backoff down to `--vram-min`; ≥ 1 GiB free post-reservation; ≤ 25% VRAM free; `mlockall` + `oom_score_adj=-1000` on daemon.
- zram (A3): default size = `min(25% of RAM, MemAvailable − 2 GiB)`, minimum `512M`; if `< 512M`, `up` warns and proceeds without zram (`VRAM` + `VHDX` only). Incompressible data in zram consumes **real RAM** — the ceiling against `MemAvailable` avoids swap-devouring-RAM (typical pages compress; incompressible is the worst-case).
- DEMOTE preferred to Failed; `down` unmounts VRAM before zram; `swapoff` before any `nbd -d` (anti-panic, confirmed by reference).
- Zero VRAM on allocation and release; root-only daemon; socket `0600`.

## 12. Kahneman Disciplines per Critical Step (v2 §12 + cascade)

| Step | Discipline | Minimum Evidence | Abort/Reversion |
|---|---|---|---|
| Architecture (cascade) | #1, #5 | `wsl2-fase0-final.md` (3 experiments) | — (decision based on data) |
| VRAM Residency (§9) | #2, #5 | canary + baseline | DEMOTE-VRAM (§9.1) |
| Tier Order (§6.2) | #9 substitution→number | priorities in `/proc/swaps` | VHDX ≥ VRAM → abort mount |
| VRAM Reservation | #3 | logged `cuMemGetInfo` | free < floor → backoff/abort |
| Acceptance (§14) | #3, #13 | `pswpout` per tier under real load | out-of-order cascade → failure |
| `down`/demote | #2 | swapoff OK before `nbd -d` | swapoff fails → recover (§13) |

## 13. Recovery — Unchanged from v2 §13

Escalated: `swapoff` (timeout) → `nbd -d`/ublk delete → free CUDA → only then, with processes in `D` > `T_stuck`, suggest `wsl --terminate` with collateral warning (OQ1: distro is primary). Detailed in v2 §13.

## 14. Acceptance Tests (real swap + cgroup — resolves V3-F6, V3-F8)

### 14.1 Detection — check reports 3 tiers; `ready`/`blocked` paired (v2 §14.1).
### 14.2 VRAM Integrity — `up --no-zram` + `test --integrity 30m` with overlapping concurrent blocks; hash without divergence (v2 §14.2).
### 14.3 Cascade Under Pressure (NEW, confined in cgroup — Phase 0 Part C method):
```sh
sudo ramshared up
# confined INCOMPRESSIBLE hog: systemd-run --scope -p MemoryMax=400M memhog 2400 45
# snapshot DURING: swapon --show ; zramctl ; pswpout per tier
```
Acceptance: `zram.Used` saturates **before** `vram.Used` increases; `vhdx.Used` unchanged while zram+VRAM have space; no OOM kills. (Replicates result already obtained: zram 1024M, VRAM 983M, VHDX untouched.)

### 14.4 DEMOTE Under Latency (NEW — the core of v3):
```sh
sudo ramshared up
# vramhog CUDA fills VRAM (oversubscription) -> canary latency spikes
```
Acceptance: canary detects (a) §9.1; daemon enters `Demoted`; swapoff of VRAM completes within `T_demote`; **no processes killed**; zram/VHDX continue; VRAM disappears from `/proc/swaps`; `status` shows `demoted`.

### 14.5 Hard Failure (SIGKILL) → `recover` (v2 §14.5).
### 14.6 `down` leaves `/proc/swaps` only with VHDX; zram removed; VRAM freed.

## 15. Definition of Done

- `cargo fmt`/`clippy -D warnings`/`test` green; no `.unwrap()` in production.
- `check` covers `ready` and `blocked`.
- §14.3 (cascade in order) and §14.4 (DEMOTE without killing processes) green.
- `down`/`recover` leave the system at baseline (only VHDX).
- Day-0: no shims; cascade by native priority; custom kernel only in Phase B (ublk/zram-writeback), documented.

## 16. Non-Goals and Future Options

Non-Goals: VRAM as hot swap; lending VRAM to the host; kernel module in MVP; DAMON/HMM/TTM/ReBAR; auto-start.

Future Options (Phase B, require custom kernel — only with documented Day-0 exception):
- **zram-writeback → VRAM:** with `CONFIG_ZRAM_WRITEBACK`, zram writes cold pages directly to the VRAM device (`backing_dev`), eliminating the separate priority tier.
- **ublk** instead of nbd (perf): v2 §10.2.
- **Automatic re-promotion** of VRAM after latency cooldown (OQ-demote).
- **userfaultfd** (PRD-5) if block layer dominates overhead.

## 17. Open Questions

- **OQ-pivot** — ✅ accepted (request for cascade pivot = acceptance of the pivot for the cascade).
- **OQ-zram** — default size of zram (proposed 25% of RAM); confirm policy.
- **OQ-demote** — ✅ DECIDIDO: MVP = demote-and-stay-down (VRAM exits and stays down until `down`/`up`). Automatic re-promotion after cooldown = future option (§16).
- **OQ2** — fair baseline: resolved conceptually by migrating acceptance to real swap (§3/§14.3); fio-vs-VHDX abandoned.
- **OQ3** — VRAM limit with shared GPU (~4.2 GiB free): backoff §6.2 covers this; confirm `--vram-size` default of 1G.

## 18. Traceability PRD-2 → SPEC (SSDV3 strict rule #4/#5)

PRD-2 is a historical record (describing VRAM as hot swap via ublk); Phase 0 (`wsl2-fase0-final.md`) revised part of its requirements. Map:

| PRD-2 | Original Text | Status in this SPEC | Section |
|---|---|---|---|
| **RF-1** | allocate 1–N GB of VRAM without freezing the GUI | **maintained** (backoff, limit, mlock) | §6.2, §11 |
| **RF-2** | ublk workers via io_uring | **REVISED** → nbd in Phase A; ublk = Phase B (custom kernel) | §2, §10 |
| **RF-3** | swapon pri=32767 (hot swap) | **REVISED** → VRAM is a COLD tier, priority 100 behind zram | §1, §6.2 |
| **RNF-perf** | saturate PCIe 10-15 GB/s | **REVISED** → bare-metal claim; GPU-PV is latency-bound | §3.2.1 |
| **RNF-estab** | mlockall anti-deadlock | **maintained** | §6.2, §11 |

Revised requirements do not go back to PRD-2 (preserving history) — they are reconciled here. Step 3 commits cite the SPEC section + RF covered (e.g., `feat(core): cascade priority — SPEC §1 / revises RF-3`).
