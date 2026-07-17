#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$RepoRoot
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
}
$inf = Get-Content (Join-Path $RepoRoot "drivers\windows\ramshared\ramshared.inf") -Raw
if ($inf -notmatch '(?im)^DefaultDestDir\s*=\s*13\s*$') {
    throw "inf_uses_dirid_13: DefaultDestDir is not 13"
}
if ($inf -notmatch '(?im)^ServiceBinary\s*=\s*%13%\\ramshared\.sys\s*$') {
    throw "inf_uses_dirid_13: ServiceBinary is not Driver Store relative"
}

$productHarness = Get-Content (Join-Path $RepoRoot "scripts\windows\Run-GuestProductOnline.ps1") -Raw
if ($productHarness -match 'System32\\drivers\\ramshared\.sys' -or
    $productHarness -match 'sc\.exe create ramshared') {
    throw "product_harness_uses_installed_service_image: manual System32 service path remains"
}
if ($productHarness -notmatch 'ImagePath') {
    throw "product_harness_uses_installed_service_image: service ImagePath is not hashed"
}

$canonicalScripts = @(
    "Install-InfAndBackend.ps1",
    "Start-RamSharedLab.ps1",
    "Install-WinDriveVm.ps1",
    "Run-GuestExhaustive.ps1",
    "Run-GuestProductOnline.ps1",
    "Run-StorportCudaPartial.ps1"
)
foreach ($name in $canonicalScripts) {
    $scriptText = Get-Content (Join-Path $RepoRoot ("scripts\windows\" + $name)) -Raw
    if ($scriptText -match 'System32\\drivers\\ramshared\.sys' -or
        $scriptText -match 'sc\.exe create ramshared') {
        throw ("canonical_installers_do_not_bypass_driver_store: " + $name)
    }
}

Write-Output "PASS Test-RamSharedInfIsolationStatic"
