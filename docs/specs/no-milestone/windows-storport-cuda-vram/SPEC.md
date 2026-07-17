# SPEC — Windows StorPort I/O backed by CUDA VRAM

> Revised in place after the 2026-07-15 Step 2.5 `no-go`: shared-ring memory ordering/rundown,
> pagefile teardown TOCTOU, ambiguous crash recovery, broker release proof, and synchronous-CUDA
> timeout semantics are now closed. Re-audit: `go`.

## Closed scope

**In now:** complete only the storage-only product path described by
`docs/specs/no-milestone/windows-storport-cuda-vram/PRD.md`: one
`ramshared-winsvc` process acquires one broker lease, creates one CUDA context and one contiguous
`DeviceMem`, exposes one ABI-v1 StorPort LUN, serves synchronous READ/WRITE/FLUSH requests through
`VramBackend<DeviceMem>`, records attributable evidence, and tears the path down without a secondary
pagefile. The existing ABI remains version 1. Small miniport corrections are in scope because code
inspection found that `QRegister`, `QCommitAndFetch`, `VdCreate`, and `VdHandleInquiry` do not yet
enforce every frozen-ABI reserved/owner/identity invariant required by RF-2 and NFR-6.

**Out now:** creating or activating a secondary pagefile; pagefile pressure or pagefile-hot kill;
surprise removal; CUDA reset injection on a physical host; sparse VRAM; multiple CUDA devices per
runtime; multiple queues; asynchronous CUDA copies; ABI or broker-protocol changes; automatic
recovery from device loss; Linux/WSL cascade changes; performance promotion; 72-hour soak;
attestation/Partner Center. `scripts/windows/WinDriveBackend.cs`,
`scripts/windows/Start-RamSharedLab.ps1`, and `scripts/windows/Stop-RamSharedLab.ps1` remain explicit
VM instruments, never a product fallback.

