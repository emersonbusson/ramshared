# AUDIT-source — adversarial VMBus source and qualification audit

Date: 2026-09-29 (audit); relocated 2026-09-30
Repository: emersonbusson/WSL2-Linux-Kernel
Branch: vmbus-ring-buffer-upstream-v2
Audited source: de5138b5ebc333e30b1854be793abf063a693de8
Pinned mainline base: 93f51579e7df248780214094418f205253383cc5

> **Provenance.** This file is internal RamShared qualification evidence. It
> previously lived in the public kernel contribution fork under
> `Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/mimoaudite.md`. It
> was moved here because it is internal process and defect accounting, not
> upstream kernel documentation, and because a public contribution fork must
> not carry our own unproven-defect ledger. Content is unchanged apart from
> this header and the filename.

This report supersedes the earlier unverified severity table in this file. It
re-reads each listed BUG, GAP, hard-coded value, and documentation claim against
the checked-out source. “Source-confirmed” means the code path is present; it
does not claim a runtime crash or host impact unless a reproducer actually
observed one.

## Evidence boundary

Hosted run [36590352003](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36590352003)
completed successfully for exactly the audited source SHA.

- The seven-patch mainline series applied to the pinned base, passed strict
  checkpatch and W=1/Sparse builds on x86_64 and arm64. Mainline x86_64 KUnit
  passed 24/24. Arm64 KUnit was skipped by the workflow.
- The separate WSL 6.18.40.1 backport passed W=1/Sparse for VMBus, NetVSC, and
  UIO, compiled DXGKRNL with W=1 but C=0, and passed KUnit 14/14.
- Sparse emitted warnings for a flexible-array declaration and a context
  imbalance in unchanged code. DXGKRNL was not checked by Sparse.
- This run is compile and KUnit evidence. It does not reproduce an mmap race,
  GPADL response/rescind interleaving, fragmented physical allocation, or CoCo
  memory transition.
- The active host kernel is not source-matched to this candidate. No boot or
  runtime result in this report qualifies the exact de5138b5 image.

## Recheck of BUG-1 through BUG-12

