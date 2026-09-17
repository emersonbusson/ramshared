# SPEC — Elastic Cooperative VRAM Tiering, Dynamic Host Borrowing, and Non-Blocking SSD Spillover

> SSDV3 Step 2 · PRD: `docs/specs/no-milestone/elastic-vram-cooperative-tier/PRD.md`

## 1. Closed Scope

### In Now
- **Dynamic Headroom Governor (`DynamicHeadroomGovernor`)** in `crates/ramshared-wsl2d`: High-frequency polling (50 ms) of GPU free memory via CUDA/DXG, with 3-state hysteresis: `Green` ($\ge 1200\text{ MiB}$), `Yellow` ($[768, 1200)\text{ MiB}$), and `Red` ($< 768\text{ MiB}$).
- **Elastic Chunk Extent Router (`ElasticExtentTable`)** in `crates/ramshared-block`: Sparse 64 MiB chunk allocation, tracking resident VRAM chunks vs SSD-backed chunks with atomic location updates.
- **Non-Blocking DMA Watchdog** in `crates/ramshared-wsl2d` / `crates/ramshared-block`: 50 ms bounded timeout circuit breaker for GPU DMA transfers, failing over to SSD Tier 3 without blocking NBD worker threads.
- **Asynchronous Eviction Worker**: Sub-100ms background eviction pipeline flushing cold VRAM chunks to SSD Tier 3 upon Governor entering `Red` state.
- **Unit and Property Tests**: Test coverage verifying state machine hysteresis, chunk eviction, and non-blocking spillover under simulated GPU pressure.

### Out Now
- Kernel-space custom block driver (`rs_vram.ko` / LKM) — Phase 2 in `trovaldo.md`.
- Hardware-level GPUDirect P2P DMA bypass to NVMe SSD without host RAM staging.
- Direct modifications to Windows host display drivers (WDDM/D3D).

### Assumed-Ready Dependencies
- Direct3D/DXG kernel device node (`/dev/dxg`) and CUDA runtime library (`libcuda.so.1`).
- Existing `AuthoritativeOriginBackend` and `BestEffortCache` traits in `crates/ramshared-block/src/isolated_origin.rs`.
- Linux NBD kernel module (`/dev/nbd0`) and backing swap partition (canonical origin partition or container).

---

## 2. Traceability

| PRD Requirement | Technical Decision | Implementation Items | Covered By Test / Evidence |
| :--- | :--- | :--- | :--- |
| **`RF-1`** (Headroom Governor) | `DT-1` | `ITEM-1` | `test_governor_three_zone_hysteresis`, `test_governor_rapid_fluctuation_damping` |
| **`RF-2`** (Sparse Extents) | `DT-2` | `ITEM-2` | `test_sparse_extent_allocation_on_demand`, `test_sparse_extent_boundary_alignment` |
| **`RF-3`** (SSD Spillover) | `DT-3` | `ITEM-3` | `test_spillover_on_dma_timeout`, `test_spillover_during_red_zone_preserves_data` |
| **`RF-4`** (Sub-100ms Eviction) | `DT-4` | `ITEM-4` | `test_eviction_worker_flushes_cold_chunks_under_deadline`, `test_eviction_frees_vram_memory` |
| **`RF-5`** (Elastic Recovery) | `DT-5` | `ITEM-5` | `test_elastic_recovery_promotes_on_green_settle` |
| **`NFR-1`** (Zero Freeze) | `DT-1`, `DT-3` | `ITEM-1`..`ITEM-5` | `PASS_ZERO_PANIC` under live 500% memory stress |
| **`NFR-2`** (Bounded Latency) | `DT-3` | `ITEM-3` | NBD request latency $\le 50\text{ ms}$ under fault injection |
| **`NFR-3`** (Data Integrity) | `DT-2`, `DT-4` | `ITEM-2`, `ITEM-4` | SHA-256 block pattern verification post-eviction |
| **`NFR-4`** (Observability) | `DT-1`..`DT-5` | `ITEM-1`..`ITEM-5` | Structured JSONL event verification in telemetry logs |

---

## 3. Technical Decisions

