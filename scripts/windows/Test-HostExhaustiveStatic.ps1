#Requires -Version 5.1
[CmdletBinding()]
param([string]$HarnessPath = "")
$ErrorActionPreference = "Stop"
if (-not $HarnessPath) {
    $HarnessPath = Join-Path $PSScriptRoot "Run-HostExhaustive.ps1"
}
$text = Get-Content $HarnessPath -Raw
foreach ($forbidden in @(
        "Lab-LeaseBroker", "TcpListener", "RS_BROKER_PORT",
        "Stop-Process -Id", "broker-lab")) {
    if ($text -match [regex]::Escape($forbidden)) {
        throw "NO_LAB_BROKER_REFERENCE failed: $forbidden"
    }
}
foreach ($required in @(
        "Run-HostAutonomousLifecycle.ps1", "ApprovePhysicalHost",
        "Manifest", "ColdBoots")) {
    if ($text -notmatch [regex]::Escape($required)) {
        throw "packaged host wrapper missing: $required"
    }
}
Write-Output "PASS Test-HostExhaustiveStatic"
