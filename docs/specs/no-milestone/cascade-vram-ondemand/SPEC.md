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

### Threading (mandatory)

- Reclaim runs **only on the CUDA I/O worker thread** (same thread that owns `SparseVramBackend` and processes `WMsg::Job`).
- Algorithm before free:
  1. Drain is natural: reclaim is scheduled between jobs (after a canary sample or timer msg), never from a side thread.
  2. Read `/proc/swaps` nbd `used_kb`.
  3. If `used_kb > 0` → log `reclaim_blocked_used`; **return** (no free).
  4. If free-floor or idle hysteresis matches → `drop` all Live chunk `mem` (Empty).
  5. Re-read `used_kb`; if now `> 0`, log `reclaim_race_used_after` (should be rare: new I/O only via this thread; kernel cannot dirt without write path). Do **not** re-alloc.

### Triggers

| Trigger | Action |
| --- | --- |
| Canary cadence (`CANARY_EVERY` = 64 I/Os) or idle timer | Sample free; maybe reclaim |
| Residency `Verdict::Demote` (latency/corruption) | Existing demote telemetry; **MVP does not** free chunks if `used_kb > 0` |
| Worker sees free &lt; floor **and** `used_kb == 0` | Free all Live chunks |
| Worker sees idle ≥ `IDLE_FREE_SEC` **and** `used_kb == 0` | Free all Live chunks |

### MVP reclaim table

No per-PTE reverse map. Track `written: bool` + `last_write_ts` per chunk.

| Case | Action |
| --- | --- |
| `up` idle | no full prealloc (RF-L1); committed ≈ **canary only** (`CANARY_BYTES` = 4096) |
| writes under pressure | chunk alloc on first write |
| `used_kb == 0` + (free &lt; floor **or** idle ≥ `IDLE_FREE_SEC`) | free **all** Live chunks |
| `used_kb > 0` + free &lt; floor | **no free**; log `reclaim_blocked_used`; kernel may use disk tier for **new** pages |
| `down` | free all chunks + canary |

### ITEM-2b (phase 2 — not in MVP IMPL)

Mid-flight spill while `used_kb > 0`: mirror Live chunks to RAM/file, free CUDA, serve I/O from mirror. Separate SPEC revision + new AUDIT-2.5.

## ITEM-3 — Telemetry

**Product path (Day-0, confirmed in code):**

- Startup stderr: `VRAM mode=sparse|prealloc` with capacity / chunk / commit_cap / reserve / committed.
- On reclaim: stderr with freed MiB + `live=` chunk count.
- In-process counters on `SparseVramBackend`: `alloc_fails`, `reclaim_frees`, `chunks_live()`, committed bytes via live×chunk.

Optional daemon `--telemetry-jsonl` is the **residency/canary** stream (broader than sparse-only). It is **not** required to emit the exact field names below as a separate sparse schema.

**Logical fields** (map to counters/logs above; may appear in future sparse JSON line):

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
| `RAMSHARED_VRAM_CHUNK_MIB` | 128 | chunk size (16..512) |
| `RAMSHARED_VRAM_IDLE_FREE_SEC` | 30 | idle free when used_kb==0 |

### Preflight (sparse default) — **revised AUDIT-2.5**

| Mode | Gate |
| --- | --- |
| **sparse** (default) | `free_vram >= MIN_VRAM_HEADROOM_MIB + ceil(CANARY_BYTES) + CHUNK_MIB` — enough to **start** and take first write. Log: `preflight sparse: capacity=VRAM_MIB MiB commit_gate=… (not full prealloc)`. |
| **prealloc** (`RAMSHARED_VRAM_PREALLOC=1`) | keep legacy: `free >= VRAM_MIB + MIN_VRAM_HEADROOM_MIB` |

- `VRAM_MIB` remains **max advertised capacity** (NBD size), not “must be free at boot”.  
- Filling the full tier under pressure still needs free VRAM at write time; alloc fail → I/O error (ITEM-1).  
- Optional phase 2: `VRAM_COMMIT_CAP_MIB` soft cap on simultaneous commit.

## ITEM-5 — Safety + tests

| Test | Expect |
| --- | --- |
| Unit: read empty → zeros, no alloc | Fake provider alloc count 0 |
| Unit: write then read | data roundtrip; alloc count 1 |
| Unit: cross-chunk write | 2 allocs |
| Unit: prealloc flag path still compiles | feature flag |
| Live: `up` VRAM_MIB=3072, used_kb=0 | `Δ free_GPU` ≤ canary + CUDA context slack **≤ 256 MiB** (driver overhead) — **not** ≈3072 |
| Live: sparse preflight | boot OK with free_vram &lt; VRAM_MIB+headroom if free ≥ sparse gate |
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
