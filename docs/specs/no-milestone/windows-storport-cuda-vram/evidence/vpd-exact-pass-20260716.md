# Exact VPD + Driver Verifier PASS — 2026-07-16

## Campaign

`C:\ramshared\artifacts\guest-exhaustive-20260716-120459`

| Gate | Result |
| --- | --- |
| Package `ramshared.sys` SHA256 | `CD7E315D0DA5B24BB05C384846D7BA8123390300D2C3A3F73B10E52F9E80BC34` |
| Guest BINARY_MATCH (package vs `System32\drivers`) | true |
| IOCTL_PASS1 | **PASS** |
| IOCTL_VERIFIER | **PASS** |
| Verifier flags | `0x2093B` on `ramshared.sys` (load 1 / unload 0) |
| `VPD_SERIAL_MATCH` (both passes) | **1** |
| Serial | `ABCDEF0123456789` |
| Size | `134217728` (128 MiB CREATE) |
| Surface | `Win32_DiskDrive` + `IOCTL_DISK_GET_LENGTH_INFO` |
| `NO_NEW_DUMP` | 1 |
| Final VM | Off |
| Host GPU | RTX 2060 OK |

The terminal state was independently recaptured read-only after the campaign: one bare GPU-PV
adapter with empty partition values, DDA count zero, VM `Off`, and host RTX 2060 `OK`. See
`terminal-state-vpd-pass-20260716T170631Z.md`.

## What was wrong before

1. **False-green harness** (invalidated older ITEM-3 PASS): size/name or PnP-presence fallbacks could set `VPD_SERIAL_MATCH=1` without the exact 16-byte serial.
2. **Placeholder LUN identity**: before CREATE the miniport advertised LUN 0 with synthetic zero serial; Windows cached that PDO. Fixed by empty REPORT LUNS + `NO_DEVICE` INQUIRY until CREATE, then `BusChangeDetected`.
3. **Ghost RAMSHARE PDOs** from older builds poisoned uniqueness until explicit `pnputil /remove-device` + post-deploy reboot.
4. **Size false-negative**: `Win32_DiskDrive.Size` is CHS-derived (observed `131604480` vs real `134217728`). Serial was already correct. Capacity is now taken from `IOCTL_DISK_GET_LENGTH_INFO` on `\\.\PhysicalDriveN`.
5. **Post-deploy image mapping**: SCM can keep the old image after file replace (`start` 1056). Harness now reboots once after deploy (PSD budget 300s).

## Remaining product PARTIAL

- Physical host BINARY_MATCH / Online still blocked (installed package ≠ lab package; no daily Online).
- GPU-PV real CUDA blocked on virtual PCI protocol `0x10006` vs channel `0x10005` (`0x8007051A`); guest/host build alignment required.
- WSL2 freeze-elimination claim still requires an isolated 2× before/action/after hang campaign (never thrash the live host).

## Artifacts

- `ioctl-guest-summary-vpd-pass.json`
- `ioctl-guest-verdict-vpd-pass.json`
- `ioctl-guest-verdict-vpd-pass-verifier.json`
- `ioctl-guest-vpd-pass-console.txt`
- `ioctl-guest-vpd-pass-verifier-console.txt`
- `ioctl-guest-vpd-pass-host-side.log`
- `vpd-lifecycle-package-20260716-111336.json`
- `terminal-state-vpd-pass-20260716T170631Z.md`
