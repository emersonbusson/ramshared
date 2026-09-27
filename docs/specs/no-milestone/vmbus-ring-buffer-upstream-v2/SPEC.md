# SPEC — Fragmentation-resilient VMBus rings across confidential guests

## Scope

Maintain the upstream v2 against Linux v7.3-rc4 and separately port its
allocation, GPADL ownership, and UIO guarantees to a source-matched WSL
6.18.40.1 tree. The running Build #6 image has allocator symbols, but its
source revision is not yet matched (EVD-0089). The raw upstream patches are
not expected to apply to the WSL tree. In scope: `drivers/hv/channel.c`,
`drivers/hv/ring_buffer.c`, `drivers/hv/hyperv_vmbus.h`,
`include/linux/hyperv.h`, `drivers/uio/uio_hv_generic.c`, the netvsc buffer
owner, and WSL-specific DXG GPADL ownership. Out of scope: balloon/watermark
changes from the former 1/2 patch and upstream transmission. The upstream tag
already contains Kameron Carr's `vmbus_alloc_buffer()` series.

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
| NFR-3 | DT-11, DT-12, ITEM-6 |

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
| DT-8 | Keep the WSL 6.18.40.1 backport in a separate source branch | The exact v7.3-rc4 series fails to apply to the WSL tree. |
| DT-9 | Promote only a sealed kernel/modules/QEMU pair through `wsl-kernel.sh apply` | The host reports `NEED_ARM`; manual image replacement is not admitted. |
| DT-10 | Preserve buffer ownership and encryption state across rescind until a teardown acknowledgement or protocol-proven terminal host revocation; distinguish remote rescind from local unload | The current rescind path can report success while retaining a nonzero GPADL handle, after which release skips the free and clears the owner structure. |
| DT-11 | Attribute runtime results only to an image, modules, and exact source revision bound by one manifest | The Build #6 image hash is known, but its source commit is not matched to the local source or candidate artifacts. |
| DT-12 | Reconcile buffer ownership with stable device GUID, channel relid, role, and allocate/free/retain result; never log kernel addresses | EVD-0089 shows growing VMBus mappings far exceed the live channel count, but `/proc/vmallocinfo` alone cannot identify owners. |

## Atomicity and rollback

Allocation, GPADL messages, and `vmap`/`vunmap` run in sleepable process
context. No spinlock is held across allocation or host response wait.
The host-ownership frontier is successful GPADL establishment, but a posted
request with an uncertain result must also retain its pending handle. Before
returning pages, teardown must be acknowledged or a terminal host revocation
must be established from explicit message origin. A generic
`channel->rescind` flag alone is insufficient: local unload can set it, and
the current code can return success from its rescind branch without clearing
the handle. That leaves `vmbus_release_buffer()` skipping the free before it
clears the owner structure. Preserve the owner until the disposition is
proven; never re-encrypt while the host may still reference a shared buffer.
Apply the same ownership rule when a partial GPADL post fails and before
allocating teardown metadata. On ambiguity, retain and account for the pages.
CoCo re-encryption failure remains an independent reason to retain the
affected chunk. No userspace or persistent host state changes occur during
patch preparation. A test kernel is rolled back only after a fresh boot
proves the previous kernel identity. The attended host test must not enable
swap, run memory pressure, or change RamShared lifecycle state.

## Kahneman map

| Stage | Discipline | Question | Minimum executable evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 | #13 refusal/legitimate | Do both private and shared rings map through the correct page state? | Named KUnit allocation/mapping tests plus CoCo lab | Any decryption on `vmap` address |
| ITEM-3 | #16 exhaustion | Does a high-order allocation failure fall to smaller chunks without exposing partial pages? | Fault-injection allocation test | Any freed page with unknown encryption state |
| ITEM-3 / ITEM-5 | #13 refusal/legitimate | Which rescind source proves host GPADL revocation? | KUnit: host rescind reclaims, synthetic hibernation and local unload retain, partial-post rescind and teardown-allocation failure paths | Any generic `channel->rescind` path frees host-referenced pages |
| ITEM-3 / ITEM-5 | #17 replay | Can a successful-looking rescind teardown erase the only owner while the GPADL handle remains live? | `vmbus_gpadl_rescind_handle_state_test`: assert pending handle, owner, and encryption state survive until explicit release; repeat cleanup | Nonzero handle with a cleared owner or premature re-encryption |
| ITEM-5 | #17 replay | Can close/error cleanup repeat without double free? | Named teardown/failure-injection test | Double free, host-visible free, or nonzero GPADL retained as safe |
| ITEM-6 | #9 number / #17 replay | Do ordinary repeated channel cycles return mappings to baseline? | `vmbus_channel_lifecycle_buffer_balance`: 100 normal open/close cycles plus allocation/free/retain accounting, reconciled with vmalloc maps | Monotonic unexplained map growth or an unclassified retained buffer |

## Security checklist

