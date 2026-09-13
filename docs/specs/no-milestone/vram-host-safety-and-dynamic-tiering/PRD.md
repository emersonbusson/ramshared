---
slug: vram-host-safety-and-dynamic-tiering
title: Host-aware VRAM safety ceiling, dynamic chunk tiering, and non-blocking spillover
milestone: —
issues: []
---

# PRD - Host-Aware VRAM Safety Ceiling, Dynamic Chunk Tiering, and Non-Blocking Spillover

## 1. Summary

RamShared's WSL2 broker daemon (`ramsharedd`) currently exhibits a critical reliability defect under multi-tier cascade memory pressure: when started with a large static slice (such as `--slice-mb 4096` on a 6,144 MB NVIDIA GPU), it immediately reserves and zeroes the entire VRAM slice up-front. Because the Windows host Desktop Window Manager (`dwm.exe`) and host applications continuously require 1.3 GB to 1.8 GB of physical VRAM, this static reservation starves the host GPU memory manager (`dxgkrnl.sys`), leaving less than 650 MB of free headroom.

During heavy memory pressure (such as `cargo build`, container workloads, or high-throughput swap drills), Linux attempts to write dirty swap pages into `/dev/nbd0`. When the physical GPU memory saturates, synchronous CUDA DMA over `/dev/dxg` locks inside the Windows kernel driver, triggering a GPU Timeout Detection and Recovery (TDR) or kernel deadlock. This freezes the Windows desktop and hangs the Linux swap subsystem in uninterruptible sleep (`D` state). Concurrently, Tier 3 (SSD swap on the backing SSD swap partition) remains 100% idle (0 MB used) because the Linux kernel strictly honors swap priorities and refuses to write to lower-priority tiers while `/dev/nbd0` reports unwritten capacity.

Applying the **SSDV3 Principle 11 (Shared Hardware & Tiering Coexistence)**, this PRD establishes the senior, host-safe architecture to eliminate this freeze vulnerability:
1. **Host-Aware VRAM Safety Clamping**: Automatically probe physical GPU memory at daemon initialization and enforce a mathematical host reserve floor (minimum 2,048 MB or 35% of total VRAM) strictly dedicated to Windows display and host applications.
2. **Elastic / Dynamic Chunk Allocation**: Transition the broker backend from greedy pre-zeroed buffers to on-demand sparse chunk commitments, touching physical VRAM only as swap pages are actively dirtied.
3. **Non-Blocking DMA Watchdog & Fast Failover**: Eliminate unbounded synchronous GPU writes in `ResilientBackend` with an explicit 50ms timeout watchdog, tripping in-process failover to RAM/SSD if DMA blocks or fails.
4. **Active Pressure Watermark & Tier 3 Spillover**: Continuously monitor GPU free memory via `/dev/dxg` and CUDA telemetry; if host free VRAM drops below the safety watermark, trigger an orderly demote (`swapoff /dev/nbd0`), forcing the Linux kernel to spill over active swap traffic seamlessly into Tier 3 (SSD) before GPU starvation can occur.

---

## 2. Technical Context & Topology

### 2.1 Hardware & Coexistence Topology (WSL2 / Windows Host)
- **Physical Host RAM**: 32 GB physical DDR4/DDR5 memory.
- **WSL2 Virtual Machine RAM**: 16 GB fixed allocation (`MemTotal: ~15.6 GiB` via `.wslconfig`). Windows retains the remaining 16 GB physical RAM. System RAM is never exhausted under these workloads.
- **Physical GPU**: NVIDIA GeForce RTX 2060 with 6,144 MB (6.0 GiB) physical VRAM.
- **Shared VRAM Model**: Unlike system RAM (partitioned by Hyper-V), GPU VRAM is shared in real time between Windows and WSL2 via the virtualized DirectX Graphics Kernel (`/dev/dxg` $\leftrightarrow$ `dxgkrnl.sys`).
- **Windows Host Display Overhead**: Windows Desktop Window Manager (`dwm.exe`), multi-monitor scanout, hardware-accelerated browsers, and Electron applications consume between 1,300 MB and 1,800 MB permanently.
- **Coexistence Invariant (SSDV3 Principle 11)**: *Tier 2 (VRAM) is strictly an opportunistic accelerator, never a host-starvation vector.* The host OS display and 3D pipeline have unconditional priority over secondary swap acceleration.

