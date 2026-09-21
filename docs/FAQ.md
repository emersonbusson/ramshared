# FAQ — Frequently Asked Questions

## Operational Invariants & Architecture

RamShared enforces strict, fail-closed operational boundaries across host and virtualized environments. All active memory tiering operates via on-demand revocable chunks backed by an authoritative SSD origin, prioritizing system stability and data integrity.

The legacy full-VRAM NBD backend composition and `RAMSHARED_VRAM_PREALLOC_LEGACY` selector were removed from executable source and are no longer available, supported, or selectable. Standard WSL2 uses NBD as its baseline transport. `ublk`/`io_uring` is qualified on native Linux or WSL2 with a compatible custom kernel, not as a universal stock-WSL2 default.

## What is RamShared intended to model?

RamShared models compressed RAM (ZRAM) first, an SSD-authoritative logical device with a clean revocable VRAM cache second, and host disk swap as the final fallback. Acknowledged data belongs to the origin, not VRAM. If GPU measurement or allocation fails, cache capacity safely falls back to zero while the origin path remains the authoritative correctness boundary.

## Can it freeze or stall my PC?

Any swap or GPU path can stall when the host, driver, storage, or teardown path
is unhealthy. RamShared reduces that risk with identity checks, swapoff-first
origin detachment, bounded admission, and fail-closed health evaluation. Open
live-host qualifications remain listed in the gap register; the software does
not claim zero stall risk on unqualified machines.

## Is this free RAM for games?

No. A game or other external workload has priority for the GPU budget. The
broker/NBD path reserves `max(1536 MiB, 20% of total VRAM)` as a capacity
boundary and separately retains `768 MiB` of reported free VRAM as a runtime
buffer. Unknown WDDM/GPU measurement yields a zero cache target. The origin
cache and StorPort use their own policies described below.

## Can I run 3D games, rendering software, or GPU workloads while RamShared is active?

Concurrent GPU workloads are supported only within the measured budget and
remain hardware- and driver-dependent. The governor can stop admission and
evict clean chunks, but it does not promise a particular reclaim latency or
frame-rate outcome.

The three current reserve policies serve different consumers:

- **Broker/NBD:** capacity reserve `max(1536 MiB, 20%)`, plus a separate
  `768 MiB` runtime free buffer.
- **Origin cache:** capacity reserve `max(2 GiB, 20%)`.
- **Windows StorPort:** `max(configured reserve, 512 MiB, 10%)`.

The reserve bounds cache capacity. The runtime buffer protects a future
allocation against live external GPU use; it is not an additional advertised
cache capacity.

## Does RamShared increase SSD wear (TBW)?

RamShared can change the amount and shape of SSD traffic, but its
authoritative write-through origin still performs storage writes. No current
evidence supports a universal TBW reduction claim. Measure the workload's
origin writes and cache hit rate before drawing an endurance conclusion.


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

The dynamic governor stops new cache allocations after the configured pressure signal, drops eligible clean chunks, and
routes cache misses through the authoritative SSD origin. Timing and application impact depend on pressure and driver behaviour. It has no
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
  exhausted, the GPU cache target is zero and the remaining host-memory and
  origin paths determine whether the requested topology can operate.

## Why use GPU memory when NVMe striped arrays reach 28 GB/s and DDR5 reaches 70 GB/s?

This distinction separates sequential streaming throughput from virtual memory
paging dynamics:

- **4KB random page latency vs sequential throughput**: A 28 GB/s striped NVMe
  array achieves peak bandwidth on large sequential blocks (128 KB–1 MB) at high
  queue depths (QD=32–128). Virtual memory swap operates in **4KB pages
  synchronously at QD=1** on page faults (`.rw_page`). At 4KB QD=1, physical
  flash drives can be much slower at low queue depth. The registered EVD-0039
  run on an RTX 2060, PCIe Gen3 x16, and a compatible WSL2 custom kernel
  measured 231 µs median for its `ublk` 4 KiB workload. That result does not
  describe standard WSL2 NBD or other hardware.
- **Flash endurance and TBW exhaustion**: NAND flash has physical write limits
  (TBW). The effect of RamShared on SSD writes depends on workload, cache hits,
  and the authoritative-origin policy and must be measured per deployment.
- **CPU compression offload**: ZRAM runs in DDR5 but consumes host CPU cores for
  LZ4/ZSTD compression. Pinned PCIe DMA offloads pages asynchronously without
  burning CPU compute cycles needed by compilers or applications.

## Is RamShared designed to transfer data for GPU processing?

No. RamShared is not a compute pipeline or a dataset loader for CUDA shaders or
PyTorch training. It is an operating system memory hierarchy tiering engine. In
typical developer workstations, dedicated GPUs sit idle with 6–16 GB of unused
VRAM. RamShared opportunistically leases that dormant silicon as a revocable L1
cache for host virtual memory. When a real GPU workload requests VRAM,
RamShared can evict clean cache chunks, but the latency and effect on concurrent
GPU compute depend on the driver, hardware, and active workload.

## Can I use RamShared inside Docker or containerized environments?

Yes. In WSL2 or native Linux hosts, containers share the host kernel's virtual memory subsystem and swap cascade. You do not need to configure RamShared inside individual containers or Dockerfiles; container memory pressure automatically leverages the host's accelerated ZRAM/VRAM/SSD tiering.

## How do I cleanly deactivate or uninstall RamShared?

The operator deactivates the cascade via `ramshared down` (or using `sudo scripts/safety/wsl2-dual-tier-swap.sh --disable`). RamShared executes a swapoff-first ordered teardown: it deactivates the virtual swap tier, flushes data to host storage, unmounts the block device, and releases all allocated VRAM back to the GPU driver cleanly.

## Where are the verified records?

[validation.md](../validation.md) is the append-only empirical log and
[reliability evidence](reliability/) records open gates. If a number is not
recorded there with context and a verdict, treat it as unverified.
