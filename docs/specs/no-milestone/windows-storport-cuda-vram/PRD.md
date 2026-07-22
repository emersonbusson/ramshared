---
slug: windows-storport-cuda-vram
title: Windows StorPort I/O backed by CUDA VRAM
milestone: —
issues:
  - 28
---

# PRD — Windows StorPort I/O backed by CUDA VRAM

## 1. Summary

**Change:** Complete issue [#28](https://github.com/emersonbusson/ramshared/issues/28) by replacing the Windows product path's lab RAM buffer with one preallocated `nvcuda.dll` device-memory region and serving the existing StorPort queue from that region. Prove that the exposed LUN is VRAM-backed with allocation evidence, NTFS format, and deterministic write/read integrity evidence.

**Outcome:** A native `ramshared-winsvc` path must load CUDA, reserve bounded VRAM, register the existing Ring-0/Ring-3 queue, expose the RamShared LUN, pass format plus write/read checksum smoke, and tear down in the documented safe order. The product path must have no silent RAM or file fallback.

**Layers:**

- [x] userspace crates
- [ ] `ramsharedd` / cascade CLI
- [ ] LKM / Linux kernel
- [ ] uAPI / sysfs / ioctl (the frozen Windows IOCTL ABI is reused, not extended)
- [x] Windows lab / WDK / winsvc
- [ ] safety or P0 script
- [x] ADR / runbook
- [ ] P0 benchmark gate (measurements are recorded, but performance is not a promotion claim for this slice)

This is an **extension of existing code**, not a new driver or CUDA wrapper. Its first live gate is storage-only: no secondary pagefile is created during this slice's acceptance run. Pagefile activation and pagefile-hot failure handling remain governed by `windows-swap-driver` and its DT-9 gates.

## 2. Technical context

### Existing implementation

| Fact | State |
| --- | --- |
| **[codebase]** `crates/ramshared-cuda` already selects `nvcuda.dll` on Windows, resolves the shared CUDA Driver API symbol table, and provides RAII `Cuda` → `Context` → `DeviceMem` ownership. | Exists; extend only with Windows-real tests or observability needed by this slice. |
| **[codebase]** `DeviceMem` implements `ramshared_vram::VramMemory`; `crates/ramshared-block::VramBackend<M>` implements `BlockBackend` over it. | Exists; this is the product data backend to reuse. |
| **[codebase]** `crates/ramshared-winsvc::DriverLink` translates READ/WRITE/FLUSH SQEs into a generic `BlockBackend`, but its executable still starts PowerShell lab scripts and the in-tree live lab backend is `scripts/windows/WinDriveBackend.cs`, backed by process RAM. | The missing connection is in the Windows service composition/lifecycle, not in the block adapter. |
| **[codebase]** `crates/ramshared-winsvc::service` contains testable provision/teardown sequencing, co-residency checks, and DT-9 refusal logic, but no production `DiskControl`, mapped-queue, CUDA, broker, or SCM composition. | Scaffold exists; extend it into one concrete product runtime. |
| **[codebase]** `drivers/windows/ramshared/protocol.h` and `crates/ramshared-winsvc/src/proto.rs` freeze ABI version 1: QD ≤256, max I/O 1 MiB, READ/WRITE/FLUSH SQEs, and five IOCTL functions. | Reuse unchanged unless SPEC discovery proves an implementation defect. |
| **[codebase]** `scripts/windows/Measure-RamSharedDiskIo.ps1` identifies the RamShared LUN, samples locale-safe PerfDisk counters, and supports SHA-256 read/write proof. | Use it for live Task Manager/PerfDisk evidence with machine-readable checksum output. |
| **[docs]** `windows-swap-driver/PREFLIGHT.md` records lab format, pagefile residency, DT-9 refusal, and SCM results as green, while product CUDA evidence and host-real authorization remain open. | This PRD closes only the product CUDA storage path gap. |
| **[docs]** ADR-0006 fixes the architecture: CUDA remains in userspace; StorPort translates SCSI to the shared queue; broker leases are logical budget; local `cuMemGetInfo` must fail closed. | Recommended design must conform to the ADR. |
| **[docs]** The degradation matrix records BugCheck `0x7A` when a pagefile-hot backend disappears and requires pagefile-off before destroy. | Storage-only acceptance is mandatory; no surprise removal is authorized here. |
| **[docs]** The issue comment dated 2026-07-14 says dynamic loading is done and leaves physical-host CUDA-backed CREATE_DISK plus format/read/write open. | Loader-only work cannot close issue #28. |
| **[inference]** A successful filesystem smoke alone cannot distinguish CUDA VRAM from the current RAM harness. | Acceptance must correlate LUN lifetime with CUDA allocation/free deltas and identify the running product binary/backend. |

### Paths in scope

- `crates/ramshared-winsvc/`: concrete Windows runtime, mapped queue, service lifecycle, configuration, structured logs, and unit/integration seams.
- `crates/ramshared-cuda/`: Windows-real load/allocation/roundtrip evidence; no second CUDA wrapper.
- `crates/ramshared-block/` and `crates/ramshared-vram/`: reused interfaces; change only if a proven contract gap blocks composition.
- `drivers/windows/ramshared/`: existing ABI/miniport validation; no planned protocol or driver change.
- `scripts/windows/`: preflight, supervised start/stop, storage-only CUDA drill, format and checksum evidence.
- `docs/specs/no-milestone/windows-swap-driver/`, `docs/decisions/ADR-0006-storport-virtual-miniport.md`, `docs/reliability/DEGRADATION-MATRIX.md`, and the Windows runbook/docs.

### Abuse and catastrophic cases

- A non-administrator or unrelated process attempts to open the control device or register arbitrary user VAs.
- A request smuggles non-zero reserved fields, unknown flags/opcodes, an invalid slot, an unaligned range, or `offset + len` overflow.
- Config requests more VRAM than the current CUDA free budget, consumes the host reserve, or selects an unavailable GPU.
- CUDA reports device lost/reset during READ or WRITE.
- The service, console, or operator is stopped while I/O is in flight.
- An existing secondary pagefile is discovered on the RamShared volume even though the requested drill is storage-only.
- A second backend instance attempts CREATE/REGISTER against an already-owned disk/queue.
- The lab C# RAM backend is accidentally launched by the product service, producing a false green result.

## 3. Recommended option

Build one concrete `ramshared-winsvc` runtime that performs, on one CUDA-affine I/O thread:

1. parse and validate product config;
2. acquire the broker lease and re-check local CUDA capacity;
3. load `nvcuda.dll`, select the configured device, create its context, and preallocate one contiguous `DeviceMem` region;
4. zero the region before exposure and wrap it in `VramBackend<DeviceMem>`;
5. CREATE_DISK, allocate/map the existing queue, REGISTER_QUEUE, and run COMMIT_AND_FETCH against that backend;
6. on supervised stop, prove no pagefile is active, stop new work, drain/complete, UNREGISTER_QUEUE, DESTROY_DISK, zero/free VRAM, and release the lease.

The same runtime function must be called by SCM and supervised console modes. Entry mode may differ; backend semantics may not.

Why this option:

- It closes the missing composition seam while reusing the accepted ADR and existing generic adapters.
- Preallocation moves CUDA allocation failure before disk exposure; the storage hot path performs only bounded synchronous copies and does not allocate VRAM.
- One CUDA-affine I/O thread matches the current `Context`/`DeviceMem` contract and the SPSC queue.
- Storage-only validation proves issue #28 without manufacturing pagefile pressure or accepting the known pagefile-hot removal risk.

Discarded alternatives:

| Alternative | Reason discarded |
| --- | --- |
| Keep `WinDriveBackend.cs` RAM mode as an automatic fallback | It can make format/read/write green without VRAM, hides CUDA failure, and violates the Day-0 single product path. It remains an explicitly named VM lab instrument only. |
| Add a second Windows-only CUDA abstraction | `ramshared-cuda` already owns the cross-platform Driver API table and RAII lifetime; duplication creates divergent semantics. |
| Allocate sparse CUDA chunks on first write | Allocation can fail in the storage hot path after the disk is online, making error containment harder. Contiguous preallocation is the Day-0 issue #28 path; sparse allocation needs a separate SPEC and failure proof. |
| Put CUDA calls in `ramshared.sys` | Wrong trust/lifecycle layer, conflicts with ADR-0006, and expands Ring-0 crash/signing risk. |
| Validate with a pagefile-hot kill | Existing evidence predicts BugCheck `0x7A`; it is destructive and does not add proof that ordinary StorPort I/O reaches CUDA. |

Trade-offs: contiguous allocation can fail under fragmented VRAM and synchronous CUDA copies constrain throughput. This slice accepts fail-closed startup and measures latency; it does not add asynchronous multi-queue behavior or a compatibility backend.

## 4. RF-N

### RF-1 — Compose the product CUDA backend

`ramshared-winsvc` must create `Cuda`, the configured CUDA device/context, one `DeviceMem` allocation, and `VramBackend<DeviceMem>` before exposing a disk. The production runtime must never select RAM/file storage when CUDA initialization or allocation fails.

**Acceptance:** a Windows-real named test or probe resolves `nvcuda.dll`, reports device identity and `free/total`, allocates the requested bytes, writes and reads known patterns at start/middle/end, and observes zero mismatches. Missing DLL, missing symbol, invalid ordinal, or allocation failure leaves no disk and returns the documented failure code.

**Abuse note:** device ordinal and size are validated before allocation; no DLL path is accepted from an untrusted client or current working directory.

### RF-2 — Bind the mapped StorPort queue to VRAM

The concrete Windows `DriverLink` must implement the existing CREATE_DISK, REGISTER_QUEUE, COMMIT_AND_FETCH, UNREGISTER_QUEUE, and DESTROY_DISK calls and serve READ/WRITE/FLUSH through `VramBackend<DeviceMem>`.

**Acceptance:** with the product binary running, raw and filesystem writes traverse ABI v1 and read back byte-for-byte; out-of-range, unaligned, invalid-op, invalid-slot, and oversized entries complete with error and do not touch VRAM outside the allocation.

**Abuse note:** queue pointers/lengths, ABI version, reserved fields, flags, indices, and arithmetic are bounded at both sides of the Ring-0/Ring-3 boundary. The control device remains administrator/service-only.

### RF-3 — Enforce budget and ownership before exposure

The service must hold a broker lease for exactly the configured disk capacity and pass a fresh local `cuMemGetInfo` check before `cuMemAlloc`. Requested capacity must leave `max(512 MiB, 10% of total VRAM)` free after allocation.

**Acceptance:** insufficient lease, stale/disconnected broker, CUDA free bytes below `size + reserve`, zero size, non-aligned size, or a second owner produces no CREATE_DISK. The granted lease is released on every failed post-grant path.

**Abuse note:** config is bounded to `size_bytes <= usize::MAX`, disk geometry constraints, queue data-area limits, and the selected device observed by the same context that allocates memory.

### RF-4 — Prove VRAM identity, format, and integrity

The supervised storage-only drill must prove that the LUN is served by the current `ramshared-winsvc` binary and a CUDA allocation, then format only a LUN whose vendor/product/serial match the RamShared identity and run deterministic checksum I/O.

**Acceptance:** evidence records PID and executable hash, GPU/device, requested and allocated bytes, `free_before`, `free_after_alloc`, `free_after_free`, LUN identity/size, format result, SHA-256 before/after, bytes transferred, duration, and exit code. Required relations are `free_before - free_after_alloc >= 95% of requested`, `free_after_free >= free_before - 64 MiB`, and checksum mismatches = 0.

**Abuse note:** format refuses a physical/non-RamShared disk and an occupied drive letter. `-Force` must not bypass identity checks.

### RF-5 — Teardown without pagefile-hot removal

Stop must query the actual OS pagefile state, refuse teardown if any pagefile on the RamShared volume remains allocated, then drain/complete I/O, unregister, destroy, wipe/free, and release in that order.

**Acceptance:** clean storage-only stop returns 0 and restores CUDA free capacity within the RF-4 tolerance; a manufactured pagefile-active state refuses before UNREGISTER/DESTROY, returns the safety code, logs the refusal, and leaves disk/allocation/lease state intact for operator recovery.

**Abuse note:** repeated stop is idempotent only after a completed teardown; it may not reinterpret an unsafe partial state as clean.

### RF-6 — Keep the lab backend visibly separate

The C# RAM backend may remain only as a VM lab harness. Product service installation and default SCM start must invoke the Rust product runtime directly and must not shell out to `Start-RamSharedLab.ps1` or `WinDriveBackend.cs`.

**Acceptance:** installed service `ImagePath` names the built `ramshared-winsvc.exe`; product logs contain `backend=cuda`; a test fails if the product configuration or service runtime selects `backend=ram`, `backend=file`, or launches the lab backend.

## 5. NFR-N

### NFR-1 — Host safety

- Acceptance is storage-only and uses at most `min(512 MiB, 10% of total VRAM)` for the first physical-host drill.
- No memory-pressure generator, secondary pagefile creation, surprise removal, forced process kill, or GPU reset is authorized by this PRD.
- All commands have finite waits: control/I/O operation timeout 5 s; clean teardown budget 30 s. Timeout fails closed and preserves state needed for recovery.
- Physical-host execution requires explicit operator go after preflight; destructive crash drills remain disposable-VM-only.

### NFR-2 — Integrity and resilience

- Every accepted WRITE must be complete before success is posted; FLUSH preserves the current synchronous-copy contract.
- One checksum mismatch, one out-of-bounds write, one unexpected disk identity, or one new BugCheck is a hard failure.
- CUDA device-lost/reset causes the active request and all safely drainable pending requests to complete with error; the service enters `FailedSafe` and must not report disk health.
- Deterministic failures are not retried. Only SPEC-enumerated transient status codes may be retried, with a bounded count and idempotency proof.

### NFR-3 — Observability

Structured JSONL and Windows Event Log entries must include timestamp, run ID, service state, backend (`cuda`), CUDA device name/ordinal, capacity/free/reserve bytes, disk ID/serial, queue depth, max I/O, operation class, status, latency in microseconds, teardown phase, and stable CUDA/Win32/NTSTATUS code. Logs must not include kernel/user virtual addresses or payload data.

Minimum counters for the drill: requests and bytes by READ/WRITE/FLUSH, errors by stable class, p50/p95/p99 latency in microseconds, outstanding depth, CUDA allocated bytes, checksum mismatches, and teardown duration in milliseconds. Miniport diagnostics use existing Event Viewer/Verifier evidence; no unproven WPP dependency is required for this slice.

### NFR-4 — Performance evidence without a false claim

Run at least three identical storage-only rounds and record throughput in MiB/s plus p50/p95/p99 latency in microseconds with automatic machine/GPU/driver/OS/build/queue context. This PRD sets no throughput promotion threshold because no CUDA-backed StorPort baseline exists in-tree. A request exceeding 5 s is a host-safety failure, not a performance comparison.

### NFR-5 — Compatibility and regressions

ABI version 1 and the existing C/Rust layouts remain byte-for-byte unchanged. Shared Linux/WSL crates must remain green. Windows product support is Day-0 x64 Windows with the NVIDIA CUDA Driver API and the already allow-listed host build for later pagefile work; this slice adds no older-driver shim.

### NFR-6 — Security boundary

Only LocalSystem/administrators may control the device/service. Unknown IOCTLs, ABI versions, flags, reserved bits, handles, sizes, alignments, and offsets are rejected once, before mapping or execution. Mapped handles and MDLs are owned by the registering process and released in reverse order on all failures.

## 6. Flows

### Happy flow

1. **Operator / preflight:** verify x64 Windows, NVIDIA driver, `nvcuda.dll`, elevation, RamShared driver package, no active RamShared pagefile, and explicit host-real authorization.
2. **`ramshared-winsvc`:** parse config, connect/register with broker, and acquire a capacity lease.
3. **CUDA runtime:** load DLL, select device, create context, read `free/total`, enforce reserve, allocate and zero VRAM.
4. **Service → miniport:** CREATE_DISK, allocate queue memory, REGISTER_QUEUE, and begin COMMIT_AND_FETCH on the CUDA-owning thread.
5. **Windows storage stack:** enumerate the uniquely identified LUN; the guarded formatter creates NTFS.
6. **Drill:** write deterministic data, flush, read it back, verify SHA-256, and record CUDA allocation plus I/O evidence for three rounds.
7. **Stop:** verify pagefile absent, block new submissions, drain, unregister, destroy, wipe/free VRAM, release lease, and record restored free capacity.

### Alternate flows

- **SCM mode:** LocalSystem invokes the same runtime and exposes progress through SCM pending states/Event Log.
- **Supervised console mode:** an elevated operator invokes the same runtime for evidence capture; only entry/control handling differs.
- **Already clean stop:** returns success without a second destroy/release and logs `idempotent=true`.
- **Format already present:** reuse is allowed only when LUN serial, expected filesystem, size, and run ownership match; otherwise refuse.

### Error contract

| Trigger | Console exit / SCM code | Required log | Resulting state |
| --- | --- | --- | --- |
| Invalid config, disk geometry, queue bounds, or device ordinal | 2 | `config_refused`, field, bounded detail | `Stopped` |
| Broker denial/disconnect or insufficient reserve before CREATE | 3 | `capacity_refused`, lease/free/need/reserve | `Stopped`; granted lease released |
| CUDA DLL/symbol/init/alloc failure | 4 | `cuda_failed`, stable operation/code | `Stopped`; no disk |
| CREATE/REGISTER/map/ABI failure | 5 | `driver_link_failed`, IOCTL/status, phase | `FailedSafe`; reverse unwind, no online LUN |
| I/O timeout, CUDA device lost, or checksum mismatch | 6 | `io_integrity_failed`, op/tag/status/latency; no payload | `FailedSafe`; no health claim |
| Pagefile found during stop | 7 | `teardown_refused_pagefile_active`, allocated/current usage | Previous online state preserved; no unregister/destroy/free |
| Clean operation | 0 | lifecycle and evidence summary | `Online` while running, then `Stopped` |

The product runtime must not translate any error row into a RAM/file fallback.

## 7. Data / state model

### Rust ownership

```text
ProductRuntime
├─ WinDriveConfig { size_bytes, block_size, cuda_device, reserve_bytes,
│                  queue_depth, max_io_bytes, broker, tenant, evidence_path }
├─ BrokerTenant -> LeaseState { lease, bytes }
├─ Cuda
│  └─ Context
│     └─ DeviceMem
│        └─ VramBackend<DeviceMem>
├─ WindowsDriverLink { control_handle, QueueMap, disk_id, serial }
└─ RuntimeEvidence { run_id, binary_sha256, cuda/disk/queue/io/teardown fields }
```

RAII order is allocation-forward and drop-reverse: library → context → VRAM → disk/queue exposure; stop explicitly performs queue/disk teardown and wipe before CUDA objects drop.

### Queue/uAPI model

ABI v1 remains the source of truth in `protocol.h`: `RAMSHARED_SQE`, `RAMSHARED_CQE`, `RAMSHARED_RING_HDR`, `RAMSHARED_REGISTER`, and `RAMSHARED_DISK_PARAMS`. Product configuration must satisfy:

- `queue_depth` is a power of two in `1..=256`;
- `max_io_bytes` is non-zero and ≤1 MiB;
- `queue_depth * max_io_bytes` is ≤4 MiB for the currently validated MDL/data-area bound;
- block size is 512 or 4096 bytes;
- disk size and each READ/WRITE range are block-aligned and overflow-safe;
- reserved fields/unknown flags are zero.

### Runtime state machine

```text
Stopped -> Leased -> CudaReady -> DiskCreated -> QueueRegistered -> Online
   ^          |          |             |               |            |
   +----------+----------+-------------+---------------+-- Stopping-+
                                      any fatal error -> FailedSafe
```

`Online` means CUDA allocation, disk, and queue are all owned by the same runtime. `FailedSafe` never implies the LUN is healthy. A pagefile-active observation blocks transition from `Online` to destructive stop phases.

## 8. Interfaces

### Service and console

- Default SCM entry: run the CUDA product runtime as `RamSharedWinSvc` under LocalSystem.
- `ramshared-winsvc console --config <absolute-path> --storage-only`: supervised use of the same runtime; the only acceptance mode for this PRD.
- `ramshared-winsvc probe-cuda --config <absolute-path>`: non-driver CUDA load/allocate/pattern/free gate, with no disk mutation.
- `ramshared-winsvc install|uninstall`: administrator-only service management. Uninstall must stop safely first and refuse if a RamShared pagefile or live disk prevents teardown.

Exact argument spelling may be closed in SPEC, but SCM and console must call one product runtime and return the error classes in §6.

### Driver ABI

Reuse ABI v1 functions 0–4: REGISTER_QUEUE, UNREGISTER_QUEUE, COMMIT_AND_FETCH, CREATE_DISK, DESTROY_DISK. No new IOCTL is required. Access policy, owner-process lifetime, size/alignment checks, idempotent cleanup, and stable status mapping must be revalidated.

### Broker

Reuse protocol version 1 with `TransportKind::WinDrive`, `LeaseRequest { bytes }`, `LeaseGranted/Denied`, heartbeat, and `LeaseRelease`. No force-revoke frame is introduced. Broker authorization is a logical budget; local CUDA free/reserve is independently authoritative.

### Configuration and evidence

Configuration is administrator-owned TOML under a fixed ProgramData path in SCM mode and an explicit absolute path in console mode. It must not contain secrets. Evidence is append-only JSONL in an administrator-writable directory with a unique run ID; retries/replays must not overwrite prior runs.

## 9. Dependencies and risks

### Dependencies

- x64 Windows with an NVIDIA driver exporting `nvcuda.dll` and a CUDA-capable GPU.
- MSVC Rust target for `ramshared-winsvc`, WDK-built/test-signed RamShared miniport for gated lab use, and administrator/LocalSystem execution.
- Existing ABI v1 miniport, broker protocol, guarded format helper, and Windows evidence scripts.
- Explicit operator approval before any physical-host driver load. Attestation remains a separate release gate; test-signing is lab-only.

### Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| False green from RAM harness | Product binary hash + `backend=cuda` + CUDA free-capacity deltas; no automatic fallback. |
| GPU reset/device loss corrupts or stalls storage | Bounded 5 s operations, stable error completion, `FailedSafe`, storage-only first gate, zero pagefile. |
| Pagefile-hot stop causes BugCheck `0x7A` | Query actual OS state and refuse before queue/disk/CUDA teardown; no kill drill in this PRD. |
| VRAM pressure harms foreground GPU work | Initial ≤512 MiB/10% allocation, 512 MiB or 10% reserve (whichever is larger), supervised run, full free on stop. |
| Malicious/buggy Ring-3 registration corrupts kernel state | Administrative ACL, owner binding, ABI/length/alignment/overflow checks, malformed-IOCTL tests under Verifier. |
| Contiguous allocation fails due to fragmentation | Fail before disk exposure and report observed free/need; do not silently switch backend. |
| Root docs currently overstate physical-host readiness relative to the SPEC gate | Update maturity statements so issue completion is not confused with pagefile/attestation production readiness. |

### Rollout

1. Pure Linux-runnable unit tests and cross-target compile.
2. Windows CUDA-only probe on the target physical GPU, without loading the RamShared driver.
3. Disposable VM driver regression tests without CUDA product acceptance.
4. Explicitly approved, supervised physical Windows storage-only run with ≤512 MiB and no pagefile.
5. Only after this PRD is green may the broader `windows-swap-driver` SPEC consider pagefile/soak/signing gates.

**Numeric rollback trigger:** disable/uninstall the product service and return to C:-only/no RamShared disk after **any 1** new BugCheck, checksum mismatch, out-of-bounds/identity violation, pagefile-active teardown attempt that reaches UNREGISTER/DESTROY, request latency above **5,000 ms**, teardown above **30,000 ms**, or failure to restore CUDA free bytes to within **64 MiB** of `free_before`. Preserve artifacts; do not retry on the physical host in the same run.

Rollback removes/disables the CUDA product feature; it does not promote the RAM lab harness into a compatibility path.

## 10. Implementation strategy

1. **Freeze evidence and refusal tests first:** name config, CUDA, queue-validation, product-no-fallback, pagefile-refusal, and idempotent-stop tests before runtime changes.
2. **Close the smallest unknown:** cross-compile `ramshared-cuda`/`ramshared-winsvc`, then run `probe-cuda` on Windows and prove allocate/pattern/free with no driver.
3. **Implement the concrete Windows control/queue adapter:** reuse ABI v1 and test malformed bounds/owner cleanup before CUDA composition.
4. **Compose one product runtime:** broker → reserve → CUDA allocation → disk/queue → I/O loop; SCM and console call it directly.
5. **Implement ordered unwind at every partial phase:** inject failures after lease, CUDA, CREATE, and REGISTER; assert reverse cleanup and no fallback.
6. **Extend guarded Windows drill:** add binary hash, backend identity, CUDA deltas, SHA-256 content verification, JSONL, and three-round context capture.
7. **Validate early in increasing-risk order:** pure tests → cross-build → CUDA-only host probe → VM driver regression → explicitly approved physical storage-only E2E.

No code implementation begins until `SPEC.md` and the mandatory Windows driver `AUDIT-2.5.md` are complete.

## 11. Documents to update

- Create `docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md`, then `AUDIT-2.5.md` and `IMPL.md` in later SSDV3 steps.
- Update `docs/specs/no-milestone/windows-swap-driver/{PREFLIGHT,IMPL,SPEC}.md` to link this focused slice and record which product CUDA gates closed.
- Confirm ADR-0006; amend it only if SPEC discovers a decision change (none is expected by this PRD).
- Add CUDA allocation/device-lost and false-backend-identity rows or status to `docs/reliability/DEGRADATION-MATRIX.md`.
- Update `drivers/windows/README.md`, `drivers/windows/ramshared/README.md`, `scripts/windows/README.md`, `docs/LIBRARIES.md`, and the Windows runbook with product-vs-lab commands and evidence.
- Reconcile `README.md`, `ROADMAP.md`, and `validation.md` maturity claims: CUDA storage-only success is not pagefile-hot safety, attestation, or general physical-host production readiness.
- Append measurements to `docs/benchmarks/results.jsonl` and `docs/BENCHMARKS.md` only when benchmark rules (automatic context and ≥3 rounds) are satisfied.
- Regenerate/check `docs/INDEX.md` after SSDV3 file additions.

## 12. Out of scope

- Creating, activating, pressuring, revoking, or surprise-removing a Windows secondary pagefile.
- Closing ITEM-9 performance promotion, ITEM-10 72-hour soak, ITEM-11 attestation/Partner Center, or general Windows production release.
- AMD/Vulkan/D3D12 backends, multiple GPUs, CUDA context migration, multi-queue/asynchronous copies, sparse/on-demand VRAM allocation, compression, encryption, or deduplication.
- Changing ABI v1, StorPort queue architecture, broker protocol, Linux/WSL cascade behavior, or LKM code.
- Automatic recovery after CUDA device loss or service crash while storage consumers remain mounted.
- Running destructive pressure, Verifier fuzz, reset, or pagefile-hot tests on the daily physical host.

## 13. Acceptance criteria

- [ ] `ramshared-winsvc` product runtime uses `nvcuda.dll` → `DeviceMem` → `VramBackend` → existing StorPort queue, with no RAM/file fallback.
- [ ] SCM and console modes call the same runtime; installed `ImagePath` is the Rust executable and does not launch the lab PowerShell/C# backend.
- [ ] Capacity/reserve, config, privilege, duplicate-owner, malformed ABI, and pagefile-active teardown refusals have paired legitimate-pass tests.
- [ ] ABI v1 C/Rust sizes and golden bytes remain unchanged.
- [ ] Windows CUDA probe reports one selected device, plausible `free/total`, one bounded allocation, start/middle/end pattern roundtrip, zero mismatch, and free restoration within 64 MiB.
- [ ] Storage-only live E2E records the product binary SHA-256 and `backend=cuda`; CUDA free capacity decreases by at least 95% of requested allocation while online.
- [ ] Guarded format targets only the uniquely identified RamShared LUN and succeeds without `-Force` bypassing identity.
- [ ] Three write/flush/read rounds return identical SHA-256 with checksum mismatches = 0 and record MiB/s plus p50/p95/p99 microseconds.
- [ ] Clean teardown completes within 30 s, performs pagefile-check → drain → unregister → destroy → wipe/free → release, and restores CUDA free bytes within 64 MiB.
- [ ] A pagefile-active stop refusal returns code 7 before UNREGISTER/DESTROY/free; a clean stop still passes, and stop 2× produces one destructive effect.
- [ ] No new BugCheck, minidump, Verifier finding, hung I/O above 5 s, kernel-address log, or physical-host pressure event occurs.
- [ ] Required Rust tests/clippy/format, slice coverage ≥80% on changed business-logic files, Windows build/static gates, VM regression, and live physical CUDA storage-only evidence are green.
- [ ] `validation.md`, focused IMPL evidence, degradation matrix, product/lab docs, and issue #28 status distinguish storage-only completion from remaining pagefile/signing gates.

## 14. Validation plan

### Unit and hermetic integration

- `ramshared-cuda`: retain error tests; add Windows-target loader candidate/path-policy tests where hermetic, while the real DLL probe remains live.
- `ramshared-winsvc`: named tests for config reserve/bounds, runtime phase unwind after each injected failure, no fallback, queue range/flag/reserved validation, backend CUDA identity propagation, pagefile-active refusal paired with clean stop, and stop replay 2× → one unregister/destroy/release.
- `ramshared-block`: retain `vram_backend_write_then_read_roundtrip`, OOB rejection, and wipe tests; add only tests required by an actual adapter contract change.
- ABI: C static assertions plus Rust size/golden tests for protocol v1.
- Gate changed Rust business-logic files individually with `node tools/ci/check-rust-slice-coverage.mjs ... --min 80`; workspace average does not substitute.

### Windows build and driver regression

- Build the MSVC Rust binary and WDK driver; run `cargo fmt`, targeted `clippy -D warnings`, tests, InfVerif, WDK Code Analysis, Driver Verifier (lab VM), and signature verification appropriate to the lab package. **SDV is N/A on VS2022/WDK 26100** (Microsoft retired the tool; see SPEC DT-30) — do not treat SDV absence as incomplete Day-0 work.
- In a disposable VM/checkpoint, rerun CREATE/REGISTER, NTFS format, raw I/O, malformed IOCTL, owner cleanup, clean teardown, and Driver Verifier smoke. This proves the unchanged driver surface did not regress; it does not prove CUDA VRAM.
- Record OS/WDK/Rust/GPU-driver versions and artifact hashes.

### Live path for this layer

1. **Before:** on the explicitly approved physical Windows GPU environment, capture preflight, no RamShared pagefile, no existing RamShared disk/backend, latest dump identity, product binary hash, GPU identity, and `cuMemGetInfo` free/total.
2. **Action A:** run CUDA-only probe with ≤512 MiB; allocate, zero, pattern-write/read start/middle/end, free.
3. **Action B:** start `ramshared-winsvc console --storage-only`; prove product PID/hash and CUDA free delta; enumerate/identity-check and format the LUN; execute three deterministic write/flush/read SHA-256 rounds while collecting queue/I/O metrics.
4. **After:** stop cleanly; prove zero pagefile throughout, disk/queue absent, lease released, free capacity restored within 64 MiB, zero new dump/BugCheck, and all time bounds met.
5. Append before/action/after evidence to `validation.md` and the focused `IMPL.md`; only then update issue #28.

### Environment-bound gaps

- This Linux/WSL workspace can validate pure logic, shared-crate regressions, and cross-compilation, but cannot supply Windows SCM, WDK, `nvcuda.dll`, physical-GPU allocation deltas, format, Verifier, or minidump evidence.
- If an approved physical Windows GPU environment or MSVC/WDK toolchain is unavailable, the slice remains **PARTIAL**. Unit tests, a VM RAM-backed run, or presence of `nvcuda.dll` must not be reported as DONE.
- Pagefile, 72-hour soak, and attestation gaps remain explicitly open under `windows-swap-driver` even after this PRD's storage-only gate passes.
