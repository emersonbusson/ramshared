# SPEC — cascade-vram-ondemand

> Implements [`PRD.md`](PRD.md). Zero creativity out of scope.  
> **Does not** change swap priorities or transport policy (NBD Day-1 on WSL2).  
> **2026-08-22 Day-0 update:** the full-VRAM NBD selector and composition were
> removed. The original PRD/IMPL remain historical records; this living SPEC
> supersedes their availability and rollback language. The source-removal gate
> is **CLOSED** only: live qualification, release promotion, and activation
> remain **BLOCKED**, and this document authorizes no host, GPU, WSL, device, or
> pressure action.

## Traceability

| PRD | ITEM |
| --- | --- |
| RF-L1, RF-L2, RF-L3, RF-L4 | ITEM-1 SparseVramBackend |
| RF-L5, RF-L6, RF-L7 | ITEM-2 Reclaim / demote free |
| RF-L8 | ITEM-3 Telemetry |
| RF-L9 (superseded), RF-L10 | ITEM-4 selector removal + origin-only product boundary |
| NFR-L1..L5 | ITEM-5 Safety + tests |

## Files

| Path | Action |
| --- | --- |
| `crates/ramshared-block/src/sparse_vram.rs` (or `vram_sparse.rs`) | **create** — chunk map + `BlockBackend` |
| `crates/ramshared-block/src/lib.rs` | export |
| `crates/ramshared-wsl2d/src/main.rs` | require origin-cache for product NBD; remove the full-VRAM composition |
| `crates/ramshared-vram/src/lib.rs` | no trait break unless `free` helper needed (Drop already frees) |
| `tools/ci/check-legacy-preallocation-removal.mjs` | reject executable selectors/composition and current-doc availability claims |
| `docs/specs/.../IMPL.md` | retain historical Passo 3 measurements; current closure is recorded in origin IMPL/validation |

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

Construction uses one typed `SparseVramConfig` for capacity, chunk geometry,
reserve floor, commit cap, and optional host budget gate. Existing convenience
constructors remain as convenience constructors. The daemon uses the named
configuration so safety limits cannot be silently transposed among positional
integer arguments.

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

**Current boundary (Day-0, confirmed in code):**

- `SparseVramBackend` retains its focused on-demand allocation and reclaim
  tests as a reusable block component.
- Product NBD startup requires an authoritative origin and reports
  `mode=origin-cache`; it has no sparse/full-allocation selector.
- Historical sparse tests still cover stderr reclaim counts and in-process
  `alloc_fails`, `reclaim_frees`, `chunks_live()`, and committed bytes.

Optional daemon `--telemetry-jsonl` is the **residency/canary** stream (broader than sparse-only). It is **not** required to emit the exact field names below as a separate sparse schema.

**Logical fields** (map to counters/logs above; may appear in future sparse JSON line):

```text
vram_capacity_mib
vram_committed_mib
vram_chunks_live
vram_chunks_total
vram_reclaim_frees
vram_alloc_fails
vram_mode=sparse
```

## ITEM-4 — Day-0 selector removal and product boundary

| Surface | Current state |
| --- | --- | --- |
| `RAMSHARED_VRAM_PREALLOC`, `RAMSHARED_VRAM_PREALLOC_LEGACY`, and `RAMSHARED_VRAM_SPARSE_EXPERIMENTAL` | These selectors were removed from executable source. |
| Single NBD startup | Requires `--origin /dev/disk/by-partuuid/<uuid>` before backend or device work. |
| `RAMSHARED_VRAM_CHUNK_MIB` / `RAMSHARED_VRAM_IDLE_FREE_SEC` | Retained only for the reusable sparse component; they do not select the product NBD backend. |

### Product preflight

- Logical capacity remains independent from physical VRAM commitment.
- The product path opens and validates the authoritative origin before NBD
  selection; unavailable GPU measurement yields a zero cache target while the
  origin remains the correctness boundary.
- Reintroducing a full-capacity NBD allocation selector is a Day-0 failure, not
  a rollback or preflight mode.

## ITEM-5 — Safety + tests

