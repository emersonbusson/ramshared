# AUDIT-2.5 — vmbus-ring-buffer-upstream-v2

## Findings

| Severity | SPEC section | Finding | Required resolution |
| --- | --- | --- | --- |
| High | DT-2/DT-3 | `co_ring_buffer` and `co_external_memory` differ; the accepted allocator currently tests only the latter. | Pass the ring confidentiality condition explicitly and avoid decryption of virtual addresses. |
| High | DT-5 | A failed GPADL teardown can leave the host owning pages even if local re-encryption succeeds. | Carry an explicit unsafe-to-free state across unwind and deferred free. |
| High | DT-5/DT-10, ITEM-3/ITEM-5 | In source commits `50715f5f7` and `418653fde`, `vmbus_teardown_gpadl()` returns success on `channel->rescind` without clearing the nonzero handle; `vmbus_release_buffer()` then skips the free and zeroes the owner. The mapping can remain allocated with no tracked owner. This is source-confirmed but not attributed to Build #6 because its source revision is unknown. | Track remote-release origin separately from local teardown; preserve the owner and encryption state until teardown acknowledgement or a protocol-proven terminal host revocation. Add KUnit coverage for the handle, owner, and page state across rescind and repeated cleanup. |
| High | DT-5/DT-10, ITEM-3/ITEM-5 | The proposed rescind helper treats the shared `channel->rescind` state as proof that Hyper-V released the GPADL. The host rescind handler sets it, and local `vmbus_free_channels()` also sets it during unload; hibernation-related hv_sock cleanup can defer removal and invalidate the relid. The helper misses partial GPADL-establishment unwind, and teardown-metadata `kzalloc()` can fail before the rescind check. | Propagate message origin and buffer ownership; reclaim only after confirmed teardown or a protocol-proven terminal host revocation. Cover partial-post unwind and teardown-allocation failure. Preserve failed CoCo re-encryption and unrelated buffer-leak state. Test host rescind, suspend/hibernation cleanup, and unload separately. |
| High | Test matrix | This host is an ordinary WSL2 guest, not CCA or no-paravisor TDX. | Keep status PARTIAL until suitable CoCo evidence exists; never claim the local host proves compatibility. |
| Medium | ITEM-5 | Netvsc defers free to process context through RCU work. | Preserve that context boundary when changing the owner type. |
| High | DT-7 | UIO maps rings as one physical extent and fails to compile after removing `ringbuffer_page`. | Use per-page virtual mapping for both UIO and sysfs, and test offset bounds. |
| High | Install boundary | Running Build #6 exports `vmbus_alloc_buffer()` / `vmbus_free_buffer()` and its installed image hash is recorded, but the checked-out Microsoft WSL source at `14794180686c2fb6307fbe359c359bec765249f3` lacks that allocator and still uses `ringbuffer_page`. The separate backport commit `50715f5f738f2793f2713401db69988df0347ecf` contains it, but neither available `bzImage` artifact matches the installed image. The exact source commit for Build #6 is unproven. The v7.3-rc4 series also fails `git apply --check` against the WSL tree in all seven touched files. | Reconcile Build #6 image to its exact source, then port the final safety fixes separately, build and seal a kernel/modules/QEMU pair, pass the attended promotion preflight, and prove rollback identity before host boot. Treat a WSL2 6.18 backport as a separate specified change, then validate and boot it in an isolated guest before any host installation. |

## Open questions

- Whether the maintainer prefers to include the broader netvsc buffer-owner
  conversion in the same series or as a preparatory patch. The local series
  will be split into reviewable commits before sending.
- Whether live CCA and no-paravisor TDX guests are available for qualification.
- Whether Build #6 uses the `50715` source snapshot. If so, its 17,350 maps of
  104 pages exceed the 2,048 configured RELID limit despite one in-tree
  `vmbus_alloc_buffer()` caller for rings. The vmalloc entries still have no
  owner, role, or open/close/GPADL/rescind correlation, and the Build #6 source
  revision remains unknown.
- Which exact source revision produced the installed Build #6 image; its
  runtime symbols identify the allocator, but its image hash does not match
  the available source-build artifacts.