| ID | Revalidated finding | Result and reproduction boundary |
| --- | --- | --- |
| BUG-1 | DXG existing-system-memory mapping is unmapped twice. | **Source-confirmed defect. Source fix `d9a1a3a8d6f6`.** create_existing_sysmem stored the vmap pointer in dxgalloc->gpadl.addr, then unconditionally vunmapped the local pointer at cleanup without clearing the field; dxgallocation_destroy later vunmapped the stale field on the normal teardown path. The fix clears `gpadl.addr` after the local `vunmap()` so destroy cannot unmap a stale pointer. No runtime warning or reused-address corruption was reproduced; a targeted runtime repro remains open. |
| BUG-2 | Reclaimer page-ref check races a new mmap. | **Static lifetime hazard; runtime UAF not reproduced. Source fix `a8042f978bc0`.** The reclaimer checked page refcounts under its retained-owner lock, then dropped the lock before the free. The free now stays under the lock, `buffer->pages` is cleared under the lock before the array is freed, and a KUnit case covers a busy page ref across a reclaim pass (`15c26f95a702` updates the WSL suite counts). A racing mmap that already loaded the pages pointer is still excluded only by the BUG-3 sysfs drain. Runtime interleaving of a new mmap against reclaim is not reproduced. |
| BUG-3 | UIO sysfs ring mmap can race ring release and pages-array free. | **Static race window confirmed; runtime UAF not reproduced. Source fix `b64d516de5fc`.** `vmbus_free_ring()` now calls `hv_remove_ring_sysfs()` before `vmbus_free_buffer()` so kernfs drains active mmap ops on every free path, including subchannel teardown, then clears `ringbuffer_pagecount`. Deterministic hold-in-mmap-while-close reproduction is still open. |
| BUG-4 | Close failure bypasses retained-owner reclamation. | **Not confirmed as a correctness bug.** The close path explicitly chooses to leak when posting close or tearing down GPADL fails. It leaves the ring ownership record attached to the channel instead of transferring it to the retained list. Reclaim and eventual channel-object cleanup are not proven, so this is a boundedness/observability concern; freeing on that failure would be unsafe. |
| BUG-5 | UIO probe error can lose the chance to resolve a live GPADL. | **Source-confirmed lifecycle gap.** On establish failure, hv_uio_probe calls vmbus_free_buffer before fail_close calls hv_uio_cleanup. Freeing clears the descriptor, so cleanup no longer sees the handle. A host-rescind result can leave the buffer LIVE; a partial post can leave it UNCERTAIN. The former can be safely resolved after confirmed host rescind, but the current ordering retains a copied state with no channel teardown attempt. UNCERTAIN retention is a separate fail-safe case. No Hyper-V integration reproduction was available. |
| BUG-6 | Mainline patch series differs from WSL branch source. | **Not a defect.** The repository intentionally contains a mainline series and a separate WSL 6.18 backport. Hosted CI applies/builds the series against pinned mainline and separately builds the WSL source. They are not byte-identical implementations. Any documentation claiming byte-for-byte parity is unsupported and must be removed. |
| BUG-7 | UNCERTAIN ownership can retain pages indefinitely. | **Intentional fail-safe behavior, not an unsafe-free bug.** The code cannot prove whether a partial create reached the host; tests require retention. No expiry or later proof mechanism exists, so memory can remain retained indefinitely. This is a real boundedness and diagnostics gap. |
| BUG-8 | DXG allocation size narrows from u64 to the u32 GPADL size field. | **Source-confirmed truncation boundary; runtime reachability not reproduced. Source fix `b99248f63e43`.** alloc_size is u64 and was assigned to vmbus_buffer.size (u32) without a bound check; requests above U32_MAX could describe a shorter GPADL than the DXG allocation. The fix rejects sizes above `U32_MAX` before pinning pages and explicitly types the GPADL size store. No greater-than-4-GiB allocation was attempted. |
| BUG-9 | PFN-backed DXG pages are unpinned before host allocation destroy. | **Source-confirmed ordering; host-use-after-unpin is not reproduced. Source fix `0dcd3ad5d996`.** `dxgallocation_stop()` no longer unpins PFN pages; `dxgallocation_release_pins()` runs only after `dxgvmb_send_destroy_allocation()` and any GPADL teardown, and `gpadl.leak` still keeps the pins. Whether the host accesses the PFNs after a destroy send needs an exact protocol/runtime trace. |
| BUG-10 | vmbus_free_buffer may sleep from a NetVSC RCU callback. | **No current violation established.** Normal NetVSC teardown calls the buffer cleanup in process context before call_rcu; the later callback sees cleared descriptors. Probe-error cleanup calls the callback function directly in process context. The invariant is documented, though not asserted. |
| BUG-11 | NetVSC says failed teardown sets buffer->leak. | **Confirmed comment defect.** Teardown does not set leak; safety currently follows from retained GPADL state/handle. This does not change runtime behavior. |
| BUG-12 | UIO computes but does not apply page_offset. | **No current misaligned caller found.** All current UIO regions are page-aligned. The generic callback would mishandle a future unaligned region, but the audit did not reproduce a present failure. |

### Confirmed items that need source fixes

BUG-1, BUG-5, BUG-8, and BUG-11 were the source-confirmed fix candidates;
all four now have landed source fixes listed below (`d9a1a3a8d6f6`,
`4ea7c35d2cd8`, `b99248f63e43`, `68700eb5aa8a`). BUG-2/BUG-3 and BUG-9 also
have designed synchronization/ownership source fixes. Every one of these
still needs the targeted runtime or KUnit reproducer named above before
claiming resolution. No CoCo qualification is represented by this audit
document. The fixes listed below are present in the Build #9 candidate
installed at `/mnt/c/wsl/kernel-ramshared-v6` (receipt source
`a5cedb4de6f8`); that installation does not substitute for runtime proof.

## Recheck of GAP-1 through GAP-18

