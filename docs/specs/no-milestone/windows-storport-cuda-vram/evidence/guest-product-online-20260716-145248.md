# Guest product Online — PARTIAL — 2026-07-16

Campaign: `C:\ramshared\artifacts\guest-product-online-20260716-145248`  
Harness: `scripts/windows/Run-GuestProductOnline.ps1` (lab lease broker + console Online).

## Summary (from campaign `summary.json`)

| Gate | Result |
| --- | --- |
| Package ↔ guest `ramshared.sys` | **BINARY_MATCH** `CD7E315D…` |
| Product Online | **true** |
| CUDA | `NVIDIA GeForce RTX 2060` |
| LUN | `N=2 Name=RAMSHARE VRAMDISK Size=67108864 Ser=[B7A9E1BD0E71541A]` |
| Drive letter | `S` |
| 3-round write/read SHA | **ROUNDS_PASS=true** |
| Graceful stop (`stop.request`) | **STOP_OK=false** (`forceKilledConsole=true` after 60s) |
| Final VM | Off (host recovered) |
| Host GPU after | RTX 2060 **OK**, `nvidia-smi` OK |

## Online line (console stderr)

```text
product Online: run_id=run-4184-1784224438802054200-1 lease=1 size=67108864
  serial=B7A9E1BD0E71541A cuda=NVIDIA GeForce RTX 2060
```

Lab broker granted lease 1 for 67108864 bytes after `register` / `lease_request`.

## Gaps / fixes applied post-run

1. **Graceful stop** did not finish within 60s → force-kill. Harness now waits up to 120s.
2. **guest-result.json** ballooned (~93 MiB) because `Get-Content -Raw` FileInfo objects were
   nested into `ConvertTo-Json` (PSDrive graph). Harness now forces plain strings only.
3. Lab broker JSON must be **UTF-8 without BOM** and hand-built snake_case (fixed before this green Online).

## Classification

**PARTIAL PASS** for guest product path:

- Online + exact serial/size disk + 3-round SHA: **PASS**
- Graceful stop / lease release proof: **FAIL / incomplete** (force kill)

Not a physical-host Online claim. Not DONE for SSDV3 product promotion.

## Terminal

```text
VM=Off
HOST_GPU=OK
GPU 0: NVIDIA GeForce RTX 2060 (UUID: GPU-1d3109d8-6193-e206-f283-3f99e0346db6)
```
