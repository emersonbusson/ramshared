# COCO-STATIC-PROOF — why the rejected pattern cannot occur

**Status:** ENFORCEABLE INFERENCE — machine-checked, not platform evidence
**Recorded:** 2026-09-30
**Authority:** [`COCO-GAP.md`](COCO-GAP.md) · [`PRD.md`](PRD.md) RF-2
**Related:** [`IMPL.md`](IMPL.md) · [`AUDIT-source.md`](AUDIT-source.md)
**Enforced by:** the `Enforce CoCo static invariants` step in the
contribution fork's `.github/workflows/vmbus-upstream.yml`, which runs
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/coco-static-invariants.py`
against the tree with the full series applied to mainline `93f51579e7df`
(Linux 7.3-rc4), then **injects the rejected pattern and requires the gate to
reject it**.

---

## What this is, and what it is not

This is a **negative proof about the source**: the guest-fatal operation
Michael Kelley named has no code path that any allocation in this series can
reach. It is stronger than a design argument and weaker than hardware
evidence.

It does **not** close COCO-1..5. Those rows require observing a real
memory-encryption transition. [`COCO-GAP.md`](COCO-GAP.md) stays open and
the send gate stays closed.

What this does do is replace "we believe the redesign handles CoCo" with
"the failing pattern is structurally unreachable from this series, and here
is the checker". That is the claim the review can verify with two commands.

---

## The rejected pattern

Kelley's 2026-09-22 review: `set_memory_decrypted()` operates on physically
contiguous direct-mapped memory. It is not valid for a `vmalloc()` virtual
range. On arm64 CCA and Intel TDX without a paravisor, a virtual-address
decryption attempt is a guest-fatal operation.

So the forbidden shape is:

```c
addr = vzalloc(size);                    /* or vmap(...) */
set_memory_decrypted((unsigned long)addr, n);   /* guest-fatal */
```

---

## The full inventory of encryption transitions

There are exactly **five** `set_memory_*` call sites in the candidate, all in
`drivers/hv/channel.c`. `drivers/uio/uio_hv_generic.c` and
`include/linux/hyperv.h` contain **none**.

| Line | Call | First argument | Reachable from this series' allocations? |
| --- | --- | --- | --- |
| 764 | `set_memory_encrypted` | `page_address(page)` | yes — chunked reclaim |
| 770 | `set_memory_encrypted` | `owner->addr` | **no** — legacy export only |
| 1358 | `set_memory_decrypted` | `kbuffer` | **no** — legacy export only |
| 1648 | `set_memory_decrypted` | `page_address(page)` | yes — chunked allocation |
| 1725 | `set_memory_encrypted` | `page_address(page)` | yes — `vmbus_free_buffer()` |

Lines 764, 1648 and 1725 all take `page_address()` of an `alloc_pages_node()`
result — a direct-map address. That is the correct operand.

Lines 770 and 1358 take a virtual address. They are the subject of the next
section, and they are **not** an oversight in this series.

---

## The legacy export, stated plainly

`vmbus_establish_gpadl()` is the **pre-existing mainline API** for caller-managed
buffers. It has always decrypted the caller's buffer in place, and this series
preserves that behaviour so the exported symbol keeps its contract:

```c
int vmbus_establish_gpadl(struct vmbus_channel *channel, void *kbuffer,
                          u32 size, struct vmbus_gpadl *gpadl)
{
        ...
        ret = __vmbus_establish_gpadl(channel, HV_GPADL_BUFFER, &buffer,
                                      0U, false);   /* memory_prepared = false */
```

Inside `__vmbus_establish_gpadl()`, decryption is gated on `gpadl->decrypted`,
which is computed as:

```c
gpadl->decrypted = !memory_prepared &&
        !((channel->co_external_memory && type == HV_GPADL_BUFFER) ||
          (channel->co_ring_buffer && type == HV_GPADL_RING));
```

So `set_memory_decrypted((unsigned long)kbuffer, ...)` runs **only** when
`memory_prepared == false`, which is **only** from `vmbus_establish_gpadl()`.
Line 770 is its symmetric undo in the reclaim worker, reached only when
`owner->raw_decrypted` is set, which is only set from `gpadl.decrypted`.

**What this series does about it:** every buffer this series allocates goes
through `vmbus_alloc_buffer_owned()`, which decrypts each chunk on its
direct-map address *before* joining them with `vmap()`, and then calls

| Caller | `memory_prepared` | Decrypts `kbuffer`? |
| --- | --- | --- |
| `vmbus_establish_gpadl_owned()` (netvsc, UIO) | `true` | no |
| `vmbus_establish_gpadl_caller_decrypted()` | `true` | no |
| ring open, `HV_GPADL_RING` | `true` | no |
| `vmbus_establish_gpadl()` (legacy export) | `false` | **yes — pre-existing** |

**In-tree callers of the legacy export after this series: zero.** `netvsc.c`
and `uio_hv_generic.c` both call `vmbus_establish_gpadl_owned()`. The legacy
symbol remains exported for out-of-tree consumers, as before.

This is a pre-existing mainline property, not a hazard this series introduces.
Whether `vmbus_establish_gpadl()` should reject vmalloc callers is a real
question about mainline and is **out of scope for this series**; it is worth
raising in review as a separate observation.

---

## Invariant 1 — the allocator decrypts only on direct-map chunk addresses

In `vmbus_alloc_buffer_owned()`:

```c
page = vmbus_alloc_pages_with_fallback(nid, gfp, &order,
                                       vmbus_alloc_pages_node, NULL);
...
ret = set_memory_decrypted((unsigned long)page_address(page), 1U << order);
```

The join happens afterwards, and the encryption API is not entered again:

```c
buffer->addr = vmap(buffer->pages, nr_pages, VM_MAP,
                    pgprot_decrypted(PAGE_KERNEL));
```

`pgprot_decrypted()` is a page-protect flag applied at map time, not a
`set_memory_*()` transition on the virtual range.

**Enforcement (INV-1):** the CI checker rejects any `set_memory_decrypted()` /
`set_memory_encrypted()` whose first argument, after stripping a C cast, is
not a `page_address(...)` expression — with exactly one allowed pair, the
legacy `kbuffer` / `owner->addr` sites, and it requires those two to remain
exactly one each.

## Invariant 2 — a prepared GPADL never re-decrypts

`vmbus_establish_gpadl_owned()`, `vmbus_establish_gpadl_caller_decrypted()`
and the ring-open `HV_GPADL_RING` call all pass `memory_prepared = true`, so
`gpadl->decrypted` is false and `__vmbus_establish_gpadl()` cannot reach
`set_memory_decrypted()`. Both `_owned` / `_caller_decrypted` wrappers
themselves contain zero `set_memory_*` calls.

The source comment states the constraint explicitly and must stay true:

> This function is the only place that changes encryption state;
> `vmbus_establish_gpadl()` must not decrypt again (it cannot:
> `set_memory_*()` does not work on vmalloc addresses in arm64 CCA /
> TDX-without-paravisor).

**Enforcement (INV-2):** the CI checker walks every `__vmbus_establish_gpadl()`
call site and requires its last argument to be the literal `true`, except
inside the legacy export; and it requires the ring call to pass `true`.

## Invariant 3 — unknown page state is retained, never freed or re-encrypted

When `set_memory_decrypted()` fails in the allocator, the page is **not**
freed and **not** re-encrypted:

```c
ret = set_memory_decrypted((unsigned long)page_address(page), 1U << order);
if (ret) {
        /*
         * set_memory_decrypted() failed; the page state is
         * unknown so it must be leaked rather than freed.
         */
        owner->encryption_unknown = true;
        goto err;
}
owner->needs_encrypt = true;
```

`goto err` calls `vmbus_release_buffer()`, which retains the owner. The
reclaim gate then refuses that owner **permanently**:

```c
static bool
vmbus_buffer_owner_can_reclaim(const struct vmbus_buffer_retained *owner)
{
        return owner->released && !owner->permanent_leak &&
               !owner->encryption_unknown &&
               (!owner->host_may_own || owner->host_revoked);
}
```

So an owner with unknown encryption state is never reclaimed: its pages are
never passed to `set_memory_encrypted()` and never reach `__free_pages`. It
is a **counted, bounded leak** — the right failure mode — rather than a page
returned to the allocator with unknown host visibility.

The same rule holds on the free path: a chunk whose re-encryption fails is
skipped, not freed:

```c
if (set_memory_encrypted((unsigned long)page_address(page), 1U << order))
        continue;          /* leak this chunk rather than free a live page */
__free_pages(page, order);
```

And the reclaim worker marks `permanent_leak = true` rather than freeing
when any re-encryption fails.

**Enforcement (INV-3):** the CI checker requires `encryption_unknown = true`
on the `set_memory_decrypted()` failure path in `vmbus_alloc_buffer_owned()`,
requires `vmbus_buffer_owner_can_reclaim()` to reject `encryption_unknown`
and `permanent_leak`, and requires `vmbus_free_buffer()` to `continue` past
`__free_pages` when re-encryption fails.

## Invariant 4 — guest-private and non-isolated buffers never touch encryption

```c
static bool
vmbus_needs_shared_pages(bool hv_isolation, bool guest_mem_encrypted,
                         bool confidential)
{
        return !confidential && (hv_isolation || guest_mem_encrypted);
}
```

When that returns false, `vmbus_alloc_buffer_owned()` takes a plain
`vzalloc()` path and returns before any `set_memory_*` call. Non-isolated
x86 VMs and guest-private buffers are out of the CoCo hazard class by
construction. The hazard class is exactly: isolated VM, host-visible buffer,
chunked allocation — which invariants 1–3 cover.

**Enforcement (INV-4):** the CI checker requires the `vzalloc()` branch to
precede any `set_memory_*`, and requires `vmbus_needs_shared_pages()` to
honour `confidential` and never touch encryption state.

## Invariant 5 — UIO exposes mapped pages, it does not re-decrypt them

`uio_hv_generic` maps the page arrays the allocator produced. It contains
**zero** `set_memory_*` calls. The ring, receive, send and monitor regions
are mapped from explicit page arrays so a non-contiguous backing cannot be
exposed as one physical extent.

**Enforcement (INV-5):** the CI checker fails on any `set_memory_*` in
`drivers/uio/uio_hv_generic.c`.

## Invariant 6 — consumers never take the legacy decrypt path

`netvsc.c` and `uio_hv_generic.c` must call `vmbus_establish_gpadl_owned()`
and never the bare `vmbus_establish_gpadl()`. That keeps every buffer this
series manages on the prepared path.

**Enforcement (INV-6):** the CI checker fails on a bare
`vmbus_establish_gpadl(` call in either consumer.

---

## The gate proves itself

A checker that only ever passes is not a checker. Every CI run injects the
exact rejected pattern — `set_memory_decrypted((unsigned long)buffer->addr, ...)`
in the allocator — and **requires the gate to exit non-zero**, then repeats
with a `set_memory_decrypted()` call inside `uio_hv_generic.c`. If either
injection is accepted, the step fails. The tree is restored before the KUnit
build.

---

## Reproduce

```bash
# after applying series/*.patch to mainline 93f51579e7df
python3 Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/coco-static-invariants.py \
        --tree <linux-tree>
```

Expected: six `PASS` lines and `COCO-STATIC-PROOF: all invariants hold.`

---

## What remains unproven (and cannot be proven statically)

| Question | Why static analysis cannot answer it |
| --- | --- |
| Does a real SEV-SNP page-state transition succeed on the direct-map chunk? | Requires the hardware and the firmware path that performs it. |
| Does TDX without a paravisor accept the same sequence? | Same; the no-paravisor path is the one Kelley named. |
| Does Arm CCA accept it? | Same; CCA is its own acceptance line in the PRD. |
| Does the failure injection actually leave the guest alive? | Requires observing a live injected failure. |
| Do the pages stay valid across `vmap()` at runtime? | Requires the running kernel on that platform. |

These are COCO-1..5. They stay open.

---

## Rollback trigger

If the `Enforce CoCo static invariants` CI step ever fails, or if a future
change adds a `set_memory_*` call whose argument is not a `page_address()`
expression outside the documented legacy pair, or passes
`memory_prepared = false` from anything other than `vmbus_establish_gpadl()`,
or drops the `encryption_unknown` retain path, treat that as a regression
against RF-2: revert the change, restore the invariant, and record the
regression in [`validation.md`](../../../../validation.md). **Do not silence
the step to land a patch**, and do not weaken the gate self-test.
