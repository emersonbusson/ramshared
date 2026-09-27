# AUDIT-2.5 — vmbus-ring-buffer-upstream-v2

## Findings

| Severity | SPEC section | Finding | Required resolution |
| --- | --- | --- | --- |
| High | DT-2/DT-3 | `co_ring_buffer` and `co_external_memory` differ; the accepted allocator currently tests only the latter. | Pass the ring confidentiality condition explicitly and avoid decryption of virtual addresses. |
| High | DT-5 | A failed GPADL teardown can leave the host owning pages even if local re-encryption succeeds. | Carry an explicit unsafe-to-free state across unwind and deferred free. |
| High | DT-5/DT-10, ITEM-3/ITEM-5 | The proposed rescind helper treats `channel->rescind` as proof that Hyper-V released the GPADL. That flag is also set by synthetic hv_sock hibernation cleanup and local `vmbus_free_channels()` unload. The helper also misses partial GPADL-establishment unwind, and `vmbus_teardown_gpadl()` can return on teardown-metadata `kzalloc()` failure before checking rescind. | Propagate message origin; reclaim only after confirmed teardown or a host-originated rescind. Cover partial-post unwind and teardown-allocation failure. Preserve failed CoCo re-encryption and unrelated buffer-leak state. Test host rescind, synthetic hibernation, and unload separately. |
| High | Test matrix | This host is an ordinary WSL2 guest, not CCA or no-paravisor TDX. | Keep status PARTIAL until suitable CoCo evidence exists; never claim the local host proves compatibility. |
| Medium | ITEM-5 | Netvsc defers free to process context through RCU work. | Preserve that context boundary when changing the owner type. |
| High | DT-7 | UIO maps rings as one physical extent and fails to compile after removing `ringbuffer_page`. | Use per-page virtual mapping for both UIO and sysfs, and test offset bounds. |
| High | Install boundary | The booted WSL2 6.18.40.1 source contains an earlier `vmbus_alloc_buffer()` backport, but the v7.3-rc4 series fails `git apply --check` in all seven touched files. Build #6 is active; `wsl-kernel.sh status` reports `NEED_ARM` because its immutable promotion receipt is missing. | Port the final safety fixes separately, build and seal a kernel/modules/QEMU pair, pass the attended promotion preflight, and prove rollback identity before host boot. |

## Open questions

- Whether the maintainer prefers to include the broader netvsc buffer-owner
  conversion in the same series or as a preparatory patch. The local series
  will be split into reviewable commits before sending.
- Whether live CCA and no-paravisor TDX guests are available for qualification.
- Whether the maintainers consider the documented host-rescind guarantee
  sufficient to reclaim CoCo-backed pages immediately, or require an explicit
  GPADL teardown acknowledgement before re-encryption and release.

## Verdict

**NO-GO for rescind-based reclamation and host installation.** The attempted
helper was removed after review because its predicate conflates host rescind,
synthetic hibernation, and local unload, while leaving partial-establishment
and allocation-failure paths uncovered. The ignored local `0007` file is not
part of the tracked series and its helper-state KUnit case does not prove the
lifecycle. First implement the source-aware ownership state and named failure
tests in SPEC; then build and qualify the WSL backport and exact upstream
series separately. Host installation still requires a sealed
kernel/modules/QEMU pair and the attended promotion gate. Upstream submission
remains blocked on Hyper-V, CoCo, and maintainer-review gates.
