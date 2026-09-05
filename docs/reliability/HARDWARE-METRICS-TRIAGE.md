# Hardware Metrics Regression Alarm & Root-Cause Triage Guide

## Overview
This document defines the official root-cause triage protocol for RamShared when an automated hardware benchmark or CI check triggers a regression alarm (🔴 **ALARM**).

All performance and memory tier measurements are evaluated across 4 physical domains with explicit optimization directions and strict tolerance thresholds. Any degradation exceeding tolerance triggers an alarm, blocks PR merging, and mandates execution of this triage playbook.

---

## Metric Domains & Alarm Thresholds

| Domain / Metric | Direction | Warning Threshold (🟡 NEUTRAL) | Alarm Threshold (🔴 ALARM) | Primary Subsystem |
| :--- | :---: | :---: | :---: | :--- |
| **Reclaim Bus Bandwidth** | 🔺 Higher is better | Degradation $\le 3\%$ | Degradation $> 3\%$ | PCIe DMA / GPU driver / ublk |
| **Reclaim Latency** | 🔻 Lower is better | Increase $\le 5\%$ (same swap vol) | Increase $> 5\%$ (same swap vol) | Kernel page-in / locks |
| **Tier 3 SSD Spillover** | 🔻 Lower is better | 0 MB (within T1+T2 budget) | $> 0\text{ MB}$ (unintended spill) | Autotier / Demotion logic |
| **Memory Pressure (PSI)** | 🔺 Higher is better | Tolerates $\ge 1.0$ index | PSI full stalls $> 0.0\%$ for $>1\text{s}$ | Scheduler / swap thrashing |
| **Post-Test RAM Restored** | 🔺 Higher = No leaks | Discrepancy $\le 100\text{ MB}$ | Discrepancy $> 100\text{ MB}$ | Memory management / Bio leak |
| **Stability Verdict** | Mandatory | N/A | Status $\neq$ `PASS_ZERO_PANIC` | Kernel Ring 0 / OOM killer |

---

## Root-Cause Triage Playbook

### 1. Reclaim Bandwidth Drop (>3% loss) or Latency Spike (>5%)
**Probable Cause:** PCIe link width degradation, GPU thermal throttling, or `ublk` worker thread starvation.

**Triage Steps:**
1. **Check PCIe link generation and width:**
   ```bash
   lspci -vv -s $(lspci | grep -i nvidia | awk '{print $1}') | grep -E "LnkCap|LnkSta"
   ```
   *Expected:* Link should negotiate to maximum supported (e.g., `Speed 8GT/s, Width x16`). If downgraded to x1 or x4, re-seat hardware or reset PCIe link.
2. **Check GPU P-State and temperature:**
   ```bash
   nvidia-smi --query-gpu=pstate,temperature.gpu,clocks.current.graphics,pcie.link.gen.current --format=csv
   ```
   *Expected:* Temperature $< 80^\circ\text{C}$, P-state P0 or P2 during active DMA. If throttling, inspect thermal limits.
3. **Inspect ublk worker thread CPU pinning:**
   ```bash
   ps -eo pid,psr,comm | grep -E "ublk|ramshared"
   ```
   *Expected:* ublk worker threads must not compete for the same logical core as the stress generator.

---

### 2. Unintended Tier 3 SSD Spillover (>0 MB)
**Probable Cause:** Premature GPU eviction, erroneous `GlobalGpuFreeFloor` headroom calculation, or ZRAM compaction failure.

**Triage Steps:**
1. **Check current ZRAM saturation and compaction efficiency:**
   ```bash
   zramctl
   ```
   *Expected:* Compression ratio $\ge 2.0:1$ on LZ4. If compressed size approaches disk size, check data compressibility.
2. **Verify GPU memory headroom and floor:**
   ```bash
   journalctl -u ramsharedd -n 50 | grep "VRAM"
   ```
   *Expected:* Tier 2 allocations must leave at least 512 MiB dynamic safety buffer for host display drivers.

---

### 3. Memory Leak / Unreleased RAM Discrepancy (>100 MB)
**Probable Cause:** Leaked kernel bio structures, unpinned DMA scatter-gather lists, or userspace heap fragmentation.

**Triage Steps:**
1. **Check kernel slab and vmalloc consumption before and after run:**
   ```bash
   cat /proc/meminfo | grep -E "MemFree|MemAvailable|Slab|VmallocUsed"
   ```
2. **Audit unreleased block device buffers in kernel:**
   ```bash
   dmesg -T | grep -E "ramshared|ublk|leak" | tail -n 20
   ```
3. **Verify userspace process termination:**
   ```bash
   ps aux | grep -E "ramshared stress|workload" | grep -v grep
   ```

---

### 4. Stability Failure (Kernel Panic, OOM, or Stalls)
**Probable Cause:** Unhandled `-ERANGE` error code, NULL pointer dereference, or cgroup memory limit exhaustion.

**Triage Steps:**
1. **Check dmesg for kernel OOPs or warnings:**
   ```bash
   dmesg -T | grep -iE "oops|panic|bug:|call trace|out of memory" | tail -n 30
   ```
2. **Verify OOM-killer activity:**
   ```bash
   grep -i "killed process" /var/log/syslog 2>/dev/null || dmesg -T | grep -i "oom-killer"
   ```
3. **Verify swapoff-first lifecycle:**
   Ensure swap was unmounted before daemon termination:
   ```bash
   cat /proc/swaps
   ```
