# SPEC - Host-Aware VRAM Safety Ceiling, Dynamic Chunk Tiering, and Non-Blocking Spillover

## 1. Closed Scope

### In Now
- **Host-aware auto-clamping** in `crates/ramshared-wsl2d/src/main.rs`: auto-detect total GPU VRAM and clamp requested broker slices to guarantee a minimum host reserve floor (2,048 MB on 6 GB GPU).
- **Non-blocking DMA watchdog** in `ResilientBackend`: trip failover to RAM mirror when GPU writes stall or fail, preventing kernel `D` state hangs.
- **Active watermark monitoring** wired into the multi-slice broker heartbeat loop (`serve_broker_jobs_with_poll_and_heartbeat`): emit `DemoteReason::GlobalGpuFreeFloor` when free VRAM drops below the low watermark.
- **Tier 3 cascade spillover validation**: ensure Linux kernel spills swap naturally from Tier 1 (ZRAM) to Tier 2 (clamped VRAM) and overflows cleanly into Tier 3 (the backing SSD swap partition) under heavy pressure without freezing the Windows host.

### Out Now
- Kernel-space LKM modifications to `mm/swapfile.c`.
- Windows WDK StorPort driver alterations (WSL2 cascade specific).
- Sparse page-table paging inside CUDA kernels (delegated to future in-tree GPU driver milestones).

### Assumed-Ready Dependencies
- `crates/ramshared-wsl2d` existing broker runtime (`run_broker`, `serve_broker_jobs`).
- `/dev/dxg` Direct3D driver and CUDA runtime (`Cuda::load()`, `provider.mem_info()`).
- Linux swap configuration with `/dev/zram0` (priority 100), `/dev/nbd0` (priority 50), and the backing SSD swap partition (priority -2).

---

## 2. Traceability

| PRD Requirement | Implementation / Decision Item | Covered By Test / Evidence |
| :--- | :--- | :--- |
| **RF-1** (Host Clamping) | `ITEM-1`, `DT-1` | `test_host_vram_clamping_rtx2060`, `test_host_vram_clamping_unconstrained` |
| **RF-2** (DMA Watchdog) | `ITEM-2`, `DT-3` | `test_resilient_backend_watchdog_failover` |
| **RF-3** (Watermark Demote) | `ITEM-3`, `DT-2` | `test_broker_heartbeat_watermark_demote` |
| **RF-4** (Tier 3 Spillover) | `ITEM-4`, `DT-4` | Live multi-tier cascade stress drill (`scripts/stress-cascade-governor.sh`) |
| **NFR-1** (Zero Freeze) | `ITEM-1`, `ITEM-2`, `ITEM-3` | `PASS_ZERO_PANIC` verdict on 6.18.40.1 kernel |
| **NFR-2** (Failover Latency) | `ITEM-2` | Latency assertion $\le 50\text{ ms}$ |
| **NFR-3** (Observability) | `ITEM-1`, `ITEM-3` | Telemetry JSONL event emission |

---

## 3. Technical Decisions

| # | Decision | Why |
| :--- | :--- | :--- |
| **DT-1** | **Host Reserve Floor Formula**: Enforce $\text{HOST\_RESERVE\_FLOOR} = \max(2048\text{ MB},\, \text{total\_vram} \times 35\%)$. | On a 6,144 MB GPU, guarantees at least 2,048 MB remains strictly available for Windows Desktop Window Manager (`dwm.exe`) and host 3D applications, completely eliminating GPU TDR lockups. |
| **DT-2** | **Broker Global Free Floor Heartbeat Hook**: Connect `observe_global_free_floor` directly to `serve_broker_jobs_with_poll_and_heartbeat`. | Closes the architectural gap where the single-worker sparse harness checked free memory, but the production multi-slice broker ran blind without active memory polling. |
| **DT-3** | **Watchdog Trip on DMA Stalls**: In `ResilientBackend`, track write latency; if GPU write exceeds 50ms or encounters an ioctl error (`-22`), immediately mark `failed_over = true` and serve subsequent I/O from RAM buffer. | Prevents synchronous CUDA memcpy from blocking the NBD worker thread when `/dev/dxg` stalls, avoiding uninterruptible sleep `D` state in Linux kernel swap. |
| **DT-4** | **Cooperative Swap Spillover via Accurate Geometry**: Advertise the clamped slice size (e.g. 2,048 MB) as the true capacity of `/dev/nbd0`. | Allows standard Linux kernel swap priority (`pri=100 zram` $\rightarrow$ `pri=50 nbd` $\rightarrow$ `pri=-2 ssd`) to overflow naturally to Tier 3 SSD when VRAM reaches 100% of its safe capacity. |

---

## 4. Atomicity and Rollback

- **Atomicity Frontier**:
  - Sizing and clamping logic executes before any CUDA allocation (`provider.alloc()`) or NBD socket binding. If clamping fails or physical memory is below the minimum operational floor, the daemon exits cleanly with status code `1` before touching any system swap.
  - Failover from VRAM to RAM mirror in `ResilientBackend` is unidirectional and lock-free (`failed_over: bool` with relaxed/SeqCst synchronization).
- **Rollback**:
  - **Userspace / Daemon**: Commit revert cleanly restores prior `ramshared-wsl2d` binary.
  - **Kernel / Module**: `swapoff /dev/nbd0` cleanly detaches the NBD block device; the backing SSD swap partition and `/dev/zram0` (ZRAM) remain fully functional.
  - **Host / Persistent**: No persistent state modified on the Windows host.

