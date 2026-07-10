#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Prepare dual-boot Linux on R: (RUSSIA) WITHOUT wiping existing NTFS data.

.DESCRIPTION
  Path 3 - closest to bare-metal for kernel-true research.
  - Shrinks R: NTFS to create unallocated space (default 100 GB)
  - Downloads Ubuntu desktop/server ISO to R:\Hyper-V\iso (shared with VM lab)
  - Writes a runbook for UEFI install (cannot finish install unattended without reboot + user)

  DOES NOT:
  - format R:
  - delete "BAIXADOS RUSSIA" or other files
  - change boot order automatically (too risky unattended)

.PARAMETER ShrinkGB
  Free space to carve for Linux (default 100). Needs that much free *inside* NTFS.

.PARAMETER ApplyShrink
  Actually call Resize-Partition. Without this, only plans.

.PARAMETER DownloadIso
  Download Ubuntu 24.04.2 live-server ISO if missing
#>
[CmdletBinding()]
param(
    [int]$ShrinkGB = 100,
    [switch]$ApplyShrink,
    [switch]$DownloadIso,
    [string]$RunbookPath = "R:\Hyper-V\dual-boot\RUNBOOK-dualboot-RUSSIA.txt"
)

$ErrorActionPreference = "Stop"
function Write-Step($m) { Write-Host "==> $m" -ForegroundColor Cyan }

if (-not (Test-Path "R:\")) { throw "R: missing" }

$vol = Get-Volume -DriveLetter R
$part = Get-Partition -DriveLetter R
$disk = Get-Disk -Number $part.DiskNumber
$freeGB = [math]::Round($vol.SizeRemaining / 1GB, 1)
$sizeGB = [math]::Round($part.Size / 1GB, 1)

Write-Step "Disk $($disk.Number) $($disk.FriendlyName) GPT=$($disk.PartitionStyle)"
Write-Step "R: label=$($vol.FileSystemLabel) size=${sizeGB}G free_inside_ntfs=${freeGB}G"

if ($freeGB -lt ($ShrinkGB + 5)) {
    throw "Not enough free space inside R: to shrink by ${ShrinkGB}G (free=$freeGB G). Free files first."
}

$newSizeBytes = [uint64]($part.Size - ($ShrinkGB * 1GB))
$newSizeGB = [math]::Round($newSizeBytes / 1GB, 1)
Write-Step "Plan: Resize-Partition R from ${sizeGB}G -> ${newSizeGB}G (unallocated ~${ShrinkGB}G for Ubuntu)"

$IsoDir = "R:\Hyper-V\iso"
$IsoPath = Join-Path $IsoDir "ubuntu-24.04.2-live-server-amd64.iso"
$IsoUrl = "https://releases.ubuntu.com/24.04.2/ubuntu-24.04.2-live-server-amd64.iso"

New-Item -ItemType Directory -Force -Path (Split-Path $RunbookPath), $IsoDir | Out-Null

if ($DownloadIso) {
    if (Test-Path $IsoPath -PathType Leaf) {
        Write-Step "ISO exists: $IsoPath"
    } else {
        Write-Step "Downloading ISO..."
        Start-BitsTransfer -Source $IsoUrl -Destination $IsoPath -DisplayName "Ubuntu-ISO-dualboot"
    }
}

if ($ApplyShrink) {
    Write-Warning "SHRINKING R: NTFS - existing files kept, but ALWAYS have backups of critical data."
    # Supported size query
    $sizeMinMax = Get-PartitionSupportedSize -DriveLetter R
    if ($newSizeBytes -lt $sizeMinMax.SizeMin -or $newSizeBytes -gt $sizeMinMax.SizeMax) {
        throw "Requested size $newSizeBytes outside supported range Min=$($sizeMinMax.SizeMin) Max=$($sizeMinMax.SizeMax)"
    }
    Resize-Partition -DriveLetter R -Size $newSizeBytes
    Write-Step "Shrink applied. Unallocated space should appear on disk $($disk.Number)."
    Get-Partition -DiskNumber $disk.Number | Format-Table PartitionNumber, DriveLetter, @{n="SizeGB";e={[math]::Round($_.Size/1GB,1)}}, Type
} else {
    Write-Host "Dry-run only. Re-run with -ApplyShrink to carve unallocated space." -ForegroundColor Yellow
}

$runbook = @"
RamShared - Dual-boot Ubuntu on R: (RUSSIA)
===========================================
Generated: $(Get-Date -Format o)
Disk: #$($disk.Number) $($disk.FriendlyName) (mechanical HDD ~500GB)
Goal: bare-metal Linux for kernel-true VRAM research (Gate A)

SAFETY
------
- NTFS data on R: is KEPT if only shrink was used (no format of R:).
- You still need a BACKUP of anything irreplaceable.
- Windows EFI bootloader must remain intact; Ubuntu installer "alongside" or
  manual partitions in the NEW unallocated space only.
- Do NOT select "use entire disk" on the Windows system disk by mistake.
  Install ONLY into free space on the Hitachi (RUSSIA) disk.

PREP (this script)
------------------
1) Free space inside R: before shrink (done plan: ${ShrinkGB} GB).
2) ApplyShrink: $(if ($ApplyShrink) { 'YES applied' } else { 'NOT YET - run Prepare-DualBootRussia.ps1 -ApplyShrink -DownloadIso' })
3) ISO: $IsoPath

