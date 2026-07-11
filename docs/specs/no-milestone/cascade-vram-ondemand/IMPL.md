# IMPL — cascade-vram-ondemand

> Passo 3 SSDV3. Implements [`SPEC.md`](SPEC.md). AUDIT-2.5: **GO**.  
> **Date:** 2026-07-11  
> **Status:** **GREEN** (live gates V3–V5; unit tests)

## Status gates

| Gate | Result | Evidence |
| --- | --- | --- |
| cargo test -p ramshared-block | **GREEN** | 32 passed (6 sparse) |
| cargo test -p ramshared-cli | **GREEN** | 23 passed |
| V3 idle up VRAM_MIB=3072 | **GREEN** | Δ free ≈ **212 MiB** (not ~3072) |
| V4 pressure order | **GREEN** | zram t=1s → nbd t=6s; exit 0 |
| V5 idle reclaim | **GREEN** | free 4067 → **4408** after ~40s idle |
| nbd stable | **GREEN** | 15s hold after up; still after pressure |
| mode log | **GREEN** | `VRAM mode=sparse capacity=3072 MiB chunk=128 MiB committed=0` |

## RF / ITEM → files

| ID | Files |
| --- | --- |
| ITEM-1 | `crates/ramshared-block/src/sparse_vram.rs`, `lib.rs` |
| ITEM-2 | `sparse_vram::try_reclaim`; `ramsharedd` worker `recv_timeout` + skip Latency/FreeFloor swapoff on sparse |
| ITEM-3 | stderr counters on reclaim (`freed … MiB live=`) |
| ITEM-4 | `cascade-preflight.sh` sparse gate; `cascade.conf.example` |
| ITEM-5 | unit tests + live numbers above |

## Small decisions

1. Sparse is **default**; `RAMSHARED_VRAM_PREALLOC=1` keeps Day-1 full alloc.  
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

- `export RAMSHARED_VRAM_PREALLOC=1` then `ramshared down && cascade-up`  
- Or revert this feature commit  

**Rollback trigger:** idle `up` Δ free ≈ VRAM_MIB; or nbd vanishes without operator down; or ghost swap.

## Traceability

PRD RF-L1..L10 → SPEC ITEM-1..5 → this IMPL.  
Commit(s): see `git log` for `cascade-vram-ondemand` / sparse.