**Assumed ready:** `ramshared_cuda::{Cuda, Context, DeviceMem}` in
`crates/ramshared-cuda/src/driver.rs`; `VramBackend<M>` in
`crates/ramshared-block/src/vram_backend.rs`; `BlockBackend` in
`crates/ramshared-block/src/request.rs`; `BrokerTenant::{register, acquire, release}` in
`crates/ramshared-winsvc/src/broker_tenant.rs`; ABI-v1 layouts in
`drivers/windows/ramshared/protocol.h` and `crates/ramshared-winsvc/src/proto.rs`; miniport entrypoints
`CtlDispatchDeviceControl`, `QRegister`, `QCommitAndFetch`, `QTeardownOnCrash`, `VdActivate`, and
`VdDeactivate`; ADR `docs/decisions/ADR-0006-storport-virtual-miniport.md`. Step 2.5 is mandatory
before implementation because this slice crosses Ring 0/3, MDL ownership, privilege, and teardown.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-1, ITEM-2, ITEM-4 |
| RF-2 | ITEM-3, ITEM-4 |
| RF-3 | ITEM-1, ITEM-2, ITEM-4 |
| RF-4 | ITEM-1, ITEM-5, ITEM-6 |
| RF-5 | ITEM-4, ITEM-5 |
| RF-6 | ITEM-4, ITEM-5 |
| NFR-1 | ITEM-1, ITEM-5, ITEM-6 |
| NFR-2 | ITEM-3, ITEM-4, ITEM-5 |
| NFR-3 | ITEM-1, ITEM-4, ITEM-6 |
| NFR-4 | ITEM-6 |
| NFR-5 | ITEM-2, ITEM-3, ITEM-7 |
| NFR-6 | ITEM-1, ITEM-3, ITEM-4, ITEM-5 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | The only product entrypoints are `ramshared-winsvc probe-cuda --config <absolute-path>`, `ramshared-winsvc console --config <absolute-path> --storage-only`, SCM default mode, and `install\|uninstall`. SCM reads exactly `C:\ProgramData\RamShared\winsvc.toml`; console/probe reject a relative or reparse-point config path. The file is opened once with reparse traversal disabled, read into an owned byte buffer capped at 64 KiB, then parsed; it is never reopened by field. Installer ACLs make `SYSTEM` and Builtin Administrators the only writers; Users may read. Evidence directories allow writes only to `SYSTEM` and Builtin Administrators. | Closes CLI spelling and the userspace TOCTOU boundary. A fixed, write-restricted SCM path is auditable and prevents current-directory DLL/config substitution. |
| DT-2 | Extend `WinDriveConfig` with `cuda_device: u32`, `reserve_bytes: u64`, `queue_depth: u32`, `max_io_bytes: u32`, `evidence_path: PathBuf`, and `volume_letter: char`; remove pagefile activation from this runtime mode. Validation requires `size_bytes >=64 MiB`, `usize`-representable, and block-aligned; QD power-of-two in `1..=256`; max I/O non-zero, block-aligned, `<=1 MiB`; checked `queue_depth * max_io_bytes <=4 MiB`; absolute evidence path; drive letter `D..Z`; and no `backend` field at all (`deny_unknown_fields`). Effective reserve is `max(config.reserve_bytes, 512 MiB, ceil(total_vram/10))`. | The 64 MiB floor makes the three 4 KiB probe positions and the required GPT/NTFS acceptance meaningful. Other checks prevent overflow/oversubscription and make RAM/file selection structurally impossible. |
| DT-3 | `probe-cuda` and product runtime call `Cuda::load()` with the built-in `nvcuda.dll` candidate only, require `cuda_device < Cuda::device_count()`, create the context on the CUDA-owning thread, call `Context::mem_info()`, require `free >= size + effective_reserve` with checked addition, allocate exactly `size_bytes`, and zero before exposure. Probe writes distinct deterministic 4 KiB patterns at offsets `0`, `align_down(size/2, 4096)`, and `size-4096`, reads each into newly owned host buffers, zeroes, frees, and rechecks capacity. | Allocation failure occurs before CREATE_DISK; three separated patterns refute a loader-only or single-offset false green. No untrusted DLL path exists. |
| DT-4 | Add a Windows-only `WindowsDriverLink` that owns one `FILE_FLAG_OVERLAPPED` control handle plus `VirtualAlloc` page-aligned SQ, CQ, and `queue_depth * max_io_bytes` data regions. One `OVERLAPPED` belongs to the single pending COMMIT_AND_FETCH; timeout/cancel uses `CancelIoEx` and `GetOverlappedResult`, never a second fetch. Refactor `DriverLink` over a safe `QueueAccess` trait; `InMemoryQueue` remains hermetic and `WindowsMappedQueue` is the only unsafe implementation. Shared headers are aligned 32-bit words. Rust publishes with `AtomicU32::store(Ordering::Release)` and observes with `load(Ordering::Acquire)`; the driver uses aligned volatile index access plus `KeMemoryBarrier()` before publishing and immediately after observing the peer tail. Each consumer copies exactly one SQE/CQE and its applicable payload into owned locals after the acquire barrier, then publishes head with release-equivalent ordering. The CQ mirrors the SQ. No shared entry or payload is reread after its snapshot. `run_windows_product` keeps `Cuda -> Context -> DeviceMem -> VramBackend<DeviceMem>` as stack locals on one thread, avoiding a self-referential owner and preserving CUDA affinity. | The present `QueueMap` is non-contiguous and has no cross-process memory-order contract. `VirtualAlloc`, one overlapped fetch, cross-language acquire/release publication, and owned snapshots close layout, cancellation, visibility, and host-buffer TOCTOU without changing ABI v1. |
| DT-5 | ABI v1 numbers and layouts do not change. `QRegister` validates descriptor sizes/ranges before MDL work, maps all regions, then validates ring magic/entry count and zero initial indices before setting `Registered`; any post-map failure fully unwinds. It also rejects non-zero `RAMSHARED_REGISTER.reserved` and `disk_id != 0`. On every submit/fetch, both sides revalidate magic, entries, wrapping `tail.wrapping_sub(head) <= queue_depth`, slot/tag, opcode/status, and zero reserved/flags; corruption transitions the queue to Failed, completes affected/inflight SRBs once with error, and accepts no further work. `VdCreate` rejects non-zero disk params reserved; zero-input IOCTLs reject non-zero input length. `VIRTUAL_DISK` and `RAMSHARED_QUEUE` gain referenced `PEPROCESS OwnerProcess`: CREATE binds the disk owner, REGISTER must match it and takes its own balanced reference, disk/queue IOCTLs require the corresponding owner, non-owner CLEANUP cannot tear either down, and each destroy/unregister/crash unwind dereferences exactly once. `VdHandleInquiry` serves VPD page `0x80` from the existing 16-byte serial. | Descriptor fields can be validated before mapping, but shared ring contents cannot; this explicitly separates those stages and treats mapped userspace memory as mutable/untrusted for the full lifetime. Owner binding and VPD identity close the other frozen-contract defects. |
| DT-6 | Driver lock order is `I/O cancel spin lock -> RAMSHARED_QUEUE.Lock`; no path acquires the cancel spin lock while holding `RAMSHARED_QUEUE.Lock`. `RAMSHARED_QUEUE.Lock` protects indices, inflight state, `PendedFetch`, and registration state only. Each inflight slot is `Free -> Reserved -> Submitted -> Completing -> Free`; a slot remains `Completing` until READ copy and `StorPortNotification(RequestComplete)` finish, so re-entry cannot reuse its data. `EX_RUNDOWN_REF IoRundown` protects every access to mapped rings/data outside the lock. Teardown first marks Closing under the lock, rejects new rundown acquisitions, cancels the pending fetch, waits for rundown release at PASSIVE_LEVEL, snapshots remaining SRBs, then completes/unmaps outside the lock. `MmProbeAndLockPages`, MDL allocation/map/unmap, event/process references, `StorPortGetSystemAddress`, `IoCompleteRequest`, and `StorPortNotification` occur outside the lock. IOCTL registration/teardown are PASSIVE-only; submit/CQ bookkeeping may run at DISPATCH_LEVEL, use preallocated nonpaged memory, and never wait or allocate. | Completion outside the spinlock alone is unsafe: without slot states and rundown, re-entry can overwrite a READ slot or teardown can unmap it during the copy. This closes the lifetime race and gives SDV/Verifier an exact contract. |
| DT-7 | Runtime phases are `Stopped -> Leased -> CudaReady -> DiskCreated -> QueueRegistered -> Online -> Stopping`, plus `FailedSafe`. Startup failure unwinds only acquired phases in strict reverse order. Deterministic config, ABI, CUDA, identity, and checksum errors are never retried. The only allowed retry is one `ERROR_BUSY`/`STATUS_DEVICE_BUSY` enumeration observation for at most 5 s after CREATE/DESTROY; it re-queries state and does not repeat the IOCTL. Runtime state in the live process, not JSONL, is authoritative. A process crash or ambiguous driver state forbids automatic resume/replay; storage-only recovery is reboot/unload after independent no-pagefile evidence. | Enumerates retry and crash policy and prevents duplicate CREATE/REGISTER effects or reconstructing authoritative state from an incomplete log. |
| DT-8 | Teardown has two authoritative gates. Before Gate A, a bounded read-only observation binds the configured drive letter to exactly one disk whose vendor/product, VPD serial, and `Get-Disk.Size` match the live CREATE target; CREATE-time shape alone is never identity. Product stop does not depend on opening `\\.\PhysicalDriveN` for a length IOCTL because Windows can deny that handle during volume teardown; the separate VPD/IOCTL harness remains the external capacity oracle for StorPort identity evidence. Gate A and Gate B each require two successful sources: configured `PagingFiles` and actual `Win32_PageFileUsage`; the union is filtered by the product letter, and missing values, parse errors, source errors, or timeouts fail closed. Read-only observations run while the product I/O pump remains available. If Gate A is clear, runtime opens and exclusively locks the proven volume while continuing to drain miniport I/O. Gate B repeats both pagefile sources while holding the lock. Ordinary identity/pagefile/lock refusal before the deadline releases any lock and resumes Online without UNREGISTER/DESTROY/free/release. When Gate B is clear, stop new fetches, call `FlushFileBuffers` and `FSCTL_DISMOUNT_VOLUME`; only then, while retaining the locked handle, may runtime unregister and destroy. It then closes the volume, zeroes/drops VRAM, confirms free restoration, sends and flushes `LeaseRelease`, and closes the broker session. Because protocol v1 has no release ACK, the drill waits up to 5 s for broker log `lease <id> liberado` correlated to the granted ID; missing proof makes the close PARTIAL/ABORT, never a release claim. | Registry configuration can diverge from a dynamically created or pending-removal active pagefile, while WMI alone misses configured next-boot state. Their fail-closed union plus an exclusive lock and second observation closes both omissions. Exact letter-to-disk revalidation prevents dismounting a foreign remap. |
| DT-9 | Stop replay is target-state idempotent within the same live process: after successful teardown, a second stop emits `idempotent=true` and performs zero additional UNREGISTER, DESTROY, wipe, or release. Evidence is diagnostic, never a recovery cursor. A partial destructive teardown may continue only in the same process from its in-memory phase. After process crash/unknown state, no effect is replayed: disable restart, retain artifacts, and require reboot/driver-unload recovery after independent pagefile-absent proof. Never recreate the disk or release the lease merely to make rollback look clean. | Append-after-effect has an unavoidable crash window. Treating JSONL as authoritative could repeat an effect and double-complete or release another generation. |
| DT-10 | Evidence schema is append-only JSONL at the validated path. `RuntimeEvidence` includes `schema=1`, run ID, UTC timestamp, mode, state/phase, `backend="cuda"`, PID, executable SHA-256, build ID, OS/driver/GPU identity, CUDA ordinal/name, requested/allocated/free/reserve bytes, lease ID/bytes, LUN number/vendor/product/serial/size, queue parameters, counters, stable error class/code, and duration. It never includes pointers, payloads, config text, or tenant credentials. Event Log receives lifecycle/refusal/fatal summaries only. | Makes the product binary, CUDA allocation, queue, and LUN attributable without leaking KASLR/user addresses or data. |
| DT-11 | LUN identity is the conjunction `vendor == "RAMSHARE"`, `product == "VRAMDISK"`, VPD serial equals the 16 uppercase hexadecimal bytes generated from the run ID, and size equals configured bytes. `Format-RamSharedLun.ps1` accepts expected serial and requires all fields; size-only fallback is deleted and `-Force` bypasses prompting only. Immediately before `Initialize-Disk`, the script re-reads the same disk number and revalidates all fields and the requested drive letter. | Friendly-name OR size is insufficient and creates a format TOCTOU/data-loss path. The serial binds the OS disk to this runtime generation. |
| DT-12 | The CUDA-owning thread records start/end for every synchronous READ/WRITE/FLUSH; a supervisor watchdog marks health false after 5,000 ms without progress and requests no new fetch. `CancelIoEx` bounds an empty pending COMMIT_AND_FETCH, but `cuMemcpy*` itself is not cancellable: the watchdog must not destroy context, terminate the worker/process, post a speculative CQE, or claim clean recovery while that call is outstanding. If the CUDA call returns, its request and safely drainable requests complete once with error and state becomes `FailedSafe`; if it remains stuck, preserve disk/allocation/lease and require supervised reboot recovery. No automatic restart is configured. The >5,000 ms observation is an immediate campaign abort even though safe recovery may exceed 5 s. | The PRD's 5 s gate cannot be honestly implemented as cancellation over synchronous CUDA Driver API calls. Separating detection from unsafe cancellation preserves state and prevents double completion or pagefile-hot destroy. |
| DT-13 | The storage-only campaign is three fresh start/format/write-flush-read/stop rounds, each using at most `min(512 MiB, floor(total_vram/10))`. It records MiB/s and nearest-rank p50/p95/p99 request latency but defines no promotion threshold. The verdict requires Online, package↔loaded `BINARY_MATCH`, all SHA rounds, console exit 0, no force-kill, correlated lease release, CUDA free restoration within 64 MiB, no new dump/BugCheck, exact terminal VM/GPU state, and teardown <=30 s. `STOP_OK` means that complete conjunction, not merely process exit. Any failed term aborts before another physical-host round. | Prevents a process crash, forced exit, missing release, or incomplete cleanup from being summarized as a graceful-stop pass. |

