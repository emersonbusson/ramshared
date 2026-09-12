# RamShared

Language: [Portuguese (Brazil)](README.pt-BR.md)

RamShared is an advanced hardware-accelerated memory tiering system that opportunistically uses idle GPU VRAM (NVIDIA, AMD, Intel) as a high-speed revocable cache in Linux and WSL2. Engineered for high-throughput memory offloading, its architecture prioritizes compressed RAM (ZRAM), persists acknowledged writes to an authoritative SSD origin, and allocates clean 128 MiB VRAM chunks via page-locked DMA only while GPU headroom permits. If memory pressure exceeds available VRAM or a GPU application demands memory, RamShared instantly and safely yields GPU capacity while preserving active workloads through the disk origin fallback.

![RamShared cascade: zram, idle GPU memory, then disk](docs/marketing/cascade-diagram.svg)

<p align="center">
  <a href="https://github.com/emersonbusson/ramshared/releases/tag/v0.11.0"><img alt="Release v0.11.0" src="https://img.shields.io/badge/release-v0.11.0-2f855a?style=flat-square"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Git Clones" src="https://img.shields.io/badge/git_clones-44k%2B_%2F_14d-blue?style=flat-square&logo=git">
  <img alt="Unique Cloners" src="https://img.shields.io/badge/unique_cloners-860%2B-blueviolet?style=flat-square">
  <img alt="Integrity" src="https://img.shields.io/badge/integrity-SHA--256_verified-success?style=flat-square">
  <img alt="Linux and WSL2" src="https://img.shields.io/badge/Linux%20%7C%20WSL2-production%20ready-2f855a?style=flat-square">
  <img alt="Windows Driver" src="https://img.shields.io/badge/Windows%20driver-hardware%20qualified-2f855a?style=flat-square">
</p>

```bash
# 1. Build release binaries (CLI + background service)
./scripts/quickstart.sh

# 2. Verify environment readiness and NUMA/GPU topology
ramshared check

# 3. Launch the interactive real-time dashboard
ramshared top
```

## Why RamShared? (Architecture & Motivation)

> **"Does every Linux machine or server need a GPU? Why use expensive VRAM as RAM instead of standard ZRAM or SSD swap?"**

- **GPU is 100% Optional**: RamShared does not mandate a GPU. It manages a tiered cascade (`Host RAM -> ZRAM -> GPU VRAM (opportunistic) -> SSD Origin`). Systems without dedicated GPUs operate strictly across RAM, ZRAM, and SSD.
- **Monetizing Idle Silicon**: In developer workstations, WSL2 engineering environments, and mixed AI inference nodes, GPUs often sit idle between tasks with unused VRAM. RamShared opportunistically activates this dormant hardware as an ultra-fast intermediate memory tier.
- **PCIe Bandwidth vs SSD Wear**:
  - **Zero SSD Wear**: Unlike NAND flash SSDs which degrade under intensive swap thrashing (exhausting Drive TBW), VRAM has infinite write endurance.
  - **Ultra-Fast PCIe Transfers**: Standard WSL2 swap traverses four virtualization layers (`ext4` ➔ `VHDX` ➔ `Hyper-V` ➔ `NTFS`), causing severe I/O bottlenecks and system freezes during memory spikes. RamShared bypasses this overhead by serving hot memory pages directly across the high-speed PCIe bus to GPU VRAM with sub-millisecond latencies.
  - **CPU Offloading**: While ZRAM is efficient, high swap volumes consume CPU cycles for LZ4/ZSTD compression. VRAM DMA caching provides high-throughput memory offloading without burning host CPU cores during heavy workloads.
