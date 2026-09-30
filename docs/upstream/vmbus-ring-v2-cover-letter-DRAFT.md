# DRAFT — cover letter for `[PATCH v2 0/7]`

> **DO NOT SEND.**
>
> This is preparation only. The PRD send gate for
> [`vmbus-ring-buffer-upstream-v2`](../specs/no-milestone/vmbus-ring-buffer-upstream-v2/PRD.md)
> is closed, and [`COCO-GAP.md`](../specs/no-milestone/vmbus-ring-buffer-upstream-v2/COCO-GAP.md)
> records that Confidential Computing evidence does not exist. Until
> COCO-1..5 are satisfied on real hardware, or the operator explicitly
> reopens the gate, this document is an unsent draft and the series is not
> a sendable v2.
>
> Two deliberate differences from the 2026-09-17 v1 cover letter, which
> overstated its evidence ("flawless vsock channel open resilience") and
> was rejected on exactly that ground: this draft separates what is proven
> from what is designed, and it claims nothing about Confidential
> Computing.

---

## Mail header (as it would be sent)

```
From: Emerson Busson <author-address-at-send-time>
Subject: [PATCH v2 0/7] hv: vmbus: fragment-resilient ring allocation
 via vmbus_alloc_buffer()
```

Target: linux-hyperv list, the Hyper-V maintainers,
and the hyperv mailing-list reviewers (addresses from MAINTAINERS at send time).
Suggested-by on the series: `Michael Kelley`

---

## Body

Hello,

This is v2 of the VMBus ring-buffer allocation series. The previous
unversioned submission used a `vzalloc()` fallback. Michael Kelley's
2026-09-22 review rejected that approach for a correct reason:
`set_memory_decrypted()` operates on physically contiguous direct-mapped
memory, so a `vmalloc()` virtual range cannot be decrypted on arm64 CCA or
on Intel TDX without a paravisor. On those platforms the naive fallback is
guest-fatal.

I have rebuilt the series around Kameron Carr's `vmbus_alloc_buffer()`
(lore 2026081160447.2529876-1-kameron.carr (Microsoft)), which
already allocates decryptable direct-map chunks and joins them with
`vmap()`. The redesign follows the five points from that review.

### The problem

`vmbus_alloc_ring()` still allocates each ring with one
`alloc_pages(GFP_KERNEL | __GFP_ZERO, 7)` — a 512 KiB physically
contiguous block. Under sustained memory churn the buddy allocator
fragments and that request fails even when many gigabytes are free in
lower orders. `vmbus_open()` then aborts and the channel never
initializes.

### The five review points, and where each landed

1. *Use the existing allocator for every ring, not only after failure.*
   Patch 1 converts ring allocation to `vmbus_alloc_buffer()`
   unconditionally. The high-order attempt becomes the allocator's own
   fast path, not a separate ring-only code path.

2. *Group the buffer fields into one object.*
   `struct vmbus_buffer` now owns the virtual address, the allocation
   chunks and page arrays, the GPADL identity, and the explicit
   unsafe-to-free state.

3. *Fold `struct vmbus_gpadl` into that object.*
   Ring and netvsc buffers share one lifetime record rather than two
   half-coupled ones.

4. *Leak safely when teardown or re-encryption cannot be proven.*
   The descriptor carries a leak flag. A create or teardown with an
   uncertain outcome retains the owner and its backing pages instead of
   returning possibly host-visible or still-decrypted pages to the
   allocator. Retained owners are reclaimed only after the GPADL is
   resolved, the page state is known, and per-page references held by
   UIO mappings have returned to baseline.

5. *Eliminate the redundant `HV_GPADL_*` types once the allocator is
   universal.*
   `HV_GPADL_BUFFER_DECRYPTED` is gone; encryption is a property the
   allocator establishes, not a GPADL type.

### What this series does and does not prove

Proven on this candidate, in hosted CI on x86_64 and arm64:

- All seven patches apply in order to v7.3-rc4
  (`93f51579e7df`), `git diff --check` is clean at every stage, and
  cumulative `scripts/checkpatch.pl --strict --no-tree` reports
  `total: 0 errors, 0 warnings, 0 checks` at each of the seven stages.
- The changed `drivers/hv/`, `drivers/net/hyperv/` and `drivers/uio/`
  objects build with `W=1` and Sparse on both architectures.
- The VMBus buffer KUnit suite passes on x86_64, covering size rounding
  and overflow, order descent to zero, uncertain-release ownership,
  partial-allocation cleanup, injected GPADL post failures, the reclaim
  gate, the reclaim schedule gate, host-revoke state, UIO mapping
  references, and repeated owner release.
- The exported allocator, free, GPADL establish, caller-decrypted
  establish and teardown signatures are preserved, so in-tree and
  out-of-tree consumers are unaffected while migrated callers use the
  descriptor-aware `_owned` entry points.
- Six CoCo static invariants hold on the patched tree, and the CI step that
  enforces them injects the rejected vmalloc-decryption pattern and
  requires the gate to reject it on every run. Full inventory of all five
  `set_memory_*` sites and what each is reachable from is recorded in
  `COCO-STATIC-PROOF.md` in my working notes.