### 2.2 Forensic Codebase Anchors
- **Confirmed in codebase (`crates/ramshared-wsl2d/src/main.rs:4615`)**: The broker backend executes `let mut mem = provider.alloc(total as usize)?; mem.zero()?;` during initialization. This immediately forces full physical allocation and page dirtiness across all 4,096 MB at boot time regardless of actual swap utilization.
- **Confirmed in codebase (`crates/ramshared-wsl2d/src/main.rs:231-238`)**: `ResilientBackend::write_at` attempts failover only when `vram.write_at(off, data)` returns `Err(e)`. When `/dev/dxg` enters GPU memory starvation or TDR, `cuMemcpyHtoDAsync` / `cuStreamSynchronize` blocks indefinitely inside the kernel ioctl, causing the NBD worker thread to hang and preventing failover from ever engaging.
- **Confirmed in codebase (`crates/ramshared-wsl2d/src/main.rs:3387-3406`)**: While `observe_global_free_floor` and `WddmBudgetPoll` exist for the experimental single-export sparse test harness, the production multi-slice broker (`DaemonAction::Broker` / `run_broker_with_setup`) does not wire global free floor monitoring into its active worker loop.
- **Inference**: High-speed memory allocations inside WSL2 (such as Rust `cargo build`, heavy LLM loading, or stress benchmarks) can generate swap ingest rates exceeding 100 MB/s. If the underlying block device hangs on GPU DMA, every Linux process allocating memory blocks in `mm/vmscan.c` mutexes, freezing the entire OS.

---

## 3. Recommended Option

Implement the **Unified Host-Safe Memory Tiering Architecture**:

1. **Host-Aware Safety Clamping (Startup Gate)**:
   - At startup, `ramsharedd` queries `total_vram` and `free_vram`.
   - It calculates:
     $$\text{HOST\_RESERVE\_FLOOR} = \max(2048\text{ MB},\, \text{total\_vram} \times 35\%)$$
     $$\text{safe\_max\_vram} = \text{total\_vram} - \text{HOST\_RESERVE\_FLOOR}$$
   - If `--slice-mb` requested exceeds `safe_max_vram`, the daemon logs a warning, clamps the slice to `safe_max_vram`, and configures `/dev/nbd0` with the clamped size.
   - On a 6,144 MB GPU: $\text{safe\_max\_vram} = 6144 - 2150 = 3994\text{ MB}$. Accounting for active host usage (1,400 MB), the maximum safe slice is clamped to **2,048 MB**, strictly preserving $\ge 2,600\text{ MB}$ free on the GPU.

2. **Non-Blocking DMA Watchdog in `ResilientBackend`**:
   - Wrap GPU write operations with a bounded watchdog timer. If a GPU write blocks for more than 50ms or returns an error, the circuit breaker immediately trips: `failed_over = true`.
   - Once tripped, writes and reads are served from the internal RAM fallback buffer, NBD requests complete without hanging, and the daemon signals an asynchronous DEMOTE event.

3. **Active Watermark Monitoring & Early Spillover**:
   - The broker heartbeat loop periodically samples physical GPU free memory.
   - If `global_free_bytes < WATERMARK_LOW` (800 MB), the daemon stops accepting new VRAM commits, marks Tier 2 as saturated, and initiates an orderly `swapoff` on `/dev/nbd0`.
   - The Linux kernel automatically redirects all subsequent swap writes to Tier 3 (the backing SSD swap partition), preserving 100% system responsiveness.

