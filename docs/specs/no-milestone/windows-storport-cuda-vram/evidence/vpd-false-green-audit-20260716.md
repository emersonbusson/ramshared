# VPD false-green audit — 2026-07-16

## Finding

The prior `Invoke-WinDriveIoctlValidation.ps1` could set `VPD_SERIAL_MATCH=1` through either a unique
size/name match or a single live PnP RAMSHARE device. Neither fallback proved the SPEC-required
16-byte VPD serial. Therefore the historical `guest-exhaustive-20260715-214831` overall ITEM-3 PASS,
including its Driver Verifier pass, is invalidated until the corrected harness is rerun.

## Correction

- `VPD_SERIAL_MATCH=1` now requires RAMSHARE/VRAMDISK identity, exact serial
  `ABCDEF0123456789`, and exact configured size on one authoritative `Win32_DiskDrive` or `Get-Disk`
  surface.
- The static harness rejects restoration of the size/name and PnP-presence fallback markers.
- The teardown identity parser now accepts the observed standard friendly name
  `RAMSHARE VRAMDISK SCSI Disk Device` as vendor `RAMSHARE` / product `VRAMDISK`; before this fix it
  treated the entire suffix as the product and falsely refused every legitimate stop. Any other
  prefix or suffix remains fail-closed.
- No physical Online, driver installation, miniport replacement, reboot, or host pressure was run.

## Safe verification

```text
Windows PowerShell 5.1 parser: PASS (three changed harnesses)
STATIC_INJECTOR_TEST=PASS
STATIC_VPD_FALLBACK_REFUSAL=PASS (negative fixture)
WDK 10.0.26100.0 staged build: BUILD_DRIVERS_OK
ramshared.sys build length: 31744 bytes
temporary Windows staging removed: true
Rust native tests: block 41 pass; CUDA 5 pass / 1 ignored; winsvc 78 pass / 1 ignored
native clippy -D warnings: PASS
MSVC cross-target clippy -D warnings: PASS
fmt/docs/diff checks: PASS
winsvc selected coverage: 84.9% through 95.5% per file
CUDA probe coverage: 80.0%
cargo audit --no-fetch: PASS
```

## Live read-only BINARY_MATCH

```text
installed: E690306FF4BD64E44118DE72143AA8ED9D9284A75E77AB0EB54CF3EF648D7FEE
package:   1E57690EA63E6287D4790A134544DC9F46253BB356D1C2B3B1D65FC812F30CFF
BINARY_MATCH=false
ramshared=Running
RamSharedWinSvc=Stopped
```

Verdict: **PARTIAL**. The corrected code and static/build gates are green; exact VPD live proof,
physical BINARY_MATCH, real CUDA Online, and the WSL2 freeze-elimination campaign remain blocked.

## Corrected live rerun

Campaign `C:\ramshared\artifacts\guest-exhaustive-20260716-104650` used corrected harness SHA
`6D7B2DC1…` and driver SHA `1E57690E…`. Both the normal and Driver Verifier passes reported every
non-VPD required verdict as 1, `NO_NEW_DUMP=1`, and `VPD_SERIAL_MATCH=0`; both aggregate statuses
were therefore `FAIL`. Verifier was active with flags `0x2093B` and listed `ramshared.sys`
load 1 / unload 0. Guest exit was 2.

The campaign completed without PSD timeout. Final state was VM Off, verifier reset best-effort, bare
GPU partition adapter restored, zero assignable devices, and host RTX 2060 OK. No retry was run.
