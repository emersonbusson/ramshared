---
slug: vmbus-ring-buffer-upstream-v2
title: Fragmentation-resilient VMBus rings across confidential guests
milestone: —
issues: []
---

# PRD — Fragmentation-resilient VMBus rings across confidential guests

## Summary

Prepare a replacement for the September 2026 VMBus ring-buffer patch. The
submitted `vzalloc()` fallback addresses high-order allocation failure, but
cannot safely establish a GPADL on arm64 CCA or TDX without a paravisor.
No upstream mail may be sent until the replacement has passed the platform
validation below and the operator separately approves sending it.

## Technical context

- **Confirmed in codebase:** Linux v7.3-rc4 still allocates each VMBus ring
  with one high-order `alloc_pages()` call in `drivers/hv/channel.c`.
- **Confirmed in codebase:** Kameron Carr's `vmbus_alloc_buffer()` allocates
  decryptable direct-map chunks and joins them with `vmap()`; it is already
  used by `netvsc` buffers.
- **Confirmed in codebase:** `hv_ringbuffer_init()` assumes a physically
  contiguous `struct page` array. GPADL type and decryption are coupled.
- **Confirmed in codebase:** `uio_hv_generic` exposes the same ring to
  userspace as one physical range; it must change when ring pages are no
  longer physically contiguous.
- **Confirmed in maintainer review:** Michael Kelley endorses fixing the ring
  allocation failure but requests the existing allocator for all rings,
  unified buffer/GPADL lifetime metadata, and a safe leak on uncertain
  teardown or re-encryption.
- **Confirmed on September 27, 2026:** The daily WSL host runs kernel Build #6
  (`6.18.40.1-microsoft-standard-WSL2+`) and exports
  `vmbus_alloc_buffer()` / `vmbus_free_buffer()`. The installed image hash is
  recorded in EVD-0051. The checked-out Microsoft WSL source at commit
  `14794180686c2fb6307fbe359c359bec765249f3` does not contain that allocator;
  the separate backport commit `50715f5f738f2793f2713401db69988df0347ecf`
  does. The installed image has not been matched to either source revision.
  Build #6 source provenance must be resolved before attributing runtime
  findings or promoting a fix. The exact mainline v7.3-rc4 series also does
  not apply to the WSL 6.18.40.1 tree.
- **Confirmed in source, not yet attributed to Build #6:** In backport commits
  `50715f5f7` and `418653fde`, rescind can make GPADL teardown return success
  without clearing its handle. Buffer release then skips freeing the mapping
  and clears the owner descriptor. In `50715`, the allocator is called only
  for combined ring buffers and the RELID limit is 2,048. EVD-0091's 24,932
  maps are consistent with cumulative retention if Build #6 contains this
  source, but its exact image/source identity remains unresolved.

## Recommended option

Use the accepted VMBus allocation mechanism for every ring, not only after
an allocation failure. Give ring and netvsc buffers one lifecycle object
containing the virtual address, allocation chunks, GPADL identity, and
explicit unsafe-to-free state. Preserve the ring-specific GPADL layout while
separating it from the decision to decrypt memory.

Discarded: a `vzalloc()` fallback, because its virtual address cannot be
decrypted on all CoCo guests. Discarded: a new high-order reserve, because it
does not remove fragmentation dependence.

## Requirements

- **RF-1:** All ring allocations use the chunked VMBus buffer allocator;
  ring-page mapping works with noncontiguous backing pages.
- **RF-2:** GPADL setup never calls `set_memory_decrypted()` on a `vmap`
  address; CCA and no-paravisor TDX use direct-map chunk decryption.
- **RF-3:** Buffer lifetime is unified for rings and netvsc. Failed GPADL
  teardown, failed re-encryption, or uncertain host ownership never returns
  exposed pages to the allocator.
- **RF-4:** Partial allocation, GPADL setup, ring-init, close, rescind, and
  replayed cleanup paths have deterministic ownership and error behavior.
  A bounded repeated open/close drill reports balanced normal buffer
  allocation/free counts and separately accounts for buffers retained after
  injected uncertain ownership.
- **RF-5:** UIO and sysfs ring mappings continue to expose the correct pages
  without assuming one physical extent or accepting an out-of-range offset.
- **NFR-1:** No allocation or unmap operation sleeps in IRQ/atomic context.
- **NFR-2:** No claimed performance or reliability gain without a matched
  before/after run on the same kernel, transport, hardware, and workload.
- **NFR-3:** Lifecycle evidence identifies each buffer by stable device/channel
  identity and role, records allocate/free/retain outcomes, and never logs
  kernel virtual addresses. An unresolved image/source identity blocks host
  attribution and installation claims.

## Flows and state

Normal: allocate chunks → decrypt if required → map virtual buffer → build
ring GPADL without re-decrypting → map ring wraparound → open → close and
teardown GPADL → unmap, re-encrypt, free. On any uncertain host ownership,
mark the buffer as unsafe to free and retain backing pages.

The lifecycle object owns one virtual address, zero or more physical chunks,
one GPADL handle, and one explicit leak flag. The ring still records the
send-page offset and total page count needed by the VMBus protocol. For
diagnosis, each buffer lifecycle also reports its stable device GUID, channel
relid, role, and terminal outcome without exposing a kernel address.

## Interfaces and risks

The change is limited to Linux VMBus and netvsc internal APIs; no userspace
ABI changes. Rollback trigger: any reproducible ring corruption, CoCo memory
state fault, kernel warning/oops, GPADL teardown regression, or >3% matched
throughput loss. Rollback means booting the previous kernel, not hot-swapping
code under active channels.

## Implementation and validation

Develop on an upstream tag containing Kameron's accepted series. Use
checkpatch, targeted kernel build, static analysis where available, and
failure-injection tests. Qualify live channel open/close and memory pressure
in an isolated Hyper-V/WSL2 lab. Before any host test, bind the kernel image,
modules, and exact source revision in one manifest. Reconcile live vmalloc
maps with per-buffer allocate/free/retain records through at least 100 normal
channel open/close cycles, then CCA and no-paravisor TDX or equivalent
maintainer-accepted guest evidence. Do not run unsupervised pressure on the
daily host. Publish no email until those gates and manual review pass.

## Out of scope

Changing the WSL2 global memory watermark, balloon policy, or RamShared swap
activation. The requested kernel test is a single attended WSL promotion, not
production qualification or automatic boot activation. It requires a sealed
kernel/modules/QEMU manifest, successful pre-install gates, and a proved
rollback path; memory-pressure stress remains out of scope on the daily host.

## Acceptance criteria

No high-order-only ring allocation remains; all ownership/error paths are
audited; 100 normal channel open/close cycles leave no unexplained mapping
growth; the image, modules, and source revision are manifest-bound; static and
build gates pass; the WSL 6.18.40.1 backport passes its own build, QEMU and
supervised host smoke gates; isolated live normal and failure paths pass; CoCo
evidence exists or the mainline patch remains a draft rather than a sendable
v2.
