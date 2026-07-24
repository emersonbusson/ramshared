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
    "split-3gib-1gib",
    "PLAN_ONLY=1",
    "-ApprovePhysicalHost",
    "-ApproveSharedDesktopWsl",
    "App-agnostic aggregate VRAM pressure",
    "external_gpu_workload_mib",
    "matrix-summary.json",
    "STATUS",
    "RUN windows-3gib via Run-HostExhaustive.ps1 with external workload",
    "ExternalWorkloadMiB",
    "256MiB margin",
    "Refusing live matrix",
    "insufficient_vram_headroom",
    "shared_wsl_watchdog_required",
    "owner_allocations_plus_margin",
    "Invoke-SharedWslPressureCampaign.ps1",
    'New-Case "wsl2-4gib" 0 4096 4096',
    "Read-SharedCampaignSummary",
    "campaignSummary.PASS",
    "*>&1",
    "matrix_row_close",
    "shared_wsl_matrix_row_not_closed",
    "-PreallocateVram"
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("vram_reclaim_matrix_static: missing " + $needle)
    }
}

if ($text -match 'split-4gib-1gib') {
    throw "uncalibrated_split_forbidden: 4 GiB + 1 GiB cannot preserve a 1 GiB floor on a 6 GiB GPU"
}

if ($text -match 'throw "Refusing \$\(\$c\.case\): WSL2 pressure') {
    throw "wsl2_partial_required: WSL2 matrix refusals must write summary artifacts"
}
if ($text -match 'throw "\$\(\$c\.case\) is not live-enabled') {
    throw "split_partial_required: split matrix refusal must write summary artifacts"
}

foreach ($forbidden in @("ExampleDccApp", "ExampleGameApp", "ExampleVideoApp", "ExampleCompositorApp")) {
    if ($text -match [regex]::Escape($forbidden)) {
        throw ("vram_reclaim_matrix_static: app-specific name forbidden: " + $forbidden)
    }
}

Write-Output "PASS Test-VramReclaimPressureMatrixStatic"
