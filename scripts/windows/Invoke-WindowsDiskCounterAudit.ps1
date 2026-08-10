#Requires -Version 5.1
<#
.SYNOPSIS
  Emit historical counter-audit metadata without executing a host campaign.

.DESCRIPTION
  This command is retained only so an operator can identify the replacement
  for historical artifacts. It cannot consume, select, or validate a live
  storage artifact. Use Invoke-WindowsStorageMatrix.ps1 for the current
  supervised matrix surface.
#>
[CmdletBinding()]
param(
    [switch]$Run,
    [switch]$ApprovePhysicalHost,
    [UInt64]$SizeBytes = 67108864,
    [string]$OutDir = "C:\ramshared\artifacts\disk-counter-audit-$(Get-Date -Format yyyyMMdd-HHmmss)"
)

$ErrorActionPreference = "Stop"

if ($Run) {
    throw "Invoke-WindowsDiskCounterAudit.ps1 is retired; use Invoke-WindowsStorageMatrix.ps1"
}

$plan = [ordered]@{
    schema = 1
    tool = "Invoke-WindowsDiskCounterAudit.ps1"
    status = "LEGACY_RETIRED"
    run_requested = $false
    requested_size_bytes = $SizeBytes
    requested_out_dir = $OutDir
    replacement = "Invoke-WindowsStorageMatrix.ps1"
    note = "Historical counter-audit artifacts are not current matrix evidence."
}
$plan | ConvertTo-Json -Depth 4
Write-Host "PLAN_ONLY=1"
Write-Host "LEGACY_RETIRED=1"
Write-Host "REPLACEMENT=Invoke-WindowsStorageMatrix.ps1"
