# Roadmap

Current release posture: **v0.14.1 stable maintenance release**. Standard WSL2
uses NBD as its baseline transport. `ublk`/`io_uring` is qualified on native
Linux or WSL2 with a compatible custom kernel (EVD-0039); product lifecycle
promotion on the custom-kernel path remains deferred. EVD-0040 covers only
zero-copy CUDA host mapping.

Evidence lives in [validation.md](validation.md) and feature IMPL files.

---

## Done

### v0.10.0 — LKML Upstream RFC v2 & Tier 3 Cascade Qualification (2026-09)

- Upstream Linux Kernel Driver RFC v2 submitted to LKML and Microsoft WSL ([microsoft/WSL#41054](https://github.com/microsoft/WSL/issues/41054)).
- Consolidated Linux kernel drivers, multi-tier memory management, and fail-safe recovery into a unified production architecture.
- Historical Tier 3 cascade saturation evidence is retained in the benchmark and validation registries. It is not EVD-0040, which records zero-copy CUDA host mapping only.
- High-resolution vector diagrams (Inter & JetBrains Mono) with infinite resolution across displays.
- Interactive terminal TUI dashboard: `ramshared top`.

### Why VRAM isn’t “hot swap”

Phase 0 on real GPU-PV: eviction keeps data intact but can make a tiny read take **~1.18 s**. So VRAM sits **behind** compressed RAM (zram), not in front of it.

### Linux / WSL2 product

- Cascade: zram → VRAM (NBD + CUDA) → disk/VHDX  
- DEMOTE without killing processes (measured: hundreds of MiB, 0 corruption)  
- Anti-hang `down`: swapoff before stopping the daemon; refuse ghost/orphan mess  
- Live demote drill (~648 MiB, ~15 s swapoff)  
- Health sampler scripts  

### Boot opt-in (2026-07)

- `ramshared-cascade.service` via `scripts/safety/install-cascade-boot.sh`  
- Fail-closed preflight; stop = ordered `down`  
- SPEC: [docs/specs/no-milestone/wsl2-cascade-boot/](docs/specs/no-milestone/wsl2-cascade-boot/)

### Control app (2026-07-10)

- `scripts/safety/cascade-app.sh` (zenity GUI + CLI) + desktop launcher  
- SPEC: [docs/specs/no-milestone/cascade-desktop-app/](docs/specs/no-milestone/cascade-desktop-app/)

### WSL2 Upstream Kernel Contribution (2026-08)

- Formal evidence and candidate branch prepared for Microsoft WSL kernel ([microsoft/WSL#41054](https://github.com/microsoft/WSL/issues/41054))
- Dual-architecture validation: x86_64 and ARM64 compiled cleanly with zero W=1 diagnostics
- Sparse C=2 static analysis and QEMU capability test with 1,024 pages written
- Public candidate branch published at [emersonbusson/WSL2-Linux-Kernel](https://github.com/emersonbusson/WSL2-Linux-Kernel/tree/config/ublk-zram-writeback-6.18)

### Windows Host Driver Miniport Architecture

Format, pagefile residency, kernel-page drill, ordered teardown (DT-9), and isolated SCM broker/consumer architecture are validated on hardware.

---

## Next (v0.15.0)

| Priority | Milestone Target | Focus |
| :--- | :--- | :--- |
| Upstream Linux & WSL2 | LKML driver review & WSL merge (#41054) | Complete lifecycle qualification without presenting `ublk`/`io_uring` as the stock WSL2 default |
| Multi-vendor Acceleration | Vulkan Memory Allocator (VMA) multi-vendor tier | AMD Radeon & Intel Arc hardware qualification |

---

## Later (gated) — kernel-true VRAM as memory

**Question:** should process pages map VRAM as real memory (HMM / NUMA / DEVICE_PRIVATE) instead of swap-over-NBD?

**Answer (by environment):** see decision PRD  
[`docs/specs/no-milestone/kernel-vram-as-memory/PRD.md`](docs/specs/no-milestone/kernel-vram-as-memory/PRD.md)

| Environment | Verdict |
| --- | --- |
| WSL2 GPU-PV | **No** for Day-0 — cascade stays (ADR-0001, ~1.18 s reclaim) |
| Bare-metal Linux + BAR/HMM | **Research GO / implement NO-GO** until measurement gates pass |
| Next SSD step | Lab inventory + Passo 0 numbers → only then `SPEC.md` |

Cascade remains the product candidate while that track and the current
stability incident are gated.

---

## How we decide

- Structural mm/lock/driver work: **SSDV3** (PRD → SPEC → IMPL).  
- Measure before bragging.  
- Prefer refuse-to-start over hang.  
- Day-0: no permanent shims pretending to be the product.
