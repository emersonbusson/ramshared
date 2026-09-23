# AUDIT-2.5 — vmbus-ring-buffer-upstream-v2

## Findings

| Severity | SPEC section | Finding | Required resolution |
| --- | --- | --- | --- |
| High | DT-2/DT-3 | `co_ring_buffer` and `co_external_memory` differ; the accepted allocator currently tests only the latter. | Pass the ring confidentiality condition explicitly and avoid decryption of virtual addresses. |
| High | DT-5 | A failed GPADL teardown can leave the host owning pages even if local re-encryption succeeds. | Carry an explicit unsafe-to-free state across unwind and deferred free. |
| High | Test matrix | This host is an ordinary WSL2 guest, not CCA or no-paravisor TDX. | Keep status PARTIAL until suitable CoCo evidence exists; never claim the local host proves compatibility. |
| Medium | ITEM-5 | Netvsc defers free to process context through RCU work. | Preserve that context boundary when changing the owner type. |
| High | DT-7 | UIO maps rings as one physical extent and fails to compile after removing `ringbuffer_page`. | Use per-page virtual mapping for both UIO and sysfs, and test offset bounds. |
| High | Install boundary | The booted WSL2 6.18.40.1 source lacks `vmbus_alloc_buffer()` and still uses `ringbuffer_page`; the v7.3-rc4 draft fails `git apply --check` in all seven touched files. | Treat a WSL2 6.18 backport as a separate specified change, then validate and boot it in an isolated guest before any host installation. |

## Open questions

- Whether the maintainer prefers to include the broader netvsc buffer-owner
  conversion in the same series or as a preparatory patch. The local series
  will be split into reviewable commits before sending.
- Whether live CCA and no-paravisor TDX guests are available for qualification.

## Verdict

**GO for a local draft only. NO-GO for upstream submission or production
kernel installation** until all named tests and platform gates pass.
