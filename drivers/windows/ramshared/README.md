# RamShared Windows StorPort miniport (skeleton)

**SPEC (source of truth):** [`docs/specs/no-milestone/windows-swap-driver/SPEC.md`](../../../docs/specs/no-milestone/windows-swap-driver/SPEC.md)  
**Preflight:** [`docs/specs/no-milestone/windows-swap-driver/PREFLIGHT.md`](../../../docs/specs/no-milestone/windows-swap-driver/PREFLIGHT.md)  
**ADR:** [`docs/decisions/ADR-0006-storport-virtual-miniport.md`](../../../docs/decisions/ADR-0006-storport-virtual-miniport.md)

## Status

| Artifact | State |
| --- | --- |
| `protocol.h` | **Frozen ABI** (ITEM-4) — mirror in `crates/ramshared-winsvc/src/proto.rs` |
| `driver.c` / `virtdisk.c` / `queue.c` / `control.c` | **Not implemented** (ITEM-5) |
| `ramshared.inf` / `.vcxproj` / package | **Not implemented** (ITEM-5 / ITEM-11) |
| `tools/poolstress/` | **Not implemented** (ITEM-8, VM-only) |

## Safety

- **Never load an unsigned or test-signed driver on the daily host** until ITEM-8 kernel-page drill has updated `DEGRADATION-MATRIX.md` and ITEM-11 attestation policy is clear.
- Pressure / fuzz / crash injection: **Hyper-V VM only** (RNF-6).
- Control device SDDL: SYSTEM + Administrators only (DT-1).

## Build (when WDK tree exists)

Target: EWDK / WDK, **x64**, `TreatWarningsAsErrors`, `/W4 /WX`, SDV + InfVerif (DT-14).  
Exact MSBuild surface is SPEC ITEM-5 (`ramshared.vcxproj` + `ramshared.sln`).

## File map (SPEC)

| File | ITEM | Role |
| --- | --- | --- |
| `protocol.h` | 4 | ABI Ring-0 ↔ Ring-3 |
| `driver.c` / `driver.h` | 5 | `DriverEntry`, StorPort init, control device |
| `virtdisk.c` / `virtdisk.h` | 5 | virtual disk + SCSI translate |
| `queue.c` / `queue.h` | 5 | SPSC rings, MDL, inflight, crash teardown |
| `control.c` / `control.h` | 5 | IOCTL dispatch |
| `ramshared.inf` | 5/11 | universal INF |
| `package/` | 5/11 | attestation package layout |
| `../tools/poolstress/` | 8 | kernel-page stress (VM, never ship) |
