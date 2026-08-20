# Draft comment for microsoft/WSL#40795

This file is a draft. Review and sanitize it before posting.

We observed two WSL VM control-plane timeouts on 2026-08-20 while running
concurrent builds, tests, containers, and development processes under a 16 GiB WSL
memory cap. The incidents occurred on WSL 2.7.11; 2.7.12 was installed only
after the second termination.

Observed terminal errors:

- `Wsl/Service/HCS_E_CONNECTION_TIMEOUT`
- `Wsl/Service/0x8007274c`
- `Wsl/0x8007000e`

Guest evidence:

- No Linux OOM-killer record and no NBD I/O error.
- First event: zram reached 1,041,436/1,048,572 KiB; lower swap tiers remained
  unused.
- Second event: the final samples showed rapidly increasing zram use followed
  by a 6m32s telemetry gap.
- Immediately before termination, many `systemd-udevd` workers handling
  VMBus `hv_sock` devices exceeded their expected duration. Docker health
  checks also timed out.
- The journal ended abruptly rather than recording a clean shutdown.

The signature appears related to this issue, but we cannot claim an exact
root cause from guest logs alone. We also compared the 2.7.12 tag with merged
PR #41252: the release tag still uses the unbounded session-leader accept path,
while current master passes `SESSION_LEADER_ACCEPT_TIMEOUT_MS`. Please confirm
whether #41252 is scheduled for a 2.7.x backport and whether the `hv_sock`
worker storm can be a consequence of the same partial connection/Relay path.

Environment qualification: this daily host currently has a `kernel=` directive
in `.wslconfig`, so its running guest kernel is not the stock kernel bundled
with the installed WSL package. We will treat a stock-kernel/config A/B on a
disposable host as required evidence before attributing the failure to a WSL
release configuration.

We can provide:

1. a sanitized timestamped journal/health bundle with SHA-256 inventory;
2. a bounded local post-incident snapshot plus official
   `collect-wsl-logs.ps1`/ETW output from a disposable reproduction;
3. WSL 2.7.12 versus a build containing #41252;
4. Microsoft stock kernel versus the official tree with config-only deltas.

The separate #41054 feature request remains focused on enabling
`CONFIG_BLK_DEV_UBLK` and `CONFIG_ZRAM_WRITEBACK`; we are not using this
crash report as evidence that those options caused the incident.

## Operator evidence procedure (do not post unreviewed artifacts)

The repository's local helper defaults to a non-mutating plan:

```powershell
.\scripts\windows\Capture-WslIncidentSnapshot.ps1
```

After reviewing the plan, capture the bounded local snapshot with `-Run`.
It writes a SHA-256 manifest under `C:\ramshared\artifacts`, does not change
WSL lifecycle, and does not start a workload or pressure test.

For the authoritative WSL bundle, download the current Microsoft script from
`https://raw.githubusercontent.com/microsoft/WSL/master/diagnostics/collect-wsl-logs.ps1`,
record its SHA-256, review it, and run it manually from an elevated PowerShell
console. Do not use its WSL-restart reproduction option on the daily host.
Sanitize paths, user identifiers, and unrelated process data before attaching
the local or official bundle to the issue.