| ID | Revalidated status |
| --- | --- |
| G1 — series differs from branch source | Expected split between mainline series and WSL backport, not a defect. The stale byte-for-byte parity claim is a documentation defect. |
| G2 — pull-request path filter omits DXGKRNL and ring_buffer.c | **Confirmed CI trigger gap.** Pushes to the named branch trigger without a path filter; pull requests changing only drivers/hv/dxgkrnl/** or drivers/hv/ring_buffer.c do not match the listed paths. |
| G3 — no DXG retention KUnit | **Confirmed coverage gap.** Current KUnit suites cover VMBus GPADL and UIO mmap helpers, not DXG vmap/GPADL/page-pin teardown. |
| G4 — DXG Sparse is disabled | **Confirmed coverage limit.** The WSL job explicitly builds drivers/hv/dxgkrnl with C=0. W=1 compilation still runs. |
| G5 — CoCo claims and proof | **CoCo qualification gap confirmed; specific UIO accusation is false.** No SEV-SNP, TDX, or Arm CCA runtime transition is covered. INT_PAGE_MAP is explicitly not mapped decrypted; monitor pages are explicitly decrypted. A PAGE_KERNEL vmap alone does not prove a CoCo defect. DXG page-sharing semantics remain unqualified. |
| G6 — arm64 KUnit | **Confirmed workflow limit.** The arm64 job builds the series but the KUnit step is conditional on x86_64. |
| G7 — sanitizers | **Confirmed coverage limit, overstated consequence.** The KUnit config does not request KASAN, KMEMLEAK, LOCKDEP, or DEBUG_VM. Those tools could help catch some runtime faults but would not guarantee detection of every listed race. |
| G8 — public API break | **False for the upstream series.** Patch 0007 adds owned entry points while retaining legacy exported adapters. The WSL backport has internal source-signature changes, but these GPL kernel symbols are not a stable out-of-tree module ABI. |
| G9 — StorVSC not converted | **Not a gap in the current ownership change.** StorVSC uses VMBus channel rings and has no direct allocator-buffer call site to migrate. |
| G10 — UIO size differs from rounded GPADL size | **False for current constants.** 16 MiB and 31 MiB are multiples of both 4 KiB and 64 KiB; current UIO allocations and exposed sizes agree. |
| G11 — referenced trovaldo.md and validation.md are missing | **False across the working repositories.** Both exist in the canonical RamShared repository. The kernel SPEC should make its cross-repository paths explicit. |
| G12 — rescind dispatch table entry is bypassed | **Source observation, not a demonstrated bug.** vmbus_onmessage handles rescind directly before table dispatch, so the table callback is redundant on that entry point. No future UAF or CoCo effect was reproduced. |
| G13 — rescind completes only the first waiter | **Code fact, conditional risk.** The loop breaks after one matching waiter. The audit did not demonstrate two concurrent GPADL waiters on one channel. |
| G14 — reclaimer shutdown does not perform a final free pass | **Safe-retention tradeoff.** Delayed work is cancelled at shutdown; unresolved owners can remain allocated. Releasing pages whose host, mapping, or encryption state is uncertain would be unsafe. No unload leak measurement was made. |
| G15 — GPADL completion waits have no timeout | **Availability dependency, not a new safety defect.** Silent host responses can block a caller; timeout handling must preserve uncertain ownership. No silent-host runtime test was run. |
| G16 — KUnit code is embedded in production source | **Maintainability observation only.** It does not establish a runtime defect. |
| G17 — UIO device mmap and sysfs ring mmap have different offsets | **Intentional separate interfaces.** Both have helper tests; no ABI failure was reproduced. |
| G18 — encrypted is used where guest-private/shared would be clearer | **Naming/readability concern.** Current comments explain the boolean contract; no incorrect transition was reproduced from naming alone. |

## Hard-coded values and CI claims

| Claim | Recheck |
| --- | --- |
| One-second reclaimer retry and one-jiffy initial delay | Real constants. They express delayed polling; no CPU or memory impact was measured. Not a demonstrated defect. |
| UIO send/receive sizes of 16/31 MiB and 2 MiB subchannel ring | Real defaults and policy. No negotiation or capacity defect was demonstrated. |
| Stale recv_name comment | Real, covered by BUG-11-class documentation cleanup: the buffer is 32 bytes and the format is recv:%u. |
| Sparse SHA duplicated across two workflow jobs | Same pinned SHA in both places. Maintenance duplication, no current drift. |
| Pinned mainline base | Intentional reproducibility. It needs an explicit refresh process, but pinning itself is not a defect. |
| Literal KUnit counts and exact seven-patch check | Brittle maintenance gates. They fail visibly when the series/test inventory changes; no false pass was demonstrated. |
| 90-minute job timeout | No runner timeout failure was observed. |
| Strict checkpatch requires zero checks | Deliberately strict policy; hosted run passed all patch stages. |
| Root *.patch whitespace rule | Applies to patch files in this repository, not the fetched Linux worktree. Broad but not a demonstrated build defect. |
| Duplicated required-test name lists | Maintenance duplication; the two lists currently agree. |

## Runtime and platform qualification still open

1. Run the exact candidate on ordinary Hyper-V while injecting GPADL create,
   teardown, host-rescind, local-rescind, close, and UIO subchannel mmap races.
2. Reproduce the ring release race with a deterministic test that holds the
   sysfs mmap callback after its state check while the close path frees the
   ring. Add an equivalent page-ref/reclaim race test before calling BUG-2/3
   resolved.
3. Test the DXG greater-than-4-GiB boundary and prove PFN pages remain pinned
   until the host confirms allocation destruction.
4. Exercise real allocator fragmentation and show order-zero fallback on the
   exact candidate; KUnit fault injection is not physical fragmentation.
5. Run SEV-SNP, TDX, and Arm CCA transition tests on supported Hyper-V hosts.
6. Boot and test the exact WSL backport image with a matching source/build
   receipt. The active host kernel is currently not matched to de5138b5.
7. Collect measured channel latency and stability data if performance claims
   are intended.

## Verdict

The hosted build/KUnit run is real and current for de5138b5, but it does not
close the source-confirmed DXG/UIO lifecycle issues or provide runtime
qualification. The original audit overstated several findings: series/source
split, API break, StorVSC migration, UIO size mismatch, missing sibling docs,
RCU sleep, and current page-offset failure are not established defects.
Confirmed source defects and risks remain listed above. Do not describe this
candidate as universally CoCo-safe, runtime-qualified, or installed on the
host.

## Source fixes applied locally

Local source edits after this audit on branch
`vmbus-ring-buffer-upstream-v2`. These are source-level fixes only.
They are not runtime, host, or CoCo qualification.

| Defect | Commit | Change |
| --- | --- | --- |
| BUG-1 | `d9a1a3a8d6f6` | `create_existing_sysmem()` clears `gpadl.addr` after the local `vunmap()` so `dxgallocation_destroy()` cannot unmap a stale pointer. |
| BUG-8 | `b99248f63e43` | `create_existing_sysmem()` rejects allocation sizes above `U32_MAX` before pinning pages; the GPADL size store is explicitly typed. |
| BUG-5 | `4ea7c35d2cd8` | `hv_uio_probe()` no longer frees buffers before `fail_close`; `hv_uio_cleanup()` tears down a live GPADL and retains uncertain pages. |
| BUG-11 | `68700eb5aa8a` | NetVSC teardown comment matches retained-GPADL safety; UIO `recv_name` comment matches the `recv:%u` format. |
| G2 | `805418bd7021` | PR path filter now includes `drivers/hv/dxgkrnl/**` and `drivers/hv/ring_buffer.c`. |
| BUG-3 | `b64d516de5fc` | `vmbus_free_ring()` calls `hv_remove_ring_sysfs()` before `vmbus_free_buffer()` and clears `ringbuffer_pagecount` after the free. |
| BUG-2 | `a8042f978bc0` | Reclaim free runs under `vmbus_retained_buffers_lock`; `buffer->pages` is cleared under that lock before the array is freed; KUnit `vmbus_reclaim_busy_ref_defers_free_test` holds a busy page ref across a reclaim pass. |
| BUG-2 CI | `15c26f95a702` | WSL KUnit gates count the new reclaim case (12 subtests, 15 total). |
| BUG-9 | `0dcd3ad5d996` | `dxgallocation_release_pins()` unpins PFN pages only after the host destroy send and GPADL teardown; `dxgallocation_stop()` is IO-space quiesce only. |

G3 (DXG KUnit) was not landed: `create_existing_sysmem()` is static and
needs a device/host fixture, so a focused test would require speculative
refactoring. BUG-9 therefore documents the unpin-after-host-destroy
invariant in `dxgadapter.c` instead of adding a mock ordering flag.
Verification of the commits above is checkpatch clean and a targeted
`W=1` rebuild of the touched objects (`channel.o`, `dxgadapter.o`);
the new KUnit case is compile-checked with `CONFIG_KUNIT=y` but not
executed in this environment. BUG-2/BUG-3/BUG-9 runtime interleaving,
every runtime/CoCo item, and the gaps in the tables above remain open
exactly as listed.

