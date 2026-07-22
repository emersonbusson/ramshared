#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Invoke-Win11WslRuntimeProbe.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

foreach ($needle in @(
    'win11-drill',
    'PowerShell Direct',
    'GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)',
    'New-ScheduledTaskPrincipal -UserId $using:User -RunLevel Highest',
    'Unregister-ScheduledTask -TaskName "RamSharedWslHighProbe"',
    'Microsoft-Windows-Subsystem-Linux',
    'VirtualMachinePlatform',
    'Where-Object { $_.Name -eq "WslService" }',
    'guest_wsl_service_missing',
    'guest_wsl_service_not_running',
    'Invoke-WslWithTimeout',
    'status_timeout',
    'list_timeout',
    'guest_wsl_runtime_unavailable',
    'guest_wsl_elevated_task_no_output',
    'DISK_MUTATION = $false'
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("win11_wsl_runtime_probe_static: missing " + $needle)
    }
}

if ($text -match 'Initialize-Disk|Format-Volume|Resize-VHD|Convert-VHD|New-VHD|Set-Content.*RAMSHARED_DRILL_PASSWORD|ConvertTo-Json.*Password') {
    throw "win11_wsl_runtime_probe_static: disk mutation or secret persistence is forbidden"
}

Write-Output "PASS Test-Win11WslRuntimeProbeStatic"
