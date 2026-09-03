# FINDING_ONLY

**Target File:** `scripts/windows/Invoke-SharedWslPressureCampaign.ps1`
**Task:** Validate that configured pressure thresholds are between 0 and 100 and that memory_high < memory_max.

## Evidence
After exploring the `scripts/windows/Invoke-SharedWslPressureCampaign.ps1` file, I found that there are no parameters or variables named `memory_high`, `memory_max`, or anything related to "pressure thresholds" that range between 0 and 100. The only related parameter is `$PressureAllocGiB`. Therefore, safe and orthogonal code modification is not possible.
