# IMPL — Windows StorPort I/O backed by CUDA VRAM

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md

## Status

**partial** · cover ✓ · E2E partial · BINARY_MATCH N/A (Windows-only; no `ramsharedd`)

| Surface | Status |
| --- | --- |
| Unit + cover ≥80% (Linux) | ✓ |
| MSVC `ramshared-winsvc.exe` | ✓ built + deployed `C:\ramshared\bin\` |
| Windows `probe-cuda` (nvcuda.dll, RTX 2060) | ✓ PASS 512 MiB, free delta 0 |
| WDK `ramshared.sys` rebuild + test-sign | ✓ package under `C:\ramshared\package\` |
| win11-drill: driver RUNNING, CREATE/REGISTER | ✓ |
| win11-drill: NTFS + SHA-256 4 MiB match | ✓ (lab `WinDriveBackend` RAM backend) |
| Product `ramshared-winsvc` Online as I/O backend | ✗ env-bound (needs broker + CUDA on same host as StorPort) |
| 3-round product campaign + Verifier | ✗ env-bound |
| Host testsigning | ✗ not enabled (guest only) |

Index status remains **PARTIAL** (not DONE).

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-winsvc/**` | ITEM-1…5 | Product config/evidence/runtime/queue/broker/service/CUDA probe/Windows adapters |
| `crates/ramshared-cuda/**` | ITEM-2 | probe planning; windows-sys 0.61 loader fix |
| `drivers/windows/ramshared/**` | ITEM-3 | owner, rundown, slot states, reserved/ring, VPD |
| `scripts/windows/**` | ITEM-5/6 | product install, lab install, drills, preflight -StorageOnly |
| `docs/specs/…/evidence/*` | ITEM-6/7 | live numbers (this turn) |

## Validation (numbers)

### Unit / fmt / clippy (Linux)

| Cmd | Exit |
| --- | --- |
| `cargo test -p ramshared-winsvc --lib` | 0 (72 + ignored probe) |
| cover gate business files | PASS (≥80%) |

### MSVC product binary

```text
C:\ramshared\bin\build-winsvc.bat  → BUILD_OK
C:\ramshared\bin\ramshared-winsvc.exe  647168 bytes
SHA256=F3453587C0AF7D432B566AA6F42C0C4370445B16E8803D12C5E3477BAD71CDDC
```

Evidence: `evidence/msvc-build-winsvc.txt`, `evidence/ramshared-winsvc-sha256.txt`.

### Live CUDA probe (DT-3) — Windows nvcuda

```text
probe-cuda --config C:\ProgramData\RamShared\winsvc.toml
device=0 name=NVIDIA GeForce RTX 2060 size=536870912
free_before=5360320512 free_after=5360320512 offsets=[0, 268435456, 536866816]
PASS
```

Evidence: `evidence/probe-cuda-windows-nvcuda-20260715.txt` (+ WSL twin `probe-cuda-wsl-20260715.txt`).

### Driver build + sign

```text
BUILD_DRIVERS_OK ramshared.sys 31232 (unsigned build) → signed package 32656
SIGN_OK + Inf2Cat ramshared.cat
```

Evidence: `evidence/build-drivers.txt`, `evidence/ramshared-sys-sha256.txt`.

### win11-drill (2 GiB static; elevated PSD)

| Step | Result |
| --- | --- |
| Start VM | OK after reducing Startup from 4G→2G (host free ~9.7 GiB) |
| sc create/start poolstress+ramshared | RUNNING |
| WinDriveBackend 64 MiB | `CREATE_DISK ok` `REGISTER_QUEUE ok` |
| Disk N=1 64 MiB | present (FriendlyName `Msft Virtual Disk`) |
| Write/flush/read 4 MiB | `sha_match=true` SHA256=`053EDE97…0AA1` |
| Teardown | sc stop; VM Off |

Evidence: `evidence/guest-driver-load.json`, `guest-create-register.json`, `guest-sha256-io.json`.

### Preflight host

`Get-WinDrivePreflight.ps1 -StorageOnly` → `PREFLIGHT_STORAGE_ONLY=PASS`  
(warn: not elevated, testsigning not Yes on host).

Evidence: `evidence/preflight-storage-only-host.txt`.

### SPEC matrix (executable)

| TestName | Evidence |
| --- | --- |
| Named cargo unit tests (config/evidence/driver_link/broker/runtime/service) | ✓ Linux |
| `probe_cuda_allocates_roundtrips_and_restores` | ✓ live Windows + WSL |
| IOCTL CREATE/REGISTER legitimate | ✓ guest via WinDriveBackend |
| SHA-256 filesystem I/O | ✓ guest 1 round 4 MiB (not 3-round product campaign) |
| Verifier / REFUSE_* verdicts | ✗ not run |
| `storage_only_cuda_three_rounds_sha256` product | ✗ not run (no CUDA in guest; host no testsigning) |

## Gaps

| Kind | Detail |
| --- | --- |
| **env-bound** | Product Online path: broker + CUDA + StorPort same process (`ramshared-winsvc` as backend) |
| **env-bound** | Host testsigning / EV signing for daily-host driver load |
| **env-bound** | Driver Verifier + full `Invoke-WinDriveIoctlValidation` refusal matrix |
| **env-bound** | Three-round product campaign with `backend=cuda` + exact VPD serial format script |
| **closed** | MSVC winsvc, nvcuda probe, WDK rebuild/sign, guest CREATE/REGISTER + checksum |
| **open** | Pagefile path intentionally out of slice |

## Rollback trigger

- BugCheck / new dump under Verifier or guest load.
- Checksum mismatch or free not restored within 64 MiB after probe.
- I/O >5 s or teardown >30 s on physical product campaign.
- Pagefile on target volume at stop → code 7 refuse.

## Traceability

| RF | ITEM | This turn |
| --- | --- | --- |
| RF-1 | 1–2 | CUDA probe live Windows |
| RF-2 | 3 | Guest CREATE/REGISTER on new miniport |
| RF-4 | 6 | Guest SHA-256 match |
| RF-6 | 5 | Product installer binary ImagePath path built |
| NFR | — | partial: live numbers without full product 3-round |
