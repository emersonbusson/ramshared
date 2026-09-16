---
slug: wsl2-vmbus-anti-fragmentation-governor
title: WSL2 VMBus Anti-Fragmentation Governor and Dedicated Ring Pool Resilience
milestone: —
issues: ["microsoft/WSL#8768", "microsoft/WSL#40795"]
---

# PRD — WSL2 VMBus Anti-Fragmentation Governor and Dedicated Ring Pool Resilience

## 1. Summary

Under Microsoft WSL2, deep memory pressure across multiple swap tiers (ZRAM, VRAM, and SSD origin) causes direct page reclaim and compaction in the guest Linux kernel. While previous headroom protections (`vm.min_free_kbytes = 512 MB` and `hv_balloon` veto) protect the gross volume of free pages for single-page atomic allocations, aggressive dirty page generation shatters contiguous physical memory blocks into isolated 4 KiB fragments.

When the buddy allocator in `Node 0 Normal` is exhausted of contiguous order-7 ($2^7 \times 4\text{ KiB} = 512\text{ KiB}$) physical chunks, incoming Hyper-V synthetic socket requests (`hvs_probe` / AF_VSOCK via `vmbus_alloc_ring`) fail with `page allocation failure: order:7, mode:0xdc0(GFP_KERNEL|__GFP_ZERO)`. Because the upstream Hyper-V driver lacks a virtual non-contiguous allocation fallback (`vzalloc`), the failure stalls the host-guest communication channel, causing Windows Host Compute System (`wslservice.exe` / HCS) commands to hang indefinitely and freezing the Windows host virtualization stack.

This PRD establishes a two-layer defense against physical fragmentation:
1. **Userspace Anti-Fragmentation Interlock (`crates/ramshared-cli/src/stress.rs`)**: An active `/proc/buddyinfo` observer in the stress governor that immediately halts memory ramp and triggers proactive compaction if contiguous order-7 blocks drop below a safe floor ($\ge 8$ blocks), paired with raising `WSL2_MIN_PHYSICAL_HEADROOM_MB` from 600 MB to 1024 MB.
2. **Upstream Linux Kernel Dedicated Ring Pool & Virtual Fallback (`drivers/hv/ring_buffer.c` / `vmbus_drv.c`)**: Formulation of upstream patch `0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch` targeting `microsoft/WSL2-Linux-Kernel`, implementing a boot-time pre-allocated pool of order-7 VMBus rings and a graceful fallback to `vzalloc` when contiguous allocation fails under pressure.

## 2. Technical Context

- **Confirmed in codebase**:
  - `crates/ramshared-cli/src/stress.rs`: Governor checks `avail_mb <= hard_floor` (clamped to 100 MB usable available memory on WSL2), but completely lacks buddy allocator inspection.
  - `trovaldo.md`: Kernel `6.18.40.1-microsoft-standard-WSL2+ #2` carries patch `0001-hv-vmbus-prevent-control-plane-starvation-under-m.patch`, enforcing `min_free_kbytes = 524288`.
  - `docs/specs/no-milestone/wsl2-kernel-vmbus-headroom/PRD.md`: PRD explicitly identified Item 3 ("Dedicated VMBus Atomic Ring Buffer Pool") as a future requirement.
- **Confirmed in forensics / telemetry**:
  - Console log `/mnt/c/wsl-forensics/kernel-console.prev.log`: Confirms `kworker/0:2: page allocation failure: order:7, mode:0xdc0(GFP_KERNEL|__GFP_ZERO)` in `vmbus_alloc_ring` -> `hvs_probe` at `07:12:25`.
  - Buddy allocator state at failure: `Node 0 Normal: 815*4kB ... 3*256kB 0*512kB 0*1024kB 0*2048kB 0*4096kB` (zero blocks of order $\ge 7$).
  - Postmortem forensics report (boot -1): Shows `ramshared` stress held 10.7 GB RSS with 13.1 GB unmanaged pressure allocation, forcing `rust-analyzer` (823 MB) into swap.
- **Inference**:
  - The Windows host HCS hangs because the VMBus socket initialization thread remains unacknowledged when ring allocation fails, creating an unrecoverable host-guest RPC deadlock.

## 3. Recommended Option

- **Option A (Discarded — Rely Solely on Kernel Min Free Kbytes)**: Increase `vm.min_free_kbytes` to 1024 MB. Discarded: `min_free_kbytes` only reserves lower watermarks for `GFP_ATOMIC` callers; it does not stop memory fragmentation from destroying contiguous order-7 physical blocks.
- **Option B (Discarded — Force Linux OOM Killer)**: Lower OOM thresholds. Discarded: Killing user processes abruptly causes data loss and violates the zero-panic/zero-hang architectural guarantee.
- **Option C (Recommended — Closed-Loop Buddyinfo Interlock + Kernel Fallback)**:
  - In userspace: Parse `/proc/buddyinfo` during every governor step; clamp allocation if order-7 blocks $< 8$; increase WSL2 headroom to 1024 MB.
  - In kernel patch: Introduce `vzalloc` fallback in `vmbus_alloc_ring` and boot-time reserved rings in `vmbus_drv.c`.

## 4. Functional Requirements (RF-N)

