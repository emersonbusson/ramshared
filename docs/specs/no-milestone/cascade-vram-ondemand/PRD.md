---
slug: cascade-vram-ondemand
title: "Cascade VRAM on-demand — capacity without full CUDA pre-alloc; return under reclaim"
milestone: —
issues: []
parents:
  - cascade-transport-policy
  - wsl2-cascade-swap
  - mainline-vram-tiering
---

# PRD — Historical on-demand VRAM capacity design

> **Status (2026-08-22):** **SUPERSEDED for product NBD.** The Day-0
> legacy-preallocation source gate is **CLOSED**: product NBD requires an
> authoritative origin and has no full-capacity allocation selector or fallback.
> This PRD retains the 2026-07-11 design record only. Live qualification, release
> promotion, and activation remain **BLOCKED**; it authorizes no host, GPU, WSL,
> device, or pressure action.
>
> Kahneman **#16** (fail-safe reclaim), **#18** (right layer), **#5** (worst
> case: game needs VRAM).

## 1. Summary

### User intent (product language)

1. Configure up to **N GiB** of VRAM-backed cold swap (e.g. 3 GiB).  
2. **If the machine does not need it**, do **not** permanently steal that much GPU memory from games/desktop.  
3. **If it needed it and later no longer does** (or the GPU needs free VRAM), **return** GPU memory.  
4. Keep kernel spill order: **zram → VRAM → SSD** (already shipped).

### Historical problem statement (2026-07-11)

| Layer | Behaviour now | User expectation |
| --- | --- | --- |
| Kernel swap | Writes to nbd only under pressure (`USED` grows) | “Only use if needed” — **OK** |
| Daemon | `provider.alloc(size)` **full** `VRAM_MIB` at `up` + zero | “Don’t hold GPU if idle” — **NOT OK** |
| Teardown | Full free on `down` / process exit | “Return when done” — **OK for stop** |
| Runtime reclaim | Canary **DEMOTE** pages to lower tier; **does not free** CUDA allocation | “Return while still up” — **GAP** |

**Confirmed in codebase:** `crates/ramshared-wsl2d/src/main.rs` — `provider.alloc(size)` then `mem.zero()` before serving NBD.  
**Confirmed live:** cascade `up` 2 GiB drops GPU free ~2 GiB; `down` restores free (~2.4 G → ~4.5 G measured 2026-07-11).

## 2. Technical context

### 2.1 How Microsoft / WSL2 thinks about memory (research)

