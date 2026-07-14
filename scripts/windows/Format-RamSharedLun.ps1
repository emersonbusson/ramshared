#Requires -Version 5.1
<#
.SYNOPSIS
  Initialize + NTFS-format the RamShared virtual LUN only (anti data-loss).

.DESCRIPTION
  Task Manager shows "Formatado: 0 MB" when PartitionStyle is RAW. This script
  only touches disks that match RamShared identity (name and/or exact size).
  Requires WinDriveBackend (or winsvc) alive so WRITE/READ succeed; otherwise
  Initialize-Disk fails with StorageWMI 40004.

.EXAMPLE
  .\Format-RamSharedLun.ps1 -ExpectedSizeBytes 67108864 -DriveLetter S -Force
#>
[CmdletBinding()]
param(
    [UInt64]$ExpectedSizeBytes = 67108864,
    [string]$DriveLetter = "S",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
function L($m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m) }

$letter = $DriveLetter.TrimEnd(':').Substring(0, 1).ToUpperInvariant()
$vol = Get-Volume -DriveLetter $letter -EA SilentlyContinue
if ($vol) {
    throw "REFUSE: letter ${letter}: already in use (Label=$($vol.FileSystemLabel)). Pick a free letter."
}

# Backend must be serving IO. Ghost RAW disks after backend exit cause 40004.
$be = Get-Process -Name WinDriveBackend -EA SilentlyContinue
if (-not $be) {
    Write-Warning "WinDriveBackend not running. CREATE_DISK without backend makes format fail (40004). Start-RamSharedLab.ps1 first."
}

$d = Get-Disk | Where-Object {
        ($_.FriendlyName -match 'RAMSHARE|RamShared|VRAMDISK') -and ($_.Size -ge 1MB)
    } | Select-Object -First 1
if (-not $d -and $ExpectedSizeBytes -gt 0) {
    $d = Get-Disk | Where-Object { $_.Number -ne 0 -and $_.Size -eq $ExpectedSizeBytes } |
        Select-Object -First 1
}
if (-not $d) { throw "no RamShared disk (start backend CREATE_DISK first)" }

$nameOk = $d.FriendlyName -match 'RAMSHARE|RamShared|VRAM'
$sizeOk = $d.Size -eq $ExpectedSizeBytes
if (-not ($nameOk -or $sizeOk)) {
    throw "REFUSE: disk N=$($d.Number) identity fail Name=$($d.FriendlyName) Size=$($d.Size)"
}

L "Target disk N=$($d.Number) Name=$($d.FriendlyName) Size=$($d.Size) Style=$($d.PartitionStyle) Sector=$($d.LogicalSectorSize)"

if (-not $Force) {
    if (-not [Environment]::UserInteractive) {
        throw "non-interactive requires -Force"
    }
    $ans = Read-Host "Initialize GPT + NTFS as ${letter}: Type YES"
    if ($ans -ne 'YES') { throw "operator declined" }
}

try {
    Set-Disk -Number $d.Number -IsOffline $false -ErrorAction SilentlyContinue
    Set-Disk -Number $d.Number -IsReadOnly $false -ErrorAction SilentlyContinue
} catch {}

if ($d.PartitionStyle -ne 'RAW' -and $d.NumberOfPartitions -gt 0) {
    L "already partitioned - refusing wipe (use manual path if intentional reformat)"
    Get-Partition -DiskNumber $d.Number | Format-Table -AutoSize
    exit 0
}

try {
    if ($d.PartitionStyle -eq 'RAW') {
        Initialize-Disk -Number $d.Number -PartitionStyle GPT -Confirm:$false
    }
    $part = New-Partition -DiskNumber $d.Number -UseMaximumSize -DriveLetter $letter -ErrorAction Stop
    $part | Format-Volume -FileSystem NTFS -NewFileSystemLabel "RAMSHARED" -Confirm:$false | Out-Null
} catch {
    $msg = $_.Exception.Message
    if ($msg -match '40004|not ready|I/O|IO device') {
        throw "FORMAT_IO_FAIL (often backend dead or TUR/sense issue): $msg. Start WinDriveBackend, then retry."
    }
    throw
}

L "FORMAT_OK ${letter}: NTFS on disk $($d.Number)"
Get-Volume -DriveLetter $letter | Format-List DriveLetter, FileSystemLabel, FileSystem, Size, SizeRemaining
exit 0
