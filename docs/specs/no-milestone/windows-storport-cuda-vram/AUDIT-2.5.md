# AUDIT-2.5 — windows-storport-cuda-vram

Audit date: 2026-07-15. Scope: adversarial Step 2.5 review of
`docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md` against its folder PRD, the current
`ramshared-winsvc`/StorPort symbols, ADR-0006, the degradation matrix, SSDV3, Kahneman #2/#5/#9/
#13/#15–#18, and Windows host-safety rules. This audit authorizes implementation planning only; it
does not claim that the named code, tests, WDK gates, or live evidence already exist.

## Findings

| Sev | SPEC § | Issue | Required fix | Resolution |
| --- | --- | --- | --- | --- |
| CRITICAL | Technical decisions DT-4/DT-6; Windows queue files | Moving `StorPortNotification` outside `RAMSHARED_QUEUE.Lock` was directionally correct but incomplete. Without explicit inflight states and rundown, re-entry could reuse a READ slot before its copy completed, while teardown could unmap MDLs during an out-of-lock copy. Cross-process head/tail publication also lacked a cross-language memory-order contract. | Define slot states, `EX_RUNDOWN_REF`, Closing/Failed transitions, PASSIVE rundown wait before unmap, and Rust Acquire/Release paired with driver volatile access plus `KeMemoryBarrier`. Require re-entry/rundown drills under Verifier. | Fixed in place in DT-4/DT-6, `queue.h`/`queue.c` change records, Kahneman ITEM-3, and the test matrix. |
| CRITICAL | Technical decision DT-8; Atomicity and rollback | A single `Win32_PageFileUsage` snapshot before teardown left a race in which an administrator or OS action could create/open a pagefile or filesystem handle before UNREGISTER/DESTROY. WMI failure/ambiguity was not explicitly unsafe. | Use fail-closed Gate A, drain, exclusive `FSCTL_LOCK_VOLUME`, fail-closed Gate B while locked, `FlushFileBuffers`, `FSCTL_DISMOUNT_VOLUME`, then cross the destructive boundary. Resume Online without destructive effects on query/lock failure. | Fixed in place in DT-8, `windows_host.rs`/`service.rs`, rollback text, Kahneman ITEM-5, and named tests/drill verdicts. |
| HIGH | Atomicity and rollback; DT-9 | JSONL was called a recovery cursor even though append-after-effect has an unavoidable crash window. Replaying from the last row could duplicate DESTROY/release or act on a new generation. | Make live in-memory phase authoritative. Evidence is diagnostic only. Permit continuation only in the same process; ambiguous post-crash state requires independent pagefile proof plus reboot/driver-unload recovery and no automatic replay. | Fixed in DT-7/DT-9 and Atomicity/rollback; added `ambiguous_crash_state_is_not_replayed`. |
| HIGH | DT-12; NFR-1/NFR-2 | The original wording treated 5 s as if synchronous `cuMemcpy*` were cancellable. It is not safe to destroy the context, post an error CQE, or terminate the worker while the CUDA call may still touch VRAM/host buffers. | Separate watchdog detection from cancellation. Bound only the overlapped COMMIT IOCTL with `CancelIoEx`; on a stuck CUDA call, stop new fetches, mark unhealthy, preserve state, abort the campaign, and require supervised reboot recovery if it never returns. | Fixed in DT-4/DT-12; added `cuda_watchdog_does_not_destroy_stuck_context`. The 5 s value remains an abort gate, not a false safe-cancellation claim. |
| HIGH | DT-5; security checklist | The initial SPEC said invalid ring contents were checked before MDL mapping, which is impossible, and only described initial ring validation. Mutable mapped memory could later smuggle index jumps, unknown CQ status, or reserved bits. | Split descriptor validation before mapping from ring validation after mapping; revalidate magic, entries, wrapping distance, tag/slot/op/status/reserved on every operation and fail the queue once on corruption. | Fixed in DT-5, security checklist interpretation, queue changes, and `REFUSE_RING_INDEX_JUMP`/reserved verdicts. |
| HIGH | DT-8; broker teardown evidence | Protocol v1 sends no ACK for `LeaseRelease`; a successful socket write alone cannot prove the broker applied the release. Inventing an ACK would violate the frozen broker scope. | Flush release, close the session, and require the live drill to observe the existing broker log correlated to the granted lease ID within 5 s. Missing evidence is PARTIAL/ABORT, not a release claim. | Fixed in DT-8, `broker_tenant.rs`, the full-product drill, and `broker_release_is_observed`. |
| MEDIUM | DT-2/DT-3 | `size_bytes` previously allowed allocations smaller than the three 4 KiB probe positions or a meaningful GPT/NTFS LUN. | Require a 64 MiB minimum in addition to checked alignment/`usize` bounds. | Fixed in DT-2 and config tests. |
| MEDIUM | Kahneman map | Several evidence cells placed two Rust test filters after one `cargo test` invocation; Cargo accepts only one filter, so the evidence was not executable as written. | Use separate `cargo test` commands joined by `&&`; keep each Windows drill as a concrete command with named verdicts. | Fixed in all critical Kahneman rows. |
| MEDIUM | Required tests matrix / file records | `same file`, `same script`, and brace-expanded C paths did not satisfy the requirement for full repository-root paths. Queue-rundown, volume-lock, WMI-error, broker-release, and ambiguous-crash cases were absent from the matrix. | Replace shorthand with explicit repo-root paths and add the missing named tests/verdicts with proper cover/E2E classifications. | Fixed in CREATE/MODIFY records and the required tests matrix. |
| MEDIUM | SCM stop behavior | “Do not claim Stopped after code 7” did not specify a valid SCM state after a safety refusal. | Return service status to `Running`, checkpoint 0, retain STOP acceptance, and emit the code-7 Event Log refusal; uninstall must not delete the running/refusing service. | Fixed in the `crates/ramshared-winsvc/src/main.rs` change record. |

