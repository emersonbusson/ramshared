# poolstress (ITEM-8) — VM-only test driver

**SPEC:** windows-swap-driver DT-11 / ITEM-8.

Forces **incompressible paged-pool** allocations toward the WinDrive pagefile so kernel-page
residency can be measured before killing `ramshared-winsvc`.

| Rule | Detail |
| --- | --- |
| Distribution | **Never ship** with the product package |
| Signing | Test-signing **inside disposable VM only** |
| Host | **Forbidden** on the daily development host (RNF-6) |

Implementation files (`poolstress.c`, `poolstress.inf`) land with ITEM-8; this directory is reserved.