- **`RF-1`**: **Buddyinfo Fragmentation Parsing**. The stress governor must parse `/proc/buddyinfo` on Linux, extracting the count of free blocks for `zone Normal` across order 7 (512 KiB) through order 10 (4096 KiB).
- **`RF-2`**: **High-Order Contiguity Interlock**. The governor must refuse further memory allocations and immediately halt the pressure ramp if `Normal` zone order-7 free chunks drop below `MIN_ORDER_7_BUDDY_CHUNKS` (minimum 4, default 8).
- **`RF-3`**: **Elevated WSL2 Headroom Floor**. On WSL2, `WSL2_MIN_PHYSICAL_HEADROOM_MB` must be raised from 600 MB to 1024 MB, ensuring sufficient physical page cushions for kernel compaction threads.
- **`RF-4`**: **Proactive Memory Compaction Trigger**. When order-7 chunks drop below 16 during multi-tier testing, the governor must issue a non-blocking compact trigger to `/proc/sys/vm/compact_memory` before continuing.
- **`RF-5`**: **Upstream Kernel Ring Buffer Fallback Patch**. Formulate patch `0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch` for `microsoft/WSL2-Linux-Kernel` adding `vzalloc` fallback to `drivers/hv/ring_buffer.c`.

## 5. Non-Functional Requirements (NFR-N)

- **`NFR-1` (Zero Hang & Zero Host Freeze)**: 100% elimination of VMBus channel allocation failures and Windows HCS hangs under maximum 3-tier cascade stress (`PASS_ZERO_PANIC`).
- **`NFR-2` (Low Observer Overhead)**: Parsing `/proc/buddyinfo` must complete in $\le 0.1\text{ ms}$ per iteration and introduce no dynamic allocations in the critical governor loop.
- **`NFR-3` (Observability)**: Interlock activation must log a clear diagnostic line: `[🛡️ VMBUS BUDDY INTERLOCK] Order-7 chunks depleted (<8). Halted at N% to prevent Hyper-V freeze.`
- **`NFR-4` (Upstream Linux Compatibility)**: The kernel patch must adhere to Linux kernel coding standards and pass `checkpatch.pl --strict` with 0 errors.

## 6. Flows

### Happy Path: Extreme Multi-Tier Stress with Buddyinfo Interlock
1. Operator invokes `ramshared stress --cascade --tier3-target-pct 99`.
2. Governor ramps memory, filling Tier 1 (ZRAM) and Tier 2 (VRAM).
3. Pressure spills into Tier 3 (SSD origin).
4. Memory compaction begins fragmenting physical blocks.
5. Governor samples `/proc/buddyinfo` and detects `order 7` dropping to 7 chunks.
6. Governor triggers `[🛡️ VMBUS BUDDY INTERLOCK]` and halts allocation at safe peak.
7. Hyper-V VMBus incoming connection finds $\ge 7$ order-7 blocks remaining and succeeds immediately.
8. Stress holds for 5 seconds and reclaims cleanly with zero host hang.

## 7. Data / State Model

```rust
pub struct BuddyinfoSnapshot {
    pub normal_order_7: u64,
    pub normal_order_8: u64,
    pub normal_order_9: u64,
    pub normal_order_10: u64,
}

impl BuddyinfoSnapshot {
    pub fn is_order_7_safe(&self, min_chunks: u64) -> bool {
        self.normal_order_7 >= min_chunks
    }
}
```

## 8. Interfaces

- **File Interface**: `/proc/buddyinfo` (read-only, line-oriented).
- **Control Interface**: `/proc/sys/vm/compact_memory` (write-only trigger).
- **CLI Telemetry**: `ramshared stress` status table includes `Order-7` block indicator.

## 9. Dependencies and Risks

- **Prerequisites**: Linux `/proc/buddyinfo` available on WSL2.
- **Risks**: Excessively high order-7 thresholds might halt stress earlier than requested. Mitigated by setting threshold to 8 chunks (4 MiB contiguous reserve), which permits 99% fill while protecting Hyper-V.
- **Rollback Trigger**: Revert changes if buddyinfo parsing causes panics on non-standard kernel zone layouts or if false-positive halts occur when order-7 is abundant.

## 10. Implementation Strategy

- **Slice 1 (Userspace)**: Buddyinfo parser + unit tests (RED -> GREEN).
- **Slice 2 (Governor Integration)**: Wire buddyinfo check into `stress.rs` loop and update headroom constant.
- **Slice 3 (Kernel Patch)**: Formulate `0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch`.
- **Slice 4 (Docs & Qualification)**: Update docs index and run docs-check.

## 11. Documents to Update

- `docs/INDEX.md`
- `docs/specs/no-milestone/wsl2-vmbus-anti-fragmentation-governor/PRD.md`
- `docs/specs/no-milestone/wsl2-vmbus-anti-fragmentation-governor/SPEC.md`
- `docs/specs/no-milestone/wsl2-vmbus-anti-fragmentation-governor/AUDIT-2.5.md`
- `docs/specs/no-milestone/wsl2-vmbus-anti-fragmentation-governor/IMPL.md`

## 12. Out of Scope

- Rewriting the Windows Host Compute System (`wslhost.exe` / `vmms.exe`).
- Disabling Hyper-V socket services on the Windows host.

## 13. Acceptance Criteria

- `AC-1`: Governor correctly parses `/proc/buddyinfo` and detects order 7 through 10 counts.
- `AC-2`: Governor refuses further memory allocation when order-7 count $< 8$.
- `AC-3`: `WSL2_MIN_PHYSICAL_HEADROOM_MB` enforced at $\ge 1024\text{ MB}$.
- `AC-4`: Upstream patch `0002-hv-vmbus-...` formatted and documented.
- `AC-5`: 100% test pass on `ramshared-cli`, slice coverage $\ge 80\%$, and docs-check exit 0.

## 14. Validation Plan

- Unit: `cargo test -p ramshared-cli -- test_buddyinfo`.
- Slice cover: `check-rust-slice-coverage.mjs -p ramshared-cli`.
- Docs: `./scripts/docs-check.sh`.