## Atomicity and rollback

- **Atomicity frontier:** one SQE is accepted from an owned snapshot and produces exactly one CQE/SRB
  completion, success or stable error. REGISTER is all-or-nothing: all three regions plus owner
  reference are validated/mapped or fully unwound. Lease grant, CUDA allocation, disk creation, queue
  registration, filesystem format, and teardown are separate effects; no cross-layer transaction is
  claimed. Evidence is appended after each completed effect but is diagnostic only; in-memory state
  is authoritative and ambiguous post-crash state is never replayed.
- **Userspace/daemon rollback:** before `Online`, unwind `REGISTER -> DESTROY -> zero/free ->
  LeaseRelease`. After `Online`, accept no new work, drain, then the same reverse sequence. Replace
  the installed service with disabled state; never switch to `WinDriveBackend.cs`, RAM, or file.
- **Windows driver rollback:** ABI stays v1. Unregister only the owning process's mappings, complete
  inflight SRBs outside `RAMSHARED_QUEUE.Lock`, destroy the storage-only LUN, then uninstall the
  test-signed package only after `Get-Disk` proves the LUN absent. A Driver Verifier/SDV failure or
  new BugCheck blocks driver promotion.
- **Host/persistent rollback:** no pagefile is created. NTFS content is disposable test data; lease,
  VRAM allocation, and LUN must be absent after clean close. JSONL/Verifier/dump artifacts are
  retained. Restore `C:`-only/no-RamShared-disk state.
- **Forward-only:** if a RamShared pagefile is observed, or destructive teardown has begun, do not
  force-kill, unregister, destroy, free, disconnect the broker, or reinstall/recreate. Refuse with
  code 7. Continue an in-progress reverse phase only in the same process from live state; after a
  crash, require independent pagefile proof plus reboot/driver-unload recovery. Any one PRD numeric
  rollback trigger disables the service and ends the physical-host campaign; it does not authorize
  unsafe cleanup.

## Kahneman map (critical only)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-1 config/evidence boundary | #13 — Illusion of validity | Do invalid/hostile configs fail while the legitimate fixed-path config still parses once? | `cargo test -p ramshared-winsvc config::tests::reject_queue_data_area_over_4mib && cargo test -p ramshared-winsvc config::tests::parse_product_config` | Refusal lacks a paired legitimate pass, or unknown `backend` is accepted. |
| ITEM-2 CUDA-only gate | #16 — Fail-safe default; #9 — measurable criterion | Does a bounded real allocation, not DLL presence, change free capacity and round-trip three offsets? | `ramshared-winsvc probe-cuda --config C:\ProgramData\RamShared\winsvc.toml`; zero mismatches; free restored within 64 MiB | Missing symbol/device, mismatch, allocation delta <95%, >5 s op, or restoration outside 64 MiB. |
| ITEM-3 Ring 0/3 boundary | #13 — Illusion of validity; #5 — Availability | Are malformed IOCTLs, foreign owner, corrupted indices, completion re-entry, and teardown during copy contained while a legitimate queue still performs I/O? | `.\scripts\windows\Invoke-WinDriveIoctlValidation.ps1 -Driver ramshared.sys -Verifier`; `PASS_VALID_QUEUE=1`, every named refusal/rundown verdict=1, zero new dump | BugCheck, SDV defect without waiver, owner bypass, slot reuse/double completion, unmap-before-rundown, or refusal without legitimate pass. |
| ITEM-4 runtime/unwind | #15 — Calibrated retry; #16 — Fail-safe; #17 — Replay idempotency | Does every injected phase failure unwind once without fallback, does stop 2x have one destructive effect, and is ambiguous crash state never replayed? | `cargo test -p ramshared-winsvc runtime::tests::failure_after_register_unwinds_reverse && cargo test -p ramshared-winsvc runtime::tests::stop_twice_has_one_effect && cargo test -p ramshared-winsvc runtime::tests::ambiguous_crash_state_is_not_replayed` | Blind retry, fallback selection, leaked lease/map/allocation, duplicate destroy/release, or JSONL-driven recovery. |
| ITEM-5 DT-9 stop | #13 — Illusion of validity; #2 — Counterfactual | Do both pagefile gates and the volume lock prevent destructive teardown while the clean paired path still succeeds? | `cargo test -p ramshared-winsvc service::tests::pagefile_active_refuses_before_mutation && cargo test -p ramshared-winsvc service::tests::gate_b_failure_resumes_online_before_destroy && cargo test -p ramshared-winsvc service::tests::pagefile_absent_tears_down_cleanly`; VM WMI/volume-lock drill | Query ambiguity, lock failure, or pagefile presence reaches UNREGISTER/DESTROY/free/release; clean paired path fails. |
| ITEM-6 physical storage-only proof | #13 — Illusion of validity; #3 — Number, not adjective | Is the live LUN attributable to this Rust binary and CUDA allocation, with identical SHA-256 over three rounds? | `scripts/windows/Invoke-CudaStorageDrill.ps1 -Config C:\ProgramData\RamShared\winsvc.toml -Rounds 3 -StorageOnly`; `VERDICT=PASS` | Any PRD numeric rollback trigger, product/lab identity ambiguity, pagefile appearance, or missing before/action/after evidence. |

## Security checklist (pre-impl)

- [ ] Privilege: `RamsharedSddl` remains `D:P(A;;GA;;;SY)(A;;GA;;;BA)`; service is LocalSystem;
  console/probe/install require an elevated token; non-owner queue IOCTLs are refused.
- [ ] User/host copy: config <=64 KiB is opened/read once; SQE and WRITE payload use owned snapshots;
  QD/max-I/O/data length, pointer ranges, arithmetic, alignment, and 4 MiB map cap are checked before
  MDL mapping or CUDA access.
