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
guest path. 2026-07-18
`C:\ramshared\artifacts\win11-wsl2-freeze-campaign-20260718-115419` remained
`PARTIAL` because PowerShell Direct rejected the current local credential before
the guest WSL2 campaign could run. No daily-host WSL2 action was used.

## Implemented

- `scripts/safety/validate-wsl2-freeze-campaign-artifact.sh`
- `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`
- `scripts/windows/Invoke-Win11Wsl2FreezeCampaign.ps1`
- `scripts/windows/Test-Win11Wsl2FreezeCampaignStatic.ps1`

## Validation

- Static: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`
- Synthetic PASS: `/tmp/ramshared-wsl2-freeze-valid` returned
  `WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS rounds=2`.
- Synthetic PARTIAL/FAIL: `/tmp/ramshared-wsl2-freeze-invalid` failed because
  `isolated-complete.txt` was missing.
