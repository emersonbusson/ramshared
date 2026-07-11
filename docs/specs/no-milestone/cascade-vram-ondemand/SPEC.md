# SPEC — cascade-vram-ondemand

> Implements [`PRD.md`](PRD.md). Zero creativity out of scope.  
> **Does not** change swap priorities or transport policy (NBD Day-1 on WSL2).  
> Parent kill-switch behaviour is mandatory for rollback.

## Traceability

| PRD | ITEM |
| --- | --- |
| RF-L1, RF-L2, RF-L3, RF-L4 | ITEM-1 SparseVramBackend |
| RF-L5, RF-L6, RF-L7 | ITEM-2 Reclaim / demote free |
| RF-L8 | ITEM-3 Telemetry |
| RF-L9, RF-L10 | ITEM-4 Flags + preflight |
| NFR-L1..L5 | ITEM-5 Safety + tests |

## Files

| Path | Action |
| --- | --- |
| `crates/ramshared-block/src/sparse_vram.rs` (or `vram_sparse.rs`) | **create** — chunk map + `BlockBackend` |
| `crates/ramshared-block/src/lib.rs` | export |
| `crates/ramshared-wsl2d/src/main.rs` | use sparse by default; prealloc if env |
| `crates/ramshared-vram/src/lib.rs` | no trait break unless `free` helper needed (Drop already frees) |
| `scripts/safety/cascade.conf.example` | document chunk + prealloc env |
| `docs/specs/.../IMPL.md` | Passo 3 after code |

## ITEM-1 — SparseVramBackend

### Constants (defaults)

| Name | Default | Notes |
| --- | --- | --- |
| `chunk_mib` | `128` | Env `RAMSHARED_VRAM_CHUNK_MIB` (power-of-two preferred; min 16, max 512) |
| `capacity` | from `--size` / VRAM_MIB | Unchanged NBD export size |

### Structure

```text
struct SparseVramBackend<P: VramProvider> {
  capacity: u64,
  chunk_bytes: u64,
  chunks: Vec<Chunk>,           // len = ceil(capacity / chunk_bytes)
  provider: P,                  // or & provider with GAT — match daemon affinity
  block_size: u32,              // existing 4096/512 policy
}

enum ChunkState { Empty, Live }
struct Chunk {
  state: ChunkState,
  mem: Option<P::Mem>,          // only Live
}
```

### Semantics

| Op | Behaviour |
| --- | --- |
| `size_bytes()` | `capacity` |
| `read_at` Empty | fill `dst` with **zeros**; no alloc |
| `read_at` Live | `mem.read_at` relative to chunk |
| `write_at` Empty | `provider.alloc(chunk_bytes)` → zero once → write → Live |
| `write_at` Live | write into mem |
| `flush` | no-op (sync copies) as today |

### Alloc failure on write

