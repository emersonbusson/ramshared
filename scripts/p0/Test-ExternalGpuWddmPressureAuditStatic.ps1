#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ScriptPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Invoke-ExternalGpuWddmPressureAudit.ps1"
}
$text = Get-Content -LiteralPath $ScriptPath -Raw

foreach ($needle in @(
    "process_attribution = `$false",
    "Invoke-GpuWorkloadGate.ps1",
    "Resolve-InputPath",
    "diagnose --events",
    "DiagnoseJsonPath",
    "GPU_GATE_OK",
    "DIAGNOSE_OK",
    "DEMOTES",
    "`$demotes -gt 0",
    "STATUS",
    "PARTIAL",
    "Aggregate external GPU pressure only"
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("external_gpu_wddm_pressure_audit_static: missing " + $needle)
    }
}

foreach ($forbidden in @(
    "Initialize-Disk",
    "Format-Volume",
    "Stop-VM",
    "Start-VM",
    "wsl --terminate"
)) {
    if ($text -match [regex]::Escape($forbidden)) {
        throw ("external_gpu_wddm_pressure_audit_static: forbidden token " + $forbidden)
    }
}

Write-Output "PASS Test-ExternalGpuWddmPressureAuditStatic"
