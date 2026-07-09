#Requires -Version 5.1
<#
.SYNOPSIS
  Create secondary pagefile on RamShared volume (RF-6 lab path).

.DESCRIPTION
  Uses WMI/CIM for pagefile config. Requires admin. Drive must be NTFS formatted.
  Does NOT remove C: pagefile (keep system stable). Sets AutomaticManagedPagefile false.
#>
[CmdletBinding()]
param(
    [string]$DriveLetter = "V",
    [int]$InitialSizeMb = 32,
    [int]$MaximumSizeMb = 64
)

$ErrorActionPreference = "Stop"
$letter = $DriveLetter.TrimEnd(':')
$path = "${letter}:\pagefile.sys"
if (-not (Test-Path "${letter}:\")) {
    throw "Drive ${letter}: not mounted"
}

Write-Host "Setup-PagefileVolume path=$path initial=$InitialSizeMb max=$MaximumSizeMb"

$cs = Get-CimInstance Win32_ComputerSystem
if ($cs.AutomaticManagedPagefile) {
    $cs | Set-CimInstance -Property @{ AutomaticManagedPagefile = $false }
    Write-Host "AutomaticManagedPagefile=false"
}

# Existing pagefiles
$existing = @(Get-CimInstance Win32_PageFileSetting -EA SilentlyContinue)
foreach ($e in $existing) {
    Write-Host "existing pagefile: $($e.Name) min=$($e.InitialSize) max=$($e.MaximumSize)"
}

$pf = $existing | Where-Object { $_.Name -like "${letter}:*" }
if (-not $pf) {
    # Create via WMI
    $null = ([wmiclass]"Win32_PageFileSetting").Create($path)
    $pf = Get-CimInstance Win32_PageFileSetting | Where-Object { $_.Name -like "${letter}:*" }
}
if (-not $pf) {
    throw "failed to create Win32_PageFileSetting for $path"
}

$pf | Set-CimInstance -Property @{
    InitialSize = $InitialSizeMb
    MaximumSize = $MaximumSizeMb
}
Write-Host "SET pagefile $path ${InitialSizeMb}-${MaximumSizeMb} MB"
Write-Host "NOTE: pagefile may require reboot to become active on some builds (DT-8)."
Write-Host "After reboot: Get-CimInstance Win32_PageFileUsage"
return 0
