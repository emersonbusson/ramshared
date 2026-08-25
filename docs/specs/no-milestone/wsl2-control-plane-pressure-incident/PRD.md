---
slug: wsl2-control-plane-pressure-incident
title: WSL2 control-plane pressure containment
milestone: —
issues: []
---

# PRD — WSL2 control-plane pressure containment

## 1. Summary

RamShared must preserve an independently recoverable WSL2 control plane while
multiple builds, containers, browser tests, and interactive sessions compete
for one 16 GiB guest budget. Admission is aggregate, pressure decisions are
preventive, and the Windows guardian may terminate only the named distro after
the guest is independently proven inaccessible. It never reboots Windows.
The 2026-08-22 terminal incident is now classified `host_volume_exhausted`;
these controls remain independent defense-in-depth and are not a claim that
RamShared caused or fixes that storage exhaustion.

## 2. Technical context

- **Confirmed in evidence:** the last incident heartbeat showed about 1 GiB of
  zram use, about 26 MiB of VRAM use, and no attributable disk-swap growth.
- **Confirmed in codebase:** the current workload launcher computes each scope
  from the same `MemAvailable` snapshot, so simultaneous launches can reuse one
  apparent allowance.
- **Confirmed in codebase:** the current Windows watchdog depends on a guest
  `systemctl kill` command and has no persistent task, independent HCS check,
  safe-mode boot, or targeted distro termination path.
- **Confirmed in codebase:** status schema v3 can report topology separately
  from protection, but it lacks supervisor and guardian states and samples only
  PSI avg10.
- **Confirmed host evidence:** NTFS Event ID 137 on volume `I:` at
  2026-08-22 07:06:17 -03:00 contains little-endian `0xC000007F`, decoded by
  Windows as `STATUS_DISK_FULL`.
- **Confirmed negative evidence:** no
  `Microsoft-Windows-Resource-Exhaustion-Detector` event was found from
  2026-08-20 through the check. This does not prove that memory pressure was
  absent.
- **Confirmed current state:** the VHDX chain is not compressed or sparse and
  the SSD/NTFS report healthy.
- **Superseded inference:** guest control-plane starvation is no longer the
  causal classification for this incident. Storage attribution to a specific
  writer, including RamShared, was not established.

## 3. Recommended option

Use one aggregate `ramshared-workloads.slice`, a protected
`ramshared-control.slice`, a serialized admission ledger, and a one-second
supervisor state machine. Disabled-definition behavior would use a persistent
plan-first Windows guardian with an
independent heartbeat, two bounded guest probes, and a bounded WSL/HCS probe
> **Disabled staging only / no execution:** The following former containment
> definition is inert evidence, not a current WSL or host command.
before at most one `wsl --terminate <SANITIZED_WSL_DISTRO>`. It would write safe
mode before restarting the distro. This is not a current execution path;
`wsl --shutdown` and automatic Windows restart are forbidden.

Discarded: independent per-process `MemAvailable` budgets (double admission),
guest-only watchdog recovery (same failure domain), and automatic host reboot
(unbounded blast radius).

## 4. Functional requirements

- **RF-1:** Calculate `control_reserve = max(4 GiB, 25% MemTotal)`, workload
  `MemoryMax = MemTotal - control_reserve`, and workload
  `MemoryHigh = MemoryMax - max(1 GiB, 10% MemTotal)`.
- **RF-2:** Serialize reservations across all managed sessions under one
  validated persistent ledger-directory inode. Refuse symlink, multiply-linked
  regular-file, wrong-owner, nonregular, or unsafe-mode authority components;
  revalidate the directory, transition owner, quarantine, and ledger identities
  around every read/write. The sum of live reservations must never exceed
  workload `MemoryMax`; dead owners are quarantined before reuse.
- **RF-3:** Support `interactive`, `build`, `browser-test`, and `batch` classes.
  Discard order is batch, browser-test, build, interactive.
- **RF-4:** Provide `ramshared run --class ... [--memory-max MiB] -- <cmd>` and
  `ramshared session --class ...`; neither stores command lines in telemetry.
- **RF-5:** Implement supervisor states `HEALTHY`, `GUARDED`, `CRITICAL`, and
  `EMERGENCY` with the exact thresholds and hysteresis in the SPEC.
