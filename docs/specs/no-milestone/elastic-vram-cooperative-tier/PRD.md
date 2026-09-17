---
slug: elastic-vram-cooperative-tier
title: Elastic cooperative VRAM tiering, dynamic host borrowing, and non-blocking SSD spillover
milestone: —
issues: []
---

# PRD — Elastic Cooperative VRAM Tiering, Dynamic Host Borrowing, and Non-Blocking SSD Spillover

## 1. Summary

RamShared accelerates memory workloads on WSL2 by utilizing GPU VRAM as an intermediate, ultra-fast Tier 2 swap layer situated between in-memory compressed ZRAM (Tier 1) and backing NVMe/SSD storage (Tier 3). Under the existing static broker model, the daemon attempts to reserve fixed VRAM blocks up-front (e.g. 4,096 MiB on a 6,144 MiB physical RTX 2060). When the Windows host Desktop Window Manager (`dwm.exe`) and host applications (browsers, 3D workloads, or games) consume memory, total GPU commitment breaches physical capacity, causing host NVIDIA driver escape failures (`dxgkio_escape: -22`). The userspace NBD daemon blocks synchronously waiting on GPU DMA, plunging the Linux kernel swap subsystem into an unrecoverable uninterruptible sleep (`D` state), deadlocking userspace and triggering a hypervisor watchdog termination (`Wsl/Service/E_UNEXPECTED`).

Adhering strictly to **SSDV3 Principle 11 (Shared Hardware & Tiering Coexistence)**, this PRD establishes an **Elastic Cooperative VRAM Tiering Architecture**. The architecture recognizes Windows as the sovereign, first-class owner of physical VRAM and treats RamShared as an elastic, opportunistic tenant. Rather than statically committing VRAM, the daemon uses thin-provisioned sparse allocation, monitors host GPU free headroom at high frequency (every 50 ms), actively evicts cold pages to Tier 3 SSD in under 100 ms whenever host VRAM pressure is detected (free headroom $< 768\text{ MiB}$), and transparently routes incoming swap writes directly to SSD without blocking the Linux kernel or failing NBD requests.

## 2. Technical Context

- **`crates/ramshared-wsl2d/src/main.rs`**: In `run_broker`, the daemon executes `provider.alloc(total)` and `mem.zero()` synchronously at startup, greedily claiming physical VRAM before any swap traffic arrives (`Confirmed in codebase`).
- **`crates/ramshared-wsl2d/src/main.rs`**: `calculate_safe_vram_slice` enforces `HOST_RESERVE_FLOOR = max(1536 MB, total_vram * 20%)`. On a 6,144 MiB GPU, this permits up to 4,608 MiB allocations at boot when host VRAM is at 1,400 MiB, leaving only ~640 MiB free and creating an immediate starvation hazard whenever Windows applications ramp up (`Confirmed in codebase`).
- **`crates/ramshared-wsl2d/src/main.rs`**: `ResilientBackend` holds an internal RAM mirror alongside VRAM, but `vram.write_at()` executes synchronous CUDA DMA. When `/dev/dxg` locks or rejects memory escapes with `-22`, the NBD worker thread stalls inside the kernel ioctl, propagating uninterruptible `D` state sleep to kernel swap threads (`Confirmed in codebase`).
- **`crates/ramshared-block/src/isolated_origin.rs`**: Defines `AuthoritativeOriginBackend<O, C: BestEffortCache>`, `BoundedCacheClient`, and `IsolatedCacheWorker`. This module already models a clean separation where the backing SSD origin is authoritative and the cache layer is strictly best-effort, non-blocking, and revocable (`Confirmed in codebase`).
- **`crates/ramshared-wsl2d/src/main.rs`**: In `DaemonAction::Nbd` when origin is present, `run_nbd` instantiates `AuthoritativeOriginBackend` with `DisabledCache`, completely disabling GPU caching in production and defaulting to raw SSD origin I/O (`Confirmed in codebase`).
- **PCIe Hardware Bandwidth**: NVIDIA RTX 2060 on PCIe 3.0 x16 achieves 12.0 to 14.0 GB/s bidirectional DMA transfer rate. Moving 1,024 MiB from VRAM to host RAM or NVMe staging buffer requires approximately 73 milliseconds (`Confirmed in docs`).
- **Linux Swap Allocation Hierarchy**: `/proc/swaps` priorities dictate strict top-down allocation: `zram0` (prio 100/200) $\rightarrow$ `nbd0` (prio 50/100) $\rightarrow$ SSD partition (prio -2). The Linux kernel will not spill writes into Tier 3 until Tier 1 and Tier 2 report 100% capacity fullness (`Confirmed in docs`).
- **Inference**: High-speed bursts of swap writes in WSL2 must never be rejected with `NBD_EIO`, as the Linux kernel marks failed swap blocks read-only and triggers immediate process SIGBUS or kernel panic. Redirection must happen transparently within userspace.

