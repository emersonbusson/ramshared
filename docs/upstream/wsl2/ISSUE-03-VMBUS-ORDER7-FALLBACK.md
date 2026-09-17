# Upstream Proposal: Add Virtual Memory Fallback for VMBus Ring Allocations Under Fragmentation

- **Target Repository:** [`microsoft/WSL2-Linux-Kernel`](https://github.com/microsoft/WSL2-Linux-Kernel) & `linux-hyperv` (LKML)
- **Kernel Subsystem:** `drivers/hv/` (Hyper-V Synthetic Transport)
- **Patch Reference:** [`docs/upstream/patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch`](../patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch)
- **Status:** Ready for Submission

---

## 1. Executive Summary

Hyper-V synthetic channel rings (`vmbus_alloc_ring()`) require contiguous physical page allocations (typically Order-7, which corresponds to 512 KiB contiguous blocks).

Under long-running WSL2 sessions with heavy memory workloads, physical memory becomes heavily fragmented. Under these conditions, `alloc_pages(GFP_KERNEL | __GFP_ZERO, 7)` fails with:

```text
page allocation failure: order:7, mode:0xdc0(GFP_KERNEL|__GFP_ZERO)
```

even when several gigabytes of physical RAM remain free across lower orders (orders 0 through 3).

When `vmbus_alloc_ring()` fails, `vmbus_open()` aborts channel initialization. The Windows Host Compute System (`wslservice.exe`) deadlocks waiting for the channel handshake or returns `Wsl/Service/E_UNEXPECTED`.

---

## 2. Solution Architecture

This patch introduces a zero-regression virtual allocation fallback mechanism:

1. **Virtual Fallback in `vmbus_alloc_ring()` (`drivers/hv/ring_buffer.c`):**
   If `alloc_pages_node()` or `alloc_pages()` fails to provide contiguous physical pages for `order > 0`, the allocator immediately falls back to `vzalloc_node()` / `vzalloc()`.
2. **Channel Flagging:**
   Flags the channel with `ringbuffer_is_vmalloc = true` and records the virtual pointer in `ringbuffer_page_virt`.
3. **GPA PFN Translation (`drivers/hv/channel.c`):**
   In `vmbus_establish_gpa_range()`, virtually mapped pages are translated to PFNs using `vmalloc_to_page()`. The Hyper-V host maps these non-contiguous physical pages into guest physical address space without any host changes.
4. **Safe Deallocation (`vmbus_free_ring()`):**
   Frees virtually allocated ring buffers with `vfree()` while releasing normal contiguous pages with `__free_pages()`.

---

## 3. Regression Analysis & Safety

- **Primary Path Preserved:** Contiguous physical allocation via `alloc_pages()` remains the first choice.
- **Fallback Trigger:** `vzalloc` executes only when `alloc_pages()` returns `NULL`.
- **Host Compatibility:** Hyper-V VMBus specification accepts arbitrary PFN ranges in the GPA descriptor table; non-contiguous pages are completely transparent to the Hyper-V host.

---

## 4. Upstream Patch

See full patch file: [`docs/upstream/patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch`](../patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch).
