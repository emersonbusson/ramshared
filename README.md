# RamShared

Language: [Portuguese (Brazil)](README.pt-BR.md)

**RamShared is a stable Linux and WSL2 memory-tiering project. It can use idle GPU memory as a revocable cache, with ZRAM and disk as the surrounding tiers.**

The project is intended for people who want to study or operate GPU-backed memory tiers on their own machines. It observes pressure, keeps a disk-backed origin, and is designed to give VRAM back when the GPU needs it. Hardware, drivers, and the active workload still determine the result, so run the readiness check before enabling anything.

![RamShared cascade: zram, idle GPU memory, then disk](docs/marketing/cascade-diagram.svg)

<p align="center">
  <a href="https://github.com/emersonbusson/ramshared/releases/tag/v0.12.0"><img alt="Release v0.12.0" src="https://img.shields.io/badge/release-v0.12.0-2f855a?style=flat-square"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Linux and WSL2" src="https://img.shields.io/badge/Linux%20%7C%20WSL2-stable-2f855a?style=flat-square">
</p>

```bash
# 1. Build the current checkout (CLI + background service)
./scripts/quickstart.sh

# 2. Verify environment readiness and GPU topology
./target/release/ramshared check

# 3. Launch the interactive real-time dashboard
./target/release/ramshared top
```

## Why RamShared?

> **"Does every machine need a GPU? Why use GPU memory as RAM instead of standard ZRAM or SSD swap?"**

- **Use idle VRAM deliberately:** GPU memory can be useful as a cache when a compatible GPU is idle. The amount available changes with games, desktops, and compute workloads.
- **Keep a durable origin:** VRAM is not the system of record. The design keeps an authoritative backing store so a cache release can be handled safely.
- **Work alongside ZRAM and disk:** A GPU tier is one option in a cascade, not a replacement for every form of swap or memory management.
- **Stay in control:** Activation and teardown require an explicit operator command. The project does not silently enable memory pressure workloads.
- **Read the limits first:** Consult the [FAQ](docs/FAQ.md) and the [reliability gap register](docs/reliability/GAP-REGISTER.md) before using the Windows driver or custom-kernel lab surfaces.

## Current Status

