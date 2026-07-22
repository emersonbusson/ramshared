#Requires -Version 5.1
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$script = Join-Path $root "scripts\windows\Invoke-SharedWslPressureCampaign.ps1"
if (-not (Test-Path -LiteralPath $script)) {
    throw "missing script: $script"
}

$text = Get-Content -LiteralPath $script -Raw
$required = @(
    "ApproveSharedDailyHost",
    "PreallocateVram",
    "ExternalWorkloadMiB",
    "Start-CudaVramWorkload.ps1",
    "external-workload.ps1",
    "external-workload.out",
    "external-workload.err",
    "[cuda-vram-workload] released",
    "external_workload_ok",
    "RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_WSL_TERMINATION",
    "RAMSHARED_WINDOWS_WATCHDOG_ARMED=1",
    "--approve-shared-daily-host",
    "--run-shared-daily-host",
    "cascade-health.sh --loop",
    "ramsharedd-logged.sh",
    "daemon.out",
    "diagnose.json",
    "canario_demotes",
    'RAMSHARED_FREEZE_REQUIRED_ROUNDS="$Rounds"',
    "validate-wsl2-freeze-campaign-artifact.sh",
    "WaitForExit",
    "wsl.exe",
    "--terminate",
    "Stop-Process -Id `$externalProc.Id",
    "ramshared down",
    "DISK_MUTATION = `$false"
)

foreach ($needle in $required) {
    if (-not $text.Contains($needle)) {
        throw "missing token: $needle"
    }
}

$forbidden = @(
    "Initialize-Disk",
    "Format-Volume",
    "Resize-VHD",
    "New-VHD",
    "New-VM",
    "Start-VM",
    "Stop-VM",
    "Remove-VM",
    "Clear-Disk",
    "diskpart"
)

foreach ($needle in $forbidden) {
    if ($text -match [regex]::Escape($needle)) {
        throw "forbidden token: $needle"
    }
}

Write-Host "STATIC_SHARED_WSL_PRESSURE_CAMPAIGN=PASS"
