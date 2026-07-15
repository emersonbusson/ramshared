# IMPL — Windows StorPort I/O backed by CUDA VRAM

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md

## Status

**partial** · cover ✓ (Linux business files ≥80%; `product_online` windows-only E2E) · E2E product Online ✓ (3-round SHA) · graceful stop ✓ · guest IOCTL single-process REFUSE ✓ · Verifier ✗ · multi-process injectors ✗ · BINARY_MATCH N/A

Product path **proven live** on physical Windows host:

`broker lease → CUDA DeviceMem → CREATE/REGISTER → NTFS I/O (backend=cuda)`  
with **3× SHA-256 match** on 4 MiB probes, then **graceful stop** via `stop.request` (not force-kill).

Still **not DONE**: Driver Verifier; multi-process `REFUSE_FOREIGN_OWNER` / re-entry / rundown injectors on the **new** miniport (guest Off; host still runs older `ramshared.sys` without reserved/owner refuse).

## Files (this turn)

| Path | Change |
| --- | --- |
| `crates/ramshared-winsvc/src/main.rs` | SCM Stop → `AtomicBool`; console `C:\ProgramData\RamShared\stop.request`; `run_product_online` owns Online→teardown; code-7 stays Running |
| `crates/ramshared-winsvc/src/product_online.rs` | Gate A volume-local pagefile filter; Gate B holds `LockedVolume`; soft-fail lock when LUN unmounted |
| `scripts/windows/Invoke-WinDriveIoctlValidation.ps1` | Live IOCTL client: legitimate queue + reserved/bad-ring/unknown/index-jump; foreign-owner PE injector scaffold |

## Validation (numbers)

### Product Online (physical host, elevated)

```text
console --storage-only --config C:\ProgramData\RamShared\winsvc-product.toml
product Online: lease=… size=67108864 serial=… cuda=NVIDIA GeForce RTX 2060
Disk: RAMSHARE VRAMDISK 64 MiB
```

Evidence JSONL phases: `Stopped → Leased → CudaReady → Online` (`backend=cuda`).

### Three-round SHA-256 (product CUDA LUN)

| Round | match | ms | sha (prefix) |
| --- | --- | --- | --- |
| 1 | true | 232 | EFF6FD0B… |
| 2 | true | 157 | 0FA5BB03… |
| 3 | true | 153 | 17318809… |

`all_match=true` letter=`S`  
Artifact: `evidence/product-cuda-3rounds.json`

### Graceful stop (no force-kill)

```text
stop: create file C:\ProgramData\RamShared\stop.request
volume lock soft-fail (unmounted LUN?): volume: FSCTL_LOCK_VOLUME win32=5
console stopped: RuntimeSummary { phase: Stopped, … exit_code: 0 }
phases: Stopped → Leased → CudaReady → Online → Stopping → Stopped
```

Artifacts: `evidence/graceful-stop-console.txt`, `evidence/graceful-stop-run.jsonl`  
Gate A filters pagefiles to product volume letter only (system `C:\pagefile.sys` does not refuse teardown).

### Guest IOCTL matrix (win11-drill, new signed miniport)

| Verdict | Value |
| --- | --- |
| PASS_VALID_QUEUE | 1 |
| REFUSE_UNKNOWN_IOCTL | 1 |
| REFUSE_RESERVED_DISK_PARAMS | 1 |
| REFUSE_RESERVED_REGISTER | 1 |
| REFUSE_BAD_RING | 1 |
| REFUSE_RING_INDEX_JUMP | 1 |
| VPD_SERIAL_MATCH | 1 |
| NO_NEW_DUMP | 1 |
| REFUSE_FOREIGN_OWNER | 0 (injector env/driver) |
| REFUSE_RESERVED_CQE / REENTRY / RUNDOWN | 0 (concurrent injectors open) |
| VERIFIER | false |

`STATUS=PASS` (required single-process keys).  
Artifacts: `evidence/ioctl-guest-verdict-pass.json`, `evidence/ioctl-guest-console.txt`

Host re-run with older loaded `ramshared.sys`: reserved/owner refuse = 0 (expected until host reloads signed package; testsigning No on daily host).

### Broker

WSL `ramsharedd --slices 2 --slice-mb 64 --arbiter-listen 127.0.0.1:19876`  
Windows tenant `127.0.0.1:19876` (mirrored localhost).

### Prior gates (still valid)

| Gate | Status |
| --- | --- |
| Unit + cover ≥80% (config/evidence/runtime/service) | ✓ |
| MSVC winsvc.exe | ✓ |
| probe-cuda nvcuda 512 MiB | ✓ free delta 0 |
| WDK rebuild + test-sign | ✓ |
| win11-drill lab backend CREATE/REGISTER + SHA | ✓ |
| Product Online CUDA + 3-round | ✓ |
| Graceful stop AtomicBool / stop.request | ✓ |
| Guest single-process REFUSE_* | ✓ |

## Gaps

| Kind | Detail |
| --- | --- |
| **env-bound / open** | Driver Verifier on checkpointed VM (guest currently Off; not on daily host) |
| **env-bound / open** | Multi-process `REFUSE_FOREIGN_OWNER` + re-entry/rundown on **new** sys (host PE DESTROY still succeeds on old binary) |
| **closed** | Product Online CUDA path + 3-round checksum |
| **closed** | Graceful stop + volume-local Gate A + LockedVolume hold |
| **closed** | Guest single-process IOCTL refuse matrix PASS |

## Rollback trigger

- BugCheck / new dump during Verifier or Online.
- Any SHA mismatch on product LUN.
- Free not restored within 64 MiB after probe/teardown.
- Pagefile-hot destroy (code 7 refuse) on product volume.
- Teardown hangs > 30 s after `stop.request` / SCM Stop.

## Traceability

| RF | Evidence |
| --- | --- |
| RF-1 | CUDA Online allocation + probe |
| RF-2 | CREATE/REGISTER via WindowsDriverLink; guest REFUSE_* |
| RF-3 | Broker lease grant (WinDrive) |
| RF-4 | 3-round SHA-256 + evidence JSONL |
| RF-5 | Gate A/B code + graceful stop live (volume lock soft-fail when unmounted) |
| RF-6 | Product binary ImagePath / console --storage-only |
| ITEM-3 | Guest single-process PASS; Verifier + multi-process open |
|
