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
- **RF-5:** UIO and sysfs ring mappings continue to expose the correct pages
  without assuming one physical extent or accepting an out-of-range offset.
- **NFR-1:** No allocation or unmap operation sleeps in IRQ/atomic context.
- **NFR-2:** No claimed performance or reliability gain without a matched
  before/after run on the same kernel, transport, hardware, and workload.

## Flows and state

Normal: allocate chunks → decrypt if required → map virtual buffer → build
ring GPADL without re-decrypting → map ring wraparound → open → close and
teardown GPADL → unmap, re-encrypt, free. On any uncertain host ownership,
mark the buffer as unsafe to free and retain backing pages.

The lifecycle object owns one virtual address, zero or more physical chunks,
one GPADL handle, and one explicit leak flag. The ring still records the
send-page offset and total page count needed by the VMBus protocol.

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
in an isolated Hyper-V/WSL2 lab, then CCA and no-paravisor TDX or equivalent
maintainer-accepted guest evidence. Do not run unsupervised pressure on the
daily host. Publish no email until those gates and manual review pass.

## Out of scope

Changing the WSL2 global memory watermark, balloon policy, RamShared swap
activation, or installing an unqualified kernel on the daily host.

## Acceptance criteria

No high-order-only ring allocation remains; all ownership/error paths are
audited; static and build gates pass; isolated live normal and failure paths
pass; CoCo evidence exists or the patch remains a draft rather than a
sendable v2.
