# Linux Kernel Telemetry Architecture & Implementation Specification

**Canonical Reference:** `docs/architecture/LINUX-KERNEL-TELEMETRY-SPEC.md`
**Subsystems:** Linux Block Layer (`blk-mq`), Memory Management (`mm`), Scheduler PSI (`kernel/sched/psi.c`), DRM GPU fdinfo (`drivers/gpu/drm/`).
**Author:** Emerson Busson
**Status:** Canonical Engineering Specification

---

## 1. Architectural Overview & The 5 Telemetry Pillars

The Linux kernel (Linus Torvalds, Jens Axboe, Greg Kroah-Hartman) adheres to strict zero-allocation, lockless, and non-blocking telemetry collection principles. RamShared implements full architectural parity with these 5 kernel telemetry pillars:

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│                             THE 5 LINUX KERNEL TELEMETRY PILLARS                                 │
├──────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 1. blk-mq Hardware Accounting     │ 2. Pressure Stall Information (PSI)                          │
│    /proc/diskstats & /sys/block/  │    /proc/pressure/memory                                     │
│    • Nanosecond I/O latency       │    • some_avg10, full_avg60 CPU stall windows                │
│    • Per-CPU lockless accumulators│    • Pre-emptive swap thrash detection                       │
├───────────────────────────────────┼──────────────────────────────────────────────────────────────┤
│ 3. VM & Swap Subsystem (MM)       │ 4. DRM GPU Client fdinfo                                     │
│    /proc/vmstat & /proc/meminfo   │    /proc/<pid>/fdinfo/<fd>                                   │
│    • pswpin / pswpout counters    │    • drm-total-vram & drm-resident-vram                      │
│    • Direct reclaim page steals   │    • GPU compute engine execution ns                         │
├───────────────────────────────────┴──────────────────────────────────────────────────────────────┤
│ 5. Dynamic Tracepoints & eBPF Event Infrastructure                                               │
│    • block:block_rq_issue & block:block_rq_complete                                              │
│    • mm:mm_vmscan_direct_reclaim_begin & mm:mm_vmscan_direct_reclaim_end                         │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Pillar 1: Block Layer Hardware Accounting (`blk-mq`)

### 2.1 Kernel Implementation in `drivers/block/ramshared/`
Every I/O transfer passing through the RamShared block driver updates lockless per-CPU atomic counters, exposed directly through `sysfs` attribute groups:

```c
/* Lockless IOPS & Transfer Accumulators */
atomic64_add(len, &rs_dev->read_bytes);
atomic64_add(len, &rs_dev->write_bytes);
atomic64_inc(&rs_dev->dma_transfers_total);
```

### 2.2 User-Space Parsing in `/proc/diskstats`
Disk throughput is parsed locklessly via `/proc/diskstats`:
$$\text{Read Throughput (MiB/s)} = \frac{\Delta \text{sectors\_read} \times 512}{\Delta t \times 1024 \times 1024}$$
$$\text{Write Throughput (MiB/s)} = \frac{\Delta \text{sectors\_written} \times 512}{\Delta t \times 1024 \times 1024}$$
$$\text{IOPS} = \frac{\Delta \text{read\_ios} + \Delta \text{write\_ios}}{\Delta t}$$

---

## 3. Pillar 2: Pressure Stall Information (`PSI`)

### 2.1 Scheduler Stall Mechanics (`kernel/sched/psi.c`)
The Linux PSI subsystem tracks the percentage of wall-clock time that tasks are delayed waiting for memory allocation and swap pages:
* `some`: At least one task is stalled waiting for memory.
* `full`: **All** runnable tasks on a CPU are stalled (system thrashing/freeze indicator).

### 2.2 Telemetry Threshold Matrix

| Metric | Normal State | Warning Threshold | Emergency Action |
| :--- | :---: | :---: | :--- |
| **`some_avg10`** | `0.00%` | `> 5.00%` | Promote active swap pages to high-speed VRAM tier |
| **`full_avg10`** | `0.00%` | `> 1.00%` | Throttle background batch workloads |
| **`full_avg60`** | `0.00%` | `> 0.50%` | Execute emergency swapout to authoritative SSD origin |

---

## 4. Pillar 3: Memory Management & Swap Counters (`mm/vmscan.c`)

### 4.1 Zero-Allocation Swap Fast-Path (`.rw_page`)
Under severe memory exhaustion (99% RAM load), allocating `struct bio` or queue request tags risks deadlock. RamShared implements `.rw_page` in `struct block_device_operations`:

```c
static int ramshared_bdev_rw_page(struct block_device *bdev, sector_t sector,
                                  struct page *page, enum req_op op);
```
* **Bypasses:** bio allocation, request queue locks, blk-mq tag allocation.
* **Executes:** Direct `memcpy_toio()` / `memcpy_fromio()` over PCIe BAR apertures.
* **Coherency:** Enforces `flush_dcache_page()` and `dma_wmb()` memory barriers.

---

## 5. Pillar 4: DRM GPU Client fdinfo (`Documentation/gpu/drm-usage-stats.rst`)

### 5.1 Standardized DRM File Descriptor Telemetry
In modern Linux kernels (5.19+ and 6.0+), every GPU process exposes structured telemetry under `/proc/<pid>/fdinfo/<fd>`:

```text
drm-driver:           nvidia / amdgpu / xe
drm-client-id:        42
drm-pdev:             0000:06:00.0
drm-total-vram:       6442450944 bytes
drm-resident-vram:    3758096384 bytes
drm-engine-compute:   128490210 ns
```

RamShared uses this data to correlate memory residency against swap tier allocation.

---

## 6. Pillar 5: Kernel Tracepoints & eBPF Observability

### 6.1 Standard Tracepoints
1. `block:block_rq_issue` — Nanosecond start timestamp of PCIe DMA request.
2. `block:block_rq_complete` — Request termination and status code validation.
3. `mm:mm_vmscan_direct_reclaim_begin` — Immediate signal of memory pressure.
