# Draft comment for microsoft/WSL#40795

This is an English, human-review draft. Do not post it or attach artifacts
without sanitization and explicit approval.

**WITHDRAWN FOR THIS INCIDENT:** later Windows System evidence identifies
NTFS Event ID 137 with `STATUS_DISK_FULL` on the backing host volume. That
storage-full event is a causal confounder, so this incident is not valid
evidence for a WSL control-plane defect. Keep this draft only as historical
context unless a storage-sufficient reproduction independently recurs.

We observed a third WSL VM control-plane timeout on 2026-08-20, this time on
WSL 2.7.12, while concurrent builds, browser tests, containers, and development
processes shared a 16 GiB guest memory cap.

The last one-second heartbeat does not show exhausted RamShared storage:

- zram was approximately 1 GiB;
- the RamShared VRAM tier held approximately 26 MiB;
- the SSD tier had not grown;
- no NBD I/O error or guest OOM-killer record was found.

This incident does not prove a RamShared failure or a VRAM-to-SSD failure. The
updated classification is `host_volume_exhausted`; the NTFS event does not
identify RamShared or any other specific writer. The final journal ended abruptly, but a
dxg `[ cut here ]` warning occurred at boot, hours outside the incident window;
we classify it separately as `kernel_warning_at_boot`, not as causal evidence
of `kernel_crash`.

Unlike the earlier observations, this incident did not contain a Relay worker
storm or an order-7 allocation failure. PR #41252 is already present in the
2.7.12 source used by this incident, so we are not requesting that change as a
missing backport. The control-plane protection from PR #40519 has not reached
the stable release we tested. Could Microsoft clarify its stable-release plan
for that protection and whether an inaccessible guest/HCS timeout can still
result from control-plane starvation before the Linux OOM killer records an
event?

We are adding local aggregate cgroup containment, an out-of-guest guardian,
and honest pressure telemetry. These mitigations are not presented as a
replacement for a WSL control-plane fix.

We can provide, after review:

1. a sanitized one-second guest JSONL ring and heartbeat;
2. the independent Windows RAM, commit, pagefile, `vmmemWSL`, GPU, and disk
   ring surrounding the incident;
3. a bounded host snapshot and SHA-256 inventory;
4. the official `collect-wsl-logs.ps1`/ETW package from a disposable
   reproduction.

The separate #41054 request remains a kernel-configuration request for ublk
and zram writeback. It is not evidence for this incident and should remain a
separate discussion.

## Evidence preparation only

Use the local capture helper in plan mode first:

```powershell
.\scripts\windows\Capture-WslIncidentSnapshot.ps1
```

Review the plan before using `-Run`. For official logs, record and review the
SHA-256 of Microsoft's current `collect-wsl-logs.ps1`, run it manually on the
disposable reproduction, then remove user names, private paths, command lines,
tokens, cookies, and unrelated process data. No repository command publishes
this draft or its evidence automatically.
