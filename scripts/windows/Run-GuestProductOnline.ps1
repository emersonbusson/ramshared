#Requires -Version 5.1
<#
.SYNOPSIS
  Compatibility entry for the packaged autonomous VM lifecycle.

.DESCRIPTION
  The historical inline TCP/PowerShell broker was removed. This wrapper has no
  independent product or SCM implementation.
#>
[CmdletBinding()]
param(
    [string]$VMName = "win11-drill",
    [string]$User = "WIN11-DRILL\drilladmin",
    [string]$Password = "",
    [UInt64]$SizeBytes = 67108864,
    [string]$Letter = "R",
    [switch]$ManufacturedPagefileRefuse
)

$ErrorActionPreference = "Stop"
if ($ManufacturedPagefileRefuse) {
    throw "Use Invoke-PagefileRefusalManufactured.ps1 for the isolated refusal drill."
}
& (Join-Path $PSScriptRoot "Run-GuestAutonomousLifecycle.ps1") `
    -VMName $VMName -User $User -Password $Password `
    -ExpectedSizeBytes $SizeBytes -Letter $Letter
