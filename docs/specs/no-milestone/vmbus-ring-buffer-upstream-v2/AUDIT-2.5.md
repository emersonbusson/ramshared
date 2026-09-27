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
| High | Install boundary | Running Build #6 exports `vmbus_alloc_buffer()` / `vmbus_free_buffer()` and its installed image hash is recorded, but the checked-out Microsoft WSL source at `14794180686c2fb6307fbe359c359bec765249f3` lacks that allocator. The separate backport commit `50715f5f738f2793f2713401db69988df0347ecf` contains it, but neither available `bzImage` artifact matches the installed image. The exact source commit for Build #6 is unproven. The v7.3-rc4 series also fails `git apply --check` against the WSL tree. | Reconcile Build #6 image to its exact source, then port the final safety fixes separately, build and seal a kernel/modules/QEMU pair, pass the attended promotion preflight, and prove rollback identity before host boot. |

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