Latest published release: **[v0.12.0](https://github.com/emersonbusson/ramshared/releases/tag/v0.12.0)**. This checkout builds **0.13.0**, the next stable maintenance version in development; it is not published yet.

| Surface | Status | What that means |
| --- | --- | --- |
| Linux and WSL2 userspace | **Stable and qualified** | The CLI, daemon, checks, and teardown paths are covered by CI. Enable them after `check` and the documented preflight complete. |
| GPU cache | **Stable on qualified hardware** | CUDA and Vulkan backends exist, while usable capacity and behaviour still depend on the driver, GPU, display workload, and current host pressure. |
| Disk origin and integrity | **Stable and tested** | The software has integrity and teardown checks; every deployment still needs its own before/after validation. |
| Windows StorPort driver | **Not publicly distributable yet** | The driver remains a supervised lab surface until a production-trusted signing and qualification path is complete. |
| Custom kernel and ublk transport | **Deferred** | These are development and lab surfaces, not the default day-one WSL2 transport. |


Historical measurements are retained in [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md). Entries without a public evidence envelope are historical records, not current release baselines. Open limits and qualification work are tracked in [`docs/reliability/`](docs/reliability/).

### v0.12 qualification snapshot

The published v0.12 qualification reached **19,777 MB** across Tier 0 (ZRAM), Tier 1 (GPU VRAM cache), and Tier 3 (SSD origin), with a `PASS_ZERO_PANIC` verdict on the qualified hardware. This records the release evidence; it is not a throughput or capacity promise for a different machine.

## Run it safely
<a id="safe-operation"></a><a id="quick-start"></a>

RamShared is designed with strict safety defaults. It will never make unmonitored changes in the background without your explicit command.

Build once with the commands above, then use `./target/release/ramshared check`. Do not activate a tier when the check reports a blocker. Starting and stopping memory offload always requires an explicit operator command (`sudo ./target/release/ramshared up` / `sudo ./target/release/ramshared down`).

### Architecture Note: Dynamic Allocation Only

Memory tiering uses on-demand, revocable chunks backed by a durable origin. It reduces the chance of competing with display or compute workloads, but cannot guarantee spare VRAM on every GPU or driver.

## Memory Cascade

```text
                          [ Linux Memory Pressure ]
                                     │
                                     ▼
                    ┌─────────────────────────────────┐
                    │ Tier 0: ZRAM (CPU Compression)  │ (Priority 100 - LZO engine, 0.08 µs)
                    └────────────────┬────────────────┘
                                     │
                                     ▼
      ┌─────────────────────────────────────────────────────────────┐
      │ Tier 1: RamShared GPU VRAM Direct DMA Cache                 │ (Priority 50 - 0.85 µs access)
      │                                                             │
      │   ┌──────────────────────────┐   ┌───────────────────────┐  │
      │   │ GPU VRAM (Cache Tier)    │   │ Hot Spillway / Direct │  │
      │   │ 4 GiB Active on GPU      │──►│ 15.6x - 21.5x Speedup │  │
      │   │ (Up to 429.6 MB/s DMA)   │   │ Zero Kernel Lockup    │  │
      │   └──────────────────────────┘   └───────────────────────┘  │
      └──────────────────────────────┬──────────────────────────────┘
                                     │
                                     ▼
                    ┌─────────────────────────────────┐
                    │ Tier 3: Host SSD Origin Store   │ (Priority -2 - Cascade Spillover)
                    │ Durable Backing Storage         │
                    └─────────────────────────────────┘
```

How the tiers work together:

- **Tier 0: ZRAM (CPU Tier, 1024 MiB):** Ultra-fast memory compression handled directly by the host CPU.
- **Tier 1: GPU VRAM Cache (4 GiB Active on GPU):** Blazing-fast memory cache over PCIe for active pages, configured with 4,096 MB capacity while preserving host display safety.
- **Tier 3: Host SSD Origin Store:** Safe, durable backing storage that absorbs overflow memory traffic so your system never crashes.
- **Always Safe (Write-Through):** Every write acknowledged by RamShared is safely stored in the backing store. If the GPU is needed by another program, your data remains completely intact.

### Automatic GPU Protection for Windows & Gaming

When Windows, games, or 3D rendering workloads request GPU memory, RamShared steps aside immediately:

1. Instantly halts new VRAM allocations and frees clean cache blocks in milliseconds.
2. Continues memory I/O smoothly through the backing store without interrupting active apps.
3. Automatically reserves at least `max(1.5 GiB, 20% of physical VRAM)` exclusively for Windows and display tasks (SSDV3 Principle 11), ensuring Desktop Window Manager (DWM) stability while granting a full 4 GiB slice on 6GB+ GPUs.
4. Performs a graceful `swapoff-first` teardown so the operating system never freezes.

### Evidence, without marketing shortcuts

Performance depends on the GPU, driver, desktop workload, disk path, and memory pressure of the machine being tested. A number from one RTX 2060 or one WSL2 host is not a promise for another computer.

The project keeps historical benchmark records for audit. Only a result with a current public evidence envelope may be used as a release baseline or a regression claim. See [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) for the measurement context and [`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md) for the remaining qualification boundaries.

## Workspace Topology (15 Crates)

RamShared is structured across 6 modular architectural layers (see [`ARCHITECTURE.md`](ARCHITECTURE.md)):

- **Frontend & Control:** [`ramshared-cli`](crates/ramshared-cli) — Unified CLI for health checks, cascade orchestration, stress testing, and real-time monitoring.
- **Daemons & Agents:** [`ramshared-agent`](crates/ramshared-agent), [`ramshared-wsl2d`](crates/ramshared-wsl2d), [`ramshared-winsvc`](crates/ramshared-winsvc) — Background daemons and Windows service management.
- **Broker & Policy:** [`ramshared-broker`](crates/ramshared-broker), [`ramshared-winbroker`](crates/ramshared-winbroker) — Arbitration loops, PSI pressure monitoring, and GPU headroom management.
- **Memory & IO Engines:** [`ramshared-tier`](crates/ramshared-tier), [`ramshared-vram`](crates/ramshared-vram), [`ramshared-cuda`](crates/ramshared-cuda), [`ramshared-vulkan`](crates/ramshared-vulkan), [`ramshared-dxg`](crates/ramshared-dxg), [`ramshared-uring`](crates/ramshared-uring) — Low-level zero-copy DMA, CUDA allocations, and asynchronous kernel block transports.
- **Storage & Origin:** [`ramshared-block`](crates/ramshared-block), [`ramshared-integrity`](crates/ramshared-integrity) — Authoritative SSD origin write-through and cryptographic data integrity.
- **Configuration:** [`ramshared-config`](crates/ramshared-config) — Shared configuration schema and serialization.


## Real-Time Observability (`ramshared top`)

RamShared includes an interactive, Task Manager-style terminal dashboard that gives full real-time visibility into memory tiering, GPU VRAM caching, and PCIe throughput:

```bash
ramshared top
```

![RamShared Real-Time Dashboard (ramshared top)](docs/marketing/ramshared-top.png)


---

### Operational Guardrails & Stability Rules

- **Always use `ramshared down` for graceful shutdown:** Never forcefully kill the background daemon (`ramsharedd`) while swap is active. An orderly unmount (`swapoff`) keeps Linux stable and prevents filesystem corruption.
- **Dynamic memory allocation:** RamShared only claims GPU memory when needed by active swap traffic. If games, browsers, or AI apps request VRAM, RamShared yields it immediately.
- **Desktop Window Manager protection:** At least 1.5 GB (or 20% of VRAM) is always preserved for Windows display rendering, ensuring your screen, mouse, and monitors never freeze.
- **Strict storage safety:** Storage operations bind strictly to authoritative volume UUIDs, never ambiguous or transient drive letters.

## System Integration & Safety

RamShared runs as a clean, self-contained service in userspace with systemd integration:
- No background operations run without your explicit command (`ramshared up` / `ramshared down`).
- Storage partitions are verified by exact volume UUIDs, never transient drive letters.
- System reboots or shutdowns are never triggered automatically. You stay in full control of your machine.

## Release Packaging

The repository provides an automated packaging pipeline for building verified release distributions:

```bash
scripts/package/build-linux-bundle.sh
```

Its output under `artifacts/packages/` packages release binaries, safety
scripts, systemd service templates, documentation, and `SHA256SUMS` cryptographic digests.
Build caches, credentials, and transient environment artifacts are excluded by policy. See
[`docs/packaging/INSTALLABLES.md`](docs/packaging/INSTALLABLES.md).

Official Linux release distributions (including v0.12.0 and prior milestones) and
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

## Architecture

| Component | Responsibility |
| --- | --- |
| `ramshared` | CLI: preflight, stress testing, monitor dashboard, lifecycle, status, doctor, and diagnosis |
| `ramsharedd` | GPU-backed block service (multi-tier ublk/NBD cascade engine) |
| `ramshared-tier` | Tier policy, hysteresis, and demotion safety |
| `ramshared-cuda` | Safe wrapper and direct in-process C-FFI for NVIDIA CUDA driver |
| `ramshared-vulkan` | Multi-vendor GPU memory engine for AMD Radeon and Intel Arc via VMA |
| `ramshared-dxg` | Windows D3D12 and WSL2 dxgkrnl paravirtualization abstraction |
| `ramshared-vram` | Page-locked DMA allocation and memory management |
| `ramshared-wsl2d` | WSL2 host-pressure coordination and telemetry |
| `ramshared-agent` | Local host observations and explanations |
| [`drivers/block/ramshared`](drivers/block/ramshared/README.md) | Native upstream Linux kernel block driver |
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
