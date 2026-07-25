# ADR-0007 — Windows local broker IPC and SCM supervision

- **Status:** Accepted
- **Date:** 2026-07-25
- **SPEC:** [`windows-autonomous-broker-service`](../specs/no-milestone/windows-autonomous-broker-service/SPEC.md)

## Context

The Windows storage consumer previously assumed an externally launched TCP
broker. That did not provide package atomicity, local peer identity, ordered
SCM lifecycle or a legitimate unattended demand-start surface.

## Decision

1. Install `RamSharedBroker` as a separate own-process SCM service running as
   `NT SERVICE\RamSharedBroker`.
2. Keep `RamSharedWinSvc` as LocalSystem and the sole owner of CUDA, queues,
   LUN exposure, I/O pumping and destructive teardown.
3. Make the consumer depend on the broker and communicate only through
   `\\.\pipe\RamSharedBroker.v1`.
4. Authenticate the exact enabled, non-deny-only consumer service SID after
   the first bounded pipe read. DACL access alone is not peer authentication.
5. Package both executables, configs and driver artifacts in one immutable
   SHA-256 manifest. Services are demand-start unless a separately accepted
   manifest explicitly promotes automatic start.
6. After Online pipe loss, never reconnect or replay. Keep the I/O pump alive
   until the existing identity/pagefile/lock teardown completes or refuses.
7. Retain demand-start after ITEM-9. Automatic start was not exercised by the
   accepted physical campaign and is not promoted while distribution remains
   test-signed.

## Consequences

- The installed daily profile has no TCP listener.
- Broker crash recovery is bounded to 5/15/40 seconds and then NONE.
- A logical lease is never authority to bypass storage safety gates.
- Public distribution still depends on production signing; this ADR closes
  only the supervised Test Mode autonomous-service topology.

## Rollback

Use the product controller's stop-first rollback to a retained complete
manifest. Never replace individual files or roll back while the consumer is
Online. Rollback triggers include peer-authentication failure, mixed manifest
state, BINARY_MATCH failure, non-zero residue or stop over 45 seconds.
