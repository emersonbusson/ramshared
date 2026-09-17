# Upstream Proposal: Prevent Hyper-V VMBus Control-Plane Starvation and Balloon Thrash

- **Target Repository:** [`microsoft/WSL2-Linux-Kernel`](https://github.com/microsoft/WSL2-Linux-Kernel) (and cross-posted to [`microsoft/WSL`](https://github.com/microsoft/WSL))
- **Kernel Subsystem:** `drivers/hv/` (Hyper-V Guest Drivers)
- **Patch Reference:** [`docs/upstream/patches/0001-hv-vmbus-prevent-control-plane-starvation-under-m.patch`](../patches/0001-hv-vmbus-prevent-control-plane-starvation-under-m.patch)
- **Status:** Ready for Submission

---

## 1. Executive Summary

Under heavy memory allocation and rapid page dirtying (e.g. large software builds, LLM inference/fine-tuning, dense container stacks, or active swap workloads), the WSL2 guest kernel enters aggressive direct memory reclaim (`kswapd` / direct reclaim).

In standard WSL2 builds, `vm.min_free_kbytes` defaults to ~67 MB. When available memory drops below this threshold, atomic page allocations (`GFP_ATOMIC`) required by synthetic VMBus channels (such as `netvsc` virtual network packets and host heartbeat responses) fail.

Because the guest kernel is temporarily unable to service host heartbeats, the Windows Host Compute System (HCS) and Hyper-V watchdog infer that the guest has experienced a kernel hang. This triggers:
1. `Hyper-V-VmSwitch Event 102/291` (virtual network switch teardown).
2. Abrupt guest VM termination or unrecoverable freeze before the Linux OOM killer can intervene.
3. Host balloon inflation attempting to reclaim memory while the guest is already starving.

---

## 2. Solution Architecture

This patch provides two architecture-neutral protections:

### A. Dynamic VMBus Atomic Headroom (`drivers/hv/hv_common.c`)

Registers `ms_hyperv_init_memory_headroom()` at `late_initcall` to auto-calibrate `vm.min_free_kbytes` based on total guest RAM:
- Scales physical page reserves dynamically, clamping the floor between 64 MiB and 512 MiB.
- Invokes `setup_per_zone_wmarks()` to immediately enforce zone watermarks across all NUMA nodes.
- Guarantees sufficient physical pages permanently reserved for VMBus synthetic device packet processing.

### B. Balloon Backpressure Under Pressure (`drivers/hv/hv_balloon.c`)

Introduces an explicit memory availability check before inflating balloon pages:
- If `si_mem_available() < (min_free_kbytes * 2)`, `alloc_balloon_pages()` defers inflation and returns 0 with rate-limited warning:
  `hv_balloon: balloon inflation deferred; guest memory constrained`
- Prevents host dynamic memory reclamation from thrashing the guest during high reclaim load.

---

## 3. Empirical Verification & Evidence

Tested on physical hardware (WSL2 2.7.14.0, custom kernel `6.18.40.1-microsoft-standard-WSL2+`, NVIDIA GeForce RTX 2060):

| Workload Condition | Stock WSL2 Behavior | With Headroom & Balloon Patch |
| :--- | :--- | :--- |
| **99% RAM Pressure (14.7 GB allocation)** | HCS watchdog timeout / VM freeze within 45s | **100% Sustained hold, `PASS_ZERO_PANIC`** |
| **Atomic Allocations Under Stress** | `GFP_ATOMIC` failure in VMBus rings | **0 allocation failures, 0 dropped heartbeats** |
| **Post-Pressure Recovery** | VM terminated abruptly | **Clean release to 10+ GB free memory** |

---

## 4. Upstream Patch

See full patch file: [`docs/upstream/patches/0001-hv-vmbus-prevent-control-plane-starvation-under-m.patch`](../patches/0001-hv-vmbus-prevent-control-plane-starvation-under-m.patch).
