#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "New-Win11Wsl2LabVm.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

foreach ($needle in @(
    'win11-wsl2-lab',
    'C:\ramshared-hyperv\win11-wsl2-lab',
    'HDD-backed lab path is too slow',
    'Win11_25H2_English_x64_v2.iso',
    'Win11_25H2_autounattend.iso',
    'VM already exists',
    'Target root exists and is not empty',
    'Target VHD already exists',
    'New-VHD -Path $vhdPath',
    'Set-VMProcessor -VMName $VMName -Count 4 -ExposeVirtualizationExtensions $true',
    'AutomaticCheckpointsEnabled $false',
    'Get-VMIntegrationService -VMName $VMName',
    'integration_services = "enabled_by_pipeline"',
    'existing_lab_disks_modified = $false'
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("win11_wsl2_lab_vm_static: missing " + $needle)
    }
}

if ($text -match 'Remove-VM|Remove-VHD|Remove-Item|Format-Volume|Initialize-Disk|Resize-VHD|Convert-VHD|Set-Content.*RAMSHARED_DRILL_PASSWORD') {
    throw "win11_wsl2_lab_vm_static: destructive operations or secret persistence are forbidden"
}

Write-Output "PASS Test-Win11Wsl2LabVmStatic"