1. Log error with chunk index.  
2. Return `IoError` to NBD layer (guest gets I/O error on that swap write).  
3. Do **not** retry in a hot loop (#15).  
4. Optional: set atomic `alloc_fail` for telemetry.

### Alignment

- Offsets must be handled across chunk boundaries (split I/O like a normal striped backend).  
- Unit tests: cross-chunk write/read, read-empty, write-fail injection with Fake provider.

## ITEM-2 — Reclaim / demote free

### Triggers

| Trigger | Action |
| --- | --- |
| Existing residency `Verdict::Demote` | Existing demote path for pages **plus** `reclaim_empty_chunks()` |
| Periodic tick (same canary cadence) | If free &lt; floor: demote content if any; then free Live chunks with **no** outstanding guest-dirty tracking |

### MVP reclaim (shippable — **GO**)

No per-PTE reverse map in MVP. Track `written: bool` + `last_write_ts` per chunk.

| Case | Action |
| --- | --- |
| `up` idle | no full prealloc (RF-L1); committed ≈ canary only |
| writes under pressure | chunk alloc on first write |
| `nbd used_kb == 0` and (free &lt; floor **or** idle ≥ `IDLE_FREE_SEC`) | free **all** Live chunks (safe: no swap pages on device) |
| `nbd used_kb > 0` and free &lt; floor | **Do not** free Live chunks (would corrupt). Log `reclaim_blocked_used`. New pressure uses disk tier via kernel priorities. |
| `down` | free all chunks + canary |

This hits the primary pain: **do not hold 3 GiB on the GPU when idle / after pressure drained**.

### ITEM-2b (phase 2 — not in MVP IMPL)

Mid-flight spill while `used_kb > 0`: mirror Live chunks to RAM/file, free CUDA, serve I/O from mirror. Separate SPEC revision when needed.

## ITEM-3 — Telemetry

Log / JSONL at least once per canary tick:

```text
vram_capacity_mib
vram_committed_mib
vram_chunks_live
vram_chunks_total
vram_reclaim_frees
vram_alloc_fails
vram_mode=sparse|prealloc
```

## ITEM-4 — Flags + preflight

| Env / conf | Default | Effect |
| --- | --- | --- |
| `RAMSHARED_VRAM_PREALLOC` | unset/0 | sparse (new default) |
| `=1` / `true` | — | full `alloc(size)` Day-1 path |
| `RAMSHARED_VRAM_CHUNK_MIB` | 128 | chunk size |
| `RAMSHARED_VRAM_IDLE_FREE_SEC` | 30 | idle free when used_kb==0 |

Preflight:

- Keep `free >= VRAM_MIB + headroom` as **capacity feasibility** (user still needs headroom to **ever** fill the tier).  
- Add note in log: `capacity check (not commit)`.  
- Optional later: `VRAM_COMMIT_CAP_MIB` to refuse writes beyond commit cap (phase 2).

## ITEM-5 — Safety + tests

| Test | Expect |
| --- | --- |
| Unit: read empty → zeros, no alloc | Fake provider alloc count 0 |
| Unit: write then read | data roundtrip; alloc count 1 |
| Unit: cross-chunk write | 2 allocs |
| Unit: prealloc flag path still compiles | feature flag |
| Live: `up` VRAM_MIB=3072, used_kb=0 | `Δ free_GPU` ≤ canary + 1 chunk (+ slack 64 MiB) **not** ≈3072 |
| Live: pressure order | `sudo bash scripts/safety/cascade-pressure-probe.sh --prove-disk` → zram → nbd → disk |
| Live: after pressure release used_kb→0 + idle | committed falls; free_GPU rises |
| Kill-switch | PREALLOC=1 → Δ free ≈ size |

**Note:** `scripts/safety/cascade-pressure-probe.sh` is a **real** host-safe harness (cgroup MemoryMax; in git since `06957fe`). Not a placeholder.

### Lock / concurrency

- All sparse ops on **daemon I/O thread** (CUDA affinity). No new locks across threads in MVP.

### Rollback trigger

- GPU free after idle `up` still drops by ≈ VRAM_MIB → bug; set `RAMSHARED_VRAM_PREALLOC=1` and revert sparse default.  
- Any WSL freeze / ghost swap after sparse → prealloc + orphan recover path; open validation entry.

## Kahneman map

| # | Application |
| --- | --- |
| #2 | Rollback: free drop ≈ full size on idle up |
| #13 | Test idle free **and** write path **and** refuse free when used&gt;0 |
| #15 | No alloc retry storm |
| #16 | Prefer not free when used&gt;0 (safe default) |
| #17 | Free twice of empty chunk = no-op |
| #18 | Fix in daemon VRAM backend (owns CUDA), not kernel hack |

## Out of SPEC (MVP)

- ITEM-2b mid-flight spill while used&gt;0  
- ublk product  
- HMM  
- Changing MS kernel  

## Implementation order

1. Fake-backed unit tests RED  
2. SparseVramBackend GREEN  
3. Wire daemon + env  
4. Live gates  
5. IMPL.md  
