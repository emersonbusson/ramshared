---
slug: wsl2-cascade-orphan-recover
title: "WSL2 cascade orphan detection and bound recovery"
milestone: —
issues: []
---

# PRD — Orphan Detection and Bound Recovery

Status: **supersedes the legacy auto-recover from 2026-07-10**. Live enumeration is
detection-only. No device without an exact binding is modified, including
when `/proc/swaps` reports `Used=0`.

## Problem

An abrupt termination of WSL can leave NBD, ublk, or zram visible without the expected
userspace runtime records. The legacy contract treated `used_kb == 0` as authorization
for `swapoff`, reset, and disconnect. That is insufficient: the entry is still
active, the name could have been reused, and enumeration does not prove ownership.

## Requirements

- **RF-R1:** Read `/proc/swaps` with a strict parser returning `Result`. Header,
  all rows, types, numbers, and uniqueness are mandatory.
- **RF-R2:** Treat any present entry as an active swap, even with zero usage.
  Missing, malformed, or ambiguous reads preserve backend and evidence.
- **RF-R3:** NBD/ublk/zram discovered without an exact RamShared binding generates refusal and
  zero mutating commands.
- **RF-R4:** A lifecycle mutation requires a sealed binding with boot ID, daemon PID and
  start time, InvocationID, socket/export, origin identity
  (PARTUUID/PTUUID/`dev_t`/UUID/hashes), identity of each device, and exact cardinality.
- **RF-R5:** Before each `swapoff`, reset, disconnect, delete, or backend stop,
  re-read and revalidate all authority. Reset/disconnect/delete require
  fresh strict proof of device absence in `/proc/swaps`.
- **RF-R6:** Foreign, duplicate, retargeted, or mismatched auxiliary records
  remain untouched and block the entire teardown.
- **RF-R7:** Failure or uncertain outcome of `swapoff` preserves daemon, backend,
  binding, and forensic markers. "Not found" messages may only advance after
  fresh strict proof of absence.
- **RF-R8:** The standalone ublk path is forbidden on WSL2 without any override. On isolated
  Linux, it executes swapoff-first and preserves everything in recoverable NO-GO.

## Flows

### Orphan Without Binding

1. Read strict snapshot.
2. Detect managed-looking device without sealed binding.
3. Return refusal; never call swapoff, zramctl, nbd-client, STOP_DEV,
   DELETE_DEV, or daemon signals.

### Bound Lifecycle

1. Open sealed binding and revalidate daemon, InvocationID, socket, origin,
   records, and live devices.
2. Execute all required `swapoff` invocations.
3. Before each reset/disconnect, obtain a fresh strict snapshot and prove exact absence.
4. Prove device disappearance before stopping daemon.
5. Remove evidence only after complete terminal success.

## Out of Scope

- Automatic recovery by device shape/name.
- `wsl --shutdown`, terminate, or restart as a CLI side effect.
- Standalone ublk on WSL2.
- Testing on real devices; source proofs are hermetic.

## Acceptance

- Foreign, ambiguous, retargeted, duplicate, and unbound fixtures execute
  zero commands.
- Active swap with zero usage and `swapoff` failure preserves backend/evidence.
- Unreadable/malformed snapshot refuses before any mutation.
- Valid teardown proves swapoff-first and fresh revalidation per action.
- No ublk override on WSL2 remains in source, template, or test.
