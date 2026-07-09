# RamShared Windows StorPort virtual miniport

SPEC: `docs/specs/no-milestone/windows-swap-driver/SPEC.md` (ITEM-5).

## Layout

| File | Role |
| --- | --- |
| `protocol.h` | Frozen ABI (source of truth, DT-17) |
| `driver.c` / `driver.h` | `DriverEntry`, StorPort HW callbacks |
| `virtdisk.c` / `virtdisk.h` | Virtual disk + SCSI translation |
| `queue.c` / `queue.h` | SPSC rings, inflight, MDL, DT-10 teardown |
| `control.c` / `control.h` | Control device IOCTLs (RNF-4) |
| `ramshared.inf` | Universal INF |
| `ramshared.vcxproj` / `.sln` | WDK build surface (H4) |

## Build (Windows + WDK/EWDK only)

```powershell
# From EWDK / VS developer shell with WDK
msbuild ramshared.vcxproj /p:Configuration=Release /p:Platform=x64 /p:TreatWarningAsError=true
InfVerif.exe /w ramshared.inf
# Static Driver Verifier (when available):
# msbuild /t:sdv /p:Inputs="/check" ramshared.vcxproj
```

## Hard gates

- **Never** load on the daily physical host before ITEM-8 kernel-page drill (DT-21 residency) in a disposable VM.
- Control device SDDL: SYSTEM + Administrators only.
- Teardown must never destroy the disk while the secondary pagefile is active (DT-9 / B1).

## Userspace counterpart

`crates/ramshared-winsvc` — ring client, broker tenant, pagefile activation.
