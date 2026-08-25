# IMPL — cascade-vram-ondemand

> Historical Passo 3 SSDV3 record for the 2026-07-11 sparse experiment.
> **Status (2026-08-22):** **SUPERSEDED for product NBD.** The source-removal
> gate is **CLOSED**, but live qualification, release promotion, and activation
> are **BLOCKED**. The retained observations below do not authorize a host, GPU,
> WSL, device, or pressure action.

## Status gates

| Gate | Result | Evidence |
| --- | --- | --- |
| Historical source/unit record | **RETAINED** | Measurements from the recorded 2026-07-11 candidate only |
| Historical idle observation | **RETAINED** | Δ free ≈ **212 MiB** in that recorded build |
| Historical pressure observation | **RETAINED** | zram t=1s → nbd t=6s; exit 0 in that recorded build |
| Current product gate | **BLOCKED** | Live origin/NBD/GPU qualification, release promotion, and activation remain open |

## RF / ITEM → files

| ID | Files |
| --- | --- |
| ITEM-1 | `crates/ramshared-block/src/sparse_vram.rs`, `lib.rs` |
| ITEM-2 | `sparse_vram::try_reclaim`; `ramsharedd` worker `recv_timeout` + skip Latency/FreeFloor swapoff on sparse |
| ITEM-3 | stderr counters on reclaim (`freed … MiB live=`) |
| ITEM-4 | `cascade-preflight.sh` sparse gate; `cascade.conf.example` |
| ITEM-5 | unit tests + live numbers above |

## Small decisions

1. The historical sparse/full runtime choice is removed from product NBD; there
   is no environment selector that restores full allocation.
2. CUDA context overhead (~200 MiB) counted as slack (SPEC was 64 MiB; live ~212 — document 256 MiB slack).  
3. Sparse does **not** swapoff on Latency/FreeFloor (false DEMOTE); only Corruption demotes via swapoff.  
4. Reclaim ticks every 5s via `recv_timeout` even without NBD I/O.

## Live numbers (2026-07-11)

| Metric | Value |
| --- | --- |
| capacity | 3072 MiB NBD |
| idle Δ free | 212 MiB |
| pressure | zram first, nbd second |
| after pressure free | 4067 MiB |
| after idle reclaim free | 4408 MiB (+341) |
| nbd | remains mounted |

## Rollback

- Keep the candidate disabled and restore only an exact reviewed origin-capable
  source snapshot needed to repair a regression.
- Do not restore a full-allocation selector or start a product path from this
  historical record.

**Rollback trigger:** idle `up` Δ free ≈ VRAM_MIB; or nbd vanishes without operator down; or ghost swap.

## Traceability

PRD RF-L1..L10 → SPEC ITEM-1..5 → this IMPL.  
Commit(s): see `git log` for `cascade-vram-ondemand` / sparse.