### Discarded Alternatives

- **Manual Sizing Only (Ad-hoc CLI flag tuning)**: Rejected. Fragile across different host GPU configurations (e.g., 4 GB, 6 GB, 8 GB, 16 GB GPUs) and fails when host applications dynamically consume VRAM after daemon startup.
- **Pure Userspace Buffer without Swapoff**: Rejected. If `/dev/nbd0` continues absorbing swap into a RAM fallback buffer, it duplicates WSL2 system RAM and defeats the purpose of tiered offloading. Proactive `swapoff` forces the kernel to use the dedicated Tier 3 SSD swap partition.

---

## 4. Functional Requirements (RF-N)

| ID | Description | Verifiable Acceptance |
| :--- | :--- | :--- |
| **RF-1** | **Host-Aware Startup Clamping** | When `--slice-mb` or `--slices` would leave less than `HOST_RESERVE_FLOOR` (2,048 MB on 6 GB GPU) free for the host, the daemon automatically clamps the effective slice size, logs `[ramsharedd] Host VRAM safety clamp engaged: requested=... clamped=... host_floor=...`, and provisions `/dev/nbd0` at the clamped boundary. |
| **RF-2** | **Non-Blocking DMA Watchdog** | `ResilientBackend` must not block indefinitely on GPU DMA ioctls. If GPU write latency exceeds 50ms or ioctls fail (`-22`, `-5`), the backend hot-swaps to RAM in < 1ms, logs `[ramsharedd] VRAM DMA watchdog tripped; hot-swapping to RAM fallback`, and completes the NBD reply with `NBD_OK`. |
| **RF-3** | **Active Watermark Demote (Tier 3 Spillover)** | When periodic GPU polling detects `global_free < WATERMARK_LOW` (800 MB) for 3 consecutive samples, the broker worker triggers `DemoteAll`. The daemon initiates `swapoff /dev/nbd0`, causing Linux to drain active swap to Tier 3 SSD without application failure. |
| **RF-4** | **Clean Tier 3 Transition Verification** | Under cascade stress exceeding Tier 1 (1 GB ZRAM) and Tier 2 (clamped VRAM), the system must cleanly spill over into Tier 3 (the backing SSD swap partition), achieving >0 MB SSD utilization with zero hung task warnings in `dmesg` and zero Windows desktop stutter. |

---

## 5. Non-Functional Requirements (NFR-N)

| ID | Category | Target Metric |
| :--- | :--- | :--- |
| **NFR-1** | **Host Safety & Stability** | `PASS_ZERO_PANIC` and `PASS_ZERO_FREEZE`: No Windows TDR resets, no DWM crash, no display freeze, and no Linux kernel `D` state hung tasks under 100% memory pressure. |
| **NFR-2** | **Failover Latency** | When GPU DMA stalls, fallback switch must complete in $\le 50\text{ ms}$, ensuring NBD client timeouts (typically 30s) are never approached. |
| **NFR-3** | **Observability** | All clamping decisions, watchdog trips, watermark events, and demote transitions must be emitted to daemon JSONL telemetry and system logs (`stderr`). |
| **NFR-4** | **Reversibility** | If external GPU pressure subsides (`global_free > WATERMARK_HIGH` for $\ge 10\text{ s}$ and VRAM tier empty), the daemon may re-arm Tier 2 swapon without restarting. |

---

## 6. Execution Flows

