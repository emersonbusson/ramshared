---
slug: wsl2-vmbus-resilience
title: WSL2 Hyper-V VMBus Memory Headroom and Anti-Starvation Governor
milestone: —
issues: ["microsoft/WSL#8768", "microsoft/WSL#4166", "microsoft/WSL#7254", "microsoft/WSL#10495"]
---

# PRD — WSL2 Hyper-V VMBus Memory Headroom and Anti-Starvation Governor

## 1. Summary

Under Microsoft WSL2 (a lightweight Hyper-V utility virtual machine), aggressive memory consumption and dirty page allocation (e.g. multi-tier swap thrash, massive builds, or AI model loading) can exhaust the memory required by synthetic Hyper-V kernel drivers (`hv_vmbus`, `vmicvmswitch`, `hv_balloon`, and `virtio-9p`). 

When guest available memory drops below ~400–500 MB under rapid dirty page generation, atomic page allocations (`GFP_ATOMIC`) for VMBus ring buffers fail, dropping host-guest heartbeat pings. The Windows host Hyper-V watchdog infers that the guest kernel has hard-locked, abruptly tearing down the virtual network switch (`Hyper-V-VmSwitch Event 102/291`) and rebooting the VM instance.

This PRD specifies a robust, multi-layer defense:
1. **RamShared Runtime Governor**: An autonomous, WSL2-aware dynamic safety floor (strictly ≥600 MB available RAM) in `crates/ramshared-cli/src/stress.rs` and the memory arbiter, preventing guest userspace pressure from ever entering the Hyper-V VMBus starvation zone.
2. **Kernel Sysctl Hardening**: Automated validation and enforcement of `vm.min_free_kbytes` (≥512 MB on WSL2) to reserve physical pages exclusively for kernel atomic drivers.
3. **Upstream Microsoft WSL2 Native Contribution**: An architectural specification and patch proposal for `microsoft/WSL2-Linux-Kernel` and `.wslconfig` defaults, introducing auto-sized VMBus page reserves and graceful PSI-triggered host backpressure instead of ungraceful VM termination.

## 2. Technical Context

- **Confirmed in codebase**:
  - `crates/ramshared-cli/src/stress.rs`: Stress battery previously lowered `MULTI_TIER_HARD_FLOOR_MB` to 200 MB during cascade testing, breaching the WSL2 synthetic bus threshold and triggering Hyper-V VM teardown.
  - `crates/ramshared-cli/src/cascade/mod.rs`: Contains `is_wsl2()` detection using kernel release strings and WSLInterop.
  - `/usr/local/bin/ramshared-vram-service.sh`: Isolates `ramsharedd` inside `/sys/fs/cgroup/ramshared-protected` with `memory.min = 512M` and `oom_score_adj = -1000`.
- **Confirmed in docs / telemetry**:
  - Windows Event Log records (`Event ID 102/291` on `Hyper-V-VmSwitch` in the incident window): VM switch teardown and reconstruction due to lost VMBus heartbeat.
  - Postmortem forensics report (boot -1): Confirms zero Linux kernel BUG/Oops, zero GPU TDR (Event 4101/4102), classifying the termination as `guest_pressure_unresponsive`.
- **Known Upstream Issues in `microsoft/WSL`**:
  - `microsoft/WSL#8768`: "WSL2 freezes completely instead of invoking OOM-killer under high memory allocation".
  - `microsoft/WSL#4166`: "WSL2 memory consumption issues and subsequent VM crash".
  - `microsoft/WSL#7254`: "vEthernet (WSL) Hyper-V-VmSwitch resets and disconnects under guest pressure".
  - `microsoft/WSL#10495`: "WSL2 terminates abruptly with code 4294967295 under direct reclaim thrash".
  - `microsoft/WSL2-Linux-Kernel#246`: "VMBus channel allocation failure under low memory conditions".

## 3. Recommended Option

- **Option A (Discarded — User-space OOM Killer / Earlyoom)**: Run third-party `earlyoom` inside WSL2. Discarded: external daemon dependency, unpredictable process kill order, can kill critical background daemons (`dockerd`, `ramshared`) before stress jobs.
- **Option B (Recommended — Closed-Loop Floor & Upstream VMBus Protection)**:
  - Inside RamShared: Embed `is_wsl2()` awareness into the governor, locking the floor to `max(opts.min_ram_mb, 600 MB, dynamic_kernel_floor)`.
  - Inside Host Environment: Configure `vm.min_free_kbytes = 524288` (512 MB) and `cgroups v2` protections on `system.slice`.
  - Upstream RFC: Propose a patch for `drivers/hv/vmbus_drv.c` in `microsoft/WSL2-Linux-Kernel` to reserve a dedicated pool for VMBus ring buffers and report high PSI pressure over Hyper-V telemetry before watchdog reset.

