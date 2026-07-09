---
slug: wsl2-cascade-swap
title: Product Requirements Document — VRAM as a Cold Tier in WSL2 Swap Cascade (zram → VRAM → VHDX)
milestone: M01
issues: []
---

# Product Requirements Document (PRD) — VRAM as a Cold Tier in WSL2 Swap Cascade

## 1. Summary

This document describes the product requirements and architecture to integrate GPU VRAM (Video RAM) into the memory pool of a WSL2 (Windows Subsystem for Linux 2) environment. 

Initially, the project explored native integration of VRAM as a NUMA (Non-Uniform Memory Access) node using kernel-level mechanisms (HMM/CXL). However, empirical findings from Phase 0 demonstrated that while VRAM access over WSL2 (via GPU-PV and CUDA) is **data-safe**, it is **latency-unsafe** under host GPU contention (which can cause read latency spikes up to ~1.18 seconds during WDDM page eviction). 

To solve this latency constraint, the architecture has pivoted. Instead of exposing VRAM as a primary raw swap or a native NUMA node directly, VRAM is integrated as a **cold swap tier** within a prioritized cascade (`zram` → `VRAM` → `VHDX`). In this setup, `zram` (RAM-compressed swap) absorbs the hot working set, VRAM acts as a cold overflow tier, and the virtual disk (`VHDX`) acts as the last-resort safety net.

## 2. Background and Architectural Context

