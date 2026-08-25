# IMPL — WSL2 control-plane pressure containment

Status: **PARTIAL — P0/P1 lifecycle correction implemented; live rollout blocked**.

## Current boundary — disabled staging only

This record describes source/static implementation and retained historical
evidence. It authorizes no control-plane install, boot enablement, WSL action,
VM lifecycle, origin/VHDX/GPU/device action, Docker action, or pressure run.
The legacy full-VRAM NBD source-removal prerequisite is closed. All managers
remain disabled/plan-only until the live gates and a separate attended approval
close.

The named removal checker and current-source scan are green. The current
TASK-0009 source checkpoint is recorded below.

## Implemented

- `ramshared-workloads.slice` owns one aggregate budget. For a 16 GiB guest it
  seals `MemoryMax=12 GiB`, `MemoryHigh≈10.4 GiB`, `TasksMax=8192`, low CPU/I/O
  weights, and systemd-oomd pressure policy. Docker/containerd/BuildKit and cron
  definitions descend from child slices.
- `ramshared-control.slice` protects monitor, supervisor, daemon, lifecycle,
  and host-gate units with `MemoryLow=1 GiB`, `MemoryMin=512 MiB`, high CPU/I/O
  weights, and oomd avoidance.
- `ramshared run/session` supports generic workload classes and locks one
  validated persistent ledger-directory inode for each complete transaction.
  Every path component is opened without following links; directory, ledger,
  transition-owner, and quarantine type/owner/mode/link-count identities are
  revalidated around each load, recovery, and store. Recovery appends a
  `create_new` quarantine snapshot, and durable publication is file sync,
  rename, then parent-directory sync. Reservations are summed before scope
  creation and released idempotently for exit, signal, and spawn failure. The
  acknowledged systemd `InvocationID` and lock-issued monotonic ordinal are
  persisted with each reservation.
- The one-second supervisor implements exact HEALTHY/GUARDED/CRITICAL/EMERGENCY
  thresholds, 60-sample recovery, cache reduction, deterministic victim
  choice, freeze/reclaim, thaw-before-TERM, and a bounded five-second KILL
  escalation. Freeze identity is durable before the effect and becomes applied
  only after success. TERM success time and target identity are committed only
  from the executor result, and later KILL can target only that same durable
  identity after restart. Recovery samples cannot repeat destructive actions.
  State schema v3 records every attempted action in execution order with typed
  success or failure, a bounded sanitized error, and no `last_action`
  compatibility field. `CloseAdmission` reports the result of the one canonical
  publication, including lock-busy failure.
- Schema v4 exposes independent control, origin, cache, guardian, and overall
  states. Missing/stale guardian or supervisor evidence, origin failure, cache
  stuck state, critical pressure, and GPU measurement errors cannot be green.
- Monitor JSONL records full PSI windows, memory availability, vmstat swap
  counters, memory events, managed/Docker totals, sanitized top-N, cache/origin
  counters, and the supervisor's ordered action results once per second. Large work outside
  the hierarchy is explicit `UNMANAGED_PRESSURE`; argv and private paths are
  not retained.
- Windows origin telemetry resolves the physical volume from the sealed VHDX
  manifest and reports path, unique volume ID, label, size, and free bytes. It
  contains no production drive-letter default and does not duplicate Guard's
  placement or cleanup policy.
- The Windows guardian is plan-first and is designed for a current-user SID at
  highest run level with restart policy and no duration limit. Its candidate
  recovery policy
  requires a heartbeat stale for 15 seconds, two bounded guest failures, an
  independent WSL/HCS failure, and closed host evidence. It records exactly one
  targeted sealed-distro recovery, writes safe mode first, proves a new boot
  ID, has no Windows reboot or broad WSL shutdown route, and never turns a
  timed-out host child into an unbounded cleanup wait.
- Safe-mode recovery is a durable transaction. Heavy units remain host-gated;
  a future resume state would require 60 healthy samples and matching
  guest/Windows incident identity before issuing a boot-bound lease.
- Plan-first, reversible Linux control-plane installation and Windows shell,
  Terminal, and VS Code launchers are packaged. The postmortem classifier uses
  the incident time window and does not label an old dxg boot warning a crash.
- The sealed candidate package rejects a release missing a required
  control-plane manager, template, or Docker/containerd/cron drop-in before it
  could reach any install target.
- Short-lived CLI helpers now share an invocation-private process-group runner
  with finite stdout/stderr capture. Linux `waitid(..., WNOWAIT)` observes exit
  without reaping so the zombie leader pins its PID/PGID while residual group
  members and inherited pipes are contained; only then is the leader reaped.
  A stable exit-125 fatal path contains an unprovable final reap. Scope-start
  acknowledgement, GPU queries, systemctl actions/identity, preflight probes,
  and cascade helpers use that trusted, non-daemonizing-helper boundary. An RAII
  guard retains exact custody if the spawn callback panics. Injected tests prove
  the fatal decision without terminating the test process.
