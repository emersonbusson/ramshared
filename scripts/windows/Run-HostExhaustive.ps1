#Requires -Version 5.1
<#
.SYNOPSIS
  Compatibility entry for the supervised packaged physical lifecycle.

.DESCRIPTION
  The historical inline TCP/PowerShell broker and force-kill cleanup were
  removed. The autonomous host harness owns the one manifest and watchdog.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Manifest,
    [switch]$ApprovePhysicalHost,
    [int]$ColdBoots = 3
)

$ErrorActionPreference = "Stop"
& (Join-Path $PSScriptRoot "Run-HostAutonomousLifecycle.ps1") `
    -Manifest $Manifest -ColdBoots $ColdBoots `
    -ApprovePhysicalHost:$ApprovePhysicalHost
