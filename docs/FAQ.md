# FAQ — Frequently Asked Questions

## Operational Invariants & Architecture

RamShared enforces strict, fail-closed operational boundaries across host and virtualized environments. All active memory tiering operates via on-demand revocable chunks backed by an authoritative SSD origin, prioritizing system stability and data integrity.

The legacy full-VRAM NBD backend composition and `RAMSHARED_VRAM_PREALLOC_LEGACY` selector were removed from executable source and are no longer available, supported, or selectable. All operations utilize the modern dual-tier device architecture (`ublk`/`io_uring` and page-locked DMA).

## What is RamShared intended to model?

RamShared models compressed RAM (ZRAM) first, an SSD-authoritative logical device with a clean revocable VRAM cache second, and host disk swap as the final fallback. Acknowledged data belongs to the origin, not VRAM. If GPU measurement or allocation fails, cache capacity safely falls back to zero while the origin path remains the authoritative correctness boundary.

## Will it freeze my PC?

No. RamShared's hardened safety contract enforces identity-checked, swapoff-first origin detachment: it never detaches a daemon while its block device is active in the swap table. Additionally, automatic GPU headroom reservation ensures that 3D and gaming workloads reclaim VRAM instantly without desktop stalls or freezes.

## Is this free RAM for games?

No. A game or other external workload has priority for the GPU budget. The
system reserves `max(2 GiB, 20% of total VRAM)` and treats unknown WDDM/GPU
measurement as zero cache target. It neither promises a fixed amount of VRAM
nor identifies applications by name.

## Why did Task Manager show an unusual virtual disk?

This refers to earlier Windows miniport polling behavior. A
64 MiB virtual LUN could appear fully busy with zero throughput or latency when
class-driver polling and a miniport readiness condition disagreed. The modern
driver correction changed the not-ready result to a standards-compliant
not-ready condition rather than an indefinitely busy response.

The validated live record used an authoritative product LUN, generated **304 MiB** of
write/read traffic during sampling, and matched a direct **8 MiB** checksum
probe. It observed non-zero busy/write/queue counters and recorded
`DISK_IO_MEASURE_OK=true`.

## What do the status terms mean?

| State | Intended meaning |
| --- | --- |
| `Armed` | The SSD-authoritative logical tier is active; cache use dynamically scales with pressure. |
| `UsingZram` | Pressure is primarily in compressed RAM. |
| `UsingVram` | The cache contains attributable active memory pages. |
| `UsingDisk` | The lower disk/VHDX tier is in active use under high memory load. |
| `Demoting` | The cache is safely releasing capacity to yield to GPU-bound applications. |
| `Degraded` | Identity, origin, control, guardian, or cache telemetry requires attention. |
| `Off` | No product cascade is active. |

Schema v4 distinguishes physical GPU use, logical capacity, cached VRAM,
authoritative-origin writes, fallback swap use, memory pressure, and control or
guardian state.

## Can the desktop control or boot integration be used?

Yes, via explicit opt-in. The desktop control and boot integration operate through modular, fail-closed scripts (`scripts/safety/`). System-level modifications require explicit operator configuration (`install-cascade-boot.sh --enable`) rather than unmonitored background activation.

## What about WSL configuration paths?

Historical evidence found that Windows-style backslashes can be interpreted as
escapes in WSL configuration. The public documentation uses standardized POSIX paths
to ensure reliable, predictable operation across environments.

## What happens under external GPU pressure?

The dynamic governor immediately stops new cache allocations, drops clean chunks over PCIe, and
routes I/O directly through the authoritative SSD origin without interrupting active workloads. It has no
broad WSL shutdown or uncoordinated host reboot path.

## Can the Windows driver be installed on a physical host?

The Windows StorPort driver is designed for high-performance hardware storage acceleration. Public distribution requires Microsoft WHQL attestation; test-signed developer builds operate under explicit testing mode with fail-safe pagefile protection. See [`docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md`](packaging/WINDOWS-DRIVER-DISTRIBUTION.md).

## Does GDDR6 mix directly with DDR4?

No. GPU and system memory are managed by different controllers; data crosses
PCIe. Transport observations reflect high-throughput DMA transfers across the physical bus.

## Does RamShared only work with NVIDIA GPUs?

No. While NVIDIA CUDA (`cuMemHostAlloc` pinned host memory) was the initial
qualified MVP path because of mature GPU-PV under WSL2, RamShared is
hardware-agnostic:

- **AMD Radeon and Intel Arc**: Supported via `crates/ramshared-vulkan` using
  the Vulkan Memory Allocator (VMA) and cross-process external memory handles.
- **Linux block driver and ublk**: Native Linux block drivers
  (`drivers/block/ramshared/`) and `ublk` (`io_uring`) operate upstream
  independently of GPU vendors.
- **Headless or GPU-less systems**: If no GPU is detected or if GPU headroom is
  exhausted, the memory cascade falls back gracefully across Host RAM, ZRAM,
  and the authoritative SSD origin with zero GPU requirement.

## Why use GPU memory when NVMe striped arrays reach 28 GB/s and DDR5 reaches 70 GB/s?

This distinction separates sequential streaming throughput from virtual memory
paging dynamics:

- **4KB random page latency vs sequential throughput**: A 28 GB/s striped NVMe
  array achieves peak bandwidth on large sequential blocks (128 KB–1 MB) at high
  queue depths (QD=32–128). Virtual memory swap operates in **4KB pages
  synchronously at QD=1** on page faults (`.rw_page`). At 4KB QD=1, physical
  flash drives drop to 30–80 MB/s. Inside virtualized environments like WSL2,
  traversing `ext4` ➔ `virtio-scsi` ➔ `Hyper-V` ➔ `NTFS` inflates 4KB latency to
  ~30,000 µs (30 ms), causing desktop lockups. Pinned PCIe DMA transfers bypass
  the storage stack entirely, moving 4KB pages in 231 µs down to 0.05 µs.
- **Flash endurance and TBW exhaustion**: NAND flash has physical write limits
  (TBW). Intensive swap thrashing writes tens of gigabytes per hour, rapidly
  degrading SSD flash cells. VRAM (GDDR6/GDDR6X/HBM) has infinite write
  durability and does not wear out silicon.
- **CPU compression offload**: ZRAM runs in DDR5 but consumes host CPU cores for
  LZ4/ZSTD compression. Pinned PCIe DMA offloads pages asynchronously without
  burning CPU compute cycles needed by compilers or applications.

## Is RamShared designed to transfer data for GPU processing?

No. RamShared is not a compute pipeline or a dataset loader for CUDA shaders or
PyTorch training. It is an operating system memory hierarchy tiering engine. In
typical developer workstations, dedicated GPUs sit idle with 6–16 GB of unused
VRAM. RamShared opportunistically leases that dormant silicon as a revocable L1
cache for host virtual memory. When a real GPU workload requests VRAM,
RamShared evicts clean cache chunks in milliseconds, leaving GPU compute
unaffected.

## Where are the verified records?

[validation.md](../validation.md) is the append-only empirical log and
[reliability evidence](reliability/) records open gates. If a number is not
recorded there with context and a verdict, treat it as unverified.