- The systemd lifecycle unit exposes only the controller as `MainPID`, uses
  `KillMode=process`, disables SIGKILL, and has no finite stop deadline. The
  controller owns an exclusive lock and durable host-visible recovery marker,
  retries teardown, and removes the marker only after managed swap absence,
  runtime-device removal, exact NBD detach, and daemon exit. Host recovery is
  plan-first, invokes the same controller, never terminates WSL, and never kills
  the backend directly.

## Current R5 hermetic source checkpoint

- `cargo test -p ramshared-cli --all-targets --no-fail-fast` passed 224/224
  unit and 6/6 integration tests. The schema-v3 monitor fixture carries
  `next_ordinal`, `issued_ordinal`, and `invocation_id`; no v2 or `last_action`
  compatibility path exists.
- Focused process-custody, directory-authority, supervisor reconstruction,
  durable-write-failure, error-sanitization, and ordinal tests passed. Each
  fixture signalled only its own exact child/process group.
- `cargo test --workspace --all-targets --no-fail-fast`, workspace rustfmt, and
  workspace Clippy with `-D warnings` passed after clean process preflights.
- Canonical current-worktree line coverage passed: `bounded_process.rs`
  607/741 (81.9%), `workload.rs` 2531/2862 (88.4%), `supervisor.rs` 1977/2206
  (89.6%), and `monitor.rs` 928/1029 (90.2%).
- No systemd unit, WSL/VM lifecycle, device, storage, swap, GPU, or pressure
  action occurred.

## Earlier TASK-0009 source checkpoint

- Two focused InvocationID tests passed; six focused provisioning/lifecycle/NBD
  rollback tests passed.
- `test-control-plane-units.sh` passed the swapoff-first, no-pre-swapoff-kill,
  durable-marker, restart, and terminal-proof fixtures.
- The packaged NBD preflight passed 43/43 cases using hermetic fake binaries;
  no release build or live backend was started.
- Origin, watchdog telemetry, and lifecycle recovery PowerShell parser/static
  tests passed. `wslconfig-ctl.sh selftest` passed without changing `.wslconfig`.
- Workspace rustfmt and targeted `ramshared-cli`, `ramshared-wsl2d`, and
  `ramshared-block` Clippy with `-D warnings` passed with one Cargo job. The
  absent machine-wide Guard broker was not started or installed; the installed
  Rust toolchain was invoked directly.
- No systemd unit, Docker component, WSL lifecycle, swap, NBD, VHDX, GPU, or
  pressure path was activated.

## Earlier recorded source evidence

The results below remain evidence for their recorded source revision. They are
not Rust verification for the current dirty worktree.

- `cargo test --workspace --no-fail-fast`: PASS. CUDA/ublk/root/device tests
  remained explicitly ignored and environment-bound.
- Selected Clippy with `-D warnings` and workspace rustfmt: PASS.
- Control SSDV3 line gate: main 80.5%, workload 88.9%, supervisor 92.0%,
  lifecycle 95.1%, monitor 87.9%.
- Named control-unit, reversible-manager, postmortem, and 38-case NBD product
  preflight shell suites: PASS.
- Windows guardian, origin, launcher, and full Windows static suites: PASS.
- PowerShell origin/guardian invocations were plan-only; no scheduled task,
  VHDX, unit, Docker configuration, or distro lifecycle was changed.

## Open evidence and rollout

1. Isolated Windows/WSL surface: multiple scopes sharing the real slice,
   control responsiveness under bounded pressure, Docker/BuildKit/cron
   ancestry, stale-monitor counterexample, exactly-one targeted terminate,
   safe boot, and terminate timeout without reboot. The existing
   `SANITIZED_VM_WSL2_LAB` was recoverably reimaged after exact identity,
   zero-checkpoint, VM-owned-VHD, and verified-backup gates. Persistent
   disposable-lab autologon, PowerShell Direct, runtime-service registration,
   bounded status/list probes, and sealed-distro registration now pass. This
   closes access/readiness only; guardian/origin/pressure evidence remains
   `PARTIAL`.
2. Daily host, attended: future staging definitions must remain disabled before
   independent benign validation, targeted restart proof, and a 24-hour
   RamShared-OFF observation. This is a required evidence shape, not an action
   instruction.
3. The source-removal prerequisite is closed. Only after all listed live gates
   and a separate attended approval may a future campaign qualify a 4 GiB
   logical device with a 1 GiB physical cap before considering a higher sealed
   cap. This record does not authorize activation; staging remains disabled.

Rollback keeps RamShared OFF, disables the guardian task, restores exact
backed-up unit/Docker configuration, and leaves the origin VHDX detached and
undeleted.

No live pressure, terminate, Docker restart, host installation, or readiness
claim is produced by this implementation record.