**Not proven, and not claimed:**

- Live GPADL response/rescind interleaving on a real Hyper-V host, and
  UIO subchannel mmap close/unregister interleaving, have not been
  exercised against this exact series.
- Real allocator fragmentation producing the order-zero fallback has not
  been forced on hardware. The KUnit tests inject deterministic failures
  at each order; that is not physical fragmentation.
- **No SEV-SNP, Intel TDX, or Arm CCA page-state transition has been
  observed.** What I can show is that the pattern you rejected has no code
  path in any allocation this series introduces: every encryption
  transition on a buffer this series allocates operates on
  `page_address()` of an `alloc_pages_node()` chunk, taken *before* the
  `vmap()` join, and an unknown page-state is retained rather than freed or
  re-encrypted. That is a machine-checked negative proof, not platform
  evidence. I am not asking anyone to accept a CoCo correctness claim from
  me.

Two related notes from the source audit, for completeness:

1. `vmbus_establish_gpadl()` — the pre-existing exported API for
   caller-managed buffers — still decrypts the caller's address in place,
   exactly as it does in mainline today. This series does not change that
   contract and does not route any buffer it allocates through it: all such
   buffers go through `vmbus_alloc_buffer_owned()` and
   `vmbus_establish_gpadl_owned()` / the ring path with
   `memory_prepared = true`. After this series, `netvsc.c` and
   `uio_hv_generic.c` have zero call sites of the legacy symbol. Whether
   that export should reject vmalloc callers is a pre-existing mainline
   question and, if you agree it is worth fixing, I would send it as a
   separate patch rather than fold it in here.
2. A checker that only ever passes is not a checker, so the CI step that
   enforces the invariants injects the rejected pattern on every run and
   requires the gate to reject it.

I am sending the design for review before investing in confidential-guest
lab time, because the failing mode you named is the one the redesign most
needs to be right about, and a design-level error there is cheaper to
find now than after platform runs. If the approach is sound, I will pursue
SEV-SNP / TDX / Arm CCA qualification and come back with that evidence
rather than with another source-level argument. If the approach is wrong,
I would rather hear it now.

### Series

```
 1/7 hv: vmbus: convert ring buffer allocation to vmbus_alloc_buffer()
 2/7 hv: vmbus: validate chunk buffer allocation and cleanup
 3/7 uio: hv_generic: allocate host-visible buffers through the owned allocator
 4/7 test(hv): correct VMBus fallback order vector
 5/7 test(hv): inject VMBus GPADL post failures
 6/7 test(hv): exercise order-zero allocation fallback
 7/7 hv: vmbus: retain buffer ownership until GPADL and page state are resolved
```

Comments welcome, especially on whether the retention policy in 7/7 is
sufficient for the teardown interleavings you described, and on what
evidence you would need to see before this could land.

Thanks,
Emerson

---

## Send gate checklist

Every box must be ticked in the same review that approves sending. A
partial tick is a refusal.

- [ ] COCO-1..5 satisfied on real SEV-SNP, TDX-no-paravisor, or Arm CCA
      hardware, **or** the operator explicitly accepts the PRD's
      "remain a draft" branch and does not send.
- [ ] Live GPADL response/rescind interleaving recorded on the exact
      seven-patch candidate (`vmbus_gpadl_*` named tests).
- [ ] UIO subchannel mmap close/unregister recorded, including the
      hold-in-mmap window (`uio_hv_ring_noncontiguous_mmap`).
- [ ] Forced order-7 fragmentation producing the order-zero fallback,
      with the channel still opening and zero `accept4 failed`
      (`PASS_ORDER0_FALLBACK`).
- [ ] 100 open/close cycles with balanced buffer accounting
      (`vmbus_channel_lifecycle_buffer_balance`).
- [ ] `scripts/checkpatch.pl --strict --no-tree` clean at all seven
      stages on the exact bytes to be sent.
- [ ] `series/SHA256SUMS` matches those exact bytes (`sha256sum -c`), so
      the CI evidence binds to the sendable series.
- [ ] `Enforce CoCo static invariants` CI step green on the send SHA,
      **including both gate self-tests**. If the step is red, the answer
      is to fix the regression, not to skip the step.
- [ ] The static proof document matches the candidate source. If any
      `set_memory_*` site moved, re-run the inventory before sending.
- [ ] Every patch carries `Subject:`, `From:`, `Date:` and
      `Signed-off-by:`; patches that implement a reviewer's suggestion
      carry `Suggested-by:`.
- [ ] The "Not proven" section above is still accurate at send time. If
      any item becomes proven, move it up and cite the evidence. Never
      delete the section.
- [ ] Operator has read this draft and recorded explicit approval.
- [ ] `git send-email --dry-run` reviewed.

## Rollback trigger

If this series is sent while the "Not proven" section still lists CoCo, or
while any checklist box is unticked, treat it as a process defect: retract
the send, restore draft status in
[`VMBUS-RING-V2-UPSTREAM-STATUS.md`](VMBUS-RING-V2-UPSTREAM-STATUS.md) and
[`GAP-REGISTER.md`](../reliability/GAP-REGISTER.md), and record the
retraction in [`validation.md`](../../validation.md).
