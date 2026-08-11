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
    'Win11_25H2_English_x64_v2_noprompt_unattend.iso',
    'Win11LabMediaContract.ps1',
    'AutounattendXml',
    'SealedAutounattendSha256',
    '$requiredTpmCommands',
    'Invoke-Win11LabPrimaryIsoContract -IsoPath $WindowsIso',
    'Assert-Win11LabPrimaryIsoContract',
    'Set-VMKeyProtector -VMName $VMName -NewLocalKeyProtector',
    'Enable-VMTPM -VMName $VMName',
    'Wait-Win11LabExactVhdGrowth -VhdPath $vhdPath',
    'Set-VMFirmware -VMName $VMName -FirstBootDevice $vhdBootDrive',
    'Stop-Win11LabVmFailSafe -Name $VMName',
    'VM already exists',
    'Target root exists and is not empty',
    'Target VHD already exists',
    'New-VHD -Path $vhdPath',
    'Set-VMProcessor -VMName $VMName -Count 4 -ExposeVirtualizationExtensions $true',
    'AutomaticCheckpointsEnabled $false',
    'virtual_tpm = "enabled_local_key_protector"',
    'next_boot_device = "installer_iso_before_first_start"',
    '$metadata["next_boot_device"] = "exact_new_vhd"',
    'Get-VMIntegrationService -VMName $VMName',
    'integration_services = "enabled_by_pipeline"',
    'existing_lab_disks_modified = $false'
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("win11_wsl2_lab_vm_static: missing " + $needle)
    }
}

if ($text -match 'AutounattendIso|Add-VMDvdDrive.*Autounattend|BypassTPMCheck|LabConfig|AllowUpgradesWithUnsupportedTPMOrCPU|Remove-VM|Remove-VHD|Remove-Item|Format-Volume|Initialize-Disk|Resize-VHD|Convert-VHD|Set-Content.*RAMSHARED_DRILL_PASSWORD') {
    throw "win11_wsl2_lab_vm_static: destructive operations or secret persistence are forbidden"
}

Write-Output "PASS Test-Win11Wsl2LabVmStatic"