## Re-audit checks

| Gate | Result |
| --- | --- |
| Only this storage-only slice; no pagefile/pressure/reset/attestation expansion | PASS |
| Exact existing Ring-0/Ring-3 symbols and full repo-root paths | PASS after revision |
| Every RF/NFR-with-code traced to contiguous ITEM-1…7 | PASS |
| ABI v1 unchanged; implementation defects handled without a new IOCTL/frame | PASS |
| IOCTL lengths/flags/reserved/owner/ring mutation validated with legitimate/refusal pairing | PASS after revision |
| Lock order, IRQL, no-sleep/no-allocation, completion re-entry, MDL rundown/unmap specified | PASS after revision |
| Userspace owned snapshots and config/format/pagefile TOCTOU boundaries specified | PASS after revision |
| Privilege, ACL, no pointer/payload logging, balanced lifetime, device-gone behavior | PASS |
| Atomicity frontier and rollback split across userspace, driver, and host/persistent state | PASS after revision |
| Forward-only behavior for active pagefile, partial teardown, and ambiguous crash | PASS after revision |
| Kahneman critical questions have executable evidence and numeric/observable aborts | PASS after revision |
| Day-0 single CUDA product path; C# RAM backend remains explicit VM-only instrument | PASS |
| Named test matrix and >=80%/N/A rationale; Windows-native WDK/SDV/Verifier/live gates | PASS after revision |
| Physical-host safety: storage-only, bounded allocation, no pressure/kill/reset, stop after first abort | PASS |
| Living docs and environment-bound PARTIAL rule | PASS |

## Open questions

No blocking design question remains for Step 3. Implementation must stop and revise the SPEC if WDK
proves that the specified `EX_RUNDOWN_REF` IRQL usage, StorPort completion ordering, volume-lock/WMI
combination, or VPD page behavior differs on the pinned target toolchain/build. In particular:

- A CUDA call that remains stuck past 5 s has no in-process safe cancellation in this design. That is
  an explicit fail-safe limitation and physical-host abort, not permission to kill the process.
- Broker protocol v1 has no release ACK. Closure is empirically proven by the existing broker log;
  absence of that log keeps IMPL PARTIAL.
- Windows/WDK/SCM/Verifier/GPU evidence remains environment-bound. Linux unit or cross-build success
  cannot close those rows.

## Verdict

Initial review: **`no-go`**.

After the mandatory in-place SPEC corrections above: **`go`** for Step 3 implementation in ITEM
order. This verdict becomes `no-go` again if implementation introduces an ABI/broker frame change,
RAM/file product fallback, forced cancellation of synchronous CUDA, teardown without both pagefile
gates plus volume lock, or recovery replay from JSONL.
