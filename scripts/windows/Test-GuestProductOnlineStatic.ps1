#Requires -Version 5.1
[CmdletBinding()]
param([string]$HarnessPath = "")
$ErrorActionPreference = "Stop"
if (-not $HarnessPath) {
    $HarnessPath = Join-Path $PSScriptRoot "Run-GuestProductOnline.ps1"
}
$text = Get-Content $HarnessPath -Raw
foreach ($forbidden in @(
        "Lab-LeaseBroker", "TcpListener", "RS_BROKER_PORT",
        "broker-lab", "Stop-Process -Force")) {
    if ($text -match [regex]::Escape($forbidden)) {
        throw "NO_LAB_BROKER_REFERENCE failed: $forbidden"
    }
}
foreach ($required in @(
        "Run-GuestAutonomousLifecycle.ps1", "ExpectedSizeBytes",
        "Invoke-PagefileRefusalManufactured.ps1")) {
    if ($text -notmatch [regex]::Escape($required)) {
        throw "packaged guest wrapper missing: $required"
    }
}
Write-Output "PASS Test-GuestProductOnlineStatic"