In GPU-virtualized environments like WSL2 (which uses Microsoft's GPU-PV), VRAM is managed by the Windows Display Driver Model (WDDM) on the host. Conventional Linux memory subsystems cannot directly leverage virtualized VRAM for general-purpose system memory.

### 2.1 The NUMA Evolution & The WSL2 Pivot
*   **Original NUMA Model (Bare-Metal Prospect):** Creating a kernel module (`ramshared.ko`) to hotplug ReBAR-exposed VRAM physical blocks into the NUMA subsystem as Node 1 via `add_memory()`. While ideal for bare-metal CXL/HMM coherent systems, this is highly complex, requires custom kernel configuration, and is prone to latency-induced freezes in virtualized host environments.
*   **WSL2 Cascade Swap Model (Pragmatic MVP):** Utilizing a userspace block device interface (Network Block Device - NBD in Phase A; ublk in Phase B) backed by a CUDA daemon (`ramshared-wsl2d`) running in WSL2. The kernel's swap subsystem handles tiering naturally via priority values assigned during `swapon`.

### 2.2 The Tiering Concept
```text
Memory Pressure ──►  zram   (RAM-compressed, lzo-rle)   Priority 200   HOT
                  └─►  VRAM   (nbd-vram, CUDA+NBD)         Priority 100   COLD
                  └─►  VHDX   (/dev/sdc, WSL2 swap disk)   Priority -2    SAFETY NET
```
1.  **zram (Hot Tier):** High performance, low latency, absorbs active transient allocations.
2.  **VRAM (Cold Tier):** Moderate-to-high bandwidth, high latency under host contention. Receives spillover pages from zram.
3.  **VHDX (Safety Net):** Default WSL2 virtual disk swap, used to drain VRAM pages during emergency demotions.

---

## 3. Recommended Implementation Option

**A userspace orchestration CLI (`ramshared`) and a CUDA-backed NBD daemon (`ramshared-wsl2d`) managing the swap hierarchy.**

*   **Rationale:** Avoids custom kernel builds or risky NUMA hotplug operations. Utilizes existing Linux swap mechanisms and prioritization.
*   **Alternative Considered (Raw VRAM Swap):** Exposing VRAM as a single high-priority swap device. *Rejected* because host VRAM eviction stalls (>1 second) freeze critical guest OS processes when they attempt to read from swap.
*   **Alternative Considered (Native NUMA Node):** Mapped via physical ReBAR. *Rejected* for the MVP due to WDDM virtualization limits in WSL2 and lack of hardware cache coherence on consumer GPUs.

---

## 4. Functional Requirements (RF)

### RF-1: Safe VRAM Allocation
The system must allocate a configurable chunk of GPU VRAM (1 to N GiB) via CUDA without causing host GUI instability or crashing existing CUDA applications. It must support dynamic size backoff if the requested allocation is rejected by WDDM.

### RF-2: Block Device Interface
The allocated VRAM must be exposed to the WSL2 kernel as a virtual block device. 
*   **Phase A (Day-0):** Expose via Network Block Device (NBD) protocol to support standard WSL2 kernels without custom modifications.
*   **Phase B:** Expose via `ublk` (Userspace Block Device) leveraging `io_uring` for improved latency once a custom WSL2 kernel is enabled.

### RF-3: Prioritized Cascade Setup
The CLI must automatically configure and mount:
1.  `zram` swap with priority `200`.
2.  `VRAM` virtual swap with priority `100`.
3.  Ensure `VHDX` swap remains active at a low priority (e.g., `-2`).

### RF-4: Graceful Watchdog Demotion (Canary)
A watchdog sampler must continuously probe the residency of VRAM pages and monitor the read latency. If a latency spike (e.g., p99 latency > K × baseline) or host VRAM reclamation is detected, the daemon must trigger a **graceful demotion** (calling `swapoff` on the VRAM block device) to safely migrate active swap pages to the lower-priority VHDX tier without killing guest processes.

---

## 5. Non-Functional Requirements (RNF)

### RNF-1: Performance & Latency Isolation
*   The primary hot memory working set must remain in RAM/zram to avoid latency stalls.
*   Block transfer over NBD/ublk should optimize sequential bandwidth, targeting up to PCIe limit boundaries under uncontended states.

### RNF-2: System Stability and Deadlock Prevention
*   The `ramshared-wsl2d` daemon must lock its own memory pages (`mlockall`) and set its OOM score adjustment (`oom_score_adj`) to `-1000` to prevent the Linux OOM killer from terminating the swap backend daemon.
*   A safety-net rule must enforce that the VRAM swap tier is only activated if a lower-priority tier (VHDX) with sufficient capacity exists to absorb demoted pages.

### RNF-3: Security and Data Isolation
*   VRAM blocks must be scrubbed with zeros (`memset`) upon allocation and prior to release back to the driver pool, preventing leakage of sensitive system data into other GPU processes.
*   The control sockets and block device interfaces must be restricted to `root` access only (permissions `0600`).

---

## 6. Memory Flow Scenarios

### 6.1 Happy Path: Transparent Spillover
1.  System memory usage exceeds 90%.
2.  The kernel begins paging out cold memory.
3.  Pages flow into the high-priority `zram` (Node 0 RAM, compressed).
4.  Once `zram` is fully saturated, subsequent page-out operations spill into the `VRAM` swap device (priority 100).
5.  Active user space processes proceed normally, with cold pages stored in VRAM.

### 6.2 Emergency Path: Graceful Demotion
1.  A high-demand GPU application (e.g., Blender, 3D Game) launches on the Windows host.
2.  WDDM initiates eviction of the `ramshared` CUDA allocation to system memory.
3.  The residency canary detects a latency spike (>1 second read delay) or `cuMemGetInfo` reports free memory dropping below the safety floor.
4.  The watchdog triggers a demotion:
    *   Initiates `swapoff` on `/dev/nbd0`.
    *   The kernel reads active swap pages from VRAM and redirects them to the lower-priority VHDX swap.
    *   The NBD connection is severed, and VRAM is deallocated.
5.  The system remains active; processes are not killed.

---

## 7. Risks, Dependencies, and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| **Host VRAM Contention Stalls** | High: guest OS hangs during swap read. | Mitigated by zram fronting and watchdog latency demotion. |
| **Out of Memory (OOM) on Demotion** | Critical: kernel panic or mass process kill if VHDX cannot absorb demoted pages. | Enforce "Safety Net Invariant": refuse to mount VRAM swap if VHDX swap is absent or lacks capacity. |
| **Nvidia Proprietary virtualization limits** | Medium: BAR1 limits in GPU-PV. | Use CUDA driver API allocation (`cuMemAlloc`) in userspace instead of direct BAR1 MMIO mapping. |
| **Daemon Crash (SIGKILL)** | Critical: block layer locks up on pending I/O. | Implement `ramshared recover` routine to force disconnect and release the block device gracefully. |

---

## 8. Kahneman Governance Gates

*   **Rule of Survival:** The block layer must never enter an unrecoverable state. Any failure in the CUDA backend must fail-closed, reverting the swap configuration back to the default VHDX.
*   **Security Isolation:** Zeroing allocations prevents cross-process information leaks. This must be validated using cryptographic hash integrity checks under multi-tenant stressors.
