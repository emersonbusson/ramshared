# IMPL - WSL2 freeze-elimination campaign evidence gate

## Status

**PARTIAL.** Validator and manufactured source gates exist. The host commit
admission/runtime guardian added in this slice has no live campaign evidence,
so it cannot close or preserve a freeze-elimination claim by itself.

**Disabled staging boundary:** This implementation records source/static and
historical evidence only. All manager definitions remain inert and disabled;
no retained command, identity, topology, or historical result authorizes a
current campaign, WSL lifecycle, VM, storage, swap, device, or pressure action.

2026-08-21 source-only hardening: the shared-host harness now calculates
`ceil(PressureAllocGiB*1024)+HostCommitReserveMiB` (default reserve 4096 MiB),
takes three one-second `Win32_OperatingSystem` CIM samples before any disk
telemetry, WSL launch, RamShared activation, or guest process, and refuses with
`host_commit_headroom_insufficient` or `host_memory_query_failed`. It writes
`host-memory-admission.json`, `host-memory.jsonl`, and additive summary fields
for the gate, headroom, required/reserve MiB, and guardian state.

During every wait phase, the harness appends a one-second host-memory sample.
A reserve breach or three consecutive invalid samples produces PARTIAL once,
stops optional external work plus the WSL launcher, and issues exactly one
selected-distro termination. No OOM-marker allowance, broad WSL shutdown,
Windows reboot/shutdown, VM action, or disk mutation route was added. The
manufactured named cases are green; live proof remains source-only PARTIAL.

2026-08-21 containment follow-up: every operation after the WSL launcher starts
is owned by one cleanup boundary. On exception, guardian trip, or outer timeout
it stops and waits for optional work, stops the launcher, stops/removes disk
telemetry, then makes one selected-distro termination attempt before any
best-effort marker or summary write. PASS also requires continuously recorded
host-memory telemetry and a mandatory `summary.json` write; a normal PASS or
normal PARTIAL summary failure propagates nonzero after cleanup. Manufactured static coverage names
`runtime_telemetry_write_failure_requests_cleanup`,
`guardian_marker_write_failure_cannot_precede_cleanup`,
`termination_start_failure_is_bounded_and_not_retried`, and
`launcher_exit_stops_external_workload`; memory coverage rejects null, missing,
and non-numeric headroom as telemetry failure. Rollback trigger: any
below-threshold admission, non-targeted termination route, automatic host
reboot/shutdown, disk/VM mutation, OOM override, telemetry fail-open, or
campaign-test regression.

2026-07-22 hardening update: the pressure probe now delegates allocation to
`scripts/safety/cascade_pressure_integrity_worker.py`, and the artifact
validator requires `round-N/integrity-result.json` with `status=PASS`, positive
allocation/verification counts, and matching before/after checksums. Existing
artifacts without that file remain historical evidence only; they cannot close
new matrix rows that require checksum integrity.

**Historical non-current / no execution:** The following closed run is retained
only as evidence. It is not disabled-staging approval and must not be repeated.
Historical 2026-07-22 live close (not current qualification evidence):
`SANITIZED_PATH_HOST_PRIVATE_ARTIFACT` from
`scripts/windows/Invoke-SharedWslPressureCampaign.ps1 -ApproveSharedDailyHost`
passed with two before/action/after rounds. The validator returned
`WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS mode=shared-daily-host rounds=2`,
`wsl_exit_code=0`, Windows watchdog did not fire, no round watchdog files were
created, `BINARY_MATCH=true`, no ghost swap was observed, telemetry JSONL was
all `ok=true`, and the terminal state was clean with only
`SANITIZED_EXISTING_WSL_SWAP_DEVICE` disk swap
active and `ramsharedd` stopped. This artifact predates the current host-memory
guard and does not change the source candidate's PARTIAL/requalification state.