### 6.1 Happy Flow: Startup with Host-Aware Clamping and Clean Tier 3 Cascade
1. Daemon starts on WSL2 with `--backend vram --slices 1 --slice-mb 4096`.
2. Daemon probes CUDA: detects total VRAM 6,144 MB, host reserve floor 2,048 MB.
3. Safe ceiling is calculated: $6,144 - 2,048 = 4,096\text{ MB}$. Current host usage is 1,400 MB $\rightarrow$ available ceiling is $6,144 - 1,400 - 2,048 = 2,696\text{ MB}$. Clamped slice: 2,048 MB.
4. Daemon allocates 2,048 MB VRAM slice. `/dev/nbd0` is provisioned as 2,048 MB swap.
5. System memory pressure ramps up:
   - Level 0–1 GB: Absorbed by Tier 1 (`/dev/zram0`, 1,024 MB).
   - Level 1–3 GB: Absorbed by Tier 2 (`/dev/nbd0`, 2,048 MB).
   - Level >3 GB: Tier 2 saturates at 100% capacity; Linux kernel naturally overflows into Tier 3 (the backing SSD swap partition).
6. Total stability maintained: host Windows desktop remains fluid at ~2.5 GB free VRAM; drill passes with `PASS_ZERO_PANIC`.

### 6.2 Alternate Flow: External GPU App Launches During Swap Activity
1. While Tier 2 holds 1 GB of swap, user launches a 3D app / video editor in Windows.
2. Windows allocates 2 GB VRAM; physical GPU free drops to 650 MB ($< 800\text{ MB}$ watermark).
3. Daemon's active watermark monitor detects constraint across 3 consecutive ticks.
4. Daemon signals `DemoteReason::HostGpuPressure`, triggers `swapoff /dev/nbd0`.
5. Linux kernel migrates pages from `/dev/nbd0` directly into Tier 3 (the backing SSD swap partition).
6. VRAM slice is released / parked; Windows app runs smoothly without crashing.

### 6.3 Error Flow: Abrupt GPU DMA Stall / Hardware Reset
1. GPU hardware experiences transient PCIe / dxg stall during write I/O.
2. `ResilientBackend` DMA watchdog detects that write has not completed within 50ms.
3. Watchdog immediately aborts GPU wait, writes payload to internal RAM fallback buffer, sets `failed_over = true`, and returns `NBD_OK` to kernel.
4. Linux swap I/O completes without hanging. Daemon initiates graceful teardown/demote of the degraded GPU tier.

---

## 7. Data and State Model

```text
               ┌───────────────────────┐
               │     STARTUP / INIT    │
               └───────────┬───────────┘
                           │ Query VRAM Total & Free
                           ▼
               ┌───────────────────────┐
               │ HOST-AWARE CLAMPING   │
               │ Slice <= Safe Ceiling │
               └───────────┬───────────┘
                           │ Alloc Clamped Slice
                           ▼
               ┌───────────────────────┐
               │    ACTIVE TIER-2      │◄────────────────────────┐
               │    (VRAM Serving)     │                         │
               └─────┬───────────┬─────┘                         │ External Pressure
                     │           │                               │ Cleared & Cooled
   GPU DMA Timeout   │           │ Free VRAM < Watermark         │
   or I/O Error      │           │                               │
                     ▼           ▼                               │
        ┌──────────────────┐   ┌──────────────────┐              │
        │ IN-PROCESS RAM   │   │ PROACTIVE DEMOTE │              │
        │ FAILOVER BUFFER  │   │  (swapoff NBD)   │──────────────┘
        └─────────┬────────┘   └─────────┬────────┘
                  │                      │
                  └──────────┬───────────┘
                             ▼
               ┌───────────────────────────┐
               │    TIER-3 SPILLOVER       │
               │ (Backing SSD Swap Active) │
               └───────────────────────────┘
```

---

## 8. Interfaces

- **CLI Flag (Optional override)**: `--host-reserve-mb <N>` (default: 2048). Allows explicit specification of the host VRAM cushion.
- **Telemetry Stream (`telemetry.jsonl`)**:
  - `{"event":"vram_clamped","requested_mb":4096,"clamped_mb":2048,"host_reserve_floor_mb":2048}`
  - `{"event":"dma_watchdog_trip","latency_us":52400,"action":"failover_to_ram"}`
  - `{"event":"watermark_demote","free_bytes":681574400,"threshold_bytes":838860800}`

---

