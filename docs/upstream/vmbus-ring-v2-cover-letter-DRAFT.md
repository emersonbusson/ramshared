# DRAFT — cover letter for `[PATCH v2 0/6]`

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
From: Emerson Busson (the account `git send-email` sends from)
Subject: [PATCH v2 0/6] hv: vmbus: fragment-resilient ring allocation
 via vmbus_alloc_buffer()
To: Michael Kelley,
    "K. Y. Srinivasan",
    Haiyang Zhang,
    Wei Liu,
    Dexuan Cui,
    Long Li,
    linux-hyperv
Cc: "David S. Miller",
    Andrew Lunn,
    Eric Dumazet,
    Jakub Kicinski,
    Paolo Abeni,
    Greg Kroah-Hartman,
    netdev,
    linux-kernel
```

Recipient **names and list names** only. Literal addresses are deliberately
absent from this repository (see [`docs/governance/README.md`](../governance/README.md);
the tree carries zero email addresses by policy). They are materialised at
send time from two authorities: the sender address is whatever account
`git send-email` is configured with, and every `To:`/`Cc:` address is the
output of `scripts/get_maintainer.pl --roles --no-git-fallback` run against
the exact six patch files in the contribution fork. Do not hand-copy
addresses into a document; generate them into the `git send-email` command.

The role mapping below is the verification surface for that run, taken on
2026-09-30 from mainline `93f51579e7df`, plus one manual addition.

- **Hyper-V/Azure CORE AND DRIVERS** (`drivers/hv/`, `include/linux/hyperv.h`):
  Srinivasan, Zhang, Liu, Cui, Li, `linux-hyperv`.
- **NETWORKING DRIVERS** (`drivers/net/hyperv/netvsc.c`,
  `drivers/net/hyperv/hyperv_net.h`): Miller, Lunn, Dumazet, Kicinski, Abeni,
  `netdev`.
- **USERSPACE I/O (UIO)** (`drivers/uio/uio_hv_generic.c`):
  Kroah-Hartman.
- **`linux-kernel`** is the open list `get_maintainer.pl` returns for any patch.

**Michael Kelley is not in MAINTAINERS** — `get_maintainer.pl` does not return
him. He is listed here because he requested the redesign this series implements,
and the two patches that implement his points carry `Suggested-by: Michael
Kelley` (the patch mails in the contribution fork hold the full trailer).
Re-run the tool at send time and diff the result against this list; a change in
MAINTAINERS is a reason to update the header, not a reason to drop him.

`From:` must match the account `git send-email` sends from. It is the same
identity every patch already carries in its own `From:`.

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
(Message-ID stem `2026081160447.2529876-1`, Kameron Carr, Microsoft), which
already allocates decryptable direct-map chunks and joins them with
`vmap()`. The redesign follows the five points from that review.

### Changes since v1

The 2026-09-17 submission was two patches: a control-plane starvation fix,
and a `vzalloc()` fallback for `vmbus_alloc_ring()`. This series is a
redesign of the second one, not an incremental fix on top of it. The
control-plane patch is not carried here and is tracked separately.

- The `vzalloc()` fallback is gone. Rings and the host-visible buffers
  allocate through `vmbus_alloc_buffer()` unconditionally. The high-order
  attempt becomes the allocator's own fast path rather than a separate
  ring-only code path.
- Buffer address, chunk and page arrays, GPADL identity and the
  unsafe-to-free state are grouped in `struct vmbus_buffer`, and
  `struct vmbus_gpadl` is folded into it.
- An uncertain create or teardown retains the owner and its backing pages
  rather than returning possibly host-visible or still-decrypted pages to
  the allocator. Reclamation waits for GPADL resolution, known page state,
  and UIO mapping references returning to their baseline.
- `HV_GPADL_BUFFER_DECRYPTED` is removed. Encryption is a property the
  allocator establishes, not a GPADL type.
- Exported allocator, free, GPADL establish, caller-decrypted establish and
  teardown signatures are preserved through adapters. In-tree callers use
  the descriptor-aware `_owned` entry points.
- Every patch is generated with `git format-patch` and carries
  `Signed-off-by:`. The two patches that implement your points carry
  `Suggested-by: Michael Kelley`.
- The KUnit order-descent fix that would otherwise have sat two commits
  after the test it corrects is folded into the patch that introduces the
  test, so the series never adds a broken case and then repairs it.

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

- All six patches apply in order to v7.3-rc4
  (`93f51579e7df`), `git diff --check` is clean at every stage, and
  cumulative `scripts/checkpatch.pl --strict --no-tree` reports
  `total: 0 errors, 0 warnings, 0 checks` at each of the six stages.
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
  requires the gate to reject it on every run. The tree has exactly five
  `set_memory_*` sites: three on `page_address()` of `alloc_pages_node()`
  chunks (allocator decrypt, reclaim re-encrypt, free re-encrypt), and two
  on a virtual address, both inside the pre-existing
  `vmbus_establish_gpadl()` export that no allocation this series
  introduces can reach.

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
 1/6 hv: vmbus: convert ring buffer allocation to vmbus_alloc_buffer()
 2/6 hv: vmbus: validate chunk buffer allocation and cleanup
 3/6 uio: hv_generic: allocate host-visible buffers through VMBus
 4/6 hv: vmbus: add KUnit tests for GPADL post failure injection
 5/6 hv: vmbus: add KUnit test for order-zero allocation fallback
 6/6 hv: vmbus: retain buffer ownership until GPADL and page state are resolved
```

Comments welcome, especially on whether the retention policy in 6/6 is
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
      six-patch candidate (`vmbus_gpadl_*` named tests).
- [ ] UIO subchannel mmap close/unregister recorded, including the
      hold-in-mmap window (`uio_hv_ring_noncontiguous_mmap`).
- [ ] Forced order-7 fragmentation producing the order-zero fallback,
      with the channel still opening and zero `accept4 failed`
      (`PASS_ORDER0_FALLBACK`).
- [ ] 100 open/close cycles with balanced buffer accounting
      (`vmbus_channel_lifecycle_buffer_balance`).
- [ ] `scripts/checkpatch.pl --strict --no-tree` clean at all six
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
- [ ] Every patch is `git format-patch` output: `From <sha>` marker,
      `---` separator, diffstat, and no text after the trailers except the
      signature. Checkpatch reports "ready for submission" on each mail
      file itself, not only on the cumulative source diff.
- [ ] No patch in the series corrects a line a previous patch in the same
      series introduced. Fixups are folded before the series is numbered.
- [ ] `scripts/get_maintainer.pl --roles --no-git-fallback` re-run on the
      six patches matches the To:/Cc: above, and Michael Kelley is still
      added manually.
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