| # | Decision | Why |
| :--- | :--- | :--- |
| **`DT-1`** | **Three-Zone Headroom Governor with 3000ms Green Settle**: Define `Green` ($\ge 1200\text{ MiB}$), `Yellow` ($[768, 1200)\text{ MiB}$), and `Red` ($< 768\text{ MiB}$), with 3000 ms stability timer before leaving `Red`/`Yellow` back to `Green`. | Prevents thrashing (rapid allocation and eviction loops) when host GPU memory fluctuates near boundaries while protecting Windows DWM with a guaranteed 768 MiB buffer. |
| **`DT-2`** | **Sparse 64 MiB Extent Geometry**: Allocate VRAM in discrete 64 MiB chunks indexed in an in-memory extent table rather than a contiguous single buffer. | Enables granular, non-blocking eviction. Freeing 64 MiB takes $< 5\text{ ms}$, allowing incremental VRAM return to Windows without re-allocating or moving other pages. |
| **`DT-3`** | **Circuit-Breaker DMA Watchdog (50 ms deadline)**: Wrap all GPU transfers in `std::sync::mpsc` or timeout channel. If transfer latency exceeds 50 ms or ioctl returns error (`-22`), immediately divert write to Tier 3 SSD. | Eliminates the single point of failure that caused today's 08:05:51 freeze. Kernel swap never blocks waiting on a stalled GPU driver. |
| **`DT-4`** | **Atomic Extent Migration**: Transitioning a chunk from `Vram` to `Ssd` flushes dirty pages to disk first, updates the pointer in `ElasticExtentTable`, and only then invokes `cudaFree`. | Guarantees zero data loss or stale reads if a read arrives concurrently during background eviction. |
| **`DT-5`** | **Integration with `AuthoritativeOriginBackend`**: Wire the elastic VRAM cache into `AuthoritativeOriginBackend` as an implementation of `BestEffortCache`. | Reuses existing, battle-tested bounded cache architecture in `crates/ramshared-block` instead of creating redundant cache abstraction layers. |

---

## 4. Atomicity and Rollback

- **Atomicity Frontier**:
  - Extent table mutations (`ChunkLocation::Vram` $\leftrightarrow$ `ChunkLocation::Ssd`) are guarded by an internal reader-writer lock (`RwLock<ElasticExtentTable>`).
  - Read/write operations obtain read locks; eviction and allocation obtain write locks only for the single chunk being updated (per-chunk granularity or fast pointer swap).
  - During eviction, the SSD write is completed and fsynced before the extent pointer is updated and VRAM is freed.
- **Rollback by Layer**:
  - **Userspace / Daemon**: If the elastic cache encounters an unrecoverable internal error, it calls `BestEffortCache::disable()`, causing `AuthoritativeOriginBackend` to fall back immediately to raw SSD origin I/O without stopping the daemon or crashing `/dev/nbd0`.
  - **Kernel / Module**: Linux kernel swap subsystem remains completely isolated behind the NBD protocol. No kernel module mutations occur.
  - **Host / Persistent**: Physical origin file on Windows SSD retains authoritative swap state. VRAM is ephemeral.

---

## 5. Kahneman Map (Critical Steps)

| ITEM / Stage | # | Question | Min Evidence | Abort |
| :--- | :--- | :--- | :--- | :--- |
| **ITEM-1** (Governor) | **#13** (Refusal + Legitimate) | Does the governor refuse VRAM allocation when free memory is $< 1200\text{ MiB}$ while accepting when $\ge 1200\text{ MiB}$? | `cargo test -p ramshared-wsl2d test_governor_three_zone_hysteresis` | Any VRAM chunk allocation accepted in Yellow or Red zones |
| **ITEM-2** (Sparse Extents) | **#17** (Idempotence & Replay) | Does accessing the same block multiple times return identical data across VRAM and SSD? | `cargo test -p ramshared-block test_sparse_extent_idempotent_read_write` | Data corruption or byte mismatch |
| **ITEM-3** (Watchdog) | **#15** (Transient Retry / Failover) | Does the watchdog divert to SSD within 50 ms when GPU DMA ioctl hangs or fails with `-22`? | `cargo test -p ramshared-block test_spillover_on_dma_timeout` | NBD request hangs $> 50\text{ ms}$ or returns `NBD_EIO` |
| **ITEM-4** (Eviction) | **#16** (Exhaustion Behavior) | Under sudden host VRAM exhaustion, can 1,024 MiB be evicted to SSD within $< 100\text{ ms}$? | `cargo test -p ramshared-block test_eviction_worker_flushes_cold_chunks_under_deadline` | Eviction takes $> 100\text{ ms}$ per GB or leaks GPU allocations |
| **ITEM-5** (Live Cascade) | **#9** (Numeric Verification) | Does multi-tier cascade stress achieve $> 0\text{ MB}$ Tier 3 SSD usage under heavy load with zero kernel oops or freezes? | Live stress execution + `/proc/swaps` telemetry + `PASS_ZERO_PANIC` | Any kernel panic, hung task in `dmesg`, or Windows desktop freeze |

---

