#Requires -Version 5.1
<#
.SYNOPSIS
  DT-9 ordered stop for lab backend (VM only).

.DESCRIPTION
  Stand-in for ramshared-winsvc stop:
    1) If secondary pagefile on -Drive still allocated -> refuse kill (or -ForceRebootMark)
    2) Else kill WinDriveBackend

  Never destroy/kill while pagefile-hot (avoids BugCheck 0x7A / c0000185).
#>
[CmdletBinding()]
param(
    [string]$Drive = "D",
    [switch]$AllowKillIfPagefileGone,
    [switch]$ForceKillUnsafe
)

$ErrorActionPreference = "Continue"
$letter = $Drive.TrimEnd(':')
$pf = Get-CimInstance Win32_PageFileUsage -EA SilentlyContinue |
    Where-Object { $_.Name -match "^${letter}:" }

if ($pf -and $pf.AllocatedBaseSize -gt 0 -and -not $ForceKillUnsafe) {
    Write-Host "REFUSE_KILL pagefile still hot: $($pf.Name) a=$($pf.AllocatedBaseSize) u=$($pf.CurrentUsage)"
    Write-Host "DT9: remove pagefile + reboot, then re-run Stop-RamSharedLab"
    exit 2
}

if ($ForceKillUnsafe) {
    Write-Warning "ForceKillUnsafe: pagefile-hot kill may BSOD 0x7A"
}

Get-Process WinDriveBackend -EA SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$alive = [bool](Get-Process WinDriveBackend -EA SilentlyContinue)
Write-Host "BACKEND_ALIVE=$alive"
if ($alive) { exit 1 }
Write-Host "STOP_OK"
exit 0