2026-07-18 dry-run baseline on the daily WSL2 host stayed `NOT_CLAIMED`.
The OOM gate now uses `RAMSHARED_FREEZE_RECENT_DMESG_SEC` (default 1800s)
instead of raw dmesg tail membership;
`SANITIZED_PATH_GUEST_PRIVATE_DRY_RUN`
reported `oom_hits=0` and refused action only because this is still the daily
WSL2 desktop.

Historical 2026-07-18 isolated-guest follow-up used
`scripts/windows/Invoke-Win11Wsl2FreezeCampaign.ps1` and recovered the local
Machine credential for `SANITIZED_PRINCIPAL_WINDOWS_LAB`, added PowerShell
Direct readiness retries, enabled WSL/VMP optional features in the guest,
copied the tracked repo to `SANITIZED_PATH_GUEST_PRIVATE_REPO`, and attempted
official Microsoft WSL 2.7.10 runtime repair.
The live gate remains `PARTIAL`: intermediate probes returned
`Wsl/CallMsi/Install/REGDB_E_CLASSNOTREG` or "WSL is not installed", and
`SANITIZED_PATH_HOST_PRIVATE_ARTIFACT` ended with
`REASON=powershell_direct_failed` after the repair attempts. The later
shared-host watchdog path closed this claim without creating another VM.

## Implemented

- `scripts/safety/validate-wsl2-freeze-campaign-artifact.sh`
- `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`
- `scripts/safety/cascade_pressure_integrity_worker.py`
- `scripts/safety/Test-CascadePressureIntegrityWorker.sh`
- `scripts/windows/Invoke-Win11Wsl2FreezeCampaign.ps1`
- `scripts/windows/Test-Win11Wsl2FreezeCampaignStatic.ps1`
- `scripts/windows/Invoke-SharedWslPressureCampaign.ps1`
- `scripts/windows/Test-SharedWslPressureCampaignStatic.ps1`
- `scripts/windows/SharedWslHostMemoryGate.psm1`
- `scripts/windows/Test-SharedWslPressureCampaignMemoryGate.ps1`

## Validation

**Historical non-current / no execution:** The live and synthetic rows below
name dated evidence only. They do not authorize a current harness invocation;
the candidate remains disabled-staging only.

- Static: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`
- Static/integration: `scripts/safety/Test-CascadePressureIntegrityWorker.sh`
- Static: `scripts/windows/Test-Win11Wsl2FreezeCampaignStatic.ps1`
- Static: `scripts/windows/Test-SharedWslPressureCampaignStatic.ps1`
- Manufactured: `scripts/windows/Test-SharedWslPressureCampaignMemoryGate.ps1`
- Live: `scripts/windows/Invoke-SharedWslPressureCampaign.ps1 -ApproveSharedDailyHost`
  produced `SANITIZED_PATH_HOST_PRIVATE_ARTIFACT` with
  validator PASS.
- Synthetic PASS: `SANITIZED_PATH_GUEST_PRIVATE_VALID_FIXTURE` returned
  `WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS rounds=2`.
- Synthetic PARTIAL/FAIL: `SANITIZED_PATH_GUEST_PRIVATE_INVALID_FIXTURE` failed because
  `isolated-complete.txt` was missing.

2026-08-21 source/live boundary: source validation is green, including the
named manufactured PowerShell branches and static/documentation checks. The
authorized R4 admission refused before campaign action because non-
interactive elevation was unavailable and sampled host commit headroom was
below the required 7087 MiB. The artifact contains only host-memory admission
and summary records; no live pressure occurred. Before the re-admission
check, the exact approved lab VM was independently validated and taken from
Running to Off through the repository's normal graceful `Stop-VM` path in
6.8 seconds. That audited VM lifecycle action is evidence for safe lab
preparation only; it is not campaign cleanup or live qualification. Status
remains PARTIAL/source-qualified only; a fresh approved run after
non-interactive elevation and post-VM-off headroom >=7087 MiB is required.