- **RF-6:** Publish schema v4 status with independent `control_state`,
  `cache_state`, `guardian_state`, and `overall_state`. `ok` is true only when
  the worst mandatory plane is healthy.
- **RF-7:** Record PSI some/full avg10/60/300, memory availability, swap-in/out,
  memory events, active managed scopes, sanitized top-N, origin/cache counters,
  and one ordered typed result for every supervisor action attempted in the
  current decision. Earlier failures must never be overwritten by a later
  success. Action errors are single-line, UTF-8-safe, control-free, explicitly
  truncated, and bounded to 1024 bytes at the type boundary.
- **RF-8:** Install the Windows guardian as an elevated scheduled task under the
  current user SID, with restart-on-failure and no execution time limit. Install
  is plan-first and preserves the previous XML and configuration for rollback.
- **RF-9:** Terminate the distro only after heartbeat age is at least 15 seconds,
  two 5-second guest probes fail, an independent WSL/HCS probe fails or times
  out, and host-side evidence is closed.
- **RF-10:** Before termination, atomically write a host-owned safe-mode gate
  bound to the configured distro and prior boot ID. On every guest boot, heavy
  services require an ephemeral host-issued resume lease and therefore cannot
  start before the guardian verifies the new boot ID. After restart the
  guardian mirrors safe mode into the guest; both gates remain until
  `ramshared recover --resume` observes 60 healthy seconds and removes only the
  matching incident gates.
- **RF-11:** Report large processes outside the managed hierarchy as
  `UNMANAGED_PRESSURE`.
- **RF-12:** Classify postmortem evidence as independent facts:
  `guest_pressure_unresponsive`, `guest_oom`, `kernel_warning_at_boot`,
  `kernel_crash`, `host_reboot`, and `wsl_terminate`.
- **RF-13:** Persist the exact systemd `InvocationID` returned after scope
  creation plus a monotonic durable issuance ordinal. Revalidate InvocationID
  immediately before freeze, thaw, TERM, or KILL. Persist pending/applied freeze
  identity before/after the effect and successful TERM identity/time before any
  later KILL. Restart, ledger replacement, unit-name reuse, or unavailable
  identity must never retarget the action.
- **RF-14:** Keep the NBD backend in a lifecycle domain that systemd cannot
  terminate until an exact swapoff proof shows the managed NBD is absent from
  `/proc/swaps`. A separate controller requests teardown; host recovery records
  and resolves an incomplete proof without broad WSL shutdown.
- **RF-15:** Classify a temporally matching NTFS Event ID 137 only as
  `host_volume_exhausted` when its evidence also contains `0xC000007F` or
  `STATUS_DISK_FULL`; keep product attribution separate and explicit.

## 5. Non-functional requirements

- **NFR-1:** Supervisor sampling cadence is 1 second; a delayed sample of at
  least 3 seconds is itself an emergency signal.
- **NFR-2:** Control units use at least `MemoryLow=1 GiB`, `MemoryMin=512 MiB`,
  `CPUWeight=1000`, and `IOWeight=1000` and are not oomd candidates.
- **NFR-3:** Workload slice uses `TasksMax=8192`, `CPUWeight=50`, `IOWeight=50`,
  oomd pressure kill, and a 10% pressure limit.
- **NFR-4:** Top-N telemetry includes only `comm`, unit/cgroup, RSS, swap, CPU,
  and I/O counters. It never records argv, private paths, or environment.
- **NFR-5:** All host and guest commands are bounded. A trusted guest helper
  uses an invocation-private process group, finite capture storage, non-reaping
  Linux exit observation, residual-group termination while the zombie leader
  pins PID/PGID, capture joins, and a bounded final direct-child reap. Spawn
  callback panic retains the same RAII custody. If group SIGKILL cannot prove
  the direct child reaped, the controller or observer enters stable nonzero
  fatal containment rather than continuing with stale state. Helpers that
  intentionally daemonize or escape the group are outside this contract. Retry
  is limited to the two specified guest probes; deterministic failures are not
  retried.
- **NFR-6:** No source or test path may contain an automatic Windows restart, `wsl --shutdown`, or a broad terminate action.
- **NFR-7:** Backend stop has no finite kill escalation. Missing swapoff proof
  leaves it running and reports blocked recovery.

## 6. Flows