- Privilege/uAPI: N/A — no new user interface.
- Host copy: GPADL physical page list remains bounded by validated buffer size.
- IRQ/atomic: all touched allocation and unmapping paths remain process-context.
- Lifetime: one buffer owns backing pages, mapping, and GPADL state.
- CoCo: direct-map decryption precedes virtual mapping; failed re-encryption leaks.
- Host revocation: only confirmed teardown or host-originated rescind clears
  GPADL ownership; local and synthetic rescind retain pages without separate
  proof.
- Host safety: one attended test promotion is allowed only after its immutable
  kernel/modules/QEMU pair passes pre-install gates; no pressure or RamShared
  swap activation is allowed on the daily WSL2 environment.
- Replay: a cleaned buffer cannot be freed a second time.

## Files and implementation order

1. **ITEM-1:** Extend `include/linux/hyperv.h` with `struct vmbus_buffer` and replace split ring/netvsc buffer fields.
2. **ITEM-2:** Update `drivers/hv/channel.c` allocation/free API to accept the correct confidentiality condition and the aggregate owner.
3. **ITEM-3:** Decouple GPADL layout from encryption state; retain host ownership on teardown uncertainty.
4. **ITEM-4:** Update `drivers/hv/ring_buffer.c` and `drivers/hv/hyperv_vmbus.h` to map the virtual ring's backing pages.
5. **ITEM-5:** Convert ring, netvsc, and UIO call sites and their failure unwinds to the aggregate lifecycle.
6. **ITEM-6:** Port the final safety changes to WSL 6.18.40.1 as a separate
   patch branch; run style/build/static/fault-injection and isolated live
   tests; write exact result in `IMPL.md`.

## Required tests matrix

| Production path | Named test | Kind | Cover |
| --- | --- | --- | --- |
| Ring allocation and mapping | `vmbus_ring_buffer_noncontiguous_pages` | KUnit / failure injection | N/A — kernel slice; targeted build + live drill |
| Allocation-order fallback | `vmbus_ring_fallback_order_zero_test`, `vmbus_buffer_order_zero_allocation_test` | KUnit helper plus injected allocation failures; patch 6 passed hosted KUnit run 36148296003 | N/A — kernel slice; live fragmentation drill still required |
| GPADL post failure | `vmbus_gpadl_post_failure_test`, `vmbus_gpadl_post_success_test`, `vmbus_gpadl_response_state_test`, `vmbus_gpadl_teardown_post_failure_test` | Callback-injected KUnit; prior five-patch hosted run passed | N/A — kernel slice; live host response/rescind interleaving remains required |
| Confidential ring GPADL | `vmbus_ring_buffer_coco_decrypt_once` | KUnit / CoCo lab | N/A — kernel slice; CoCo evidence |
| GPADL teardown and buffer free | `vmbus_buffer_failed_teardown_leaks` | KUnit / failure injection | N/A — kernel slice; targeted build + live drill |
| GPADL rescind ownership | `vmbus_gpadl_host_rescind_reclaims_test`, `vmbus_gpadl_synthetic_rescind_retains_test`, `vmbus_gpadl_unload_rescind_retains_test`, `vmbus_gpadl_partial_post_rescind_test`, `vmbus_gpadl_teardown_alloc_failure_test`, `vmbus_gpadl_rescind_handle_state_test` | Callback-injected KUnit plus host-origin runtime trace | N/A — kernel slice; tests and runtime proof remain open |
| Partial allocation | `vmbus_buffer_partial_allocation_cleanup` | KUnit / failure injection | N/A — kernel slice; targeted build + live drill |
| Netvsc buffer migration | `netvsc_buffer_lifecycle` | integration / Hyper-V lab | N/A — kernel slice; live drill |
| UIO ring mapping | `uio_hv_ring_noncontiguous_mmap` | integration / Hyper-V lab | N/A — kernel slice; live drill |
| VMBus buffer ownership | `vmbus_channel_lifecycle_buffer_balance` | isolated Hyper-V drill; 100 normal open/close cycles, then injected uncertain teardown | N/A — kernel slice; runtime owner accounting and vmalloc reconciliation |

## Observability and living docs

Kernel warnings and lifecycle counters report stable device GUID, channel relid,
buffer role, and outcome; no kernel addresses. The live drill records allocate,
free, and intentionally retained counts and reconciles them with
`/proc/vmallocinfo` before and after repeated channel cycles.
Update this SPEC, `AUDIT-2.5.md`, `IMPL.md`, `trovaldo.md`, and
`validation.md` with observed results. The public README is unchanged until
new qualification exists.

## Validation checklist

- [ ] RED tests execute against unmodified upstream source.
- [ ] Named tests above execute and pass.
- [ ] `scripts/checkpatch.pl` accepts every patch.
- [ ] Targeted Hyper-V and netvsc build succeeds; sparse succeeds if enabled.
- [ ] Isolated Hyper-V normal, failure, rescind and pressure tests pass.
- [ ] Build #6 image, modules, and source revision are bound in one manifest before host attribution or promotion.
- [ ] One hundred normal open/close cycles leave no unexplained VMBus mapping growth.
- [ ] CCA and no-paravisor TDX evidence is recorded; otherwise PARTIAL.
- [ ] No patch is emailed without a separate operator review and approval.
