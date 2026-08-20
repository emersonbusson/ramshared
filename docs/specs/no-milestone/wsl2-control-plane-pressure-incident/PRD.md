---
title: "WSL2 control-plane pressure incident hardening"
status: "implementation"
---

# PRD — WSL2 control-plane pressure incident hardening

## Problem

On 2026-08-20 the daily WSL2 VM became unreachable twice during concurrent
build, test, container, and agent workloads. The previous RamShared health
contract described swap topology but did not prove that the VRAM tier could
accept writes before the WSL control plane degraded.

The incident is not attributed to one component without a controlled
reproduction. Recorded evidence shows zero VRAM and disk-swap use, increasing
zram use, a final telemetry gap, and an `hv_sock`/udev timeout storm. No Linux
OOM-killer, NBD error, or safe terminal state was recorded.

## User journeys

- As a WSL2 operator, I need `ok` to become false before topology-only health
  can be mistaken for effective protection.
- As a GPU owner, I need a requested 4 GiB profile to fall back to a fully
  committed 2 or 1 GiB profile instead of advertising storage that may fail
  later.
- As a developer, I need heavy work to run in a bounded scope that preserves
  memory for VMBus and recovery.
- As an upstream maintainer, I need a sanitized, stock-kernel-compatible
  reproducer with official WSL traces.

## Requirements

1. Product NBD swap advertises only capacity committed and zeroed before
   activation. Sparse capacity is experimental and cannot be auto-started.
2. A 4 GiB maximum uses deterministic 4→2→1 GiB admission with a reserve of
   `max(1 GiB, 20% of total VRAM)`.
3. Managed workloads preserve `max(2 GiB, 20% of MemTotal)` and target only
   their own systemd slice during recovery.
4. Status separates topology from protection and cannot report stale or
   unverified capacity as healthy.
5. Guest telemetry is sealed, sampled every two seconds, and mirrored to a
   Windows-visible heartbeat. The Windows watchdog never changes WSL VM
   lifecycle automatically.
6. Boot activation stays disabled until isolated and supervised gates pass.
7. A local incident snapshot is plan-only by default. Its explicit capture
   mode bounds every `wsl.exe` query, records relevant Windows service/event
   state plus sanitized `.wslconfig` kernel/swap state with a SHA-256 manifest,
   and points the operator to Microsoft's current collector for manual,
   reviewed, elevated use.
8. Status attributes disk swap only to growth above an activation baseline;
   pre-existing disk pages remain visible but are not product use.
9. One typed observer feeds the terminal dashboard, JSONL log, and atomic host
   heartbeat. Samples expose age and measurement errors, bound GPU queries to
   two seconds, retain five minutes of default UI history, and rotate logs at
   50 MiB.
10. A Windows viewer reads only the host heartbeat. btop integration is
    plan-first, backed up, and explicitly reversible.

## Non-goals

- Claiming that RamShared caused the VM termination.
- Automatically repairing Relay state or terminating the WSL VM.
- Running pressure on the daily host without the existing approval harness.
- Combining the crash report with microsoft/WSL#41054.
- Downloading or automatically executing Microsoft's diagnostic collector.
- Treating all physical GPU allocation as RamShared swap use.
- Adding activation, pressure, or lifecycle controls to the monitor.