## 3. Recommended Option

### Option 1 (Recommended): Elastic Cooperative Sparse VRAM Tiering with Non-Blocking SSD Spillover

1. **Host-First Sovereignty & Dynamic Headroom Governor**:
   - Establish a three-zone dynamic VRAM headroom state machine sampled every 50 ms via CUDA/DXG telemetry:
     - **Green Zone (`free_vram >= 1,200 MiB`)**: Stable headroom. Daemon may allocate dynamic 64 MiB chunks to absorb swap writes up to the configured logical size.
     - **Yellow Zone (`768 MiB <= free_vram < 1,200 MiB`)**: Neutral hold. No new VRAM chunks are committed; existing cached chunks remain active.
     - **Red Zone (`free_vram < 768 MiB`)**: Active Host Recall. Windows demands VRAM. Daemon initiates asynchronous eviction of cold VRAM chunks to SSD Tier 3 and invokes `cudaFree` in 64 MiB increments until free headroom returns to $\ge 1,024\text{ MiB}$.
2. **Thin-Provisioned NBD Block Layer**:
   - `/dev/nbd0` advertises full logical capacity (e.g. 4,096 MiB) to the Linux kernel, preventing kernel partition resizes or frequent `swapoff`/`swapon` cycles.
   - Behind `/dev/nbd0`, the storage engine dynamically maps 64 MiB chunk extents:
     - If the chunk is resident in VRAM and headroom is Green, writes update VRAM.
     - If headroom is Red or GPU DMA fails, writes spill over immediately and synchronously into the Tier 3 SSD origin backing file/partition.
3. **Non-Blocking DMA Watchdog & Fast Spillover**:
   - Wrap all GPU DMA transactions with a 50 ms bounded watchdog timer. If an ioctl call or DMA transfer blocks or returns an error (such as `-22`), the block engine trips the chunk's target to SSD Tier 3 immediately.
   - The NBD client request returns `NBD_OK` within $< 5\text{ ms}$, entirely preventing kernel `D` state sleep.
4. **Sub-100ms Page Eviction Engine**:
   - When transitioning to Red Zone, an asynchronous background eviction worker streams cold chunks from VRAM to SSD Tier 3 using direct I/O, updating the chunk mapping table and freeing GPU memory at a rate of $\ge 10\text{ GB/s}$ over PCIe.

### Discarded Alternatives

- **Static VRAM Clamping to 2,048 MiB**: Statically capping VRAM allocation at 2,048 MiB avoids GPU starvation under light desktop usage, but permanently wastes 2,000 to 2,500 MiB of high-speed GPU memory when Windows is idle, while still remaining vulnerable if a heavy host game requests 5,000 MiB of VRAM.
- **Kernel-Level Swapoff on Pressure**: Executing `swapoff /dev/nbd0` from userspace when VRAM is low forces Linux to migrate all swap pages back to RAM or Tier 3 synchronously. Under high memory pressure, `swapoff` itself can take 15–30 seconds, inducing catastrophic latency spikes and potential OOM livelocks.

## 4. Functional Requirements (RF-N)

- **`RF-1` (Dynamic Headroom Governor)**: The daemon must poll physical GPU free memory at a configurable interval (default: 50 ms) and maintain an active telemetry state machine classifying headroom into `Green` ($\ge 1200\text{ MiB}$), `Yellow` ($[768, 1200)\text{ MiB}$), and `Red` ($< 768\text{ MiB}$).
- **`RF-2` (Thin-Provisioned Sparse Extents)**: The block device must allocate physical VRAM on-demand in fixed 64 MiB chunk extents upon first write, rather than allocating and zeroing the full logical capacity at daemon startup.
- **`RF-3` (Non-Blocking SSD Spillover)**: When a write request targets an uncommitted chunk while the Headroom Governor is in `Red` or `Yellow` state, or when GPU DMA experiences an error or latency $> 50\text{ ms}$, the daemon must write the block directly to the Tier 3 SSD backing store and complete the NBD request with `NBD_OK`.
- **`RF-4` (Sub-100ms Eviction / Host Recall)**: When the Headroom Governor enters `Red` state, an asynchronous worker must select the least recently accessed VRAM chunks, flush dirty pages to Tier 3 SSD, update extent mappings to SSD, and release GPU allocations via `cudaFree` within $< 100\text{ ms}$ per gigabyte evicted.
- **`RF-5` (Transparent Page Promotion / Elastic Recovery)**: When headroom returns to `Green` for $\ge 3$ consecutive seconds, subsequent read/write accesses to cold SSD chunks may be asynchronously promoted back to VRAM without service interruption or lock contention.

