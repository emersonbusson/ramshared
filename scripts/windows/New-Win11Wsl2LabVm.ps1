#Requires -Version 5.1
<#
.SYNOPSIS
  Create a disposable Windows 11 Hyper-V VM for isolated WSL2 freeze campaigns.

.DESCRIPTION
  This script creates a new VM with a new dynamic VHD under a dedicated lab
  directory. It never modifies win11-drill, never formats disks, and refuses to
  run when the target VM or target VHD already exists. The goal is to provide a
  clean WSL2-capable guest surface without reimaging an existing lab disk.
  The default VHD root is on C: because the HDD-backed lab path is too slow for
  Windows setup, Windows Update, and WSL package registration.
#>
[CmdletBinding()]
param(
    [string]$VMName = "win11-wsl2-lab",
    [string]$Root = "C:\ramshared-hyperv\win11-wsl2-lab",
    [string]$WindowsIso = "E:\Hyper-V\iso\Win11_25H2_English_x64_v2.iso",
    [string]$AutounattendIso = "E:\Hyper-V\iso\Win11_25H2_autounattend.iso",
    [int]$VhdSizeGB = 80,
    [int]$StartupMemoryGB = 4,
    [int]$MinMemoryGB = 2,
    [int]$MaxMemoryGB = 8,
    [string]$SwitchName = "Default Switch",
    [switch]$Start
)

$ErrorActionPreference = "Stop"

function Fail([string]$Message) {
    Write-Error $Message
    exit 2
}

if (Get-VM -Name $VMName -ErrorAction SilentlyContinue) {
    Fail "VM already exists: $VMName"
}
if (-not (Test-Path -LiteralPath $WindowsIso)) {
    Fail "Windows ISO not found: $WindowsIso"
}
if (-not (Test-Path -LiteralPath $AutounattendIso)) {
    Fail "Autounattend ISO not found: $AutounattendIso"
}
if (Test-Path -LiteralPath $Root) {
    $children = @(Get-ChildItem -LiteralPath $Root -Force -ErrorAction SilentlyContinue)
    if ($children.Count -gt 0) {
        Fail "Target root exists and is not empty: $Root"
    }
} else {
    New-Item -ItemType Directory -Force -Path $Root | Out-Null
}

$vhdDir = Join-Path $Root "Virtual Hard Disks"
$vmConfigDir = Join-Path $Root "Virtual Machines"
New-Item -ItemType Directory -Force -Path $vhdDir | Out-Null
New-Item -ItemType Directory -Force -Path $vmConfigDir | Out-Null

$vhdPath = Join-Path $vhdDir "$VMName.vhdx"
if (Test-Path -LiteralPath $vhdPath) {
    Fail "Target VHD already exists: $vhdPath"
}

New-VHD -Path $vhdPath -SizeBytes ([int64]$VhdSizeGB * 1GB) -Dynamic | Out-Null
New-VM -Name $VMName `
    -Generation 2 `
    -MemoryStartupBytes ([int64]$StartupMemoryGB * 1GB) `
    -VHDPath $vhdPath `
    -Path $vmConfigDir `
    -SwitchName $SwitchName | Out-Null

Set-VM -Name $VMName `
    -CheckpointType Disabled `
    -AutomaticCheckpointsEnabled $false `
    -AutomaticStartAction Nothing `
    -AutomaticStopAction ShutDown `
    -DynamicMemory `
    -MemoryMinimumBytes ([int64]$MinMemoryGB * 1GB) `
    -MemoryMaximumBytes ([int64]$MaxMemoryGB * 1GB)

Set-VMProcessor -VMName $VMName -Count 4 -ExposeVirtualizationExtensions $true
Set-VMFirmware -VMName $VMName -EnableSecureBoot On -SecureBootTemplate "MicrosoftWindows"
Get-VMIntegrationService -VMName $VMName |
    Enable-VMIntegrationService -ErrorAction SilentlyContinue

$windowsDvd = Add-VMDvdDrive -VMName $VMName -Path $WindowsIso -Passthru
Add-VMDvdDrive -VMName $VMName -Path $AutounattendIso | Out-Null
Set-VMFirmware -VMName $VMName -FirstBootDevice $windowsDvd

$metadata = [ordered]@{
    vm = $VMName
    root = $Root
    vhd = $vhdPath
    windows_iso = $WindowsIso
    autounattend_iso = $AutounattendIso
    vhd_size_gb = $VhdSizeGB
    nested_virtualization = $true
    integration_services = "enabled_by_pipeline"
    disk_mutation = "new_vhd_only"
    existing_lab_disks_modified = $false
}
$metadata | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $Root "ramshared-wsl2-lab.json")

if ($Start) {
    Start-VM -Name $VMName
}

$metadata | ConvertTo-Json -Depth 4
