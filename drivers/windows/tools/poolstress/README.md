# poolstress (ITEM-8) — VM-only test driver

**SPEC:** windows-swap-driver DT-11 / ITEM-8.

Forces **incompressible paged-pool** allocations toward the WinDrive pagefile so kernel-page
residency can be measured before killing `ramshared-winsvc`.

| Rule | Detail |
| --- | --- |
| Distribution | **Never ship** with the product package |
| Signing | Test-signing **inside disposable VM only** |
| Host | **Forbidden** on the daily development host (RNF-6) |

## Files

| File | Role |
| --- | --- |
| `poolstress.c` | ALLOC / READBACK / FREE IOCTLs |
| `poolstress.inf` | Root-enumerated test INF (lab only) |

## IOCTLs

1. `ALLOC(n_gb)` — `ExAllocatePool2(POOL_FLAG_PAGED)` + `BCryptGenRandom` + touch every page
2. `READBACK` — walk pages (force page-in after service kill)
3. `FREE` — release pool

## Drill

Use `scripts/windows/Invoke-KernelPageDrill.ps1` (DT-21: `% Usage` pagefile-VRAM > 0 before kill).