- [ ] Flags/IOCTL codes: unknown IOCTL/opcode, non-zero `flags`/`reserved`, ABI mismatch, unexpected
  input length, invalid ring header/index/tag/slot, and duplicate owner are rejected.
- [ ] Info-leak: JSONL, Event Log, NTSTATUS, and CUDA errors contain no kernel/user addresses or
  payload/config data.
- [ ] IRQ/atomic or IRQL: DT-6 lock order is enforced; no wait/allocation/map/completion under
  `RAMSHARED_QUEUE.Lock`; PASSIVE-only setup/teardown; nonpaged bounded work at DISPATCH_LEVEL.
- [ ] Lifetime: control handle, `PEPROCESS`, MDLs, events, queue, disk, VRAM, context/library, and lease
  are released exactly once in reverse order; every partial startup has an injected unwind test.
- [ ] Hot-unplug / device-gone: CUDA device loss maps to stable class/code and `FailedSafe`; no UAF,
  automatic DESTROY, health claim, or physical-host reset injection.
- [ ] Host safety: no live WSL2 pressure; malformed/Verifier/device-gone drills run only in checkpointed
  VM; physical GPU run is supervised storage-only, <=512 MiB and <=10% total VRAM.
- [ ] Replayable ops: stop 2x and release 2x produce one effect; CREATE/REGISTER are not blindly
  retried; evidence rows use unique run/event IDs.

## Files to CREATE / MODIFY / DELETE

### CREATE

**`crates/ramshared-winsvc/src/runtime.rs`**
- Purpose: pure phase machine, `RuntimeOps` injection seam, reverse unwind, stable exit/error classes,
  no-fallback policy, and idempotent stop.
- RF / DT: RF-1, RF-3, RF-5, RF-6; DT-7, DT-9, DT-12.
- Types / fns: `RuntimePhase`, `RunMode`, `RuntimeErrorClass`, `RuntimeError`, `RuntimeSummary`,
  `trait RuntimeOps`, `run_runtime<O: RuntimeOps>(cfg: &WinDriveConfig, ops: &mut O)`, and
  `stop_runtime<O: RuntimeOps>(state: &mut RuntimeState, ops: &mut O)`.
- Reference pattern in this repo: `ServiceState`, `provision_after_lease`, and `teardown` in
  `crates/ramshared-winsvc/src/service.rs`.