## 5. Non-Functional Requirements (NFR-N)

- **`NFR-1` (Host Stability & Zero Hangs)**: `PASS_ZERO_PANIC` and zero kernel `D` state hang under all workloads. The Windows Desktop Window Manager (`dwm.exe`) must never freeze, and WSL2 watchdog must never experience `Wsl/Service/E_UNEXPECTED`.
- **`NFR-2` (Bounded I/O Latency)**: Every NBD write operation must complete within $\le 50\text{ ms}$ regardless of GPU driver contention, memory pressure, or fallback spillover.
- **`NFR-3` (Storage Integrity & Crash Consistency)**: Data written to the block device must maintain absolute consistency. If a page is evicted from VRAM to SSD, the extent map update must be atomic such that a subsequent read is guaranteed to fetch the exact written bytes.
- **`NFR-4` (Structured Observability)**: State transitions (`Green` $\leftrightarrow$ `Yellow` $\leftrightarrow$ `Red`), chunk allocations, eviction counts, DMA timeouts, and spillover events must be emitted to the structured telemetry stream (`--telemetry-jsonl`) and status endpoint.

## 6. Execution Flows

### 6.1 Happy Flow: Normal Dynamic Allocation and Green Zone Burst
1. `ramshared up` initializes `/dev/nbd0` with 4,096 MiB logical size backed by thin-provisioned sparse VRAM.
2. At startup, physical VRAM has 4,700 MiB free (`Green` state). Zero physical VRAM chunks are pre-allocated.
3. Linux swap initiates writes to `/dev/nbd0` under memory pressure.
4. Block engine commits 64 MiB chunks into VRAM via CUDA as pages are dirtied.
5. Headroom remains $> 1,200\text{ MiB}$; transfer achieves $> 600\text{ MB/s}$ DMA write speed.

### 6.2 Host Pressure Flow: Windows Demands VRAM (Sub-100ms Recall)
1. An external application on Windows (e.g. 3D game or video editor) launches, rapidly consuming 3.5 GB of VRAM.
2. Poller detects GPU free memory drops from 2,000 MiB to 600 MiB (`Red` state).
3. Eviction worker immediately engages:
   - Selects oldest 64 MiB chunks in VRAM.
   - Streams chunk contents to Tier 3 SSD via direct I/O.
   - Atomically updates chunk routing table: chunk target $\rightarrow$ `SSD`.
   - Releases GPU memory via `cudaFree`.
4. Within 85 ms, 1,024 MiB of VRAM is returned to the Windows driver.
5. Windows application allocates VRAM without encountering TDR or allocation rejection.

### 6.3 Fallback Spillover Flow: Unbounded DMA Failure / Starvation
1. Linux writes a 4 KiB swap block to `/dev/nbd0`.
2. GPU DMA ioctl experiences transient stall or driver returns `-22`.
3. Watchdog fires after 50 ms: GPU write is aborted.
4. Block router redirects the write directly to the Tier 3 SSD origin.
5. Write completes successfully; NBD reply `NBD_OK` sent to Linux kernel.
6. Linux kernel swap continues without entering `D` state.

## 7. Data and State Model

### Headroom State Machine
```text
           free >= 1200 MiB
        ┌─────────────────────┐
        │     State: GREEN    │◄─────────────────┐
        └──────────┬──────────┘                  │
                   │                             │
  free < 1200 MiB  │                             │  free >= 1200 MiB
                   ▼                             │  for >= 3.0s
        ┌─────────────────────┐                  │
        │    State: YELLOW    │──────────────────┤
        └──────────┬──────────┘                  │
                   │                             │
   free < 768 MiB  │                             │  free >= 1024 MiB
                   ▼                             │
        ┌─────────────────────┐                  │
        │     State: RED      │──────────────────┘
        │   (Evict to SSD)    │
        └─────────────────────┘
```

### Extent Mapping Structs (Rust)
```rust
pub enum ChunkLocation {
    Uncommitted,
    Vram {
        buffer_index: usize,
        last_accessed_epoch: u64,
        is_dirty: bool,
    },
    Ssd {
        origin_offset: u64,
        is_dirty: bool,
    },
}

pub struct ElasticExtentTable {
    chunk_size_bytes: u64,
    logical_size_bytes: u64,
    chunks: Vec<ChunkLocation>,
    vram_chunks_committed: usize,
    ssd_chunks_committed: usize,
}
```

## 8. Interfaces

- **Daemon Command Line**:
  - `--vram-governor-poll-ms <MS>`: Polling cadence for GPU headroom (default: `50`).
  - `--vram-headroom-low-mb <MIB>`: Eviction trigger floor (default: `768`).
  - `--vram-headroom-high-mb <MIB>`: Green restoration ceiling (default: `1200`).
  - `--vram-chunk-size-mb <MIB>`: Allocation and eviction granularity (default: `64`).