## 6. Security Checklist (Pre-Impl)

- [x] **Privilege**: Daemon requires standard `CAP_SYS_ADMIN` inside WSL2 for NBD block device attachment; no elevated Windows administrator privileges required.
- [x] **User/Host Copy**: Block buffers are strictly bounded to chunk sizes (64 MiB); bounds checks validated at every request boundary.
- [x] **Flags/IOCTL Codes**: Uses standard CUDA memory management calls (`cudaMalloc`, `cudaFree`, `cudaMemcpyAsync`); rejects unknown ioctls.
- [x] **Info-Leak**: Ephemeral VRAM chunks are zeroed upon initial allocation before exposure to swap; no residual host memory leaked to Linux.
- [x] **IRQ / IRQL**: Ring-3 userspace operations only; no sleep-in-atomic or invalid IRQL contexts.
- [x] **Lifetime**: Allocated VRAM chunk pointers tracked in extent table with drop guards guaranteeing `cudaFree` execution on daemon exit.
- [x] **Hot-Unplug / Device-Gone**: If `/dev/dxg` drops, the watchdog triggers immediate permanent demotion to SSD origin (`BestEffortCache::disable()`).
- [x] **Host Safety**: Enforces strict minimum 768 MiB physical VRAM cushion for Windows host display at all times.
- [x] **Shared-Hardware Cushion**: Mathematical host reserve floor dynamically enforced; zero greedy static allocations.
- [x] **Bounded DMA / Foreign Driver Calls**: 50 ms watchdog ensures no worker thread hangs in foreign driver ioctls.
- [x] **Cooperative Cascade Spillover**: Writes automatically spill to Tier 3 SSD when VRAM accelerator is saturated or constrained.
- [x] **Replayable Ops**: Block reads, writes, and eviction flushes are idempotent (#17).

---

## 7. Files to CREATE / MODIFY / DELETE

### CREATE

**`crates/ramshared-block/src/elastic_cache.rs`**
- **Purpose**: Implement `ElasticVramCache` implementing `BestEffortCache`, containing `ElasticExtentTable`, dynamic 64 MiB chunk commitment, and sub-100ms eviction logic.
- **RF / DT**: RF-2, RF-3, RF-4, RF-5; DT-2, DT-3, DT-4, DT-5.
- **Types / Functions**:
  ```rust
  pub struct ElasticVramCache<P: VramProvider> { ... }
  impl<P: VramProvider> BestEffortCache for ElasticVramCache<P> { ... }
  pub struct ElasticExtentTable { ... }
  ```
- **Required tests**: `crates/ramshared-block/src/elastic_cache.rs` :: `test_sparse_extent_allocation_on_demand`, `test_spillover_on_dma_timeout`, `test_eviction_worker_flushes_cold_chunks_under_deadline`.
- **Cover target**: $\ge 80\%$.
- **Kahneman**: #13, #15, #16, #17.

**`crates/ramshared-wsl2d/src/governor.rs`**
- **Purpose**: Implement `DynamicHeadroomGovernor` tracking host VRAM free memory, 3-zone hysteresis, and eviction signals.
- **RF / DT**: RF-1, RF-4; DT-1.
- **Types / Functions**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum HeadroomZone { Green, Yellow, Red }
  pub struct DynamicHeadroomGovernor { ... }
  impl DynamicHeadroomGovernor {
      pub fn new(low_watermark_bytes: u64, high_watermark_bytes: u64, settle_duration: Duration) -> Self;
      pub fn sample(&mut self, free_bytes: u64) -> (HeadroomZone, bool);
  }
  ```
- **Required tests**: `crates/ramshared-wsl2d/src/governor.rs` :: `test_governor_three_zone_hysteresis`, `test_governor_rapid_fluctuation_damping`.
- **Cover target**: $\ge 80\%$.
- **Kahneman**: #13.

### MODIFY

**`crates/ramshared-block/src/lib.rs`**
- **Purpose**: Export `elastic_cache` module and public types.
- **RF / DT**: DT-5.
- **Changes**: Add `pub mod elastic_cache;` and re-export `ElasticVramCache`.

**`crates/ramshared-wsl2d/src/main.rs`**
- **Purpose**: Wire `DynamicHeadroomGovernor` and `ElasticVramCache` into `DaemonAction::Nbd` when origin is present, replacing `DisabledCache` with active elastic tiering.
- **RF / DT**: RF-1..RF-5, DT-1..DT-5.
- **Changes**:
  - Wire governor poller thread sampling CUDA free memory every 50 ms.
  - Connect governor signals to `ElasticVramCache::evict_batch()` and spillover modes.
- **Tests**: `cargo test -p ramshared-wsl2d`.

---

## 8. Observability

| Signal | Destination | Level / Type | Description |
| :--- | :--- | :--- | :--- |
| `headroom_transition` | Telemetry JSONL / Stderr | INFO | Headroom zone changed (`Green` $\leftrightarrow$ `Yellow` $\leftrightarrow$ `Red`) with free VRAM MiB |
| `chunk_committed` | Telemetry JSONL | DEBUG | New 64 MiB chunk committed to VRAM |
| `chunk_evicted` | Telemetry JSONL / Stderr | INFO | Chunk evicted from VRAM to SSD with latency ms |
| `dma_watchdog_tripped` | Telemetry JSONL / Stderr | WARN | DMA transfer exceeded 50 ms; spillover engaged |
| `cache_stats` | `ramshared status --json` | JSON | Active VRAM chunks, SSD chunks, and current zone |

---

## 9. Living Docs

| Document | Action |
| :--- | :--- |
| `ARCHITECTURE.md` | Document elastic VRAM cache, thin-provisioning, and non-blocking Tier 3 spillover. |
| `docs/INDEX.md` | Register `elastic-vram-cooperative-tier`. |
| `docs/reliability/DEGRADATION-MATRIX.md` | Record host GPU memory pressure handling and sub-100ms eviction. |
| `validation.md` | Record live multi-tier cascade stress qualification. |

---

## 10. Implementation Order

- **`ITEM-1`**: Implement `DynamicHeadroomGovernor` in `crates/ramshared-wsl2d/src/governor.rs` with three-zone hysteresis and unit test coverage.
- **`ITEM-2`**: Implement `ElasticExtentTable` in `crates/ramshared-block/src/elastic_cache.rs` managing sparse 64 MiB chunk mapping and allocation.
- **`ITEM-3`**: Implement bounded DMA watchdog and non-blocking spillover logic in `ElasticVramCache`.
- **`ITEM-4`**: Implement asynchronous eviction worker in `ElasticVramCache` guaranteeing sub-100ms VRAM release upon Red zone transition.
- **`ITEM-5`**: Wire `DynamicHeadroomGovernor` and `ElasticVramCache` into `crates/ramshared-wsl2d/src/main.rs`, replacing `DisabledCache` in the authoritative origin path.

---

## 11. Required Tests Matrix

| Production Path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| :--- | :--- | :--- | :--- | :--- |
| `crates/ramshared-wsl2d/src/governor.rs` | `governor::tests::test_governor_three_zone_hysteresis` | unit | #13 | $\ge 80\%$ |
| `crates/ramshared-wsl2d/src/governor.rs` | `governor::tests::test_governor_rapid_fluctuation_damping` | unit | #13 | $\ge 80\%$ |
| `crates/ramshared-block/src/elastic_cache.rs` | `elastic_cache::tests::test_sparse_extent_allocation_on_demand` | unit | #17 | $\ge 80\%$ |
| `crates/ramshared-block/src/elastic_cache.rs` | `elastic_cache::tests::test_spillover_on_dma_timeout` | unit | #15 | $\ge 80\%$ |
| `crates/ramshared-block/src/elastic_cache.rs` | `elastic_cache::tests::test_eviction_worker_flushes_cold_chunks_under_deadline` | unit | #16 | $\ge 80\%$ |
| `crates/ramshared-block/src/elastic_cache.rs` | `elastic_cache::tests::test_sparse_extent_idempotent_read_write` | unit | #17 | $\ge 80\%$ |

---

## 12. Validation Checklist

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test -p ramshared-block -p ramshared-wsl2d`
- [ ] Cover gate: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-block,ramshared-wsl2d --files crates/ramshared-block/src/elastic_cache.rs,crates/ramshared-wsl2d/src/governor.rs --min 80 --report-json tmp/elastic-vram-cooperative-tier-cov.json`
- [ ] `./scripts/docs-check.sh`
- [ ] Every matrix row has a real test name
- [ ] Kahneman critical rows have executable evidence

<!-- rust-slice-structural-contract-v1
{
  "schema_version": 1,
  "id": "elastic-vram-cooperative-tier-structural",
  "kind": "rust-structural-contract",
  "files": [
    "crates/ramshared-wsl2d/src/lib.rs"
  ],
  "verifications": [
    {
      "source": "crates/ramshared-wsl2d/src/lib.rs",
      "package": "ramshared-wsl2d",
      "cargo_test": [
        "cargo",
        "test",
        "-p",
        "ramshared-wsl2d",
        "--lib"
      ]
    }
  ]
}
-->

