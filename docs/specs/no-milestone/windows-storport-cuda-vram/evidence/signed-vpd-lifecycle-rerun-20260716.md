# Signed VPD lifecycle rerun — 2026-07-16

## Package proof

- Isolated WDK 10.0.26100 build: `BUILD_DRIVERS_OK` with `/W4 /WX /wd4324`.
- `/wd4324` suppresses only the WDK `storport.h` aligned-structure warning; project warnings stayed
  fatal.
- Existing PFX/password used without logging secret material and without changing host trust stores.
- Inf2Cat: no errors or warnings.
- `signtool verify /pa` and `Get-AuthenticodeSignature`: valid for `ramshared.sys`,
  `ramshared.cat`, and `poolstress.sys`.
- Signed/package/guest-installed `ramshared.sys` SHA256:
  `CD7E315D0DA5B24BB05C384846D7BA8123390300D2C3A3F73B10E52F9E80BC34`.
- Signed image length: 33,680 bytes.
- Corrected harness source/staged SHA256:
  `6D7B2DC1DFD15D9B78F7734882D535DE190FD963DECF3A26926D7EBAA008BFC4`.
- Package manifest: `vpd-lifecycle-package-20260716-111336.json`.

## Single bounded campaign

Artifact: `C:\ramshared\artifacts\guest-exhaustive-20260716-111439`.

Before CREATE, the `Get-Disk` snapshot contained only the 80 GiB OS disk and the 64 MiB answer
disk; no RAMSHARE LUN was surfaced there. The PnP disk snapshot was not clean, however: it retained
several historical RAMSHARE PDOs, including one `OK` instance. This fails the stronger no-stale-child
gate even though the storage-disk surface had no pre-CREATE RAMSHARE disk.

```text
IOCTL_PASS1=FAIL
IOCTL_VERIFIER=FAIL
VERIFIER_RAN=true
VPD_SERIAL_MATCH=0 (both passes)
all other required ITEM-3 verdicts=1 (both passes)
NO_NEW_DUMP=1 (both passes)
GUEST_EXIT=2
```

The guest-installed SHA matched the signed package. Driver Verifier returned after a normal guest
reboot in 129 seconds, flags `0x2093B` were active, and `ramshared.sys` showed load 1 / unload 0.
There were no minidumps. Both exact-identity polls exhausted without finding a unique authoritative
candidate containing vendor/product + serial `ABCDEF0123456789` + 134,217,728-byte size.

This is a product failure, not a harness timeout or checksum ambiguity. The signed live result
disproves promotion of the current `BusChangeDetected` absent→present correction in the presence of
the retained child PDO. No retry was made.

## Terminal state

- `win11-drill`: Off.
- Driver Verifier: reset best-effort before VM stop.
- GPU-PV: one bare adapter; instance/minimum/optimal/maximum fields empty.
- DDA: zero assignable devices.
- Host GPU: `OK | NVIDIA GeForce RTX 2060`.
- Isolated Windows build tree: removed.
- Physical host driver/service/trust stores: not changed by this campaign.

Raw host-side files are preserved in `guest-exhaustive-20260716-111439/`.
