# IMPL - WSL2 freeze-elimination campaign evidence gate

## Status

**PARTIAL.** Validator and synthetic gates exist. Live closure still requires
two passing isolated-lab rounds from a real WSL2 environment that is not the
daily desktop.

2026-07-18 dry-run baseline on the daily WSL2 host stayed `NOT_CLAIMED` and
refused action with `daily_host_refused_without_isolated_lab_flag` plus
`recent_oom_marker`. OOM markers are explicit blockers for campaign closure.

## Implemented

- `scripts/safety/validate-wsl2-freeze-campaign-artifact.sh`
- `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`

## Validation

- Static: `scripts/safety/test-wsl2-freeze-campaign-artifact-static.sh`
- Synthetic PASS: `/tmp/ramshared-wsl2-freeze-valid` returned
  `WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS rounds=2`.
- Synthetic PARTIAL/FAIL: `/tmp/ramshared-wsl2-freeze-invalid` failed because
  `isolated-complete.txt` was missing.
