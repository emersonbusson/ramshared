#Requires -RunAsAdministrator
# Dual-boot prep on E: (ESPANHA) — status / re-shrink helper.
# 2026-07-10: ~32 GB unallocated already carved on disk 0.
param(
    [switch]$StatusOnly
)

$ErrorActionPreference = "Stop"
Set-Location C:\

Write-Host "=== Dual-boot / kernel-true disk status ===" -ForegroundColor Cyan

$r = Get-PartitionSupportedSize -DriveLetter R
Write-Host ("R: RUSSIA shrinkable GB = {0:N2}" -f (($r.SizeMax - $r.SizeMin)/1GB))

$e = Get-PartitionSupportedSize -DriveLetter E
Write-Host ("E: ESPANHA shrinkable GB = {0:N2}" -f (($e.SizeMax - $e.SizeMin)/1GB))

$disk0 = Get-Disk -Number 0
Write-Host ("Disk0 {0} LargestFreeExtent GB = {1:N2}" -f $disk0.FriendlyName, ($disk0.LargestFreeExtent/1GB))

Get-Partition -DiskNumber 0 | Select PartitionNumber, DriveLetter, @{n="SizeGB";e={[math]::Round($_.Size/1GB,2)}}, Type | Format-Table -AutoSize

$iso = "R:\Hyper-V\iso\ubuntu-24.04.2-live-server-amd64.iso"
Write-Host ("Ubuntu ISO present: {0}" -f (Test-Path $iso))
Write-Host "Docs: docs/labs/DUALBOOT-KERNEL-TRUE.md"
Write-Host "Next: boot USB installer; use ONLY the unallocated space on SAMSUNG HD154UI (E: disk)."
