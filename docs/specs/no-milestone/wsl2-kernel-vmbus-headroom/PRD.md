---
slug: wsl2-kernel-vmbus-headroom
title: Native Linux Kernel VMBus Atomic Headroom and Hyper-V Balloon Protection
milestone: —
issues: ["microsoft/WSL#8768", "microsoft/WSL#4166", "microsoft/WSL#7254", "microsoft/WSL#10495", "microsoft/WSL#40795"]
---

# PRD — Native Linux Kernel VMBus Atomic Headroom and Hyper-V Balloon Protection

## 1. Summary

In Microsoft WSL2, aggressive memory workloads (such as intense multi-tier memory tiering, heavy compiler parallelism, or LLM inference) drive guest memory into deep direct reclaim (`kswapd`). When available memory drops below ~400–500 MB under sustained page dirtying, atomic page allocations (`GFP_ATOMIC`) for the Hyper-V synthetic communication bus (`hv_vmbus`, `netvsc`, and `hv_balloon`) fail.

Because the guest kernel cannot allocate atomic buffers to service host heartbeat requests, the Windows Host Compute System (HCS) and Hyper-V watchdog infer that the guest has hard-locked. Consequently, Hyper-V resets the virtual network switch (`Hyper-V-VmSwitch Event 102/291`) and terminates or restarts the VM before the in-guest Linux OOM killer can execute.

This PRD specifies an upstream-compatible Linux kernel patchset targeting `microsoft/WSL2-Linux-Kernel`:
1. **Late-Init Memory Headroom Auto-Calibration (`arch/x86/kernel/cpu/mshyperv.c`)**: Automatically sets `vm.min_free_kbytes` to `max(512 MB, totalram_pages / 32)` during Hyper-V guest boot, reserving a dedicated physical page pool for atomic drivers.
2. **Cooperative Hyper-V Balloon Veto (`drivers/hv/hv_balloon.c`)**: Aborts balloon inflation requests from the Windows host whenever guest available memory drops below `2 * min_free_kbytes`, preventing the host from stealing memory during active reclaim emergencies.
3. **Dedicated VMBus Atomic Ring Buffer Pool (`drivers/hv/vmbus_drv.c`)**: Pre-allocates an emergency memory pool for VMBus control-plane packet rings.

## 2. Technical Context

- **Confirmed in codebase**:
  - `trovaldo.md`: Documents kernel 6.18.40.1 qualification, `ublk` integration, and upstream submission history.
  - `docs/upstream/microsoft-wsl-40795-comment-draft.md`: Identifies PR #40519 and the missing control-plane starvation fix in stable WSL releases.
- **Confirmed in telemetry / postmortem**:
  - Windows Event Log records (`Hyper-V-VmSwitch Event ID 102/291`): Host teardown of VM network switch during deep memory exhaustion.
  - Linux Kernel Logs: Zero `BUG:`, zero `Oops:`, zero `hung_task`. The termination is purely external by the host watchdog (`guest_pressure_unresponsive`).
- **Target Subsystems in `microsoft/WSL2-Linux-Kernel`**:
  - `arch/x86/kernel/cpu/mshyperv.c` (Hyper-V CPU initialization)
  - `drivers/hv/hv_balloon.c` (Dynamic memory ballooning)
  - `drivers/hv/vmbus_drv.c` (VMBus driver and channel management)

## 3. Recommended Option

- **Option A (Discarded — Pure Userspace Daemon)**: Run userspace earlyoom or watchdog in guest. Discarded: Cannot guarantee `GFP_ATOMIC` ring buffer availability in Ring-0 when page allocator locks are congested.
- **Option B (Recommended — In-Tree Kernel Headroom & Balloon Backpressure)**: Modify `mshyperv.c` and `hv_balloon.c` in `microsoft/WSL2-Linux-Kernel`. Ensures hardware-level memory preservation for synthetic buses before any userspace task runs.

## 4. Functional Requirements (RF-N)

- **`RF-1`**: **Automated Kernel Memory Floor on Hyper-V Boot**. The kernel must detect Hyper-V hypervisor presence and automatically scale `vm.min_free_kbytes` to a safe floor (at least 512 MiB on systems with ≥4 GiB RAM), without requiring user sysctl intervention.
- **`RF-2`**: **Pressure-Aware Ballooning Invariant**. `hv_balloon` must check `si_mem_available()` before servicing host balloon inflation requests; if available memory is below `2 * min_free_kbytes`, return `-EBUSY` to the host.
- **`RF-3`**: **Upstream Patch Formulation**. The patch must pass Linux kernel `checkpatch.pl --strict` with 0 errors, 0 warnings, and adhere strictly to LKML / Microsoft kernel coding style.

## 5. Non-Functional Requirements (NFR-N)

- **`NFR-1` (Zero Hang)**: Eliminate guest unresponsiveness and HCS watchdog restarts under 100% memory saturation (`PASS_ZERO_PANIC`).
- **`NFR-2` (Performance Overhead)**: Zero CPU overhead on the fast path; initialization occurs once during `late_initcall`.
- **`NFR-3` (Clean Revert / No Host Impact)**: Fully backward-compatible; if running on standard KVM or bare-metal, the Hyper-V hooks remain dormant.

## 6. Flows

### Happy Path: Extreme Memory Saturation with Patched Kernel
1. Workload (e.g. `ramshared stress --cascade`) consumes 100% of allocatable memory.
2. Free memory approaches 512 MB.
3. Linux kernel `page_alloc.c` locks out non-atomic userspace allocations.
4. Userspace is throttled or triggers the native in-guest OOM killer.
5. Meanwhile, `hv_vmbus` atomic packets continue flowing smoothly using the reserved 512 MB headroom.
6. Hyper-V watchdog receives heartbeats without delay. The VM remains 100% responsive.

## 7. Data and State Model

```c
struct ms_hyperv_headroom_config {
    unsigned long min_headroom_kb;
    bool calibrated;
};
```

## 8. Interfaces

- Linux kernel sysctl: `/proc/sys/vm/min_free_kbytes`
- Hyper-V VMBus protocol: VMBus channel packet rings

## 9. Dependencies and Risks

- **Risk**: High `min_free_kbytes` on low-memory VMs (e.g. 1 GB RAM).
- **Mitigation**: Scale dynamically: `min(512 * 1024, totalram_pages * (PAGE_SIZE / 1024) / 16)`. On a 1 GB VM, headroom is 64 MB; on 16 GB VM, headroom is 512 MB.

## 10. Documents to Update

- `trovaldo.md`
- `docs/upstream/patches/`
- `docs/INDEX.md`

## 11. Acceptance Criteria

1. Patch applies cleanly to `microsoft/WSL2-Linux-Kernel` (Linux 6.6 / 6.18+).
2. `checkpatch.pl --strict` passes with 0 errors, 0 warnings.
3. Boot verification confirms `min_free_kbytes` automatically elevated to ≥512 MB.