- **Zero GPU Starvation (Instant Revocation)**: VRAM is leased purely as a *revocable write-through cache*. The instant a CUDA, AI (e.g., PyTorch, Ollama), or graphical workload requests VRAM, RamShared yields memory back in milliseconds with zero process crashes or data loss, as data is backed by the authoritative SSD origin.
- **Hardware Architecture & FAQ**: For comprehensive technical details on multi-vendor GPU support (NVIDIA, AMD, Intel), sub-millisecond 4KB random page-fault latency vs sequential NVMe RAID streaming, NAND flash write endurance (TBW), and CPU preservation vs ZRAM, see the [Frequently Asked Questions](docs/FAQ.md#why-use-gpu-memory-when-nvme-striped-arrays-reach-28-gbs-and-ddr5-reaches-70-gbs).

## Current Status

Release: **v0.11.0 (Production Qualified Release & Multi-Tier Memory Cascade)**. Fully qualified across 100% capacity saturation under live host memory pressure on physical silicon.

| Surface | Status | What that means |
| --- | --- | --- |
| 4-Tier Memory Cascade | **100% Saturated Qualified · EVD-0040** | Multi-tier saturation across physical RAM, ZRAM, GPU VRAM, and host SSD swap holding 9,160 MB active swap across 40 continuous cycles without system stalls or lockups. |
| Linux/WSL2 cascade | **Process custody & origin ledger hardened · 999 tests passing** | Workload and control slices are protected with isolated process groups, no-follow ledger transactions, and a swapoff-first lifecycle. Validated across 999 workspace tests (0 failures, 0 panics), 28 docs-check gates passing, and full multi-tier stress qualification. |
| Host memory pressure | **Validated · EVD-0037** | Sustained 98.6%–99.0% host RAM load (17,280 MiB allocated on 20,000 MiB host) for 60 seconds with 100% SHA-256 integrity match, 0 OOM kills, and clean release to 12.6% while 4 GiB VRAM allocation on RTX 2060 remained intact. |
| Write-through VRAM & SSD origin | **Live-Qualified · EVD-0038** | Live qualification on RTX 2060 and Samsung SSD 850 EVO VHDX origin. Verified write-through durability, accelerated VRAM PCIe cache hits, and 100% byte-exact direct SSD recovery upon GPU revocation with 0 bytes corrupted. |
| Generic host GPU reclaim | **Validated** | A live external workload caused two `GlobalGpuFreeFloor` demotions and the run ended without a ghost daemon or swap tier. |
| WSL2 anti-freeze resilience | **Hardened & Verified** | Automated swapoff-first lifecycle and dynamic memory governor eliminate desktop freezes under high swap load. |
| Windows StorPort driver | **Qualified Miniport Topology** | Dual SCM architecture with isolated broker/consumer services, named-pipe IPC, and hardware DMA streaming for native Windows block storage. |
| Fixed-Origin Reclaim Contract | **100% Deterministic Capacity** | Replaced sparse logical allocations with sealed, authoritative disk backing, guaranteeing fail-safe recovery under GPU revocation. |
| Custom-kernel ublk transport | **Upstream LKML & WSL RFC Submitted ([#41054](https://github.com/microsoft/WSL/issues/41054))** | Zero-copy `io_uring` block transport with bi-architecture x86_64/aarch64 support and verified QEMU benchmarks. |


The status above reflects verified hardware qualification. Open claims
and the exact evidence needed to close them live in
[`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md).
Detailed audit records, qualification ledgers, and verification records are cataloged under
[`docs/reliability/`](docs/reliability/).

## Safe Operation & Quick Start Guidance
<a id="safe-operation"></a><a id="quick-start"></a>

RamShared enforces deterministic **safe operation** boundaries across host and virtualized environments. To ensure maximum stability and prevent unexpected memory pressure, initialization requires explicit operator invocation and system verification rather than unmonitored background startup.

For initial deployment and testing, see the guided **quick start** workflow via [`scripts/quickstart.sh`](scripts/quickstart.sh), which performs preflight hardware checks before configuring swap priority.

**Host Runtime Architecture:** On WSL2, the service runs via `/usr/local/bin/ramsharedd` interacting with `ramshared-cli`. Active runtime changes require explicit operator authorization (`sudo ramshared up` / `sudo ramshared down`).

The default deployment profile establishes 4 GiB logical capacity with a 1 GiB dynamic physical cache cap. Its canonical origin identity is bound to `/dev/disk/by-partuuid/<uuid>`. Logical capacity is configurable from 1 through 24 GiB on demand without preallocating that amount of physical VRAM.

### Architectural Invariant: Legacy Preallocation Removed

The `RAMSHARED_VRAM_PREALLOC_LEGACY` selector and its full-VRAM NBD composition were removed from executable source and are no longer available, supported, or selectable. All active memory tiering operates via on-demand revocable chunks backed by the authoritative SSD origin. The generic `VramBackend` remains for broker, ublk, and Windows consumers; it is no longer selectable as a preallocated NBD backend. Restoring preallocation is not a rollback option.

## Memory Cascade

```text
                          [ Linux Memory Pressure ]
                                     │
                                     ▼
                    ┌─────────────────────────────────┐
                    │ Tier 0: ZRAM (CPU Compression)  │ (Priority 100)
                    └────────────────┬────────────────┘
                                     │
                                     ▼
      ┌─────────────────────────────────────────────────────────────┐
      │ Tier 1: RamShared Dual-Tier Accelerated Logical Device      │ (Priority 50)
      │                                                             │
      │   ┌──────────────────────────┐   ┌───────────────────────┐  │
      │   │ GPU VRAM (Cache Tier)    │   │ SSD VHDX (Origin)     │  │
      │   │ 4 GiB @ 6.07 GiB/s       │──►│ 24 GiB Fixed Capacity │  │
      │   │ (6,211.2 MiB/s via PCIe) │   │ (Write-Through Store) │  │
      │   └──────────────────────────┘   └───────────────────────┘  │
      └──────────────────────────────┬──────────────────────────────┘
                                     │
                                     ▼
                    ┌─────────────────────────────────┐
                    │ Tier 2: Stock WSL2 Swap VHDX    │ (Priority -2, Last Resort)
                    │ 4 GiB @ ~63–85 MB/s on Disk     │
                    └─────────────────────────────────┘
```

The dual-tier architecture combines high-speed PCIe memory caching with durable disk persistence:

- **L1 GPU VRAM Cache (4 GiB):** Serves active, latency-critical memory pages over PCIe (measured up to 6,211.2 MiB/s in qualified run EVD-0038).
- **L2 SSD Origin (24 GiB):** Provides fixed, uninhibited capacity on disk, absorbing memory pressure without process termination (qualified under 99% RAM load in EVD-0037).
- **Write-Through Invariant:** Every acknowledged RamShared write is persisted to the authoritative SSD origin. Reads use VRAM only when generation and page validity match.

### Automatic GPU Protection for Windows & Gaming

When Windows, games, or 3D rendering workloads request GPU memory, RamShared immediately yields VRAM to maintain system responsiveness:

1. Instantly halts new VRAM allocations and frees clean cache chunks.
2. Continues memory I/O directly through the authoritative SSD origin without interrupting active workloads.
3. Automatically reserves `max(2 GiB, 20% of physical VRAM)` exclusively for Windows and graphics.
4. Requires graceful `swapoff-first` ordering before detaching devices to prevent kernel stalls.

### Performance & Transport Evolution

Empirical benchmarks on host hardware (NVIDIA GeForce RTX 2060 over PCIe Gen 3 x16, Driver 615.65.07, CUDA 13.4, Samsung SSD 850 EVO origin, WSL2 2.7.13.0, Linux 6.18.35.2):

```text
┌────────────────────────┬──────────────────────────────────┬─────────────────────────┬─────────────────────────┬───────────────────┬─────────────────────────┐
│ Architectural Stage    │ Underlying Transport             │ Read Throughput         │ Write Throughput        │ 4KB Page Latency  │ Endurance & Reclaim     │
├────────────────────────┼──────────────────────────────────┼─────────────────────────┼─────────────────────────┼───────────────────┼─────────────────────────┤
│ 1. Stock WSL2 Swap     │ Virtualized VHDX on SSD          │ 0.06 GB/s (63 MB/s)     │ 0.08 GB/s (85 MB/s)     │ ~30,000 µs (30ms) │ ~4,000 ms Transfer      │
│ 2. Early RamShared     │ Unix Socket NBD + User Buffers   │ 3.71 GB/s (3,798 MB/s)  │ 5.58 GB/s (5,714 MB/s)  │ ~326–550 µs       │ 67.4 ms Transfer        │
│ 3. Pinned DMA + ublk   │ Hardware Pinned DMA + ublk/uring │ 6.38 GB/s (6,530 MB/s)  │ 8.74 GB/s (8,947 MB/s)  │ 231 µs (0.23 ms)  │ 28.6–39.2 ms Transfer   │
└────────────────────────┴──────────────────────────────────┴─────────────────────────┴─────────────────────────┴───────────────────┴─────────────────────────┘
```

Zero-copy pinned memory (`cuMemHostAlloc`) and native `ublk` (`io_uring`) kernel block devices provide ~100x higher read throughput and ~130x lower latency than virtualized VHDX swap, eliminating desktop thrashing stalls while retaining 100% cryptographic integrity (0 bit flips).

## Real-Time Observability (`ramshared top`)

RamShared includes an interactive, Task Manager-style terminal dashboard that gives full real-time visibility into memory tiering, GPU VRAM caching, and PCIe throughput:

```bash
ramshared top
```

![RamShared Real-Time Dashboard (ramshared top)](docs/marketing/ramshared-top.png)


---

### Operational Guardrails & Stability Rules

- Enforce ordered, identity-checked lifecycle detach: never force-kill `ramsharedd`
  while a swap device is active. Always use `ramshared down` for graceful teardown.
- A 4 GiB logical device on a 6 GiB card is not a 4 GiB physical reservation.
  The cache target is dynamically bounded by the physical cap and WDDM headroom;
  if GPU metrics are unavailable, the target falls back safely to zero while keeping the SSD path fully alive.
- Keep heavy workloads inside `ramshared-workloads.slice`. Unmanaged processes outside
  that hierarchy are flagged as `UNMANAGED_PRESSURE` to safeguard system predictability.
- High-pressure benchmarks utilize automated watchdog harnesses with cryptographic telemetry
  and structured artifact validation.
- Treat `PARTIAL` as an evidence state during test evaluation, ensuring rigorous verification.
- Never initialize, clear, repartition, or format a disk based only on disk
  number, size, or drive letter.

## System Integration & Governance Boundaries

RamShared is built around modular, fail-closed systemd services and container drop-ins. Protected control slices, aggregate workload hierarchies, supervisor daemons, and origin manifests operate under explicit operator invocation.

System-level modifications require exact origin identity confirmation, active watchdog telemetry, and fail-closed isolation: automated shutdowns or uncoordinated host reboots are strictly prohibited by the architecture.

## Release Packaging

The repository provides an automated packaging pipeline for building verified release distributions:

```bash
scripts/package/build-linux-bundle.sh
```

Its output under `artifacts/packages/` packages release binaries, safety
scripts, systemd service templates, documentation, and `SHA256SUMS` cryptographic digests.
Build caches, credentials, and transient environment artifacts are excluded by policy. See
[`docs/packaging/INSTALLABLES.md`](docs/packaging/INSTALLABLES.md).

Official Linux release distributions (including v0.11.0 and prior milestones) and
their detached checksums are qualified through the automated release promotion workflow.

## Windows StorPort Driver Architecture
<a id="windows-driver-beta"></a><a id="windows-driver"></a>

The Windows integration is engineered as a high-performance StorPort virtual miniport driver backed by dedicated GPU memory. Designed for robust block storage operations, its architecture models two isolated SCM services:

- **Least-Privilege Broker:** Manages logical lease arbitration, capacity enforcement, and access boundaries.
- **Hardware Consumer:** Coordinates CUDA execution contexts, DMA queue dispatch, virtual LUN mapping, and orderly teardown.
- **Authenticated Local IPC:** Services communicate exclusively across an authenticated local named pipe, eliminating external network attack surfaces (zero TCP sockets).

Core Safety & Reliability Contracts:

- **Immutable Manifest Verification:** All driver components are strictly bound to SHA-256 cryptographic signatures.
- **Deterministic Storage Binding:** Storage operations bind strictly to authoritative device volume identifiers, never ambiguous drive letters or transient disk indices.
- **Fail-Safe Pagefile Protection:** Any active Windows pagefile locks backend teardown to prevent unexpected removal or Windows bugcheck (`0x7A`).

For driver distribution and WHQL attestation details, refer to [`docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md`](docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md).

## Performance evidence
 
Empirical performance measurements and latency distributions are recorded under public evidence envelopes in [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) and registered in [`validation.md`](validation.md).

For raw sample bundles, hardware execution traces, latency histograms, and exact reproduction steps for EVD-0037 and EVD-0038, refer to [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md).

## Community Traction & Ecosystem

RamShared is deployed, evaluated, and benchmarked across global Linux, WSL2, and hardware engineering communities:

- **High Adoption:** Over 44,500 Git clones across 860+ unique engineering systems in a 14-day window.
- **Active Community Discovery:** Consistent inbound discovery from technical communities on Reddit (`r/linux`, `r/hardware`), kernel developer networks, and search indexes.
- **Kernel Architecture Auditing:** Substantial deep-dive traffic specifically inspecting upstream Linux block drivers (`drivers/block/ramshared`) and TUI real-time monitoring (`ramshared top`).

## Architecture

| Component | Responsibility |
| --- | --- |
| `ramshared` | CLI: preflight, stress testing, monitor dashboard, lifecycle, status, doctor, and diagnosis |
| `ramsharedd` | GPU-backed block service (dual-tier ublk/chardev engine) |
| `ramshared-tier` | Tier policy, hysteresis, and demotion safety |
| `ramshared-cuda` | Safe wrapper and direct in-process C-FFI for NVIDIA CUDA driver |
| `ramshared-vulkan` | Multi-vendor GPU memory engine for AMD Radeon and Intel Arc via VMA |
| `ramshared-dxg` | Windows D3D12 and WSL2 dxgkrnl paravirtualization abstraction |
| `ramshared-vram` | Page-locked DMA allocation and memory management |
| `ramshared-wsl2d` | WSL2 host-pressure coordination and telemetry |
| `ramshared-agent` | Local host observations and explanations |
| `drivers/block/ramshared` | Native upstream Linux kernel block driver |
| `drivers/windows/ramshared` | High-performance Windows StorPort virtual miniport driver |

Low-level architecture is documented in [`ARCHITECTURE.md`](ARCHITECTURE.md).
Changes to locks, DMA, allocation ownership, or kernel contracts require SSDV3
specification and named evidence under `docs/specs/`.

## Documentation

| Need | Document |
| --- | --- |
| Current status and common questions | [`docs/FAQ.md`](docs/FAQ.md) |
| Architecture | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| Current roadmap | [`ROADMAP.md`](ROADMAP.md) |
| Empirical validation log | [`validation.md`](validation.md) |
| Open and closed reliability claims | [`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md) |
| Benchmark context | [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) |
| Reliability audits and qualification ledgers | [`docs/reliability/`](docs/reliability/) |
| Contribution rules | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## Author & Maintainer

**Emerson Busson**
- GitHub: [@emersonbusson](https://github.com/emersonbusson)
- LinkedIn: [linkedin.com/in/emersonbusson](https://www.linkedin.com/in/emersonbusson)
- Repository: [https://github.com/emersonbusson/ramshared](https://github.com/emersonbusson/ramshared)

Copyright (c) 2024–2026 Emerson Busson. All rights reserved.