## 9. Dependencies and Risks

- **Prerequisites**: Functional CUDA driver (`/dev/dxg`) in WSL2; active swap devices for Tier 1 (`/dev/zram0`) and Tier 3 (the backing SSD swap partition).
- **Risks & Mitigations**:
  - *Risk*: `swapoff` under high memory pressure might take several seconds.
    *Mitigation*: The `ResilientBackend` RAM mirror absorbs writes during the transition so NBD never rejects I/O with errors while `swapoff` completes.
- **Rollback Trigger**:
  - Any regression in Tier 2 baseline read throughput ($< 1.5\text{ GB/s}$) or any unexpected unmount during normal unconstrained operations triggers rollback to commit `f3ad9b0`.

---

## 10. Implementation Strategy

1. **Phase 1 (Immediate Safety Sizing)**: Introduce `HostSafetyCeiling` in `crates/ramshared-wsl2d` to compute and enforce safe VRAM slice boundaries based on host GPU capacity.
2. **Phase 2 (Resilient DMA Watchdog)**: Enhance `ResilientBackend` to prevent blocking threads on unresponsive `/dev/dxg` ioctls.
3. **Phase 3 (Watermark Broker Hook)**: Connect `observe_global_free_floor` directly into the multi-slice broker heartbeat loop (`serve_broker_jobs_with_poll_and_heartbeat`).
4. **Phase 4 (Live Qualification)**: Execute the 4-phase stress battery, proving seamless overflow into Tier 3 SSD with 100% host stability.

---

## 11. Documents to Update

- `docs/specs/no-milestone/vram-host-safety-and-dynamic-tiering/PRD.md` (This document)
- `docs/specs/no-milestone/vram-host-safety-and-dynamic-tiering/SPEC.md`
- `docs/specs/no-milestone/vram-host-safety-and-dynamic-tiering/IMPL.md`
- `ARCHITECTURE.md` (Update Tier 2 resilience and cascade overflow principles)
- `docs/reliability/GAP-REGISTER.md`

---

## 12. Out of Scope

- Modifying the Windows display driver or WDDM kernel components (`dxgkrnl.sys`).
- Replacing Linux kernel `mm/swapfile.c` priority logic (we cooperate with kernel priority via device sizing and orderly demote).
- Implementing Windows kernel-mode StorPort changes in this PRD (this is WSL2/Linux cascade specific).

---

## 13. Acceptance Criteria

1. Running `ramsharedd --backend vram --slices 1 --slice-mb 4096` on a 6 GB GPU automatically clamps the allocation to a safe boundary ($\le 2,048\text{ MB}$), logging the exact reservation and leaving $\ge 2.5\text{ GB}$ free for Windows.
2. Under memory pressure exceeding Tier 1 (1 GB) and Tier 2 (clamped VRAM), the Linux kernel begins writing dirty pages into Tier 3 (the backing SSD swap partition), reaching $>0\text{ MB}$ SSD utilization without freeze.
3. If GPU memory drops below 800 MB, the broker initiates a clean demote without kernel panic or desktop stutter.
4. All unit and integration tests pass with $\ge 80\%$ coverage on newly touched logic.

---

## 14. Validation Plan

- **Unit Tests**:
  - `crates/ramshared-wsl2d`: Test clamping math with synthetic 4 GB, 6 GB, 8 GB, and 24 GB GPU sizes.
  - `crates/ramshared-wsl2d`: Test DMA watchdog failover trigger under simulated stalled writes.
  - `crates/ramshared-wsl2d`: Test watermark demote streak counter and resets.
- **Live Cascade Qualification**:
  - Execute `bash scripts/stress-cascade-governor.sh --full` on the active 6.18.40.1 kernel.
  - Confirm Tier 1 $\rightarrow$ Tier 2 $\rightarrow$ Tier 3 cascade transition.
  - Confirm `PASS_ZERO_PANIC` and zero Windows desktop freeze.
