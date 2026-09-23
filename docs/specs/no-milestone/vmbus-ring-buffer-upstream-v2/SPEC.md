# SPEC — Fragmentation-resilient VMBus rings across confidential guests

## Closed scope

Prepare a locally testable upstream v2 against Linux v7.3-rc4. In scope:
`drivers/hv/channel.c`, `drivers/hv/ring_buffer.c`,
`drivers/hv/hyperv_vmbus.h`, `include/linux/hyperv.h`,
`drivers/uio/uio_hv_generic.c`, and the netvsc
buffer owner. Out of scope: balloon/watermark changes from the former 1/2
patch, WSL deployment, and upstream transmission. The upstream tag already
contains Kameron Carr's `vmbus_alloc_buffer()` series.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-2, ITEM-4 |
| RF-2 | ITEM-2, ITEM-3 |
| RF-3 | ITEM-1, ITEM-3, ITEM-5 |
| RF-4 | ITEM-3, ITEM-5, ITEM-6 |
| RF-5 | ITEM-4, ITEM-5, ITEM-6 |
| NFR-1 | ITEM-2, ITEM-5 |
| NFR-2 | ITEM-6 |

## Technical decisions

| ID | Decision | Reason |
| --- | --- | --- |
| DT-1 | One `struct vmbus_buffer` owns address, chunks, GPADL, and leak state | Avoid split lifetime metadata and make unsafe-to-free explicit. |
| DT-2 | Ring and netvsc pass their own confidentiality flag to allocation | `co_ring_buffer` and `co_external_memory` are distinct contracts. |
| DT-3 | GPADL layout (`BUFFER` vs `RING`) is separate from whether the caller already handled encryption | A ring needs gap/offset encoding but may already be decrypted. |
| DT-4 | Ring wraparound maps `vmalloc_to_page()` results from the virtual buffer | The allocator no longer promises one contiguous `struct page` array. |
| DT-5 | Failed teardown or unknown re-encryption retains backing pages; cleanup is idempotent | The host may still access them, or their private/shared state may be unknown. |
| DT-6 | Keep exported legacy GPADL interfaces only where existing external consumers require them | Avoid an unrelated exported-API migration in this series. |
| DT-7 | Give the ring owner a page-pointer array for UIO/sysfs mapping, and expose UIO memory as virtual | A single physical range is no longer valid. |

## Atomicity and rollback

Allocation, GPADL messages, and `vmap`/`vunmap` run in sleepable process
context. No spinlock is held across allocation or host response wait.
The host-ownership frontier is successful GPADL establishment; before
returning pages, teardown must be confirmed or channel rescind must be
established according to the current VMBus contract. On ambiguity, leak and
log. No userspace or persistent host state changes occur during patch
preparation. A test kernel is rolled back by rebooting the prior image.

## Kahneman map

| Stage | Discipline | Question | Minimum executable evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 | #13 refusal/legitimate | Do both private and shared rings map through the correct page state? | Named KUnit allocation/mapping tests plus CoCo lab | Any decryption on `vmap` address |
| ITEM-3 | #16 exhaustion | Does a high-order allocation failure fall to smaller chunks without exposing partial pages? | Fault-injection allocation test | Any freed page with unknown encryption state |
| ITEM-5 | #17 replay | Can close/error cleanup repeat without double free? | Named teardown/failure-injection test | Double free, host-visible free, or nonzero GPADL retained as safe |

## Security checklist

- Privilege/uAPI: N/A — no new user interface.
- Host copy: GPADL physical page list remains bounded by validated buffer size.
- IRQ/atomic: all touched allocation and unmapping paths remain process-context.
- Lifetime: one buffer owns backing pages, mapping, and GPADL state.
- CoCo: direct-map decryption precedes virtual mapping; failed re-encryption leaks.
- Host safety: no pressure or kernel install on the daily WSL2 environment.
- Replay: a cleaned buffer cannot be freed a second time.

## Files and implementation order

1. **ITEM-1:** Extend `include/linux/hyperv.h` with `struct vmbus_buffer` and replace split ring/netvsc buffer fields.
2. **ITEM-2:** Update `drivers/hv/channel.c` allocation/free API to accept the correct confidentiality condition and the aggregate owner.
3. **ITEM-3:** Decouple GPADL layout from encryption state; retain host ownership on teardown uncertainty.
4. **ITEM-4:** Update `drivers/hv/ring_buffer.c` and `drivers/hv/hyperv_vmbus.h` to map the virtual ring's backing pages.
5. **ITEM-5:** Convert ring, netvsc, and UIO call sites and their failure unwinds to the aggregate lifecycle.
6. **ITEM-6:** Run style/build/static/fault-injection and isolated live tests; write exact result in `IMPL.md`.

## Required tests matrix

| Production path | Named test | Kind | Cover |
| --- | --- | --- | --- |
| Ring allocation and mapping | `vmbus_ring_buffer_noncontiguous_pages` | KUnit / failure injection | N/A — kernel slice; targeted build + live drill |
| Confidential ring GPADL | `vmbus_ring_buffer_coco_decrypt_once` | KUnit / CoCo lab | N/A — kernel slice; CoCo evidence |
| GPADL teardown and buffer free | `vmbus_buffer_failed_teardown_leaks` | KUnit / failure injection | N/A — kernel slice; targeted build + live drill |
| Partial allocation | `vmbus_buffer_partial_allocation_cleanup` | KUnit / failure injection | N/A — kernel slice; targeted build + live drill |
| Netvsc buffer migration | `netvsc_buffer_lifecycle` | integration / Hyper-V lab | N/A — kernel slice; live drill |
| UIO ring mapping | `uio_hv_ring_noncontiguous_mmap` | integration / Hyper-V lab | N/A — kernel slice; live drill |

## Observability and living docs

Kernel warnings report only stable error codes and buffer role; no addresses.
Update this SPEC, `AUDIT-2.5.md`, `IMPL.md`, `trovaldo.md`, and
`validation.md` with observed results. The public README is unchanged until
new qualification exists.

## Validation checklist

- [ ] RED tests execute against unmodified upstream source.
- [ ] Named tests above execute and pass.
- [ ] `scripts/checkpatch.pl` accepts every patch.
- [ ] Targeted Hyper-V and netvsc build succeeds; sparse succeeds if enabled.
- [ ] Isolated Hyper-V normal, failure, rescind and pressure tests pass.
- [ ] CCA and no-paravisor TDX evidence is recorded; otherwise PARTIAL.
- [ ] No patch is emailed without a separate operator review and approval.
