# Guest product Online — STOP_OK PASS — 2026-07-16

Campaign: `guest-product-online-20260716-174238`  
Artifact: `C:\ramshared\artifacts\guest-product-online-20260716-174238`

## Results

| Gate | Result |
| --- | --- |
| BINARY_MATCH | **PASS** `CD7E315D0DA5B24BB05C384846D7BA8123390300D2C3A3F73B10E52F9E80BC34` |
| Product Online + CUDA | **PASS** serial `E688A3B1F1D1F0C0` |
| Disk | `N=2 Name=RAMSHARE VRAMDISK Size=67108864` letter `S` |
| 3-round SHA | **PASS** |
| Graceful stop (`stop.request`) | **PASS** `STOP_OK=true` forceKilled=false |
| Lease release | **PASS** `lease 1 liberado` |

## Stop root-cause (fixed)

Teardown previously hung because:

1. **Identity** used PowerShell `Get-Partition` / `Path::exists("S:\")` which hang under GPU-PV.
2. **Gate A** used PowerShell CIM pagefile query (multi-second timeouts).
3. **Volume lock** `CreateFile(\\.\S:)` deadlocked when the product I/O loop was already stopped (NTFS waits on miniport; miniport waits on COMMIT).

### Fix (Day-0)

- Identity: CREATE-time serial + letter + size (no PowerShell / no volume root exists).
- Gate A/B: registry `PagingFiles` (no PowerShell).
- Volume lock: keep serving COMMIT while `CreateFile`/`FSCTL_LOCK` runs on a helper thread, then dismount → unregister → destroy → wipe → lease release.

Teardown wall time after stop.request: ~2s (Identity→Stopped).

## Terminal

VM Off after harness; host RTX 2060 OK.

## Summary JSON

```json
{
  "ONLINE": true,
  "BINARY_MATCH": true,
  "ROUNDS_PASS": true,
  "STOP_OK": true,
  "LETTER": "S",
  "DISK": "N=2 Name=RAMSHARE VRAMDISK Size=67108864 Ser=[E688A3B1F1D1F0C0]"
}
```