## 4. Functional Requirements (RF-N)

- **`RF-1`**: **WSL2 Runtime Environment Detection**. The stress governor and arbiter must reliably detect Microsoft WSL2 execution via kernel release inspection (`/proc/sys/kernel/osrelease`), `/proc/sys/fs/binfmt_misc/WSLInterop`, and environment markers.
- **`RF-2`**: **Dynamic WSL2 Safety Floor Enforcement**. When running in WSL2, the governor must enforce a strict floor of at least 600 MB available RAM, clamping all stepped allocations (`safe_alloc_mb`) relative to this boundary regardless of `--cascade` or aggressive CLI flags.
- **`RF-3`**: **Kernel Min Free Reservation Verification**. Preflight safety checks must query `/proc/sys/vm/min_free_kbytes` and ensure at least 256 MB (ideally 512 MB) is reserved for kernel atomic operations.
- **`RF-4`**: **Upstream Microsoft WSL Contribution Spec**. Formulate a complete technical RFC and patch proposal for `microsoft/WSL` and `microsoft/WSL2-Linux-Kernel` defining:
  - Kernel-level VMBus atomic reserve buffers.
  - Native `.wslconfig` parameter `guestHeadroomMb` and default `autoMemoryReclaim=gradual`.

## 5. Non-Functional Requirements (NFR-N)

- **`NFR-1` (Safety)**: 100% prevention of Hyper-V watchdog VM terminations (`PASS_ZERO_PANIC`). Zero guest unresponsiveness events under any valid CLI stress configuration.
- **`NFR-2` (Performance)**: Floor calculation and memory probe overhead must not exceed 0.05 ms per governor sampling interval.
- **`NFR-3` (Observability)**: When the WSL2 safety floor is approached, the CLI and telemetry log must explicitly output `🛑 WSL2 SAFETY FLOOR REACHED (≥600 MB)`.

## 6. Flows

### Happy Path: Cascade Stress Run on WSL2
1. User executes `ramshared stress --cascade --target 100`.
2. Governor detects `is_wsl2() == true`.
3. Governor computes `hard_floor = max(opts.min_ram_mb, 600, dynamic_kernel_floor)`.
4. Ramps up allocation into Tier 1 (ZRAM) and Tier 2 (VRAM).
5. As available memory reaches 620 MB, `safe_alloc_mb` throttles to zero.
6. Governor halts safely, holds peak without dropping Hyper-V heartbeats, benchmarks reclaim speed, and exits with code 0 (`PASS_ZERO_PANIC`).

### Alternate Path: Bare-Metal Linux
1. Governor detects `is_wsl2() == false`.
2. Sets `hard_floor` to bare-metal multi-tier floor (256 MB).
3. Utilizes higher memory density while relying on native Linux kernel OOM killer rather than Hyper-V watchdog.

## 7. Data and State Model

```rust
pub struct Wsl2SafetyGovernor {
    pub is_wsl: bool,
    pub min_safe_floor_mb: u64, // 600 MB on WSL2, 256 MB on bare-metal
    pub kernel_min_free_mb: u64,
}
```

## 8. Interfaces

- CLI: `ramshared stress [--min-ram-mb <N>] [--cascade]`
- Sysfs / proc: `/proc/sys/vm/min_free_kbytes`, `/proc/pressure/memory`, `/proc/sys/kernel/osrelease`.

## 9. Dependencies and Risks

- **Risk**: User explicitly requests `--min-ram-mb 100` on WSL2.
  - **Mitigation**: Governor clamps to minimum 600 MB on WSL2 regardless of CLI input, emitting an informational warning.
- **Rollback Trigger**: Any reproducible Hyper-V VM teardown or VMBus packet drop when running under the safety floor.

## 10. Documents to Update

- `ARCHITECTURE.md` (WSL2 Hyper-V Coexistence section)
- `docs/reliability/GAP-REGISTER.md`
- `docs/specs/no-milestone/wsl2-vmbus-resilience/`

## 11. Acceptance Criteria

1. Stress test with `--cascade --target 100` runs to completion on live WSL2 host without VM restart.
2. `postmortem.sh` after heavy runs verifies zero `Hyper-V-VmSwitch` reset events.
3. 100% pass on all unit tests asserting WSL2 floor invariants.
