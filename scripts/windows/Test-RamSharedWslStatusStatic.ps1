#Requires -Version 5.1
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$target = Join-Path $PSScriptRoot "Show-RamSharedWslStatus.ps1"
if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
    throw "ramshared_wsl_status: target script is missing"
}
$source = Get-Content -Raw -LiteralPath $target
foreach ($required in @(
    'state = "PLAN"',
    "ConvertFrom-Json",
    "LastWriteTimeUtc",
    "sample_age_ms",
    "disk_growth_kib",
    "protection_state",
    "memory_psi_full_avg10"
)) {
    if (-not $source.Contains($required)) {
        throw "ramshared_wsl_status: missing contract $required"
    }
}
foreach ($forbidden in @(
    "wsl.exe",
    "Start-Process",
    "Invoke-Expression",
    "--shutdown",
    "--terminate",
    "Set-Content",
    "Add-Content"
)) {
    if ($source.Contains($forbidden)) {
        throw "ramshared_wsl_status: forbidden host mutation $forbidden"
    }
}
Write-Output "PASS ramshared_wsl_status_reads_heartbeat_only"
