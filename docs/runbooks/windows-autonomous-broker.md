# Runbook — Windows autonomous broker service

This runbook covers the supervised Test Mode demand-start product. It does not
authorize normal-Windows public deployment; production signing remains a
separate gate.

## Install or repair

1. Require an elevated administrator shell, Test Mode, the expected loaded
   `ramshared.sys` SHA-256, no RamShared disk, and both services stopped.
2. Read the immutable manifest's `winsvc_config` volume letter. Refuse if that
   letter exists in the drive map or pagefile set. Never remap an existing
   volume.
3. Install:

   ```powershell
   C:\ramshared\bin\ramshared-winsvc.exe install --manifest C:\absolute\product-manifest.json
   ```

4. Confirm both services are demand-start, `RamSharedWinSvc` depends on
   `RamSharedBroker`, and `status --json` reports matching artifact hashes.
5. Repair uses the same command with `repair --manifest`; it must be
   idempotent and must not start either service.

## Start and observe

Start only the consumer; SCM starts the broker dependency:

```powershell
Start-Service RamSharedWinSvc
C:\ramshared\bin\ramshared-winsvc.exe status --json
```

Require one exact-size `RAMSHARE VRAMDISK`, broker registration and lease,
broker/winsvc/driver BINARY_MATCH, and no pagefile on the product volume for
the storage-only drill.

## Supported stop

1. Request consumer stop. PowerShell may report a service-specific request
   error even when safe teardown completes; still require `Stopped` within
   30 seconds.
2. Confirm identity, pagefile gates, volume lock, flush/dismount, queue/LUN
   removal and lease release evidence.
3. Stop the broker only after the consumer is stopped.
4. Require zero RamShared disks within 10 seconds and full product stop within
   45 seconds. Do not force-kill either service.

## Broker loss

- Before exposure: reverse confirmed effects and return broker error code 3.
- After Online: keep the I/O pump alive, enter FailedSafe and use supported
  teardown. Never reconnect, reacquire or treat EOF as lease-release proof.
- If identity/pagefile/lock gates refuse, leave the consumer alive for
  operator recovery. Do not force-destroy the disk.

## Rollback or uninstall

Rollback is stop-first and switches only between complete retained manifests.
Uninstall refuses a missing/corrupt active pointer, running services, owned
storage or ambiguous residue. Never delete or replace one version-owned file
in place.

## Physical acceptance

Use `Run-HostAutonomousLifecycle.ps1` only with explicit physical-host
approval. The harness requires one manifest across three cold boots, a
SYSTEM startup task, a 600-second shutdown watchdog, three SHA rounds per
boot and zero residue before every reboot. Preserve failed evidence; do not
count a corrected rerun as proof that the failed attempt passed.
