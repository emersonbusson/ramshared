# Guest product Online re-run — PARTIAL — 2026-07-16

Campaign: `guest-product-online-20260716-151304`  
Harness: `Run-GuestProductOnline.ps1` with stop.request re-assert every 2s (180s budget).

## Results

| Gate | Result |
| --- | --- |
| BINARY_MATCH | **PASS** `CD7E315D0DA5B24BB05C384846D7BA8123390300D2C3A3F73B10E52F9E80BC34` |
| Product Online + CUDA | **PASS** serial `A0B4FCE26201BD5D` |
| Disk | `N=2 Name=RAMSHARE VRAMDISK Size=67108864 Ser=[A0B4FCE26201BD5D]` letter `S` |
| 3-round SHA | **PASS** |
| Graceful stop | **FAIL** forceKilledConsole=True |
| Lease release log | **FAIL** (no `lease liberado`) |

### Round SHAs
```
[
  {
    "round": 1,
    "match": true,
    "sha": "E03F9C1E8478A197602B1A6013F25A4144EE8F7FA76DC676E64B00AA207CB025"
  },
  {
    "round": 2,
    "match": true,
    "sha": "25F084BF69C4027D09174FF47137DF1E47F0F871ABB845F9E7021FD3295CB73F"
  },
  {
    "round": 3,
    "match": true,
    "sha": "9173B0E11A3C7A9E03CEB66A6C38CE07C2752274E894016FDA852E41411FBF3F"
  }
]
```

### Online line
```
product Online: run_id=run-4580-… lease=1 size=67108864 serial=A0B4FCE26201BD5D cuda=NVIDIA GeForce RTX 2060
```

## Stop root-cause (evidence-based)

Console stderr never printed `Stopping` / `FailedSafe` / teardown refuse text before force-kill.
Broker continued receiving `psi` heartbeats until TCP close on force-kill.

Interpretation: runtime either never observed `stop`, or Gate A/identity/lock refused (code 7),
cleared the stop flag, and resumed Online. Re-asserting `stop.request` for 180s still did not exit.
Next: capture `teardown refused` stderr by unbuffering, and pre-stop exclusive volume probe to
classify lock vs identity.

## Terminal

VM Off (forced after campaign); host RTX 2060 OK.

## Summary JSON

```json
{
  "ARTIFACT": "C:\\ramshared\\artifacts\\guest-product-online-20260716-151304",
  "ONLINE": true,
  "BINARY_MATCH": true,
  "SYS_SHA": "CD7E315D0DA5B24BB05C384846D7BA8123390300D2C3A3F73B10E52F9E80BC34",
  "ROUNDS_PASS": true,
  "STOP_OK": false,
  "LETTER": "S",
  "DISK": "N=2 Name=RAMSHARE VRAMDISK Size=67108864 Ser=[A0B4FCE26201BD5D]"
}
```
