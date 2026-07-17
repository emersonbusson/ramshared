# WDK Code Analysis project-clean — 2026-07-16

Scope: `drivers/windows/ramshared/{driver.c,virtdisk.c,queue.c,control.c}`  
Toolchain: MSVC/WDK 10.0.26100.0, `cl /kernel /W4 /analyze`  
Log: `C:\ramshared\src\artifacts\code-analysis-ramshared.log`

## Result

Project-file warnings: **0**.

The WDK headers still emit analyzer warnings in `wdm.h`, `ntddk.h`, and `storport.h` under this
toolchain. The gate filters those as environment/toolchain warnings and fails on warnings whose path
is under `C:\ramshared\src\drivers\windows\ramshared\*.c`.

## Fixes made before the clean run

- Added `DRIVER_DISPATCH` prototypes for control-device dispatch routines so MSVC Code Analysis
  recognizes the function class before assignment into `DriverObject->MajorFunction`.
- Added a `DRIVER_CANCEL` prototype for `QCommitCancel`.
- Replaced broad `__except (EXCEPTION_EXECUTE_HANDLER)` around `MmProbeAndLockPages` with
  `QProbeAndLockExceptionFilter`, which handles only expected probe/lock faults and continues search
  for unexpected exceptions.

## Still unavailable locally

Static Driver Verifier (`sdv.exe` / `StaticDV.exe`) was not installed in the local WDK image used for
this run. No SDV PASS is claimed.
