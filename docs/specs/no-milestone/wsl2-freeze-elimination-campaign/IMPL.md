# IMPL - WSL2 freeze-elimination campaign evidence gate

## Status

**PARTIAL.** Validator and synthetic gates exist. Live closure still requires
two passing isolated-lab rounds from a real WSL2 environment that is not the
daily desktop.

2026-07-18 dry-run baseline on the daily WSL2 host stayed `NOT_CLAIMED`.
The OOM gate now uses `RAMSHARED_FREEZE_RECENT_DMESG_SEC` (default 1800s)
instead of raw dmesg tail membership; `/tmp/ramshared-wsl2-freeze-windowed-1784385445`
reported `oom_hits=0` and refused action only because this is still the daily
WSL2 desktop.

`scripts/windows/Invoke-Win11Wsl2FreezeCampaign.ps1` is the isolated Windows
guest path. 2026-07-18 follow-up recovered the local Machine credential for
`WIN11-DRILL\drilladmin`, added PowerShell Direct readiness retries, enabled
WSL/VMP optional features in the guest, copied the tracked repo to
`C:\ramshared\src`, and attempted official Microsoft WSL 2.7.10 runtime repair.
The live gate remains `PARTIAL`: intermediate probes returned
`Wsl/CallMsi/Install/REGDB_E_CLASSNOTREG` or "WSL is not installed", and
`C:\ramshared\artifacts\win11-wsl2-freeze-campaign-20260718-123613` ended with
`REASON=powershell_direct_failed` after the repair attempts. No daily-host WSL2
pressure action was used.

## Implemented

- `scripts/safety/validate-wsl2-freeze-campaign-artifact.sh`
- `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`
- `scripts/windows/Invoke-Win11Wsl2FreezeCampaign.ps1`
- `scripts/windows/Test-Win11Wsl2FreezeCampaignStatic.ps1`

## Validation

- Static: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`
- Static: `scripts/windows/Test-Win11Wsl2FreezeCampaignStatic.ps1`
- Synthetic PASS: `/tmp/ramshared-wsl2-freeze-valid` returned
  `WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS rounds=2`.
- Synthetic PARTIAL/FAIL: `/tmp/ramshared-wsl2-freeze-invalid` failed because
  `isolated-complete.txt` was missing.
