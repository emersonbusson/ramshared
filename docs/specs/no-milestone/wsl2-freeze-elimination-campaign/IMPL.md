# IMPL - WSL2 freeze-elimination campaign evidence gate

## Status

**PASS.** Validator, synthetic gates, and a real supervised shared-host WSL2
campaign exist.

2026-07-22 hardening update: the pressure probe now delegates allocation to
`scripts/safety/cascade_pressure_integrity_worker.py`, and the artifact
validator requires `round-N/integrity-result.json` with `status=PASS`, positive
allocation/verification counts, and matching before/after checksums. Existing
artifacts without that file remain historical evidence only; they cannot close
new matrix rows that require checksum integrity.

2026-07-22 live close:
`C:\ramshared\artifacts\shared-wsl-pressure-20260722-002748` from
`scripts/windows/Invoke-SharedWslPressureCampaign.ps1 -ApproveSharedDailyHost`
passed with two before/action/after rounds. The validator returned
`WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS mode=shared-daily-host rounds=2`,
`wsl_exit_code=0`, Windows watchdog did not fire, no round watchdog files were
created, `BINARY_MATCH=true`, no ghost swap was observed, telemetry JSONL was
all `ok=true`, and the terminal state was clean with only `/dev/sdc` disk swap
active and `ramsharedd` stopped.

2026-07-18 dry-run baseline on the daily WSL2 host stayed `NOT_CLAIMED`.
The OOM gate now uses `RAMSHARED_FREEZE_RECENT_DMESG_SEC` (default 1800s)
instead of raw dmesg tail membership; `/tmp/ramshared-wsl2-freeze-windowed-1784385445`
reported `oom_hits=0` and refused action only because this is still the daily
WSL2 desktop.

`scripts/windows/Invoke-Win11Wsl2FreezeCampaign.ps1` remains the isolated Windows
guest path. 2026-07-18 follow-up recovered the local Machine credential for
`WIN11-DRILL\drilladmin`, added PowerShell Direct readiness retries, enabled
WSL/VMP optional features in the guest, copied the tracked repo to
`C:\ramshared\src`, and attempted official Microsoft WSL 2.7.10 runtime repair.
The live gate remains `PARTIAL`: intermediate probes returned
`Wsl/CallMsi/Install/REGDB_E_CLASSNOTREG` or "WSL is not installed", and
`C:\ramshared\artifacts\win11-wsl2-freeze-campaign-20260718-123613` ended with
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

## Validation

- Static: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`
- Static/integration: `scripts/safety/Test-CascadePressureIntegrityWorker.sh`
- Static: `scripts/windows/Test-Win11Wsl2FreezeCampaignStatic.ps1`
- Static: `scripts/windows/Test-SharedWslPressureCampaignStatic.ps1`
- Live: `scripts/windows/Invoke-SharedWslPressureCampaign.ps1 -ApproveSharedDailyHost`
  produced `C:\ramshared\artifacts\shared-wsl-pressure-20260722-002748` with
  validator PASS.
- Synthetic PASS: `/tmp/ramshared-wsl2-freeze-valid` returned
  `WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS rounds=2`.
- Synthetic PARTIAL/FAIL: `/tmp/ramshared-wsl2-freeze-invalid` failed because
  `isolated-complete.txt` was missing.
