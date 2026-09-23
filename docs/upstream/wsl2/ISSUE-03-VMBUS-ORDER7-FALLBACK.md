# Upstream Proposal: Add Virtual Memory Fallback for VMBus Ring Allocations Under Fragmentation

- **Target Repository:** [`microsoft/WSL#41634`](https://github.com/microsoft/WSL/issues/41634) (combined proposal) · [`microsoft/WSL#40795`](https://github.com/microsoft/WSL/issues/40795#issuecomment-5716513649) (solution comment) & Linux Hyper-V Subsystem (LKML)
- **Kernel Subsystem:** `drivers/hv/` (Hyper-V Synthetic Transport)
- **Patch Reference:** [`docs/upstream/patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch`](../patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch)
- **Status:** v1 proposal submitted; v2 patch is a local draft with partial validation

---

## 1. Problem Statement

Hyper-V synthetic channel rings (`vmbus_alloc_ring()`) require high-order contiguous physical page allocations—specifically Order-7 ($2^7 \times 4\text{ KiB} = 512\text{ KiB}$ contiguous blocks).

Under extended WSL2 developer sessions with heavy memory churn (compilers, language servers, containers, memory tiering), physical memory fragments heavily. Under these conditions, `alloc_pages(GFP_KERNEL | __GFP_ZERO, 7)` fails with:

```text
page allocation failure: order:7, mode:0xdc0(GFP_KERNEL|__GFP_ZERO)
```

even when multiple gigabytes of physical RAM remain free across lower orders (orders 0 through 3).

When `vmbus_alloc_ring()` fails, `vmbus_open()` aborts channel initialization. The Windows Host Compute System (`wslservice.exe`) deadlocks waiting for the channel handshake or fails with:
`Wsl/Service/E_UNEXPECTED (0x8000ffff)`
freezing the guest instance and requiring a full `wsl --shutdown`.

---

## 2. Pre-fix evidence status

The previously quoted order-7 kernel stack, buddy allocator snapshot, and
Windows `E_UNEXPECTED` output have no preserved run identity or raw artifact
linked to this proposal. They are treated as an unverified historical report,
not a reproduced trace. A future upstream submission needs the original log,
kernel build identity, time window, and a direct mapping from the VMBus
allocation failure to the observed host symptom.

---

## 3. Root Cause Analysis

Upstream Hyper-V guest drivers assume that physical memory contiguity can always be granted by the buddy allocator for order-7 requests. When high-order fragmentation occurs, there is no virtual allocation fallback in `vmbus_alloc_ring()`, which can cause channel initialization to fail. A direct causal link to the reported WSL host failure remains unproven here.

---

## 4. The Fix (Patch Series v2: `vmbus_alloc_buffer` Architecture)

### A. Non-Contiguous Chunked Buffer Allocation (`drivers/hv/channel.c`, `include/linux/hyperv.h`)
Instead of a naive `vzalloc()` fallback that risks virtual address decryption panics on Confidential VMs, the v2 architecture implements upstream-aligned `vmbus_alloc_buffer()` and `vmbus_free_buffer()` centered around `struct vmbus_buffer`:
- Automatically attempts high-order contiguous physical allocations first (`alloc_pages_node()`).
- Under physical fragmentation, dynamically falls back to decomposing the requested buffer into smaller contiguous physical chunks down to Order-0 individual pages.
- Maps the physical chunks into a contiguous kernel virtual address range via `vmap()` / `vm_map_pages()`.

### B. Confidential Computing (CoCo VM) Page Decryption per Chunk
On modern Confidential VMs (Azure CVM, ARM64 CCA, Intel TDX, AMD SEV-SNP without a paravisor), `set_memory_decrypted()` requires direct-mapped physical pages and crashes on non-contiguous virtual address ranges.
- The v2 fix iterates through each allocated contiguous physical chunk, decrypting each chunk individually *while physically contiguous*.
- Only after all physical chunks are safely decrypted are they joined into the virtual address space with `pgprot_decrypted(PAGE_KERNEL)`.
- Upon teardown, `vmbus_free_buffer()` safely re-encrypts chunks before releasing pages to the buddy allocator.

### C. Unified Buffer Lifecycle Management
Unifies ring buffers and generic VMBus buffers into `struct vmbus_buffer`:
- Stores contiguous and non-contiguous buffer representations, GPADL descriptors, and teardown flags uniformly.
- NetVSC, StorVSC, and UIO drivers adopt the unified buffer lifecycle with zero regression.

---

## 5. Validation status

Build #5 boot and swap observations are useful smoke evidence, but do not prove
that order-7 allocation failed and the fallback path ran. The previous stress
report (EVD-0046) is unqualified for physical VRAM residency and performance;
see EVD-0047. No measured channel latency, GPADL leak audit, or CoCo VM
decrypt/re-encrypt test is available. The v2 patch remains **PARTIAL** until
fault-injection, teardown, and confidential-VM tests pass on the exact patch.

---

## 6. Full Patch Reference

See the local [v2 patch draft](../patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch).

---

## 7. Reference implementation

A development implementation is maintained in the [emersonbusson/WSL2-Linux-Kernel](https://github.com/emersonbusson/WSL2-Linux-Kernel) repository. The commits below are implementation references, not release qualification:

- **Repository:** [`emersonbusson/WSL2-Linux-Kernel`](https://github.com/emersonbusson/WSL2-Linux-Kernel)
- **Reference Branches:** [`main`](https://github.com/emersonbusson/WSL2-Linux-Kernel/tree/main) (default) & [`linux-msft-wsl-6.18.y`](https://github.com/emersonbusson/WSL2-Linux-Kernel/tree/linux-msft-wsl-6.18.y)
- **Implementation commits:**
  - Backport `vmbus_alloc_buffer` and `struct vmbus_buffer` for CoCo safety: [`812533440`](https://github.com/emersonbusson/WSL2-Linux-Kernel/commit/812533440)
  - Documentation and Enterprise Qualification: [`0f2c68208`](https://github.com/emersonbusson/WSL2-Linux-Kernel/commit/0f2c68208)
- **Testing on Host:** Follow the deployment guide in the fork's README to point `.wslconfig` directly to the compiled kernel.

