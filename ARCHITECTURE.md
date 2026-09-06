# Architecture

RamShared models **idle GPU memory** as a clean, revocable cache for an SSD-authoritative origin store, orchestrating a high-throughput multi-tier memory cascade across Linux and WSL2.

## Operational Invariants & System Status

RamShared enforces deterministic fail-closed execution boundaries and strict identity bindings:
- **Write-Through Invariant:** Every acknowledged write is persisted to the authoritative SSD origin before cache mutation. VRAM eviction or reclamation affects performance, not data integrity.
- **Ordered Teardown:** Swapoff-first ordering guarantees that devices are never detached while active in the kernel swap table.
- **Dynamic Headroom Protection:** GPU memory is dynamically bounded by WDDM headroom, automatically reserving `max(2 GiB, 20% total VRAM)` for 3D and graphics workloads.
- **Legacy Preallocation Sunset:** The legacy full-VRAM NBD source composition and `RAMSHARED_VRAM_PREALLOC_LEGACY` selector were removed from executable source and are no longer available or supported.

| Track | Status | Deployment Architecture |
| --- | --- | --- |
| Linux / WSL2 cascade | Production Qualified (EVD-0040) | Multi-tier cascade via ublk/io_uring, page-locked DMA, and ZRAM |
| Windows StorPort | Hardware Miniport Qualified | Isolated SCM broker/consumer services communicating over local named pipes |

---

## Track 1 — Linux / WSL2 Dual-Tier Topology

```text
Pressure  →  zram                 priority 100   hot (CPU LZ4)
          →  RamShared Device     priority  50   accelerated logical tier
                ├─ VRAM cache                    clean + revocable (PCIe DMA)
                └─ SSD origin                    authoritative store (disk)
          →  WSL swap VHDX        priority  -2   last resort fallback
```

**Why tiered?** Under WDDM reclaim, a 4 KB read measured ~**1.18 s**. Behind
zram, only cooler pages reach the RamShared device. Acknowledged writes reach
the origin and `sync_data` before cache mutation, so VRAM allocation failure or
revocation changes performance, not block correctness.

**Dynamic Reclaim Policy:** The controller frees clean 128 MiB cache
chunks under memory restriction or GPU demand, and scales allocations dynamically
when healthy headroom is verified. Missing WDDM/GPU measurement sets the
physical target safely to zero. Origin detach strictly retains swapoff-first ordering.

**Lifecycle observability:** schema v4 derives topology plus independent
`control_state`, `origin_state`, `cache_state`, `guardian_state`, and
`overall_state`. A usable cache cannot mask pressure, a stale guardian, or an
origin failure.

**Invariant A1:** demotion is valid only if lower tiers can absorb pages
(disk swap or sufficient free RAM). The controller verifies this before any lifecycle
transition.

On WSL2, Windows WDDM/VidMm remains the memory authority. The physical target
is the minimum of logical capacity, the sealed cache cap, and the measured
budget after external use and `max(2 GiB, 20% total VRAM)` headroom.

### Control-Plane Containment

RamShared manages workloads within a dedicated `ramshared-workloads.slice` budget.
Unmanaged memory allocations outside this hierarchy are monitored and flagged as `UNMANAGED_PRESSURE`
to protect system responsiveness. Control units occupy `ramshared-control.slice` with protected
memory and elevated CPU/I/O weights.

The supervisor's policy closes admission in `GUARDED`,
shrinks cache and manages discardable scopes in `CRITICAL`,
and enforces bounded termination sequences in `EMERGENCY`.

### Modular Architecture — 15 Workspace Crates

The codebase is organized into 15 focused Rust crates across 6 architectural tiers:

| Layer | Crates | Role & Responsibility |
| :--- | :--- | :--- |
| **Layer 1: Frontend & CLI** | [`ramshared-cli`](crates/ramshared-cli/README.md) | Primary operator interface (`doctor`, `stress`, `monitor`, `top`, `cascade`, `diagnose`). |
| **Layer 2: Daemons & Agents** | [`ramshared-wsl2d`](crates/ramshared-wsl2d/README.md)<br>[`ramshared-agent`](crates/ramshared-agent/README.md)<br>[`ramshared-winsvc`](crates/ramshared-winsvc/README.md)<br>[`ramshared-winbroker`](crates/ramshared-winbroker/README.md) | In-guest block device daemon (`ublk`/NBD), local kernel swap agent, Windows StorPort worker service, and SCM broker daemon. |
| **Layer 3: Broker & Policy** | [`ramshared-broker`](crates/ramshared-broker/README.md)<br>[`ramshared-config`](crates/ramshared-config/README.md)<br>[`ramshared-tier`](crates/ramshared-tier/README.md) | Logical lease arbitration, fail-closed configuration parsing, and 3-tier cascade state machine (N1/N2/N3 hysteresis). |
| **Layer 4: Memory & I/O** | [`ramshared-vram`](crates/ramshared-vram/README.md)<br>[`ramshared-cuda`](crates/ramshared-cuda/README.md)<br>[`ramshared-vulkan`](crates/ramshared-vulkan/README.md)<br>[`ramshared-uring`](crates/ramshared-uring/README.md) | Hardware-agnostic VRAM allocator abstraction, NVIDIA CUDA DMA, cross-vendor Vulkan allocator (AMD/Intel), and Linux `io_uring` engine. |
| **Layer 5: Storage & Origin** | [`ramshared-block`](crates/ramshared-block/README.md)<br>[`ramshared-integrity`](crates/ramshared-integrity/README.md)<br>[`ramshared-dxg`](crates/ramshared-dxg/README.md) | Authoritative SSD origin persistence, SHA-256 block corruption prevention, and `/dev/dxg` WDDM memory budget query. |
| **Layer 6: Kernel Drivers** | `drivers/block/ramshared`<br>`drivers/windows/ramshared` | Native upstream Linux kernel block driver and high-performance Windows StorPort virtual miniport driver (C). |


### Anti-hang rules (learned the hard way)

1. Never detach the origin daemon while NBD/ublk remains in the swap table.
2. Cache chunks are clean and independently revocable; origin detach is swapoff-first.
3. Refuse a lifecycle transition on ghost `(deleted)` swap or an unsealed/non-block PARTUUID.
4. Missing guardian, GPU measurement, or supervisor status is never green.
5. All system integrations operate under fail-closed, operator-invoked boundaries.

Related design record: [docs/specs/no-milestone/wsl2-cascade-boot/](docs/specs/no-milestone/wsl2-cascade-boot/)

---

## Track 2 — Windows StorPort Architecture

The native path is a StorPort virtual disk whose I/O is completed by
`RamSharedWinSvc`. A separate least-privilege `RamSharedBroker` SCM service
owns only logical lease arbitration. The consumer depends on the broker and
uses an authenticated local named-pipe boundary (zero external TCP network listeners).
Both services and their immutable configs are validated by a single SHA-256 product manifest.

**Hard rule:** Never tear the disk down under an active pagefile (BugCheck **0x7A** prevention).
Ordered teardown (DT-9) enforces this strictly.

---

## Process

Structural work uses **SSDV3** (PRD → SPEC → IMPL).  
We write failure modes in [docs/reliability/DEGRADATION-MATRIX.md](docs/reliability/DEGRADATION-MATRIX.md).  
We don’t thrash the live WSL you work in for “fun” benchmarks.
