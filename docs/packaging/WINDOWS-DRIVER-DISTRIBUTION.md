# Windows driver distribution contract

The Windows StorPort path is supported only as a supervised beta until a
Microsoft-attested or production-trusted package is available. Test-signing is
lab evidence, not a public distribution mechanism.

## Validated compatibility

| Component | Validated boundary |
| --- | --- |
| Guest/host OS | Windows 11 25H2, build 26200 |
| WDK | 10.0.26100 |
| GPU | NVIDIA GeForce RTX 2060, 6 GiB |
| Driver mode | StorPort virtual miniport |
| Package verification | `InfVerif`, `SignTool verify /pa`, package/running `BINARY_MATCH` |

Other Windows builds, GPUs, and unattended deployment remain unsupported until
the same exhaustive campaign passes on that exact surface.

## Production signing gate

A distributable package requires all of the following:

1. release `ramshared.sys`, INF, and catalog built from a clean tagged commit;
2. Inf2Cat and InfVerif pass;
3. the catalog is signed by a production-trusted certificate or returned by
   Microsoft attestation signing;
4. `SignTool verify /pa /all` passes without enabling Windows test-signing;
5. the package SHA-256 is published beside the release manifest;
6. an install, rollback, and recovery drill passes on a disposable VM.

`scripts/windows/Sign-Drivers.ps1` is a test-signing lab helper. It must not be
used to label a package production-signed.

## Install

1. Verify package SHA-256 and Authenticode chain.
2. Run the read-only preflight and require a clean RAMSHARE identity state.
3. Install with `pnputil /add-driver ramshared.inf /install`.
4. Reboot when replacing an already loaded miniport.
5. Run the bounded 64 MiB Online campaign before any GiB-scale allocation.

## Rollback

1. Stop through the product lifecycle and require lease release.
2. Require `LUN_GONE`, `WIN32_GONE`, and `PNP_GONE`.
3. Remove the driver package with the exact published OEM INF identity.
4. Reboot and verify `RamSharedCtl`, RAMSHARE disks, and the root adapter are
   absent.
5. Restore the previous signed package only after its SHA-256 is verified.

Rollback trigger: any bugcheck, checksum mismatch, identity ambiguity,
teardown above 30 seconds, CUDA restoration error above 64 MiB, or residual
LUN/PnP node.

## Recovery

If graceful stop refuses, do not force-kill the backend while the volume or a
pagefile is active. Preserve the lease, capture diagnostics, reboot through the
supervised recovery path, then remove the exact RAMSHARE device and package.
Broad disk-number deletion, `Clear-Disk`, and physical-disk fallback are
forbidden.
