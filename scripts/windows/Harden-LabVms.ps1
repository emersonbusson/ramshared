#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Safe Hyper-V lab hardening: stop disk-fill and protect host C:.

.DESCRIPTION
  - Disables checkpoints (main cause of multi-GB avhdx piles)
  - Caps dynamic memory
  - Sets host default VM/VHD paths to R: (never C:)
  - Does NOT delete VHDs, merge disks, or Convert-VHD
  - Does NOT touch the host Windows install

.PARAMETER AlsoGha
  Also harden gha-ubuntu-2404 (default: skip)
#>
param([switch]$AlsoGha)

$ErrorActionPreference = "Continue"
Set-Location C:\

$labs = @("win11-drill", "linux-kernel-lab")
if ($AlsoGha) { $labs += "gha-ubuntu-2404" }

New-Item -ItemType Directory -Force -Path "R:\Hyper-V\VMs", "R:\Hyper-V\VHDs" | Out-Null
try {
    Set-VMHost -VirtualMachinePath "R:\Hyper-V\VMs" -VirtualHardDiskPath "R:\Hyper-V\VHDs"
    Write-Host "OK VMHost defaults -> R:\Hyper-V (C: protected)"
} catch {
    Write-Host "WARN VMHost: $($_.Exception.Message)"
}

foreach ($name in $labs) {
    $v = Get-VM -Name $name -ErrorAction SilentlyContinue
    if (-not $v) {
        Write-Host "SKIP missing $name"
        continue
    }
    Write-Host "Harden $name (path=$($v.Path))"
    try { Set-VM -Name $name -AutomaticCheckpointsEnabled $false } catch { Write-Host "  $($_.Exception.Message)" }
    try { Set-VM -Name $name -CheckpointType Disabled } catch { Write-Host "  $($_.Exception.Message)" }
    if ($name -eq "win11-drill") {
        Set-VM -Name $name -AutomaticStartAction Nothing -AutomaticStopAction ShutDown -ErrorAction SilentlyContinue
        try {
            Set-VMMemory -VMName $name -DynamicMemoryEnabled $true -StartupBytes 4GB -MinimumBytes 2GB -MaximumBytes 8GB
        } catch { Write-Host "  mem: $($_.Exception.Message)" }
    }
    if ($name -eq "linux-kernel-lab") {
        Set-VM -Name $name -AutomaticStartAction StartIfRunning -AutomaticStopAction ShutDown -ErrorAction SilentlyContinue
        try {
            Set-VMMemory -VMName $name -DynamicMemoryEnabled $true -MinimumBytes 2GB -MaximumBytes 8GB
        } catch { Write-Host "  mem: $($_.Exception.Message)" }
    }
    $sn = @(Get-VMSnapshot -VMName $name -ErrorAction SilentlyContinue)
    if ($sn.Count -gt 0) {
        Write-Host "  WARN: $name has $($sn.Count) snapshot(s). Not auto-deleting (merge can fill disk). Remove manually in Hyper-V Manager when safe."
    }
}

Write-Host ""
Write-Host "Disk caps (dynamic VHD will not exceed MaxGB):"
Get-VMHardDiskDrive win11-drill, linux-kernel-lab -ErrorAction SilentlyContinue | ForEach-Object {
    try {
        $h = Get-VHD -Path $_.Path
        "{0,-18} file={1,6:N2}G  max={2,5:N0}G  {3}" -f $_.VMName, ($h.FileSize / 1GB), ($h.Size / 1GB), $_.Path
    } catch {
        Write-Host "$($_.VMName): $($_.Exception.Message)"
    }
}

Write-Host ""
Write-Host "Free: C=$([math]::Round((Get-Volume C).SizeRemaining/1GB,1))G  R=$([math]::Round((Get-Volume R).SizeRemaining/1GB,1))G  E=$([math]::Round((Get-Volume E).SizeRemaining/1GB,1))G"
Write-Host "DONE Harden-LabVms (no destructive disk ops)"
