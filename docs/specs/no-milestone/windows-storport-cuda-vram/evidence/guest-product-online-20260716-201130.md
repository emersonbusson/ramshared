# Guest product Online PASS — 2026-07-16

Campaign: `guest-product-online-20260716-201130`  
Artifact: `C:\ramshared\artifacts\guest-product-online-20260716-201130`  
VM: `win11-drill`  
Product exe: `C:\ramshared\bin\ramshared-winsvc.exe` SHA-256 `C6C9EB921D9C24C061D6404A3D58F99583DDDB4141F4949BA26851D48B7BE338`  
Miniport package SHA-256: `E297B73F0544C1B9F68BEF7373C1C7DC170DBC390B7C459A9ED5D1255F6E31C0`

## Summary

```json
{
  "ARTIFACT": "C:\\ramshared\\artifacts\\guest-product-online-20260716-201130",
  "LIFECYCLE_ROUNDS": 3,
  "ONLINE": true,
  "BINARY_MATCH": true,
  "ROUNDS_PASS": true,
  "CONSOLE_EXIT_ZERO": true,
  "NO_FORCE_KILL": true,
  "LEASE_RELEASED": true,
  "CUDA_RESTORED": true,
  "NO_NEW_DUMP": true,
  "TEARDOWN_WITHIN_BUDGET": true,
  "TERMINAL_SAFE": true,
  "PASS": true
}
```

## Round evidence

| Round | Serial | BINARY_MATCH | SHA I/O | Stop | Lease | CUDA restored | No dumps | Teardown |
| ---: | --- | --- | --- | --- | --- | --- | --- | ---: |
| 1 | `277CE1C45A2668AB` | PASS | PASS `F00F5436…` | PASS | PASS | PASS | PASS | 5039 ms |
| 2 | `73955CFD93434573` | PASS | PASS `BC5D2945…` | PASS | PASS | PASS | PASS | 4045 ms |
| 3 | `A52D6B62FBC9644C` | PASS | PASS `320D703D…` | PASS | PASS | PASS | PASS | 4009 ms |

All three rounds used a 64 MiB `RAMSHARE VRAMDISK` on `S:` with real GPU-PV CUDA on `NVIDIA GeForce RTX 2060`.
Each round stopped without force-kill, logged `Stopped: teardown_after_lock + lease release OK`,
observed broker log `lease 1 liberado`, restored CUDA free memory within the 64 MiB allowance, and
created no new dumps.

## Fixes proven by this campaign

- DriverStore/package BINARY_MATCH is checked before product start; package and guest image both
  reported `E297B73F…`.
- The product waits for exact startup LUN identity before logging Online and pumps COMMIT while
  waiting so Windows enumeration cannot starve itself.
- Teardown identity binds configured letter, VPD serial, and exact size without requiring a
  `PhysicalDriveN` length IOCTL that can fail with access denied during teardown.
- Volume lock runs with the I/O pump active, then Gate B, flush/dismount, unregister/destroy, CUDA
  free restoration, and lease release complete in order.
- Harness verdict requires Online, BINARY_MATCH, SHA I/O, exit 0, no force-kill, lease release, CUDA
  restoration, no dumps, teardown budget, VM Off, and host GPU OK.

## Remaining non-claims

This closes the isolated GPU-PV storage-only product gate. It does not claim SDV/Code Analysis,
physical daily-host authorization, the dedicated live StartIo READ-copy race campaign, or the WSL2
freeze-elimination campaign.