---

## 5. Kahneman Map (Critical Steps)

| ITEM / Stage | # | Question | Min Evidence | Abort |
| :--- | :--- | :--- | :--- | :--- |
| **ITEM-1** (Clamping) | **#13** (Refusal + Legitimate) | Does the clamping logic strictly refuse unsafe allocations while accepting legitimate sub-floor requests? | `cargo test -p ramshared-wsl2d test_host_vram_clamping` | Any allocation that leaves $< 2,048\text{ MB}$ free on 6 GB GPU |
| **ITEM-2** (Watchdog) | **#15** (Transient Retry / Failover) | Does the backend switch to RAM mirror within 50ms without hanging the caller thread? | `cargo test -p ramshared-wsl2d test_resilient_backend_watchdog` | Thread blocks $> 100\text{ ms}$ or returns `NBD_EIO` |
| **ITEM-3** (Heartbeat) | **#16** (Exhaustion Behavior) | When physical VRAM is starved, does the broker emit `DemoteAll` before the host GPU driver crashes? | Unit simulation + telemetry JSONL event `watermark_demote` | Windows desktop freeze or GPU TDR |
| **ITEM-4** (Spillover) | **#9** (Numeric Verification) | Does SSD utilization exceed 0 MB during the multi-tier stress drill while VRAM remains clamped? | `/proc/swaps` showing the backing SSD swap partition used $> 0\text{ MB}$ + `PASS_ZERO_PANIC` | Any freeze, hung task, or zero SSD usage under $>3\text{ GB}$ swap |

---

## 6. Security Checklist (Pre-Impl)

- [x] **Privilege**: Daemon requires `CAP_SYS_ADMIN` inside WSL2 to manage NBD and swap; no privilege escalation to Windows host.
- [x] **User/Host Copy**: DMA buffers strictly bounded to allocated slice length; bounds checked on every NBD request.
- [x] **Flags/IOCTL Codes**: Direct ioctl calls to `/dev/dxg` handled with validation; return codes checked.
- [x] **Info-Leak**: No kernel virtual memory addresses leaked in telemetry or logs.
- [x] **IRQ / IRQL**: Userspace daemon runs in user mode; no illegal sleeping in atomic context.
- [x] **Lifetime**: Allocated VRAM explicitly zeroed on release; NBD disconnect signals clean worker teardown.
- [x] **Hot-Unplug / Device-Gone**: If GPU device disappears, `ResilientBackend` hot-swaps to RAM without panicking.
- [x] **Host Safety**: Enforces strict minimum 2,048 MB VRAM cushion for Windows host display.
- [x] **Shared-Hardware Cushion**: Mathematical host reserve floor enforced; no greedy static allocation of shared VRAM/RAM.
- [x] **Bounded DMA / Foreign Driver Calls**: Watchdog/timeout ensures no thread hangs indefinitely in foreign driver ioctls.
- [x] **Cooperative Cascade Spillover**: Lower tiers (the backing SSD swap partition) verified to receive traffic when accelerator tier saturates or degrades.
- [x] **Replayable Ops**: Clamping and demote state transitions are idempotent (#17).

---

## 7. Files to CREATE / MODIFY / DELETE

### MODIFY

**`crates/ramshared-wsl2d/src/main.rs`**
- **Purpose**: Implement `calculate_safe_vram_slice`, wire host-aware clamping into `DaemonAction::Broker`, add watchdog timing to `ResilientBackend::write_at`, and hook `observe_global_free_floor` to broker heartbeat.
- **RF / DT**: RF-1, RF-2, RF-3; DT-1, DT-2, DT-3.
- **Key Changes**:
  - Add helper function:
    ```rust
    fn calculate_safe_vram_slice(
        requested_bytes: u64,
        total_vram_bytes: u64,
        free_vram_bytes: u64,
        host_reserve_floor_bytes: u64,
    ) -> (u64, bool)
    ```
  - In `run_broker_with_setup`, compute safe slice bytes before calling `provider.alloc()`.
  - In `ResilientBackend`, add `last_write_latency: Duration` and failover logic on timeout.
  - In `serve_broker_jobs_with_poll_and_heartbeat`, invoke global free floor evaluation.
- **Required Tests**:
  - `crates/ramshared-wsl2d/src/main.rs :: test_host_vram_clamping_rtx2060`
  - `crates/ramshared-wsl2d/src/main.rs :: test_host_vram_clamping_unconstrained`
  - `crates/ramshared-wsl2d/src/main.rs :: test_resilient_backend_watchdog_failover`
- **Cover Target**: $\ge 80\%$ on new business-logic lines.

---

## 8. Observability

| Signal | Where | Level / Type |
| :--- | :--- | :--- |
| `vram_clamped` | `stderr` + `telemetry.jsonl` | INFO / Structured JSON |
| `dma_watchdog_tripped` | `stderr` + `telemetry.jsonl` | WARN / Structured JSON |
| `watermark_demote` | `stderr` + `telemetry.jsonl` | WARN / Structured JSON |
| `tier3_spillover_active` | `scripts/stress-cascade-governor.sh` | INFO / Live terminal bar |

---

## 9. Living Docs

| Document | Action |
| :--- | :--- |
| `ARCHITECTURE.md` | Document Tier 2 host-aware safety floor and dynamic Tier 3 spillover invariant. |
| `docs/reliability/GAP-REGISTER.md` | Link this SPEC as the permanent resolution for WSL2 VRAM freeze under heavy swap pressure. |