- **Telemetry JSONL (`event` types)**:
  - `{"event": "headroom_transition", "from": "Green", "to": "Red", "free_vram_mib": 680}`
  - `{"event": "chunk_eviction", "chunk_index": 12, "duration_ms": 14, "destination": "ssd"}`
  - `{"event": "dma_spillover", "offset": 1048576, "reason": "dma_timeout_50ms"}`

## 9. Dependencies and Risks

- **Dependencies**:
  - CUDA runtime API (`cudaMemGetInfo`, `cudaMalloc`, `cudaFree`, `cudaMemcpyHtoDAsync`) via `/dev/dxg`.
  - Authoritative SSD origin device (canonical origin partition or sealed origin container).
- **Risks & Mitigations**:
  - *Risk:* Multiple rapid allocations in Windows cause VRAM free memory to plummet faster than 50 ms polling.
    *Mitigation:* Keep the Red Zone floor at a generous 768 MiB, providing enough buffer for Windows to allocate while the 100 ms eviction drains VRAM.
  - *Risk:* Heavy I/O to SSD during eviction starves regular disk operations.
    *Mitigation:* Limit eviction batching to 128 MiB concurrent inflight I/O using asynchronous direct I/O.
- **Numeric Rollback Trigger**:
  - Any occurrence of `dxgkio_escape: -22`, any kernel `D` state hang reported in `dmesg`, or NBD request latency exceeding 250 ms triggers immediate fail-closed demotion to SSD-only mode.

## 10. Implementation Strategy

1. **Slice 1 (Governor & Telemetry)**: Implement `DynamicHeadroomGovernor` with periodic CUDA memory polling and unit tests for state machine hysteresis.
2. **Slice 2 (Elastic Chunk Router)**: Implement `ElasticExtentTable` managing dynamic allocation across VRAM and SSD backends.
3. **Slice 3 (Non-Blocking Watchdog)**: Add bounded DMA watchdog in `ResilientBackend` tripping to SSD on 50 ms timeout or ioctl failure.
4. **Slice 4 (Asynchronous Eviction Worker)**: Implement background eviction worker flushing cold VRAM chunks to SSD when Governor signals `Red`.
5. **Slice 5 (Integration & Benchmark)**: Wire components into `ramsharedd` and validate with multi-tier cascade stress test.

## 11. Documents to Update

- `docs/INDEX.md`: Register `elastic-vram-cooperative-tier`.
- `docs/specs/README.md`: Add row for new cooperative tiering specification.
- `ARCHITECTURE.md`: Document dynamic VRAM borrowing and eviction topology.
- `docs/reliability/DEGRADATION-MATRIX.md`: Record degradation behavior under host GPU pressure.

## 12. Out of Scope

- Kernel-space custom Linux block driver (`rs_vram.ko` / LKM) — scheduled for Phase 2.
- Direct GPU-to-NVMe Peer-to-Peer DMA (GPUDirect Storage) without host RAM buffering.
- Windows-side driver development (WDK/StorPort).

## 13. Acceptance Criteria

- [ ] Polling detects host VRAM changes within $\le 50\text{ ms}$.
- [ ] Daemon allocates VRAM sparsely in 64 MiB increments; startup memory footprint on GPU is $< 64\text{ MiB}$.
- [ ] When host free VRAM drops below 768 MiB, eviction frees $\ge 1,024\text{ MiB}$ of VRAM within $\le 100\text{ ms}$.
- [ ] During simulated GPU DMA stalls, write requests fail over to SSD within $\le 50\text{ ms}$ and return `NBD_OK`.
- [ ] Full cascade stress test reaches 100% ZRAM and spills into SSD origin with zero kernel panics, zero oops, zero `D` state hangs, and `PASS_ZERO_PANIC` verdict.

## 14. Validation Plan

- **Unit Tests**:
  - `cargo test -p ramshared-wsl2d test_governor_headroom_state_machine`: Verifies Green $\rightarrow$ Yellow $\rightarrow$ Red transitions and hysteresis.
  - `cargo test -p ramshared-block test_elastic_extent_spillover`: Verifies that uncommitted chunks spill to SSD without blocking.
  - `cargo test -p ramshared-wsl2d test_dma_watchdog_timeout_failover`: Simulates hung DMA ioctl and verifies sub-50ms failover.
- **Coverage Target**: $\ge 80\%$ line coverage on touched files via `check-rust-slice-coverage.mjs`.
- **Live E2E Execution**: Validate multi-tier cascade stress test generating $> 5\text{ GB}$ of swap, and verify clean spillover to Tier 3 SSD origin while Windows DWM runs uninterrupted.
