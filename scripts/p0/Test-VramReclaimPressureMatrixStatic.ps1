#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ScriptPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Invoke-VramReclaimPressureMatrix.ps1"
}
$text = Get-Content -LiteralPath $ScriptPath -Raw

foreach ($needle in @(
    "windows-smoke",
    "windows-3gib",
    "wsl2-1gib",
    "wsl2-4gib",
    "split-4gib-1gib",
    "PLAN_ONLY=1",
    "-ApprovePhysicalHost",
    "-ApproveSharedDesktopWsl",
    "App-agnostic aggregate VRAM pressure",
    "external_gpu_workload_mib",
    "windows+wsl2+external+reserve",
    "Refusing live matrix"
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("vram_reclaim_matrix_static: missing " + $needle)
    }
}

foreach ($forbidden in @("ExampleDccApp", "ExampleGameApp", "ExampleVideoApp", "ExampleCompositorApp")) {
    if ($text -match [regex]::Escape($forbidden)) {
        throw ("vram_reclaim_matrix_static: app-specific name forbidden: " + $forbidden)
    }
}

Write-Output "PASS Test-VramReclaimPressureMatrixStatic"
