#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Inventory + optional Discrete Device Assignment (DDA) of NVIDIA GPU to a Hyper-V VM.

.DESCRIPTION
  Path 2 - "almost real GPU". WARNING:
  - Host Windows LOSES the GPU while assigned to the VM (display may fall back / break).
  - Consumer GeForce (RTX 2060) is community/experimental on Hyper-V client - not a clean vendor SLA.
  - Default is -Inventory only. Use -Apply explicitly.

.PARAMETER Inventory
  List display adapters, DDA-capable location paths, current VM assignable devices.

.PARAMETER Apply
  Dismount GPU from host and Add-VMAssignableDevice to -VmName.

.PARAMETER Release
  Remove assignment and try to remount GPU on host.

.PARAMETER VmName
  Target VM (default linux-kernel-lab)

.PARAMETER Force
  Skip interactive confirmation on -Apply
#>
[CmdletBinding(DefaultParameterSetName = "Inventory")]
param(
    [Parameter(ParameterSetName = "Inventory")]
    [switch]$Inventory,

    [Parameter(ParameterSetName = "Apply")]
    [switch]$Apply,

    [Parameter(ParameterSetName = "Release")]
    [switch]$Release,

    [string]$VmName = "linux-kernel-lab",
    [switch]$Force,
    [string]$ReportPath = "R:\Hyper-V\linux-kernel-lab\dda-inventory.txt"
)

$ErrorActionPreference = "Stop"

function Write-Step($m) { Write-Host "==> $m" -ForegroundColor Cyan }

function Get-NvidiaDisplayDevices {
    Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue |
        Where-Object {
            ($_.Class -eq "Display" -or $_.Class -eq "DisplayAdapters") -and
            ($_.Manufacturer -match "NVIDIA|Microsoft" -or $_.FriendlyName -match "NVIDIA|GeForce|RTX")
        }
}

function Get-LocationPath([string]$InstanceId) {
    # Prefer Get-PnpDeviceProperty
    try {
        $p = Get-PnpDeviceProperty -InstanceId $InstanceId -KeyName "DEVPKEY_Device_LocationPaths" -ErrorAction Stop
        if ($p.Data) {
            if ($p.Data -is [array]) { return $p.Data[0] }
            return [string]$p.Data
        }
    } catch {}
    # Fallback: parse from pnputil is harder; return empty
    return $null
}

if (-not $Inventory -and -not $Apply -and -not $Release) {
    $Inventory = $true
}

$lines = New-Object System.Collections.Generic.List[string]
function L($s) { $lines.Add($s); Write-Host $s }

Write-Step "DDA / GPU inventory (Hyper-V)"
L "Date: $(Get-Date -Format o)"
L "Host: $env:COMPUTERNAME"
L "VmName target: $VmName"
L ""

# Feature note
L "=== Hyper-V feature ==="
try {
    $hv = Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V -ErrorAction SilentlyContinue
    L "Microsoft-Hyper-V: $($hv.State)"
} catch {
    L "Microsoft-Hyper-V: (query failed) $($_.Exception.Message)"
}
L ""

L "=== PnP display-ish devices ==="
$devs = Get-NvidiaDisplayDevices
if (-not $devs) {
    $devs = Get-PnpDevice -PresentOnly | Where-Object { $_.Class -eq "Display" }
}
foreach ($d in $devs) {
    L ("- {0} | Status={1} | Class={2} | Id={3}" -f $d.FriendlyName, $d.Status, $d.Class, $d.InstanceId)
    $loc = Get-LocationPath $d.InstanceId
    if ($loc) { L "  LocationPath: $loc" } else { L "  LocationPath: (not resolved - may need manual Get-PnpDeviceProperty)" }
}
L ""

L "=== Get-VMHostAssignableDevice (if available) ==="
try {
    Get-VMHostAssignableDevice -ErrorAction Stop | ForEach-Object {
        L ("- InstanceId={0} LocationPath={1}" -f $_.InstanceId, $_.LocationPath)
    }
} catch {
    L "Get-VMHostAssignableDevice: $($_.Exception.Message)"
    L "Tip: dismount device first (Dismount-VMHostAssignableDevice) on Server; client Hyper-V may differ."
}
L ""

if (Get-VM -Name $VmName -ErrorAction SilentlyContinue) {
    L "=== VM $VmName assignable devices ==="
    try {
        Get-VMAssignableDevice -VMName $VmName -ErrorAction SilentlyContinue | ForEach-Object {
            L ("- {0} {1}" -f $_.Name, $_.InstanceId)
        }
    } catch {
        L "(none or error) $($_.Exception.Message)"
    }
    L "State: $((Get-VM -Name $VmName).State)"
} else {
    L "VM '$VmName' not found yet - create with New-LinuxKernelLabVm.ps1 first."
}

