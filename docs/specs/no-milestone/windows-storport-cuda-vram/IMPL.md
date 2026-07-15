# IMPL — Windows StorPort I/O backed by CUDA VRAM

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md

## Status

**partial** · cover ✓ · E2E product Online ✓ (3-round SHA) · Verifier ✗ · BINARY_MATCH N/A

Product path **proven live** on physical Windows host:

`broker lease → CUDA DeviceMem → CREATE/REGISTER → NTFS I/O (backend=cuda)`  
with **3× SHA-256 match** on 4 MiB probes.

Still **not DONE**: Driver Verifier + full `REFUSE_*` IOCTL matrix not executed; clean SCM stop (code-7 path) under kill was force-stopped.

## Files (this turn)

| Path | Change |
| --- | --- |
| `crates/ramshared-winsvc/src/product_online.rs` | **NEW** full Online composition |
| `crates/ramshared-winsvc/src/main.rs` | console/SCM call product Online |
| prior winsvc/cuda/driver/scripts | as previous IMPL |

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

### Broker

WSL `ramsharedd --slices 2 --slice-mb 64 --arbiter-listen 127.0.0.1:19876`  
Windows tenant `127.0.0.1:19876` (mirrored localhost).

### Prior gates (still valid)

| Gate | Status |
| --- | --- |
| Unit + cover ≥80% | ✓ |
| MSVC winsvc.exe | ✓ |
| probe-cuda nvcuda 512 MiB | ✓ free delta 0 |
| WDK rebuild + test-sign | ✓ |
| win11-drill lab backend CREATE/REGISTER + SHA | ✓ |

## Gaps

| Kind | Detail |
| --- | --- |
| **env-bound / open** | Driver Verifier + `Invoke-WinDriveIoctlValidation` full refusal matrix |
| **open** | Graceful stop under AtomicBool (force-kill used in lab); volume lock Gate B polish |
| **closed** | Product Online CUDA path + 3-round checksum |
| **closed** | MSVC binary, probe-cuda, guest StorPort load |

## Rollback trigger

- BugCheck / new dump during Verifier or Online.
- Any SHA mismatch on product LUN.
- Free not restored within 64 MiB after probe/teardown.
- Pagefile-hot destroy (code 7 refuse).

## Traceability

| RF | Evidence |
| --- | --- |
| RF-1 | CUDA Online allocation + probe |
| RF-2 | CREATE/REGISTER via WindowsDriverLink |
| RF-3 | Broker lease grant (WinDrive) |
| RF-4 | 3-round SHA-256 + evidence JSONL |
| RF-5 | Gate A/B code present; force-kill in lab |
| RF-6 | Product binary ImagePath / console --storage-only |
