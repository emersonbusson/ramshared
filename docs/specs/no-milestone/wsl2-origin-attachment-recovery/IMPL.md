# IMPL — Attended recovery of the sealed WSL2 origin attachment

Status: **PARTIAL — local manufactured/static gates green; live VHDX proof
environment-bound**.

The implementation must remain limited to the SPEC: one identity-bound,
attended bare-VHD attachment followed by independent host-gate validation.
No live attachment, swap, NBD, GPU, or pressure result is claimed in this
record until the named checks and before → action → after proof are complete.

## Local Step 3 results (2026-10-01)

| Gate | Exact result |
| --- | --- |
| `Manage-RamSharedOrigin.ps1 -Action test -Run` | 17 passed, 0 failed, exit 0 |
| Named row `origin_attach_decision_is_idempotent_and_fail_closed` | PASS |
| `Test-RamSharedOriginStatic.ps1` | exit 0, `PASS origin_test_mode_skips_live_host_discovery` |
| `./scripts/docs-check.sh` | `✓ docs-check OK`, exit 0 |
| Swap/cascade/stress side effects | none — zero references in either script; test mode exits before any action switch |

## Environment-bound (not claimed)

- Live VHDX proof, exact PARTUUID visibility, and host-gate receipt require a
  physical Windows host with a sealed origin VHDX. This row stays open.