L ""
L "=== Checklist (human) ==="
L "[ ] Second display or iGPU so Windows host keeps a console when dGPU is assigned"
L "[ ] VM Generation 2, offline before Apply"
L "[ ] Disable Automatic Checkpoints"
L "[ ] Increase MMIO space if GPU fails to init (Set-VM -Low/HighMemoryMappedIoSpace)"
L "[ ] Guest: install proprietary NVIDIA driver AFTER DDA works (lspci shows 10de)"
L "[ ] Expect: host loses RTX for games while VM holds GPU"
L "[ ] Consumer GeForce = experimental; if Apply fails, use dual-boot for kernel-true"
L "[ ] Release path tested before long session"
L ""
L "=== Commands reference ==="
L "# Survey location path:"
L '  Get-PnpDevice -PresentOnly | ? Class -eq Display | ft Status,FriendlyName,InstanceId'
L '  Get-PnpDeviceProperty -InstanceId ''<id>'' -KeyName DEVPKEY_Device_LocationPaths'
L "# Prepare (Server-style):"
L '  Disable-PnpDevice -InstanceId ''...'' -Confirm:$false'
L '  Dismount-VMHostAssignableDevice -LocationPath ''...'' -Force'
L '  Set-VM -Name linux-kernel-lab -GuestControlledCacheTypes $true'
L '  Set-VM -Name linux-kernel-lab -LowMemoryMappedIoSpace 3GB -HighMemoryMappedIoSpace 64GB'
L '  Add-VMAssignableDevice -LocationPath ''...'' -VMName linux-kernel-lab'
L "# Release:"
L '  Remove-VMAssignableDevice -VMName linux-kernel-lab -LocationPath ''...'''
L '  Mount-VMHostAssignableDevice -LocationPath ''...'''
L '  Enable-PnpDevice -InstanceId ''...'' -Confirm:$false'

$reportDir = Split-Path $ReportPath -Parent
if ($reportDir -and -not (Test-Path $reportDir)) {
    New-Item -ItemType Directory -Force -Path $reportDir | Out-Null
}
$lines | Set-Content -Path $ReportPath -Encoding UTF8
Write-Step "Wrote $ReportPath"

if ($Apply) {
    if (-not $Force) {
        Write-Warning "Apply will REMOVE the NVIDIA GPU from the Windows host and assign it to VM '$VmName'."
        Write-Warning "Your desktop may go black if this is the only GPU. Re-run with -Force to proceed."
        Write-Host "Refusing Apply without -Force (safety)." -ForegroundColor Yellow
        return
    }
    $vm = Get-VM -Name $VmName -ErrorAction Stop
    if ($vm.State -ne "Off") {
        throw "VM must be Off for DDA Apply (current: $($vm.State)). Stop-VM -Name $VmName -Force"
    }

    $gpu = Get-PnpDevice -PresentOnly | Where-Object {
        $_.Class -eq "Display" -and $_.FriendlyName -match "NVIDIA|GeForce|RTX"
    } | Select-Object -First 1
    if (-not $gpu) { throw "No NVIDIA display device found for DDA" }

    $loc = Get-LocationPath $gpu.InstanceId
    if (-not $loc) {
        throw "Could not resolve LocationPath for $($gpu.InstanceId). Fill manually from inventory report."
    }

    Write-Step "Preparing VM MMIO / cache types"
    Set-VM -Name $VmName -GuestControlledCacheTypes $true -ErrorAction SilentlyContinue
    # Large BARs for modern GPUs
    Set-VM -VMName $VmName -LowMemoryMappedIoSpace 3GB -HighMemoryMappedIoSpace 64GB -ErrorAction SilentlyContinue

    Write-Step "Disable-PnpDevice $($gpu.FriendlyName)"
    Disable-PnpDevice -InstanceId $gpu.InstanceId -Confirm:$false -ErrorAction SilentlyContinue

    Write-Step "Dismount-VMHostAssignableDevice $loc"
    Dismount-VMHostAssignableDevice -LocationPath $loc -Force -ErrorAction Stop

    Write-Step "Add-VMAssignableDevice -> $VmName"
    Add-VMAssignableDevice -LocationPath $loc -VMName $VmName -ErrorAction Stop

    Write-Host "DDA Apply done. Start VM and check guest: lspci | grep -i nvidia" -ForegroundColor Green
    Write-Host "To undo: .\Prepare-DdaGpu.ps1 -Release -Force -VmName $VmName" -ForegroundColor Yellow
}

if ($Release) {
    if (-not $Force) {
        Write-Host "Release requires -Force" -ForegroundColor Yellow
        return
    }
    $assigned = Get-VMAssignableDevice -VMName $VmName -ErrorAction SilentlyContinue
    foreach ($a in $assigned) {
        Write-Step "Remove-VMAssignableDevice $($a.LocationPath)"
        Remove-VMAssignableDevice -VMName $VmName -LocationPath $a.LocationPath -ErrorAction SilentlyContinue
        Write-Step "Mount-VMHostAssignableDevice $($a.LocationPath)"
        Mount-VMHostAssignableDevice -LocationPath $a.LocationPath -ErrorAction SilentlyContinue
    }
    Get-PnpDevice -PresentOnly | Where-Object {
        $_.Class -eq "Display" -and $_.FriendlyName -match "NVIDIA|GeForce|RTX" -and $_.Status -ne "OK"
    } | ForEach-Object {
        Write-Step "Enable-PnpDevice $($_.FriendlyName)"
        Enable-PnpDevice -InstanceId $_.InstanceId -Confirm:$false -ErrorAction SilentlyContinue
    }
    Write-Host "Release attempted - check Device Manager / display." -ForegroundColor Green
}