| Test | Expect |
| --- | --- |
| Unit: read empty → zeros, no alloc | Fake provider alloc count 0 |
| Unit: write then read | data roundtrip; alloc count 1 |
| Unit: cross-chunk write | 2 allocs |
| Static: `legacy_preallocation_removed_before_day0_deadline` | selector aliases, profile chooser, and NBD full-VRAM composition absent |
| Live: `up` VRAM_MIB=3072, used_kb=0 | `Δ free_GPU` ≤ canary + CUDA context slack **≤ 256 MiB** (driver overhead) — **not** ≈3072 |
| Live: sparse preflight | boot OK with free_vram &lt; VRAM_MIB+headroom if free ≥ sparse gate |
| Live: pressure order | `sudo bash scripts/safety/cascade-pressure-probe.sh --prove-disk` → zram → nbd → disk |
| Live: after pressure release used_kb→0 + idle | committed falls; free_GPU rises |
| Product selection | originless NBD refuses before backend/device mutation; origin-backed NBD remains legitimate |
| Safe daemon wiring: `daemon_args_refuse_invalid_or_unsafe_combinations_before_backend`, `daemon_command_timeout_terminates_child_without_hang` | injected argv and harmless child process; unsafe plans fail before CUDA, swap, NBD client, or ublk setup |

**Note:** `scripts/safety/cascade-pressure-probe.sh` is a **real** host-safe harness (cgroup MemoryMax; in git since `06957fe`). Not a placeholder.

### Lock / concurrency

- All sparse ops on **daemon I/O thread** (CUDA affinity). No new locks across threads in MVP.

### Rollback trigger

- Any reappearance of an executable selector or full-VRAM NBD composition
  fails the named checker and blocks the source gate.
- Any origin-backed NBD, broker, ublk, Windows, or existing-test regression
  keeps the candidate off and restores only the exact reviewed origin-capable
  source snapshot. The removed selector is never restored.

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

1. Add the named removal checker test and observe RED.
2. Remove selector aliases, profile selection, and the NBD full-VRAM
   `VramBackend` composition while retaining generic consumers.
3. Require origin-backed NBD at the parsed action boundary.
4. Run focused source/static, formatting, lint, and documentation gates.
5. Record source closure without promoting any live gate.

### Entry-point boundary (shared with memory-broker DT-46)

`crates/ramshared-wsl2d/src/main.rs` keeps origin-cache and hardware selection
behind a parsed, validated plan. Originless product NBD refuses before CUDA,
swap, NBD-client, or ublk work. Safe local fixtures cannot stand in for live
GPU/NBD evidence; `BINARY_MATCH` and live E2E remain explicitly deferred until
an approved disposable environment is used.

The named sparse-policy construction and convenience constructors are covered by:

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-block --files crates/ramshared-block/src/sparse_vram.rs --min 80 --report-json tmp/cascade-vram-ondemand-policy-cov.json
```

<!-- rust-slice-test-only-localization-differential-v1
{
  "schema_version": 1,
  "id": "vulkan-icd-dispatch-validation-test",
  "kind": "rust-test-only-localization-differential",
  "files": [
    "crates/ramshared-vulkan/src/lib.rs"
  ],
  "verifications": [
    {
      "source": "crates/ramshared-vulkan/src/lib.rs",
      "package": "ramshared-vulkan",
      "test_module": "tests",
      "cargo_test": [
        "cargo",
        "test",
        "-p",
        "ramshared-vulkan",
        "--lib"
      ],
      "ignored_gpu_tests": [
        {
          "name": "open_enumerates_device_and_heap",
          "command": [
            "cargo",
            "test",
            "-p",
            "ramshared-vulkan",
            "--",
            "--ignored",
            "--test-threads=1"
          ],
          "evidence": "validation.md"
        },
        {
          "name": "vulkan_roundtrip_write_then_read",
          "command": [
            "cargo",
            "test",
            "-p",
            "ramshared-vulkan",
            "--",
            "--ignored",
            "--test-threads=1"
          ],
          "evidence": "validation.md"
        }
      ]
    }
  ]
}
-->
