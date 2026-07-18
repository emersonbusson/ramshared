#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ScriptPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Get-WinDrivePreflight.ps1"
}
$text = Get-Content -LiteralPath $ScriptPath -Raw

foreach ($needle in @(
    "\\.\RamSharedCtl",
    "\\.\GLOBALROOT\Device\RamSharedCtl",
    "RamSharedCtlOpen",
    "CreateFile",
    "ramshared service is RUNNING but RamSharedCtl is absent",
    "reboot/unload/redeploy before physical Online",
    "Get-PnpDevice -PresentOnly:`$false",
    "SCSI\DISK&VEN_RAMSHARE&PROD_VRAMDISK",
    "Stale RamShared PnP disk node(s) present",
    "Driver image/package mismatch",
    "Driver image matches package SHA256"
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("control_path_fail_closed: missing " + $needle)
    }
}
if ($text -notmatch 'if \(\$svcRunning -and -not \$ctlOk\)') {
    throw "control_path_fail_closed: missing running-without-control branch"
}
if ($text -notmatch 'if \(\$StorageOnly\).*Bad') {
    throw "control_path_fail_closed: storage-only mode must fail hard"
}
if ($text -match 'Test-Path \$ctl') {
    throw "control_path_testpath_forbidden: device namespace must be opened with CreateFile"
}

Write-Output "PASS Test-WinDrivePreflightStatic"