- Whether host rescind terminally revokes every completed, partial, or
  in-flight GPADL. The [Linux VMBus documentation](https://github.com/torvalds/linux/blob/master/Documentation/virt/hyperv/vmbus.rst)
  says neither side retains state after a device is rescinded, and current
  [Linux teardown code](https://github.com/torvalds/linux/blob/master/drivers/hv/channel.c)
  treats rescind as a successful teardown path. The Microsoft
  [GPADL creation API](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/vmbuskernelmodeclientlibapi/nc-vmbuskernelmodeclientlibapi-fn_vmb_channel_create_gpadl_from_buffer)
  says the client buffer remains locked until the GPADL is torn down; its
  [delete API](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/vmbuskernelmodeclientlibapi/nc-vmbuskernelmodeclientlibapi-fn_vmb_channel_delete_gpadl)
  waits while the server still maps it. Obtain maintainer/protocol confirmation
  that rescind revokes all GPADLs before applying the same reclaim rule to
  shared CoCo pages, especially across partial or in-flight creation.

## Verdict

**NO-GO for rescind-based reclamation and host installation.** Source review
confirmed that the `50715`/`418653` rescind path can lose ownership after a
successful-looking teardown, which fits cumulative retention but is not
proven to be in the running Build #6 image. The attempted helper was removed
because it did not preserve ownership across partial establishment and
teardown allocation failure, and the shared flag also covers local unload.
The ignored local `0007` file is not part of the tracked series and its
helper-state KUnit case does not prove lifecycle safety. First implement
source-aware ownership state and named failure tests in SPEC; then build and
qualify the WSL backport and exact upstream series separately. Host
installation still requires a sealed kernel/modules/QEMU pair and the
attended promotion gate. Upstream submission remains blocked on Hyper-V, CoCo,
and maintainer-review gates.

**GO for a local draft only. NO-GO for upstream submission or production
kernel installation** until all named tests and platform gates pass.

## 2026-09-28 source re-audit

The current exact candidate adds a production retained-owner list and delayed
reclaimer, not only the earlier KUnit cleanup helper. Teardown acknowledgments
and host rescind update the owner state; reclaim waits for a known encryption
state and a baseline page reference count. UIO's per-page mappings acquire
page references, which the reclaimer checks after the sysfs/UIO entry points
are removed. Named tests exercise the state gates and page reference count,
but they do not create real VMBus message races or prove that a host rescind
orders after the host has stopped accessing every GPADL page.

The Linux VMBus documentation says that rescinding removes the device and
that neither side retains the device's previous state. It does not explicitly
specify when GPADL access is revoked relative to guest teardown. The current
host-rescind reclamation rule therefore remains an assumption requiring an
ordinary Hyper-V runtime interleaving test and maintainer review; it is not
closed by that documentation alone.

The source also preserves the previous exported allocator, free, GPADL
establish, caller-decrypted establish, and teardown signatures through
compatibility adapters. In-tree migrated callers use the descriptor-aware
`_owned` APIs. Exact-source `git diff --check`, strict checkpatch, sequential
application of all seven patches to the pinned base, and byte-for-byte source
comparison pass. The latest compatibility-adapter edits have not yet run in
hosted compile or KUnit. No install or runtime test was performed.

**Updated verdict:** local source/apply gates pass; hosted build/KUnit, live
ordinary Hyper-V GPADL/UIO interleavings, and CoCo transitions remain open.
The series remains **NO-GO for upstream transmission and kernel installation**.

## 2026-09-28 UIO page-protection re-audit

The exact candidate source had another mmap gap: the owned allocator marks
some ring and NetVSC UIO buffers shared/decrypted, but the `/dev/uio` mapping
path and sysfs ring path supplied the VMA's default page protection. On an
encrypted guest that can create a userspace alias whose encryption attribute
does not match its backing pages. The monitor page is also explicitly
decrypted during VMBus setup and needs a decrypted userspace alias.

The candidate now uses one page-selection helper for the UIO ring, interrupt,
monitor, receive, and send maps. It rejects private mappings for direct page
insertion, preserves the UIO no-expand/no-dump flags, sets decrypted protection
for shared ring/receive/send buffers and the decrypted monitor page, and leaves
the private `vzalloc()` buffers and interrupt page at their normal protection.
The sysfs ring mmap uses the same protection/range helper. Four named UIO KUnit
cases cover range checks, map selection, MAP_SHARED refusal, and protection
selection. These cases have not run on this revised candidate.

The source fix is still only locally reviewed. Hosted compilation/KUnit and
live UIO close/unregister plus CoCo page-state tests remain required; the
host-rescind GPADL ordering assumption is still open. This finding does not
change the PARTIAL verdict or authorize host installation or upstream send.

## 2026-09-28 WSL backport page-reference re-audit

The WSL backport already had a KUnit assertion that an outstanding mapping
reference must prevent re-encryption, but its production free helper did not
inspect page references. It could re-encrypt/free VMBus pages while a UIO VMA
still mapped them. The corresponding private `vzalloc()` path also released
its vmalloc area immediately. In addition, `vmbus_onmessage_work()` returned
without freeing its allocated message context after the connection entered
`DISCONNECTED`.

The source now checks folio references for chunk and vmalloc-backed pages,
retains live mappings, and retries after one second only when GPADL ownership
and encryption state permit reclamation. Module exit cancels delayed work and
waits for active reclaim work. The disconnected message path frees its
context. The WSL UIO callback also maps all five regions with page references,
enforces shared mappings, and selects decrypted page protection for shared
backing. The sysfs ring path applies its offset once. KUnit now has one new
`vzalloc()` reference test and three UIO mapping/protection cases; the WSL CI
job runs both suites and uploads their configuration and log.

This is a source-level repair only until the hosted W=1/Sparse build and KUnit
job passes. It does not qualify live `/dev/uio` or sysfs mmap teardown, host
GPADL acknowledgment/rescind ordering, or CoCo page transitions. Keep the
kernel and WSL deployment gates PARTIAL.
