# Guest exhaustive IOCTL + Driver Verifier PASS — 2026-07-16

Campaign: `guest-exhaustive-20260716-224913`  
Artifact: `C:\ramshared\artifacts\guest-exhaustive-20260716-224913`  
VM: `win11-drill`  
Miniport package SHA-256: `97FD7B373ED7DD5AE7F38204070F8B89E08A2B25616AA2A128995E8D1FBFF34F`

## Summary

```json
{
  "ARTIFACT": "C:\\ramshared\\artifacts\\guest-exhaustive-20260716-224913",
  "IOCTL_PASS1": "PASS",
  "IOCTL_VERIFIER": "PASS",
  "VERIFIER_RAN": true,
  "LEAVE_VM_ON": false
}
```

## Verdicts

Both normal and Verifier passes reported every required ITEM-3 verdict as `1`:

- `PASS_VALID_QUEUE`
- `REFUSE_FOREIGN_OWNER`
- `REFUSE_RESERVED_REGISTER`
- `REFUSE_BAD_RING`
- `REFUSE_RING_INDEX_JUMP`
- `REFUSE_RESERVED_CQE`
- `REFUSE_UNKNOWN_IOCTL`
- `REFUSE_RESERVED_DISK_PARAMS`
- `COMPLETION_REENTRY_NO_SLOT_REUSE`
- `RUNDOWN_UNMAP_AFTER_COPY`
- `VPD_SERIAL_MATCH`
- `NO_NEW_DUMP`

Driver Verifier was active with flags `0x2093B`; `verifier /query` listed
`MODULE: ramshared.sys (load: 1 / unload: 0)`. No new dumps were observed. The harness reset
Verifier best-effort and stopped the VM at the end.

## Deploy hardening proven by this run

- DriverStore/package `BINARY_MATCH` was enforced on SHA `97FD7B37…`.
- The initial DriverStore purge removed stale `ramshared.inf` packages before install.
- After a post-deploy reboot, the root-enumerated `ROOT\RAMSHARED\0000` device was absent; the
  harness recreated it via SetupAPI (`rootRecreateAfterReboot=OK reboot=False`) before pass 1.
- Before each IOCTL pass, the harness required both `ROOT\RAMSHARED\0000` and the `SCSIAdapter`
  surface to be `OK|problem=0`.

This closes the current signed package's IOCTL/Verifier/VPD gate. It does not claim a dedicated
StartIo READ-copy live race campaign beyond the existing ring/IOCTL concurrency injectors.
