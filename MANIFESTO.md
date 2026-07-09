# RamShared Manifesto

VRAM on personal workstations and servers often remains 90% idle during non-graphical workloads, while system RAM limits are frequently exhausted. This causes excessive swapping on storage drives that are dozens of times slower than VRAM.

**Our goal is to treat all silicon with the maximum efficiency possible, breaking down the artificial barrier between RAM and VRAM at the operating system level.**

## Principles

1.  **Bare-Metal First**
    We avoid context-switching overhead. Wherever possible, solutions must operate at the lowest feasible architectural layer without compromising system stability. We prefer HMM (Heterogeneous Memory Management), NUMA nodes, and CXL over userspace emulation wrappers.

2.  **Predictability Over Hacks**
    Kernel panics caused by dangling pointers in VRAM are unacceptable. Code must be rigorously audited against hardware power state changes (D3hot/D3cold) and sudden GPU resets.

3.  **Respecting the I/O Queue**
    Communication with hardware over PCIe must respect latency constraints. Blocking I/O by polling on spinlocks is discouraged. DMA (Direct Memory Access) must be the gold standard.

4.  **Kahneman System 2 for Kernel**
    At the Ring 0 (kernel space) layer, there is no room for trial-and-error development. Every structural change must explicitly address interrupt latency, page fault handling, and memory leakage. Kernel development demands structured, methodical reflection before any code is compiled.
