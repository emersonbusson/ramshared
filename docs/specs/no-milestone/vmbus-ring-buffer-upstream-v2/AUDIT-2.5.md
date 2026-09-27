# AUDIT-2.5 — vmbus-ring-buffer-upstream-v2

## Findings

| Severity | SPEC section | Finding | Required resolution |
| --- | --- | --- | --- |
| High | DT-2/DT-3 | `co_ring_buffer` and `co_external_memory` differ; the accepted allocator currently tests only the latter. | Pass the ring confidentiality condition explicitly and avoid decryption of virtual addresses. |
| High | DT-5 | A failed GPADL teardown can leave the host owning pages even if local re-encryption succeeds. | Carry an explicit unsafe-to-free state across unwind and deferred free. |
| High | Test matrix | This host is an ordinary WSL2 guest, not CCA or no-paravisor TDX. | Keep status PARTIAL until suitable CoCo evidence exists; never claim the local host proves compatibility. |
| Medium | ITEM-5 | Netvsc defers free to process context through RCU work. | Preserve that context boundary when changing the owner type. |
| High | DT-7 | UIO maps rings as one physical extent and fails to compile after removing `ringbuffer_page`. | Use per-page virtual mapping for both UIO and sysfs, and test offset bounds. |
| High | Install boundary | The booted WSL2 6.18.40.1 source contains an earlier `vmbus_alloc_buffer()` backport, but the v7.3-rc4 series fails `git apply --check` in all seven touched files. Build #6 is active; `wsl-kernel.sh status` reports `NEED_ARM` because its immutable promotion receipt is missing. | Port the final safety fixes separately, build and seal a kernel/modules/QEMU pair, pass the attended promotion preflight, and prove rollback identity before host boot. |

## Open questions

- Whether the maintainer prefers to include the broader netvsc buffer-owner
  conversion in the same series or as a preparatory patch. The local series
  will be split into reviewable commits before sending.
- Whether live CCA and no-paravisor TDX guests are available for qualification.

## Verdict

**GO for a separate WSL backport draft. NO-GO for host installation** until
the WSL backport builds, passes static and failure tests, has a sealed
kernel/modules/QEMU pair, and is admitted by the attended promotion gate. The
upstream series remains unsendable until its Hyper-V and CoCo platform gates
pass.
