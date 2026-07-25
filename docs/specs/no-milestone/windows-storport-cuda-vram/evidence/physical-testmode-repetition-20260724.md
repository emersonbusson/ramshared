# Physical Windows Test Mode Repetition — 2026-07-24

## Environment

- Windows build: `26200.8875`
- GPU: NVIDIA GeForce RTX 2060, 6144 MiB
- Driver mode: Windows Test Mode (`testsigning Yes`)
- Package signer: `CN=RamShared Test Signing`
- Driver SHA-256:
  `324CC7C95A17BE3C245865F55EFC3E87B443D9CF711249068A4221DD86DEDBFA`

The host was rebooted once to activate Test Mode and again after deploying the
miniport. The final campaign therefore did not reuse an image loaded before the
DriverStore update.

## Preflight

Artifact:
`C:\ramshared\artifacts\physical-testmode-final-preflight-20260724-224916`

The elevated storage-only preflight passed:

- exact package/running binary match;
- `RamSharedCtl` open;
- no RAMSHARE disk, Win32 disk, PnP ghost, or pagefile;
- no minidump;
- CUDA and NVIDIA telemetry available.

## Repeated physical campaigns

| Artifact | Online | SHA rounds | Direct I/O | Graceful | Lease | Terminal storage identity |
| --- | --- | ---: | --- | --- | --- | --- |
| `exhaustive-20260724-224946` | PASS | 3/3 | PASS | PASS | released | none |
| `exhaustive-20260724-225047` | PASS | 3/3 | PASS | PASS | released | none |
| `exhaustive-20260724-225124` | PASS | 3/3 | PASS | PASS | released | none |

Every lifecycle used an exact 64 MiB `RAMSHARE VRAMDISK` identity and a private
mount below `C:\ProgramData\RamShared\mounts`. No physical-disk fallback,
drive-letter formatting, pagefile, external pressure, or Driver Verifier fuzz
was used on the daily host.

Final elevated preflight:
`C:\ramshared\artifacts\physical-testmode-final-20260724-225209`

It passed with no RAMSHARE storage identity or minidump. The miniport service
and control path remain available for supervised tests.

## Claim boundary

This evidence closes supervised physical Test Mode functionality on this exact
host. It does not close:

- Microsoft attestation or production-trusted signing;
- Secure Boot and anti-cheat compatibility;
- autonomous daily SCM operation, because the product configuration requires a
  supervised broker dependency at `127.0.0.1:7700`.