Sources: local clone `~/WSL2-Linux-Kernel` (MS tree + `Microsoft/config-wsl`), [WSL2-Linux-Kernel README](https://github.com/microsoft/WSL2-Linux-Kernel), [WSL config docs](https://learn.microsoft.com/en-us/windows/wsl/wsl-config), WSL issues/blog on memory reclaim.

| Pattern | What MS does | Class |
| --- | --- | --- |
| **Host RAM reclaim** | `autoMemoryReclaim` = `dropCache` / `gradual` / `disabled` — **experimental → default-bound**; reclaim **unused** guest cache to Windows | Confirmed docs |
| **Sparse disks** | Production default omits `sparseVhd`. WSL 2.7.12 refuses live sparse enablement because of potential data corruption unless an unsafe override is forced; RamShared never forces it. | Confirmed host/runtime evidence |
| **Balloon** | `drivers/hv/hv_balloon.c` in MS kernel — give pages back to Hyper-V host under pressure | Confirmed codebase (MS tree) |
| **Swap** | `.wslconfig` `swap` + `swapFile` on **disk VHDX** — cold tier, not pre-filled with content | Confirmed docs |
| **GPU** | GPU-PV + `dxgkrnl`; GPU for **compute/graphics**, **not** first-class “system RAM tier” in product WSL | Confirmed docs + tree (`drivers/hv/dxgkrnl`) |
| **Kernel contribution model** | Bugs/features → **microsoft/WSL issues**; kernel code changes preferred **upstream Linux**; community PRs to MS kernel repo not the product gate | Confirmed README-Microsoft.WSL2 |
| **zram** | Not stock-enabled for all; custom kernel/module path (community issues) | Confirmed issues + our P1 work |

**Inference — “how MS would do VRAM-as-swap” if forced to ship something:**

1. **Would not** ship “cuMemAlloc full size at boot” as the long-term story (conflicts with reclaim culture + multi-app GPU).  
2. **Would** prefer: **capacity advertised**, **physical commit on demand**, **reclaim under host/GPU pressure**, **opt-in experimental → harden → default**.  
3. **Would not** put CUDA NBD into `config-wsl` as a first-class subsystem; they keep **mm/balloon/swap** generic and leave vendor GPU to **DX/GPU-PV**.  
4. **Would** align with **sparse** semantics (like sparse VHD): logical size ≫ committed resource until use.  
5. **Would** fail closed under GPU contention (game wins or documented policy), not thrash the host.

**What MS will not do for us:** merge RamShared into stock WSL kernel. Our product stays **userspace + optional custom kernel modules** (ublk already on that track).

### 2.2 How mainline Linux would do it (long-term)

Already PRD’d in `mainline-vram-tiering`: **memory tier + demotion**, not NBD.  
This feature is **L0→L1 bridge polish**: make userspace cascade **behave more like** tiering (commit on use, demote+free), without claiming mainline.

### 2.3 Repo facts (RamShared)

| Fact | Class |
| --- | --- |
| `VramProvider::alloc` / RAII free on drop | Confirmed `ramshared-vram` |
| Full size alloc at daemon start | Confirmed `ramsharedd` NBD path |
| Residency canary + DEMOTE verdict | Confirmed `residency.rs` |
| Swap priorities zram > nbd > disk | Confirmed `ramshared-tier` + live |
| Orphan live enumeration | Detection-only; recovery exige lifecycle binding exato |
| ublk product on WSL2 | NO-GO (`cascade-transport-policy`) |

## 3. Recommended option

### Option R1 — **Sparse chunk map (GO for SPEC)**

- NBD **advertises** full `VRAM_MIB` (e.g. 3 GiB) so cascade capacity is unchanged.  
- Physical CUDA allocations are **chunks** (e.g. 64–256 MiB) allocated on **first write** to that range.  
- Unwritten ranges: read as zeros **without** CUDA commit (or single shared zero page).  
- On DEMOTE / swapoff of region / idle reclaim: **free** chunks with `used==0` after content moved to lower tier.  
- Free-floor canary: demote + free until `mem_info.free` above floor or no freeable chunks.

### Option R2 — Full pre-alloc (status quo)

- Keep `alloc(size)` — **NO-GO** as end state for user goal.

### Option R3 — Wait for HMM/mainline

- Correct long-term; **does not** ship user goal on WSL Day-1 — keep as parent track only.

### Option R4 — Shrink advertised size dynamically

- Change nbd size at runtime — **NO-GO** (ABI/resize hell; ghosts).

**Decision for this PRD:** **R1**.

## 4. Functional requirements

| ID | Requirement |
| --- | --- |
| RF-L1 | At `up`, daemon must **not** require `cuMemAlloc(VRAM_MIB)` for the full export size as a single blob |
| RF-L2 | NBD `size_bytes` remains `VRAM_MIB` (capacity contract unchanged for cascade/swapon) |
| RF-L3 | First **write** to an uncommitted range allocates ≥1 chunk; failure → I/O error or demote-to-disk path (defined in SPEC), never hang host |
| RF-L4 | Reads of never-written ranges return zeros without committing VRAM (except optional tiny metadata) |
| RF-L5 | When free VRAM &lt; free-floor (existing canary), **DEMOTE** content to lower swap tier and **free** reclaimable chunks |
| RF-L6 | When chunk has no resident swap pages and is idle past hysteresis, free chunk (bounded reclaim) |
| RF-L7 | `down` still frees **all** chunks + canary (existing anti-hang order) |
| RF-L8 | Telemetry: `vram_committed_bytes`, `vram_capacity_bytes`, `chunks_live`, demote/free counters |
| RF-L9 | **Superseded:** no full-allocation environment selector exists. Rollback keeps the candidate disabled and repairs the origin-backed source path. |
| RF-L10 | **Superseded for product NBD:** origin identity and a bounded physical cache cap replace the historical sparse/full allocation preflight split. |

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-L1 | No WSL hard freeze; swapoff-first + no kill -9 with live nbd (existing) |
| NFR-L2 | Alloc path single-threaded with CUDA affinity (existing daemon model) |
| NFR-L3 | Reclaim single-pass / rate-limited (Kahneman #15 — no thrash loop) |
| NFR-L4 | Host safety: live-host pressure requires the bounded cgroup probe and Windows watchdog; no unsupervised thrash |
| NFR-L5 | Default path remains NBD on WSL2 (ublk NO-GO unchanged) |

## 6. Flows

### 6.1 Idle after boot

1. Boot unit → `up` → nbd 3 G **capacity**, **committed ≈ 0 + canary**.  
2. GPU free remains high (modulo canary + other apps).  
3. `swapon` shows nbd USED=0.

### 6.2 Pressure fills VRAM tier

1. zram fills (prio 200).  
2. Kernel writes to nbd → chunks allocate on write.  
3. `vram_committed` rises toward min(pressure, VRAM_MIB, free-floor guard).

### 6.3 Game needs VRAM

1. Canary sees free &lt; floor.  
2. DEMOTE: move pages to disk tier (prio −2).  
3. Free emptied chunks → GPU free rises.  
4. If still below floor → continue demote; never kill -9.

### 6.4 Cascade down

1. swapoff → disconnect → free all chunks → exit.

## 7. Data model

```text
capacity_bytes     = VRAM_MIB * 1MiB          # advertised NBD size
chunk_bytes        = configurable (e.g. 128MiB)
chunk[i].state     = Empty | Committed | Demoting
chunk[i].cuda_mem  = Option<DeviceMem>        # only if Committed
committed_bytes    = sum(Committed)
```

## 8. API / interfaces

- **No** new uAPI for kernel.  
- Daemon: internal `SparseVramBackend` implementing `BlockBackend`.  
- The reusable sparse component may retain its own bounded configuration, but it
  does not select or fall back the product NBD backend.
- Telemetry JSONL fields (extend existing).

## 9. Dependencies and risks

| Risk | Mitigation |
| --- | --- |
| Sparse read/write bugs → corruption | Chunk CRC optional lab; property tests; keep product NBD disabled until the origin path is corrected |
| Alloc under write fails mid-I/O | Fail I/O; trigger demote; never partial silent |
| Fragmentation of CUDA allocs | Fixed chunk size; free list |
| Latency spike on first write | Historical cold-start trade-off; no full-allocation fallback exists |
| WSL dxgkrnl + many alloc/free | Rate-limit free; reuse pools |

## 10. Implementation strategy

1. PRD (this) → SPEC → AUDIT-2.5 **GO**.  
2. Implement `SparseVramBackend` behind trait; unit tests with `FakeVram`.  
3. Product NBD now requires the authoritative origin before backend selection;
   there is no legacy fallback selector.
4. Future isolated qualification must prove the source contract under a separate
   attended approval; it is **BLOCKED** in this record.

## 11. Documents to update

- This folder PRD/SPEC/AUDIT-2.5/IMPL  
- `cascade-transport-policy` pointer (capacity vs commit)  
- `validation.md` after live gates  
- `docs/INDEX.md`  

## 12. Out of scope

- Product ublk on WSL2  
- HMM / mainline LKM (see `mainline-vram-tiering`)  
- Changing swap priorities  
- MS kernel PR (issue-only advocacy if needed)  
- Automatic resize of `VRAM_MIB` without conf  

## 13. Acceptance criteria

- [ ] `up` with VRAM_MIB=3072: GPU free drop ≤ canary + O(chunk) slack, **not** ≈3072 MiB  
- [ ] After pressure: nbd USED &gt; 0 ⇒ committed ≥ used (accounting)  
- [ ] After demote/reclaim idle: committed falls; GPU free rises (measured)  
- [x] The historical full-allocation selector is removed and cannot restore a
  product path.
- [ ] Pressure order still zram → nbd → disk  
- [ ] No ghost swap; down clean  

## 14. Historical validation plan (not current authorization)

The commands and live measurements below describe the original proposal only.
They do not close the current source gate and must not be executed from this
document. Current live qualification, release, and activation remain **BLOCKED**.

- Unit: sparse map pure logic.  
- Integration: daemon + nbd with FakeVram or CUDA.  
- Historical live proposal: compare idle GPU free, pressure order, and demotion
  telemetry only on a separately approved isolated surface.
- **Not** a full-VM thrash instruction for a live WSL2 host.

## 15. Microsoft alignment summary (one screen)

| MS principle | Our RF |
| --- | --- |
| Reclaim unused resources (`autoMemoryReclaim`) | RF-L5, RF-L6 |
| Sparse provision (`sparseVhd`) | Unsafe-lab-only opt-in; excluded from RF-L1–L4 production behavior |
| Balloon / return under pressure | RF-L5 + free chunks |
| Opt-in experimental → default | RF-L9 kill-switch; feature flag in conf |
| Don’t freeze host | NFR-L1, NFR-L4 |
| Don’t fork product into stock MS kernel | Userspace + optional custom modules only |