MAKE BOOT MEDIA
---------------
Option A - USB (recommended):
  - Use Rufus or: 
    # From elevated PowerShell with ISO + USB disk N:
    #  (identify USB carefully with Get-Disk)
  - Or Ventoy copy ISO.

Option B - Mount ISO and copy kernel (advanced; skip if unsure).

INSTALL
-------
1) Firmware: disable Secure Boot temporarily if Ubuntu fails to boot-sign, or keep SB and use default Ubuntu SB support.
2) Boot USB, "Try or Install Ubuntu".
3) Partitioning: "Something else" / manual:
   - Create ESP only if this disk has none AND you know what you are doing;
     prefer installing GRUB to the EXISTING EFI System Partition on the Windows disk
     (usually disk 0), OR create EFI on RUSSIA if policy allows dual ESP.
   - Root ext4 on the unallocated region of RUSSIA (~${ShrinkGB}G).
   - Optional swap partition 8-16G on RUSSIA (not on C:).
4) Finish install, reboot, pick Ubuntu in UEFI boot menu (F12/Boot Options).

AFTER BOOT (Ubuntu bare-metal)
------------------------------
  sudo apt update && sudo apt install -y build-essential git linux-headers-\$(uname -r) \\
      flex bison libssl-dev libelf-dev dwarves
  # NVIDIA proprietary if you need CUDA/HMM experiments:
  # ubuntu-drivers devices
  # sudo ubuntu-drivers autoinstall
  lspci -nn | grep -i nvidia
  ls -la /dev/dri
  # If you see 10de:xxxx and /dev/dri - Gate A for kernel-true is closer to PASS

RELATION TO Hyper-V LAB
-----------------------
- VM on R:\Hyper-V\linux-kernel-lab = generic kernel without taking host GPU.
- Dual-boot = true metal for driver/mm experiments.
- Do not run Resize while VMs have open VHDX on R: - shut down linux-kernel-lab first.

ROLLBACK
--------
- If only shrink: Windows still boots; unallocated space unused until you create partitions.
- To give space back to R:: delete Linux partitions in diskmgmt.msc, then
  Resize-Partition -DriveLetter R -Size <max> (Get-PartitionSupportedSize).
- If Windows won't boot: Windows recovery / bcdedit / UEFI boot order - do not panic-format.

"@

Set-Content -Path $RunbookPath -Value $runbook -Encoding UTF8
Write-Step "Runbook: $RunbookPath"
Write-Host "DONE Prepare-DualBootRussia" -ForegroundColor Green