- Required tests: `crates/ramshared-winsvc/src/runtime.rs` :: `no_fallback_after_cuda_failure`,
  `failure_after_lease_releases_once`, `failure_after_cuda_frees_before_release`,
  `failure_after_create_destroys_before_free`, `failure_after_register_unwinds_reverse`,
  `deterministic_failure_is_not_retried`, `busy_observation_is_bounded`,
  `stop_twice_has_one_effect`, `ambiguous_crash_state_is_not_replayed`,
  `cuda_watchdog_does_not_destroy_stuck_context` (#15/#16/#17).
- Cover target: >=80%.
- Kahneman: ITEM-4 row.

**`crates/ramshared-winsvc/src/windows_driver.rs`**
- Purpose: Windows control-handle/IOCTL adapter and contiguous page-aligned mapped queue; isolate all
  Windows unsafe code and expose safe `QueueAccess`/`DiskControl` behavior.
- RF / DT: RF-2; DT-4, DT-5, DT-6.
- Types / fns: `WindowsDriverLink`, `WindowsMappedQueue`, `IoctlError`,
  `WindowsDriverLink::open()`, `create_disk(&DiskParams)`, `register_queue()`,
  `commit_and_fetch(Duration)`, `cancel_fetch()`, `unregister_queue()`, `destroy_disk()`, and
  `WindowsMappedQueue::registration(disk_id) -> Register`.
- Reference pattern in this repo: IOCTL structs/codes in `scripts/windows/WinDriveBackend.cs` and
  `crates/ramshared-winsvc/src/proto.rs`; do not reuse its RAM backend.
- Required tests: `scripts/windows/Invoke-WinDriveIoctlValidation.ps1` ::
  `PASS_VALID_QUEUE`, `REFUSE_FOREIGN_OWNER`, `REFUSE_RESERVED_REGISTER`, `REFUSE_BAD_RING`,
  `REFUSE_RING_INDEX_JUMP`, `REFUSE_RESERVED_CQE`, `REFUSE_UNKNOWN_IOCTL`,
  `COMPLETION_REENTRY_NO_SLOT_REUSE`, `RUNDOWN_UNMAP_AFTER_COPY` (#13).
- Cover target: N/A — E2E-only; Windows handle/MDL behavior requires WDK driver plus Verifier.
- Kahneman: ITEM-3 row.

**`crates/ramshared-winsvc/src/evidence.rs`**
- Purpose: schema-1 evidence structs, stable event/error names, append-only writer, latency histogram
  summarization, and redaction boundary.
- RF / DT: RF-4; DT-10, DT-13; NFR-3/NFR-4.
- Types / fns: `RuntimeEvidence`, `IoCounters`, `LatencySummary`, `EvidenceWriter::open`,
  `EvidenceWriter::append`, `nearest_rank_percentile`, and `redacted_error`.
- Reference pattern in this repo: append-only JSONL in `docs/benchmarks/results.jsonl`.
- Required tests: `crates/ramshared-winsvc/src/evidence.rs` :: `append_preserves_prior_rows`,
  `schema_has_no_pointer_or_payload_fields`,
  `nearest_rank_percentiles_are_deterministic`, `stable_error_redacts_payload`.
- Cover target: >=80%.

**`crates/ramshared-winsvc/src/windows_host.rs`**
- Purpose: elevated-token check, single-open/reparse-safe config read, executable hash, Event Log summary,
  WMI pagefile query, LUN/VPD identity query, and bounded OS waits.
- RF / DT: RF-4, RF-5, RF-6; DT-1, DT-8, DT-10, DT-11.
- Types / fns: `WindowsHostState`, `LunIdentity`, `PagefileIdentity`, `LockedVolume`, `HostError`,
  `read_owned_config`, `is_elevated`, `active_pagefiles`, `lock_volume`, `find_lun`, `binary_sha256`,
  and `emit_event`.
- Reference pattern in this repo: authoritative `Win32_PageFileUsage` checks in
  `scripts/windows/Stop-RamSharedLab.ps1` and identity guards in
  `scripts/windows/Format-RamSharedLun.ps1`.
- Required tests: `crates/ramshared-winsvc/src/windows_host.rs` under Windows ::
  `relative_config_is_rejected`, `reparse_config_is_rejected`,
  `pagefile_query_matches_canonical_volume`, `pagefile_query_error_is_unsafe`,
  `exclusive_volume_lock_closes_pagefile_race`,
  `lun_identity_requires_vendor_product_serial_and_size`.
- Cover target: N/A — E2E-only; COM/WMI, token, VPD, ACL, and Event Log are Windows integration.
- Kahneman: ITEM-5 row for pagefile query.

**`crates/ramshared-winsvc/winsvc.example.toml`**
- Purpose: documented storage-only product configuration with 512 MiB allocation, 512 MiB reserve,
  QD 4, max I/O 1 MiB, fixed ProgramData evidence path, and no backend/pagefile selector.
- RF / DT: RF-1, RF-3, RF-6; DT-1, DT-2.
- Types / fns: N/A — configuration fixture.
- Reference pattern in this repo: `[win_drive]` parser fixture in
  `crates/ramshared-winsvc/src/config.rs`.
- Required tests: `crates/ramshared-winsvc/src/config.rs` :: `example_config_parses`.
- Cover target: N/A — boilerplate; parsed verbatim by the named test.

**`scripts/windows/Install-RamSharedLabService.ps1`**
- Purpose: retain the existing C# `RamSharedWinSvc.cs` installer under an explicit lab-only name after
  the product installer stops compiling/launching it.
- RF / DT: RF-6; DT-1.
- Types / fns: PowerShell entrypoint requires `-LabVm` and prints `BACKEND=ram LAB_ONLY=1`.
- Reference pattern in this repo: current body of `scripts/windows/Install-RamSharedService.ps1`.
- Required tests: script :: `-LabVm -WhatIf` emits `LAB_ONLY=1` and never installs the Rust ImagePath.
- Cover target: N/A — E2E-only; explicit VM harness.

**`scripts/windows/Invoke-WinDriveIoctlValidation.ps1`**
- Purpose: checkpointed-VM legitimate/refusal harness for ABI-v1 IOCTL, process-owner, malformed-ring,
  cleanup, and Driver Verifier evidence.
- RF / DT: RF-2; DT-5, DT-6; NFR-6.
- Types / fns: script entrypoint `-Driver`, `-Verifier`, `-ArtifactDir`; named verdict keys listed under
  `windows_driver.rs` plus `NO_NEW_DUMP=1`.
- Reference pattern in this repo: `scripts/windows/Invoke-WinDriveBackend.ps1` and
  `scripts/windows/Invoke-DriverSoak.ps1`.
- Required tests: `scripts/windows/Invoke-WinDriveIoctlValidation.ps1` :: `PASS_VALID_QUEUE` plus
  every `REFUSE_*`, `COMPLETION_REENTRY_NO_SLOT_REUSE`, and `RUNDOWN_UNMAP_AFTER_COPY` verdict (#13).
- Cover target: N/A — E2E-only; kernel boundary drill.
- Kahneman: ITEM-3 row.

**`scripts/windows/Invoke-CudaStorageDrill.ps1`**
- Purpose: supervised before/action/after orchestration for probe, Rust product start, exact LUN
  identity, guarded format, three checksum rounds, metrics, teardown, CUDA restoration, dump check,
  and JSON verdict.
- RF / DT: RF-4, RF-5, RF-6; DT-8 through DT-13; NFR-1/NFR-3/NFR-4.
- Types / fns: entrypoint `-Config`, `-Rounds 3`, mandatory `-StorageOnly`, `-ArtifactDir`, and explicit
  `-ApprovePhysicalHost`; emits `VERDICT=PASS|ABORT|PARTIAL`.
- Reference pattern in this repo: `scripts/windows/Get-WinDrivePreflight.ps1`,
  `scripts/windows/Measure-RamSharedDiskIo.ps1`, and `scripts/windows/Format-RamSharedLun.ps1`.
- Required tests: `scripts/windows/Invoke-CudaStorageDrill.ps1` ::
  `storage_only_cuda_three_rounds_sha256`, `pagefile_present_aborts_before_start`,
  `volume_lock_failure_aborts_before_destroy`, and `broker_release_is_observed` (#13/#16).
- Cover target: N/A — E2E-only; physical Windows GPU evidence.
- Kahneman: ITEM-6 row.

### MODIFY

**`crates/ramshared-winsvc/src/config.rs`** — What: make `WinDriveConfig` the closed DT-2 product
shape and add `#[serde(deny_unknown_fields)]`; how: checked validation plus `from_reader` over the one
owned config buffer; why: RF-1/RF-3 boundary and no fallback. Before -> after: pagefile-oriented
fields and minimal geometry -> CUDA/queue/evidence fields with reserve and map caps. Callers:
`runtime.rs`, `main.rs`, `windows_host.rs`. Tests: `parse_product_config`, `reject_unknown_backend`,
`reject_zero_size`, `reject_size_over_usize`, `reject_unaligned_max_io`,
`reject_queue_data_area_over_4mib`, `reserve_cannot_lower_policy_floor`, `example_config_parses`.
Cover: >=80%. Kahneman: ITEM-1.

**`crates/ramshared-winsvc/src/driver_link.rs`** — What: extract `QueueAccess`, rename current pure
storage to `InMemoryQueue`, make `DriverLink<Q>` generic, validate `Sqe.flags`, and snapshot SQE/WRITE
data before backend access; how: safe owned buffers at the trait boundary; why: RF-2, DT-4, userspace
TOCTOU. Before -> after: separate header/entry `QueueMap` usable only in-process -> pure fake plus
registrable Windows implementation. Callers: `FakeDriver`, `runtime.rs`, `windows_driver.rs`. Tests:
existing `roundtrip_write_read_flush`, `oob_returns_einval`, `reject_bad_queue_depth`; add
`unknown_flags_return_einval`, `overflow_range_returns_einval`,
`write_uses_owned_payload_snapshot`, `invalid_slot_does_not_touch_backend`. Cover: >=80%.

**`crates/ramshared-winsvc/src/service.rs`** — What: narrow `provision_after_lease`/`teardown` into
runtime helpers and replace cached pagefile removal with injected Gate-A query, exclusive volume lock,
and Gate-B query; how: delegate phase ownership to `runtime.rs`, resume Online if either gate is not
safe, and preserve paired pure tests; why: RF-5/DT-8/DT-9. Before -> after: `pagefile_active` boolean
can be cleared by callback -> two authoritative OS gates protect the destructive frontier. Callers:
`runtime.rs`. Tests: `pagefile_active_refuses_before_mutation`,
`pagefile_query_error_refuses_before_mutation`, `gate_b_failure_resumes_online_before_destroy`,
`pagefile_absent_tears_down_cleanly`, `stop_refusal_preserves_online_state`,
`clean_teardown_order_is_drain_lock_recheck_flush_dismount_unregister_destroy_unlock_wipe_release`.
Cover: >=80%.
Kahneman: ITEM-5.

**`crates/ramshared-winsvc/src/broker_tenant.rs`** — What: require granted bytes equal requested
bytes and make release generation-aware; how: retain `ReleaseSent` state through write+flush, make a
second same-process release a no-op, close the session after release, and leave broker-log correlation
to the drill because protocol v1 has no ACK; why: RF-3 and DT-8/DT-9 replay/proof honesty. Symbols:
`LeaseState`,
`BrokerTenant::wait_lease_outcome`, `BrokerTenant::release`. Tests:
`granted_bytes_must_equal_requested`, `release_flushes_before_session_close`,
`release_twice_writes_once`, existing `register_win_drive` and `lease_denied`. Cover: >=80%.

**`crates/ramshared-winsvc/src/main.rs`** — What: delete `run_start_scripts`, `run_stop_scripts`,
`START_PS1`, and `STOP_PS1` from the product binary; parse DT-1 commands and call one
`run_windows_product` from console and `run_service`; make SCM stop report code 7 without claiming
Stopped after refusal (return status to `Running`, checkpoint 0, STOP accepted, and emit the code-7
Event Log refusal); install own executable/config ACL and disable failure auto-restart; uninstall
performs safe stop first. Why: RF-6 and no false RAM green. Tests: CLI parser in `runtime.rs` ::
`console_requires_storage_only`, `product_cli_has_no_lab_backend_command`,
`scm_and_console_select_same_runtime`. Cover: >=80% for extracted parser/runtime; main SCM glue
N/A — E2E-only.

**`crates/ramshared-winsvc/src/lib.rs`** — What: export new runtime/evidence abstractions and gate
`windows_driver`/`windows_host` with `cfg(windows)`; how: preserve non-Windows stub and isolate unsafe;
why: test pure business logic on Linux while using real Windows integration. Tests:
`cargo test -p ramshared-winsvc --lib`. Cover: N/A — boilerplate module surface.

**`crates/ramshared-winsvc/Cargo.toml`** — What: add `ramshared-cuda` as a direct product dependency,
`serde_json = "1"`, and only the precise `windows-sys` features required for file mapping, IOCTL,
token, COM/WMI, Event Log, SCM, and CNG SHA-256 (`BCrypt*`); how:
Windows-only dependencies stay target-gated; why: RF-1/RF-4. No generic subprocess or alternate
backend dependency. Test: `cargo build -p ramshared-winsvc --target x86_64-pc-windows-msvc`.
Cover: N/A — manifest.

**`crates/ramshared-cuda/src/driver.rs`** — What: retain the device ordinal in `Device`, expose
`Device::ordinal()`, and add a reusable bounded three-offset probe helper without logging pointers;
how: reuse `Context::{mem_info,alloc}` and `DeviceMem::{write_at,read_at,zero}`; why: RF-1/RF-4
identity evidence, not a second CUDA wrapper. Tests: existing CUDA error tests plus Windows live
`probe_cuda_allocates_roundtrips_and_restores` through the CLI. Cover: >=80% for new pure offset and
pattern planning logic; hardware call path N/A — E2E-only.

**`drivers/windows/ramshared/queue.h`** — What: add `PEPROCESS OwnerProcess` and document DT-6 lock
order/IRQL; add inflight state enum plus `EX_RUNDOWN_REF IoRundown` and Closing/Failed state; how:
internal structs only, no protocol layout change; why: RF-2/NFR-6. Tests: `REFUSE_FOREIGN_OWNER`,
`COMPLETION_REENTRY_NO_SLOT_REUSE`, `RUNDOWN_UNMAP_AFTER_COPY`, SDV, Driver Verifier. Cover: N/A —
WDK/SDV/Verifier.

**`drivers/windows/ramshared/queue.c`** — What: modify `QRegister`, `QCommitAndFetch`,
`QTeardownOnCrash`, and `QUnlockAll` for reserved/ring/owner validation, exactly-once owner release,
per-operation ring-index validation, inflight state transitions, rundown-protected copies, and
completion outside `RAMSHARED_QUEUE.Lock`; how: reserve/snapshot under lock, hold rundown across
mapped access, perform StorPort/IRP callbacks after unlock, and wait for rundown before unmap; why:
RF-2, DT-5/DT-6. Tests: all named IOCTL validation/rundown/re-entry verdicts, SDV and Verifier.
Cover: N/A — WDK/SDV/Verifier. Kahneman: ITEM-3.

**`drivers/windows/ramshared/control.c`** — What: in `CtlDispatchDeviceControl`, validate zero-input
lengths and owner before UNREGISTER/COMMIT/DESTROY; in `CtlDispatchCleanup`, teardown only the owning
process; unknown IOCTL remains `STATUS_INVALID_DEVICE_REQUEST`; how: route checks through queue
helpers; why: RF-2/NFR-6. Tests: `REFUSE_FOREIGN_OWNER`, `REFUSE_UNKNOWN_IOCTL`, paired
`PASS_VALID_QUEUE`. Cover: N/A — WDK/SDV/Verifier.

**`drivers/windows/ramshared/virtdisk.c`** and **`drivers/windows/ramshared/virtdisk.h`** — What:
add `VIRTUAL_DISK.OwnerProcess`; make `VdCreate`/`VdActivate` bind the CREATE requestor and reject
`Params->reserved != 0`; make `VdDestroy` release that owner; change `VdHandleInquiry` to receive the
disk and serve standard inquiry plus supported VPD pages `0x00` and unit serial `0x80` from the
existing 16-byte serial; how: balanced `ObReferenceObject`/`ObDereferenceObject` plus bounded
CDB/data-length parsing at DISPATCH-safe nonpaged code; why: RF-2 ownership enforcement and RF-4
exact identity. Tests: VM `REFUSE_FOREIGN_OWNER`, `REFUSE_RESERVED_DISK_PARAMS`, `VPD_SERIAL_MATCH`,
legitimate format.
Cover: N/A — WDK/SDV/Verifier.

**`scripts/windows/Install-RamSharedService.ps1`** — What: install only a supplied MSVC-built
`ramshared-winsvc.exe`, copy/ACL `winsvc.toml`, set ImagePath to that executable, and never compile C#
or copy Start/Stop lab scripts; how: verify SHA-256 and query ImagePath after install; why: RF-6.
Before -> after: lab C# SCM installer -> Rust product installer. Test: `PRODUCT_IMAGEPATH_MATCH=1`,
`NO_LAB_SCRIPT_REFERENCE=1`. Cover: N/A — E2E-only.

**`scripts/windows/Format-RamSharedLun.ps1`** — What: require expected vendor/product/VPD serial/size,
remove identity `OR` and size-only fallback, revalidate disk number and free letter immediately before
mutation, and make `-Force` skip prompt only; how: owned CIM snapshot then second query; why: RF-4,
DT-11/data-loss prevention. Tests: `refuse_physical_same_size`, `refuse_wrong_serial`,
`force_does_not_bypass_identity`, `format_exact_ramshared_lun`. Cover: N/A — E2E-only.

**`scripts/windows/Measure-RamSharedDiskIo.ps1`** — What: replace length-only `$match` with
SHA-256 equality, deterministic run-seeded data, explicit flush, three-round input, percentile and
JSONL output; how: retain locale-safe PerfDisk lookup and add exact product PID/hash/serial fields;
why: RF-4/NFR-3/NFR-4. Tests: `checksum_mismatch_exits_6`,
`three_rounds_emit_p50_p95_p99`, `matching_checksum_exits_0`. Cover: N/A — E2E-only.

**`scripts/windows/Get-WinDrivePreflight.ps1`** — What: add explicit storage-only authorization,
no-active-RamShared-pagefile/disk/backend checks, product binary/config hash, test-signing/driver
package state, CUDA probe prerequisites, latest dump identity, and finite timeout reporting; how:
read-only queries only; why: NFR-1 and DT-13. Test: drill's `PREFLIGHT_STORAGE_ONLY=PASS` and refusal
when a target pagefile exists. Cover: N/A — E2E-only.

**`drivers/windows/README.md`**, **`drivers/windows/ramshared/README.md`**, and
**`scripts/windows/README.md`** — What: separate Rust CUDA product commands from explicit C# RAM VM
harness, document storage-only bounds and evidence fields; why: RF-6/operator safety. Tests:
`./scripts/docs-check.sh`. Cover: N/A — docs.

**`docs/specs/no-milestone/windows-swap-driver/SPEC.md`**,
**`docs/specs/no-milestone/windows-swap-driver/PREFLIGHT.md`**, and
**`docs/specs/no-milestone/windows-swap-driver/IMPL.md`** — What: link this focused slice and mark
only CUDA storage gates closed when evidence exists; keep pagefile/soak/attestation open; why: prevent
maturity overclaim. Test: `./scripts/docs-check.sh`. Cover: N/A — docs.

**`docs/reliability/DEGRADATION-MATRIX.md`**, **`README.md`**, **`ROADMAP.md`**,
**`ARCHITECTURE.md`**, **`docs/LIBRARIES.md`**, and **`validation.md`** — What: record CUDA
allocation/device-loss/false-backend states, product-vs-lab architecture, dependency justification,
and append actual before/action/after evidence only on close; why: NFR-2/NFR-3 and honest maturity.
Tests: `./scripts/docs-check.sh`. Cover: N/A — docs.

### DELETE

None. The lab backend remains an explicitly named VM instrument; its references are deleted only from
the product binary and product installer.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| Runtime phase and backend identity | ProgramData evidence JSONL + Windows Event Log | lifecycle; `backend=cuda` |
| CUDA device/capacity/allocation/free restoration | evidence JSONL | gauge bytes + stable CUDA code |
| Lease ID/bytes/acquire/release | evidence JSONL | lifecycle/counter; no tenant secret |
| LUN vendor/product/VPD serial/size | evidence JSONL + drill artifact | identity |
| READ/WRITE/FLUSH requests, bytes, errors, outstanding | evidence JSONL | counters/gauge |
| p50/p95/p99 and max request latency | evidence JSONL | microseconds |
| Pagefile refusal and teardown phase/duration | evidence JSONL + Event Log | safety event/milliseconds |
| Binary PID/SHA-256/build and driver package hash | evidence JSONL | artifact identity |
| Checksum mismatch and new dump/BugCheck | drill verdict JSON | hard abort counter/identity |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Alter: Rust CUDA product composition and lab-only C# boundary. |
| `docs/decisions/ADR-0006-storport-virtual-miniport.md` | Confirm; no architecture decision changes. |
| `docs/reliability/DEGRADATION-MATRIX.md` | Alter: CUDA allocation/device-loss and false-backend-identity rows. |
| `validation.md` | Append on close with before/action/after, hashes, three rounds, and remaining gates. |
| `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl` | Append only if the three-round rules are met; no P0/promotion claim. |
| `.claude/rules/*` · `CLAUDE.md` · `AGENTS.md` | N/A — no convention change. |
| `README.md` · `ROADMAP.md` · `docs/LIBRARIES.md` | Alter maturity/product-vs-lab text and dependency record. |
| `docs/specs/no-milestone/windows-swap-driver/{SPEC,PREFLIGHT,IMPL}.md` | Alter links/status without closing pagefile, soak, or signing gates. |
| `docs/INDEX.md` | Regenerate after this SPEC is added. |

## Implementation order

1. **ITEM-1 — Contracts first:** write named config, evidence, CLI, refusal/pass, and runtime failure
   tests; close DT-1/DT-2/DT-10 types and example config before hardware code.
2. **ITEM-2 — CUDA-only gate:** extend the existing wrapper minimally and implement `probe-cuda`;
   cross-build, then prove bounded allocate/pattern/zero/free without loading the driver.
3. **ITEM-3 — Ring 0/3 correctness:** implement contiguous mapped queue and the minimal miniport
   owner/reserved/VPD/lock fixes; run ABI golden tests, InfVerif, SDV, and checkpointed-VM malformed
   IOCTL plus legitimate queue tests.
4. **ITEM-4 — Product runtime:** compose broker -> reserve -> CUDA -> CREATE -> REGISTER -> I/O on one
   CUDA-affine thread; inject failure at every phase and prove reverse unwind/no fallback.
5. **ITEM-5 — SCM and teardown:** route console and SCM through the same runtime; install the Rust
   binary; implement authoritative pagefile refusal, clean stop, FailedSafe, and stop replay.
6. **ITEM-6 — Storage-only live proof:** harden identity/format/checksum scripts and run approved
   before/action/after three-round physical-GPU campaign within DT-13 bounds.
7. **ITEM-7 — Close documentation:** run coverage/docs gates, append evidence and degradation rows,
   reconcile maturity, and leave pagefile/soak/attestation explicitly open.

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-winsvc/src/config.rs` | `crates/ramshared-winsvc/src/config.rs` :: `parse_product_config`; `reject_unknown_backend`; `reject_queue_data_area_over_4mib`; `reserve_cannot_lower_policy_floor` | unit | #13 | >=80% |
| `crates/ramshared-winsvc/src/evidence.rs` | `crates/ramshared-winsvc/src/evidence.rs` :: `append_preserves_prior_rows`; `nearest_rank_percentiles_are_deterministic`; `stable_error_redacts_payload` | unit | #9 | >=80% |
| `crates/ramshared-winsvc/src/driver_link.rs` | `crates/ramshared-winsvc/src/driver_link.rs` :: `roundtrip_write_read_flush`; `unknown_flags_return_einval`; `write_uses_owned_payload_snapshot`; `invalid_slot_does_not_touch_backend` | unit | #13 | >=80% |
| `crates/ramshared-winsvc/src/broker_tenant.rs` | `crates/ramshared-winsvc/src/broker_tenant.rs` :: `granted_bytes_must_equal_requested`; `release_flushes_before_session_close`; `release_twice_writes_once`; `failed_release_retains_lease_and_is_not_replayed`; `lease_denied` | unit | #13/#17 | >=80% |
| `crates/ramshared-winsvc/src/runtime.rs` | `crates/ramshared-winsvc/src/runtime.rs` :: `no_fallback_after_cuda_failure`; `failure_after_register_unwinds_reverse`; `deterministic_failure_is_not_retried`; `stop_twice_has_one_effect`; `ambiguous_crash_state_is_not_replayed`; `cuda_watchdog_does_not_destroy_stuck_context` | unit | #15/#16/#17 | >=80% |
| `crates/ramshared-winsvc/src/service.rs` | `crates/ramshared-winsvc/src/service.rs` :: `pagefile_active_refuses_before_mutation`; `pagefile_query_error_refuses_before_mutation`; `gate_b_failure_resumes_online_before_destroy`; `pagefile_absent_tears_down_cleanly`; `stop_refusal_preserves_online_state` | unit | #13/#17 | >=80% |
| `crates/ramshared-winsvc/src/host_safety.rs` | `crates/ramshared-winsvc/src/host_safety.rs` :: `pagefile_sources_are_unioned`; `either_pagefile_source_error_fails_closed`; `wildcard_or_ambiguous_pagefile_path_is_unsafe`; `lock_deadline_never_resumes_online`; `complete_campaign_verdict_requires_every_safety_term` | unit | #13/#16 | >=80% |
| `crates/ramshared-block/src/vram_backend.rs` | `crates/ramshared-block/src/vram_backend.rs` :: `vram_backend_into_inner_allows_explicit_release_order` | unit | #16/#17 | >=80% |
| `crates/ramshared-cuda/src/driver.rs` | `ramshared-winsvc probe-cuda` :: `probe_cuda_allocates_roundtrips_and_restores` | integration | #9/#16 | N/A — E2E-only; real `nvcuda.dll`/GPU |
| `drivers/windows/ramshared/control.c`<br>`drivers/windows/ramshared/queue.c`<br>`drivers/windows/ramshared/virtdisk.c` | `scripts/windows/Invoke-WinDriveIoctlValidation.ps1` :: `PASS_VALID_QUEUE`; all named `REFUSE_*`; `COMPLETION_REENTRY_NO_SLOT_REUSE`; `RUNDOWN_UNMAP_AFTER_COPY`; `VPD_SERIAL_MATCH` | WDK/SDV/Verifier | #5/#13 | N/A — WDK/Verifier |
| `crates/ramshared-winsvc/src/windows_host.rs` | `crates/ramshared-winsvc/src/windows_host.rs` :: `pagefile_query_matches_canonical_volume`; `pagefile_query_error_is_unsafe`; `exclusive_volume_lock_closes_pagefile_race`; `lun_identity_requires_vendor_product_serial_and_size` | integration | #13 | N/A — E2E-only; Windows COM/VPD |
| `crates/ramshared-winsvc/src/main.rs` | `scripts/windows/Install-RamSharedService.ps1` :: `PRODUCT_IMAGEPATH_MATCH`; `NO_LAB_SCRIPT_REFERENCE` | drill/E2E | #13/#18 | N/A — E2E-only; SCM |
| `scripts/windows/Format-RamSharedLun.ps1` | `scripts/windows/Format-RamSharedLun.ps1` :: `refuse_physical_same_size`; `refuse_wrong_serial`; `force_does_not_bypass_identity`; `format_exact_ramshared_lun` | drill/E2E | #13 | N/A — E2E-only; guarded disk mutation |
| `scripts/windows/Measure-RamSharedDiskIo.ps1` | `scripts/windows/Measure-RamSharedDiskIo.ps1` :: `checksum_mismatch_exits_6`; `three_rounds_emit_p50_p95_p99`; `matching_checksum_exits_0` | drill/E2E | #3/#13 | N/A — E2E-only; live filesystem I/O |
| `crates/ramshared-winsvc/src/main.rs` + Windows product surface | `scripts/windows/Invoke-CudaStorageDrill.ps1` :: `storage_only_cuda_three_rounds_sha256`; `pagefile_present_aborts_before_start`; `volume_lock_failure_aborts_before_destroy`; `broker_release_is_observed` | drill/E2E | #3/#13/#16 | N/A — E2E-only; physical Windows GPU |
| `scripts/windows/Run-GuestProductOnline.ps1` | `scripts/windows/Test-GuestProductOnlineStatic.ps1` :: `verdict_requires_complete_graceful_stop`; `pre_stop_probe_is_absent`; `three_fresh_rounds_are_required` | static + isolated drill | #3/#13/#16 | N/A — harness |
| `scripts/windows/Run-GuestExhaustive.ps1` | `scripts/windows/Test-GuestExhaustiveStatic.ps1` :: stale DriverStore purge; post-reboot root recreate; PnP/SCSI ready before IOCTL; bounded load timeout | static + isolated drill | #3/#13/#16 | N/A — harness |

## Validation checklist

- [x] `cargo fmt --all -- --check`
- [x] `cargo clippy -p ramshared-cuda -p ramshared-block -p ramshared-winsvc --all-targets -- -D warnings`
- [x] `cargo test -p ramshared-cuda -p ramshared-block -p ramshared-winsvc`
- [x] `cargo build -p ramshared-winsvc --target x86_64-pc-windows-msvc`
- [x] `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-winsvc --files crates/ramshared-winsvc/src/config.rs,crates/ramshared-winsvc/src/evidence.rs,crates/ramshared-winsvc/src/driver_link.rs,crates/ramshared-winsvc/src/broker_tenant.rs,crates/ramshared-winsvc/src/runtime.rs,crates/ramshared-winsvc/src/service.rs --min 80`
  (also CUDA probe cover ≥80% when `crates/ramshared-cuda/src/probe.rs` is in the gate set)
- [ ] If pure planning logic changes in `crates/ramshared-cuda/src/driver.rs`, include that file in a
  separate `ramshared-cuda` cover gate at >=80%; hardware-only lines remain live-E2E evidence.
- [x] WDK Release x64 build with `/W4 /WX /wd4324 /Z7` (canonical `Build-Drivers.ps1`; UNC `/Zi`
  C1041 fixed). Evidence: `evidence/wdk-build-audit-20260716T171026Z.md`.
- [x] `InfVerif.exe /w drivers/windows/ramshared/ramshared.inf` — WDK 10.0.26100.0 exit 0 after
  DIRID 13 + build-16299 model-floor migration; evidence `evidence/infverif-dirid13-pass-20260716.md`.
- [x] WDK Code Analysis project-clean for `drivers/windows/ramshared/*.c`; WDK header analyzer
  warnings are toolchain-scope noise. Static Driver Verifier is not installed in this local WDK
  image, so no SDV PASS is claimed. Evidence:
  `evidence/code-analysis-project-clean-20260716.md`.
- [x] Checkpointed lab VM (`win11-drill`): Driver Verifier `0x2093B` for `ramshared.sys`, all
  `Invoke-WinDriveIoctlValidation.ps1` legitimate/refusal/VPD exact verdicts, CREATE/REGISTER/raw
  I/O/owner cleanup/clean teardown, zero new BugCheck/dump; package↔guest BINARY_MATCH and
  artifact hashes recorded. Current signed package campaign `guest-exhaustive-20260716-224913`.
  Evidence: `evidence/guest-exhaustive-20260716-224913.md`.
- [x] Guarded NTFS format + three-round storage SHA on the isolated GPU-PV product Online path.
  Evidence: `evidence/guest-product-online-20260716-220848.md`.
- [ ] CUDA-only physical probe: <=512 MiB and <=10% total, three-offset roundtrip, allocation delta
  >=95%, zero mismatch, free restoration within 64 MiB, no driver loaded by this action.
- [ ] Explicitly approved physical Windows storage-only live path:
  `scripts/windows/Invoke-CudaStorageDrill.ps1 -Config C:\ProgramData\RamShared\winsvc.toml -Rounds 3 -StorageOnly -ApprovePhysicalHost`;
  before -> action -> after, no secondary pagefile/pressure/reset/force-kill.
- [x] Each isolated product round proves Rust PID/executable SHA-256, `backend=cuda`, lease bytes, CUDA allocation delta,
  exact vendor/product/VPD serial/size, guarded NTFS format, identical SHA-256 after write/flush/read,
  p50/p95/p99, clean teardown <=30 s, disk/queue/lease absent, and free restored within 64 MiB.
- [ ] Physical-host campaign abort policy remains unexecuted: any request >5,000 ms, teardown
  >30,000 ms, checksum/OOB/identity violation, pagefile-active destructive attempt, new
  BugCheck/dump/Verifier finding, or free-restoration miss aborts; no retry on the physical host in
  that campaign.
- [x] Shared Linux regression gate (this slice): `cargo test -p ramshared-block -p ramshared-cuda`;
  no Linux LKM, checkpatch, kselftest, `cascade-health`, `/proc/swaps`, or BINARY_MATCH claim is made
  for this Windows-only StorPort work.
- [x] `./scripts/docs-check.sh` and docs index hygiene; every matrix row retains its named test and
  every critical row retains executable evidence.
- [x] Isolated GPU-PV storage-only product gate is PASS (`guest-product-online-20260716-220848`);
  physical daily-host authorization, SDV, StartIo READ-copy race strengthening, and WSL2 freeze
  elimination remain separate non-claims.
