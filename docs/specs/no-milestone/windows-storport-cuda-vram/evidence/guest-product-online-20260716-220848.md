# Guest product Online PASS — 2026-07-16

Campaign: `guest-product-online-20260716-220848`  
Artifact: `C:\ramshared\artifacts\guest-product-online-20260716-220848`  
VM: `win11-drill`  
Product exe SHA-256: `AAD4566897C9CF262F14AB783CCC6B2B2A43C8233A2E85ECA1FC562003246352`  
Miniport package SHA-256: `97FD7B373ED7DD5AE7F38204070F8B89E08A2B25616AA2A128995E8D1FBFF34F`  
Catalog SHA-256: `5A61758AEF426FEC9E9A2B5CA483D42F4DBB89C2DAD47280A17EC111D0A25EDE`

## Summary

```json
{
  "ARTIFACT": "C:\\ramshared\\artifacts\\guest-product-online-20260716-220848",
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

| Round | Serial | DriverStore/package | SHA I/O | Stop | CUDA free before→after | CUDA wait | Teardown |
| ---: | --- | --- | --- | --- | --- | ---: | ---: |
| 1 | `26C3F25FEB1CB32D` | PASS `97FD7B37…` | PASS `7DA1E743…` | PASS | 4462→4462 MiB | 106 ms | 9064 ms |
| 2 | `FB420D502E6A5720` | PASS `97FD7B37…` | PASS `06622A3E…` | PASS | 4462→4471 MiB | 76 ms | 5026 ms |
| 3 | `BE9EFA44890FECC0` | PASS `97FD7B37…` | PASS `4BE5ADF3…` | PASS | 4471→4471 MiB | 57 ms | 4018 ms |

All rounds ran real GPU-PV CUDA on `NVIDIA GeForce RTX 2060`, created a 64 MiB
`RAMSHARE VRAMDISK` on `S:`, performed one write/flush/read SHA proof, and stopped without
force-kill. The product logged exact live identity, Gate A/B pagefile absence, volume lock with
I/O pump, flush/dismount, unlock, destroy, and correlated broker `lease 1 liberado`.

## Fixes additionally proven by this run

- Stale DriverStore packages are purged selectively by published INF whose original name is
  `ramshared.inf` before install. The earlier `E297B73F…` ghost image was removed; the guest image
  matched the new package `97FD7B37…`.
- CUDA free restoration is still required within 64 MiB, but the harness now polls briefly before
  failing the gate so asynchronous GPU-PV/NVIDIA release does not create a false negative.
- Terminal state: VM Off, host RTX 2060 OK, no new dumps.

## Non-claims

This is the isolated GPU-PV storage-only product gate. It is not a physical daily-host storage
claim, not a pagefile/pressure claim, not an isolated WSL2 freeze-elimination claim, and not a
Partner Center attestation claim.
