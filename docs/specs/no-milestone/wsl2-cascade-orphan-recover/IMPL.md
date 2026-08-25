# IMPL — Orphan Detection and Fail-Closed Lifecycle

Status: **source corrected; hermetic serial validation complete; live blocked**.

## Implemented

- Strict `/proc/swaps` parser with explicit errors and temporal seams.
- NBD/ublk/zram enumeration for detection only.
- Atomic, sealed binding with complete daemon identity, socket, origin, and
  devices, plus exact cardinality.
- Fresh authorization and revalidation before each action.
- Exact live cardinality per stage; missing bound device is NO-GO,
  not a reason to skip mutation and proceed.
- All `swapoff` operations before reset/disconnect/daemon stop.
- Strict absence required before reset/disconnect/delete.
- Preservation of binding, records, daemon, and forensics on any NO-GO.
- Standalone ublk permanently refused on WSL2 and swapoff-first on isolated Linux.
- zram registered/revalidated before `mkswap`; rollback requires the exact record,
  and unowned sysfs fallback was removed.

## Current Evidence

With Rust 1.98 and single-threaded execution, the focused lifecycle suite passed 49/49.
The complete validation of `ramshared-cli` passed 191 unit tests and 6 dispatch
tests without failure. `cargo check -p ramshared-cli` also passed. Hermetic
fixtures cover foreign device, missing bound device, stage cardinality,
active zero-use swap, unreadable/malformed snapshot, uncertain swapon/swapoff outcome,
zram rollback without record, and NBD detach post-check failure. No test called `mkswap` on a real device.

Legacy live results from 2026-07-10 belong to the old implementation. They do not
qualify the current lifecycle and do not authorize restoring zero-use auto-recovery.

## Remaining Live Gates

- None in this source patch is executed automatically.
- Future isolated validation must prove real identity, terminal detach,
  and absence of ghost without using the daily host.
- WSL2 standalone ublk remains NO-GO regardless of QEMU tests.