1. A managed command requests a class reservation.
2. The admission ledger locks, reaps dead owners, verifies safe mode and
   supervisor state, sums reservations, and issues a unique token.
3. `systemd-run` creates a scope under the aggregate slice with the reserved
   per-scope ceiling. Admission persists the observed `InvocationID`; completion
   releases the token, while control actions require the same live identity.
4. The supervisor samples once per second and closes admission in `GUARDED`.
5. `CRITICAL` requests cache shrink and freezes the highest discard-priority,
   heaviest managed scope.
6. `EMERGENCY` sends TERM to the selected scope and KILL after five seconds
   only if recovery has not occurred.
7. If the guest stops answering, the Windows guardian completes all four proof
   gates, captures evidence, writes the host-owned safe-mode gate, targets only
   the configured distro, restarts it without a resume lease, verifies a new
   boot ID, and only then mirrors safe mode into the guest.

Errors are fail-closed: admission refusal exits nonzero without a scope;
guardian uncertainty preserves artifacts and alerts without escalation.

## 7. Data / state model

The admission ledger stores token, durable issuance ordinal, PID, scope unit,
class, and reserved bytes. Its canonical directory inode persists and is
protected by a nonblocking exclusive advisory file lock; a separate transition
owner and append-only quarantine retain crash evidence. The supervisor record
stores state, state age, healthy duration, and ordered typed action results with
action, success/failure status, and an optional bounded canonical error, plus
sample delay, memory reserve, MemAvailable, and PSI full. Separate durable
records bind pending/applied freeze and pending/successful TERM/KILL to one exact
unit+InvocationID across restart. Admission lock and reservation owners contain
boot ID, PID, `/proc/<pid>/stat` start time, and a random nonce so PID reuse is
not ownership.
The host safe-mode gate stores the configured distro, reason, UTC time,
incident ID, and prior boot ID. Schema v4 embeds those typed planes without
command lines.

## 8. Interfaces

- `ramshared run --class <interactive|build|browser-test|batch> [--memory-max MiB] -- <cmd>`
- `ramshared session --class <...> [--memory-max MiB]`
- `ramshared supervise [--once]`
- `ramshared recover --status|--resume`
- `ramshared monitor [--compact|--jsonl]`
- `ramshared status --json` schema v4
- Windows guardian: `install`, `status`, `capture`, `uninstall`, and internal
  scheduled `watch` mode.

## 9. Dependencies and risks

Docker/containerd/BuildKit and cron placement require an attended later rollout
because Docker restart changes live host state. The source provides sealed
drop-ins and verification, but does not activate them in this implementation.

Rollback trigger: any admitted sum above workload `MemoryMax`; monitor or
daemon unresponsive for 3 seconds under a bounded synthetic unit test; more
than one targeted terminate for one incident; any host reboot route; or a
guardian terminate while either independent guest probe succeeds.

## 10. Implementation strategy

Implement pure budget, reservation, and state transition tests first; then CLI
wiring, unit files, schema v4, guardian static/manufactured tests, and
postmortem fixtures. Host installation and pressure remain separate attended
rollout gates.

## 11. Documents to update

`README.md`, `README.pt-BR.md`, `ARCHITECTURE.md`, `docs/FAQ.md`, the incident
report, upstream draft, `docs/reliability/DEGRADATION-MATRIX.md`,
`docs/reliability/GAP-REGISTER.md`, `validation.md`, and this SSDV3 folder.

## 12. Out of scope

Live Docker restart, task installation, VHDX creation/attachment, induced WSL
inaccessibility, artificial pressure on the daily host, Windows reboot, and
external publication.

## 13. Acceptance criteria

All named tests in the SPEC pass; changed Rust business-logic files reach at
least 80% line coverage; PowerShell static/manufactured tests prove the four
guardian gates and forbidden actions; source-only before/action/after uses
`/bin/true`; live VM evidence remains explicitly partial.

## 14. Validation plan

Unit: budget, aggregate ledger, state thresholds, status precedence, sanitized
telemetry, recovery. Integration: concurrent reservation processes and
`systemd-run /bin/true` when permitted. Static Windows: scheduled-task XML,
probe ordering, single terminate, safe boot, and no reboot. Live destructive
proof is VM-only and env-bound for this source implementation.
