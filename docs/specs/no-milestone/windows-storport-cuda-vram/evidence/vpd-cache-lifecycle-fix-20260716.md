# VPD cache lifecycle fix — 2026-07-16

## Diagnosis

Before CREATE, the miniport advertised LUN 0, succeeded standard INQUIRY, advertised VPD page 0x80,
and returned sixteen synthetic `0` bytes as its unit serial. Windows consequently created a child
PDO and cached placeholder identity before the run-specific serial and size existed.
`BusChangeDetected` after CREATE rescanned capacity but did not replace that already-present PDO,
which explains `VPD_SERIAL_MATCH=0` in both corrected runs and the stale RAMSHARE PnP identities.

## Correction

- The control device remains independently available for CREATE.
- Before CREATE, REPORT LUNS reports zero LUNs; INQUIRY and READ CAPACITY return NO_DEVICE.
- CREATE first publishes the complete disk state, then `BusChangeDetected` performs an actual
  absent→present enumeration. DESTROY performs present→absent.
- Synthetic zero serial was removed. CREATE accepts exactly sixteen uppercase `[0-9A-F]` bytes.
- VPD 0x80 copies exactly those validated bytes; it never invents or normalizes a serial.
- Standard INQUIRY and VPD 0x00/0x80 honor the CDB allocation length and short transfers.
- READ CAPACITY(10) returns a saturated 32-bit last LBA; READ CAPACITY(16), service action 0x10,
  returns the 64-bit last LBA and block length with a bounded 32-byte response.

The static design expected `BusChangeDetected` to be sufficient because the visible bus topology
changes from zero to one LUN and back. The signed live campaign below disproved that expectation for
the current guest state: an old child PDO remained `OK` before CREATE and retained stale identity.

## Verification

```text
STATIC_SCSI_LIFECYCLE_TEST=PASS
STATIC_INJECTOR_TEST=PASS
STATIC_NO_LUN_REFUSAL=PASS (negative fixture)
WDK 10.0.26100.0: /W4 /WX /wd4324 BUILD_DRIVERS_OK
/wd4324 scope: WDK storport.h aligned-structure warning only
unsigned ramshared.sys length: 32256 bytes
unsigned ramshared.sys SHA256: 5A1B7C830935F8C8B79DEA552D4CBB098548E5E5894B3F23672D099EA92674EC
temporary Windows staging removed: true
git diff --check: PASS
```

## Signed live result

The one authorized no-retry campaign used signed `ramshared.sys` SHA256
`CD7E315D0DA5B24BB05C384846D7BA8123390300D2C3A3F73B10E52F9E80BC34` and artifact
`C:\ramshared\artifacts\guest-exhaustive-20260716-111439`.

`Get-Disk` contained no RAMSHARE disk before CREATE, but the PnP snapshot still contained multiple
historical RAMSHARE disk PDOs, including one with status `OK`. After CREATE, neither normal nor
Verifier pass produced one authoritative vendor/product + serial `ABCDEF0123456789` + 128 MiB
candidate. Both passes failed only `VPD_SERIAL_MATCH`; all other ITEM-3 verdicts and
`NO_NEW_DUMP` were 1. Verifier flags `0x2093B` were active on `ramshared.sys`.

Therefore the absent→present correction is not promoted: the no-stale-child gate failed and the
exact VPD identity remains BLOCKED. There was no retry. The VM was left Off, Verifier was reset
best-effort, GPU-PV was restored/preserved as one bare adapter, DDA remained zero, host RTX 2060
was `OK`, and the isolated build tree was removed. See
`signed-vpd-lifecycle-rerun-20260716.md` and the raw
`guest-exhaustive-20260716-111439/` evidence directory.

## Live proof (same day)

Isolated campaign `guest-exhaustive-20260716-120459` with package SHA `CD7E315D…` returned
`IOCTL_PASS1=PASS` and `IOCTL_VERIFIER=PASS` with exact serial `ABCDEF0123456789` and size
`134217728`. See `vpd-exact-pass-20260716.md`.
