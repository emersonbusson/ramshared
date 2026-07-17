#Requires -Version 5.1
[CmdletBinding()]
param([string]$ScriptPath)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Invoke-SdvProbe.ps1"
}
$src = Get-Content -LiteralPath $ScriptPath -Raw
$required = @(
    "NOT_CLAIMED",
    "sdv.exe_not_on_path",
    "msbuild_target_sdv_missing",
    "/t:sdv",
    "SDV_CLAIM",
    "WindowsDriver.Sdv.targets"
)
foreach ($t in $required) {
    if ($src -notmatch [regex]::Escape($t)) {
        throw "SDV probe missing token: $t"
    }
}
if ($src -match 'SDV_CLAIM=PASS' -and $src -notmatch 'NOT_CLAIMED') {
    throw "SDV probe must default to NOT_CLAIMED"
}
Write-Host "STATIC_SDV_PROBE=PASS"
exit 0
