# StartIo READ-copy race claim — 2026-07-17

## Status

**CLAIMED** on isolated guest `win11-drill` under Driver Verifier `0x2093B`.

## Root cause of prior SKIP/RED

1. `Get-Disk` (MSFT_Disk) only appears while the userspace queue is registered **and** enumeration READs are pumped (product Online pattern).
2. Deferred StartIo (end of harness) found Win32-only LUN after earlier UNREGISTER — CreateFile hung or timed out with `sq=0/0`.
3. Stopping the queue pump after Get-Disk hit left CreateFile/READ blocked on pending SQEs.

## Fix (harness only)

`scripts/windows/Invoke-WinDriveIoctlValidation.ps1`:

- Run `Invoke-StartIoReadCopyRaceInjection` **early post-CREATE** (before other IOCTL probes).
- Keep `StartQueuePump` alive for Wait-MsftDisk + PhysicalRead + UNREGISTER race.
- Require MSFT_Disk before `\\.\PhysicalDriveN` open; hang-safe timed COMMIT only when SQ non-empty.

## Campaigns

| Campaign | Verifier | STARTIO | Notes |
| --- | --- | --- | --- |
| `startio-probe-20260717-092819` | no | **1** | `readOk=1 drained=4 sq=4/4 unregOk=1`; full ITEM-3 PASS |
| `startio-verifier-20260717-092950` | **0x2093B** | **1** | `readOk=1 drained=5 sq=5/5 unregOk=1`; `ramshared.sys` load 1/unload 0; NO_NEW_DUMP |

Package: `97FD7B373ED7DD5AE7F38204070F8B89E08A2B25616AA2A128995E8D1FBFF34F`.

## Terminal

- VM `win11-drill` Off after campaigns.
- Verifier `/reset` scheduled on guest before stop.

## Non-claims (unchanged)

- Physical daily-host Online (lab-only + BINARY_MATCH policy)
- SDV (tool absent)
- WSL2 freeze-elimination (isolated lab only; scaffold dry-run refuses daily host)
