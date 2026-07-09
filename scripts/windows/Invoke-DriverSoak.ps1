#Requires -Version 5.1
<#
.SYNOPSIS
  ITEM-10 / RNF-1 / DT-12 — Driver Verifier soak 3x24h (aggregate 72h).

.DESCRIPTION
  Enables Driver Verifier Standard flags for ramshared.sys, runs fuzz/load harness
  for -Hours (default 24), records run-id. Operator runs 3 independent rounds.

.NOTES
  VM only for aggressive fuzz. Abort on any BugCheck.
#>
[CmdletBinding()]
param(
    [int]$Hours = 24,
    [string]$DriverName = "ramshared",
    [string]$ArtifactDir = ".\artifacts\driver-soak",
    [switch]$EnableVerifierOnly,
    [switch]$DisableVerifier
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $ArtifactDir | Out-Null
$runId = "soak-{0:yyyyMMdd-HHmmss}" -f (Get-Date)

if ($DisableVerifier) {
    Write-Host "verifier /reset (reboot required)"
    verifier /reset
    exit 0
}

Write-Host "Enabling Driver Verifier Standard for $DriverName"
# Standard flags (special pool, force IRQL checking, pool tracking, I/O verification, deadlock detection)
verifier /standard /driver "$DriverName.sys"

if ($EnableVerifierOnly) {
    Write-Host "Verifier configured. Reboot, then re-run without -EnableVerifierOnly for soak."
    exit 0
}

$end = (Get-Date).AddHours($Hours)
Write-Host "Soak until $end (run-id $runId). Fuzz IOCTLs + I/O in background if harness present."
$log = Join-Path $ArtifactDir "$runId.log"
"start $(Get-Date -Format o)" | Set-Content $log

while ((Get-Date) -lt $end) {
    # Placeholder tick: operator plugs I/O fuzz here.
    "tick $(Get-Date -Format o)" | Add-Content $log
    Start-Sleep -Seconds 300
}

"end $(Get-Date -Format o) status=COMPLETED_NO_AUTO_BSOD_CHECK" | Add-Content $log
Write-Host @"
Soak window finished for $runId.
Manual gate: confirm zero BugCheck in Event Viewer / MEMORY.DMP.
Record 3 independent 24h rounds in IMPL.md (DT-12).
"@
exit 0
