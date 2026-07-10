# RamShared Manifesto

VRAM on personal workstations and servers often remains 90% idle during non-graphical workloads, while system RAM limits are frequently exhausted. This causes excessive swapping on storage drives that are dozens of times slower than VRAM.

**Our goal is to treat all silicon with the maximum efficiency possible, breaking down the artificial barrier between RAM and VRAM at the operating system level.**

## Principles

1.  **Bare-Metal First**
    We avoid context-switching overhead. Wherever possible, solutions must operate at the lowest feasible architectural layer without compromising system stability. We prefer HMM (Heterogeneous Memory Management), NUMA nodes, and CXL over userspace emulation wrappers.

    **Bridge vs destination:** on WSL2/GPU-PV the shippable path is the **swap cascade** (zram → VRAM → disk) — not because we love userspace forever, but because the guest does not own VRAM (WDDM). The long-term kernel-true destination (process pages in device memory) is tracked as a **gated** SSDV3 track: [`docs/specs/no-milestone/kernel-vram-as-memory/PRD.md`](docs/specs/no-milestone/kernel-vram-as-memory/PRD.md). Do not pretend the bridge is the destination, or the destination is ready on WSL.

2.  **Predictability Over Hacks**
    Kernel panics caused by dangling pointers in VRAM are unacceptable. Code must be rigorously audited against hardware power state changes (D3hot/D3cold) and sudden GPU resets.

3.  **Respecting the I/O Queue**
    Communication with hardware over PCIe must respect latency constraints. Blocking I/O by polling on spinlocks is discouraged. DMA (Direct Memory Access) must be the gold standard.

4.  **Kahneman System 2 for Kernel**
    At the Ring 0 (kernel space) layer, there is no room for trial-and-error development. Every structural change must explicitly address interrupt latency, page fault handling, and memory leakage. Kernel development demands structured, methodical reflection before any code is compiled.
