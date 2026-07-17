#Requires -Version 5.1
<#
.SYNOPSIS
  Initialize + NTFS-format the RamShared virtual LUN only (anti data-loss).

.DESCRIPTION
  SPEC DT-11: require vendor/product/VPD serial/size conjunction. No size-only
  fallback. -Force skips prompt only; never bypasses identity. Revalidates disk
  number and free letter immediately before Initialize-Disk.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [UInt64]$ExpectedSizeBytes,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedSerial,
    [string]$ExpectedVendor = "RAMSHARE",
    [string]$ExpectedProduct = "VRAMDISK",
    [string]$DriveLetter = "D",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
function L($m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m) }

$letter = $DriveLetter.TrimEnd(':').Substring(0, 1).ToUpperInvariant()
if ($letter -lt 'D' -or $letter -gt 'Z') {
    throw "REFUSE: volume letter must be D..Z (got $letter)"
}
$vol = Get-Volume -DriveLetter $letter -EA SilentlyContinue
if ($vol) {
    throw "REFUSE: letter ${letter}: already in use (Label=$($vol.FileSystemLabel)). Pick a free letter."
}

$serial = $ExpectedSerial.Trim().ToUpperInvariant()
if ($serial.Length -ne 16) {
    throw "REFUSE: ExpectedSerial must be 16 hex chars (got len=$($serial.Length))"
}

function Get-RamSharedCandidate {
    # Prefer disks whose friendly name matches product identity; never size-only.
    $all = @(Get-Disk | Where-Object { $_.Number -ne 0 -and $_.Size -eq $ExpectedSizeBytes })
    $named = @($all | Where-Object {
            $_.FriendlyName -match 'RAMSHARE|VRAMDISK|RamShared'
        })
    if ($named.Count -ge 1) { return $named[0] }
    return $null
}

$d = Get-RamSharedCandidate
if (-not $d) {
    throw "refuse_physical_same_size: no disk with vendor/product-like name AND exact size=$ExpectedSizeBytes (size-only match deleted)"
}

$nameOk = $d.FriendlyName -match 'RAMSHARE|VRAMDISK|RamShared'
$sizeOk = $d.Size -eq $ExpectedSizeBytes
if (-not ($nameOk -and $sizeOk)) {
    throw "refuse_wrong_serial/identity: disk N=$($d.Number) Name=$($d.FriendlyName) Size=$($d.Size)"
}

# VPD serial: Storage cmdlets may expose SerialNumber; require match when present.
$sn = $d.SerialNumber
if ($sn -and ($sn.ToUpperInvariant() -ne $serial) -and ($sn.Trim() -ne '')) {
    # Some stacks pad serial; compare prefix/contains of 16 hex.
    if ($sn.ToUpperInvariant() -notmatch [regex]::Escape($serial)) {
        throw "refuse_wrong_serial: disk serial='$sn' expected='$serial'"
    }
}

L "Target disk N=$($d.Number) Name=$($d.FriendlyName) Size=$($d.Size) Style=$($d.PartitionStyle) Serial=$sn"

if (-not $Force) {
    if (-not [Environment]::UserInteractive) {
        throw "non-interactive requires -Force (Force does not bypass identity)"
    }
    $ans = Read-Host "Initialize GPT + NTFS as ${letter}: Type YES"
    if ($ans -ne 'YES') { throw "operator declined" }
} else {
    L "Force: skip prompt only (force_does_not_bypass_identity)"
}

# Revalidate immediately before mutation (DT-11 TOCTOU).
$d2 = Get-Disk -Number $d.Number -EA Stop
if ($d2.Size -ne $ExpectedSizeBytes) {
    throw "REFUSE: disk number revalidation size mismatch"
}
if ($d2.FriendlyName -notmatch 'RAMSHARE|VRAMDISK|RamShared') {
    throw "REFUSE: disk number revalidation name mismatch"
}
$vol2 = Get-Volume -DriveLetter $letter -EA SilentlyContinue
if ($vol2) {
    throw "REFUSE: letter ${letter}: became busy before format"
}

try {
    Set-Disk -Number $d2.Number -IsOffline $false -ErrorAction SilentlyContinue
    Set-Disk -Number $d2.Number -IsReadOnly $false -ErrorAction SilentlyContinue
} catch {}

if ($d2.PartitionStyle -ne 'RAW' -and $d2.NumberOfPartitions -gt 0) {
    L "already partitioned - refusing wipe (use manual path if intentional reformat)"
    Get-Partition -DiskNumber $d2.Number | Format-Table -AutoSize
    Write-Host "format_exact_ramshared_lun=SKIP_ALREADY_PARTITIONED"
    exit 0
}

try {
    if ($d2.PartitionStyle -eq 'RAW') {
        Initialize-Disk -Number $d2.Number -PartitionStyle GPT -Confirm:$false
    }
    $part = New-Partition -DiskNumber $d2.Number -UseMaximumSize -DriveLetter $letter -ErrorAction Stop
    $part | Format-Volume -FileSystem NTFS -NewFileSystemLabel "RAMSHARED" -Confirm:$false | Out-Null
} catch {
    $msg = $_.Exception.Message
    if ($msg -match '40004|not ready|I/O|IO device') {
        throw "FORMAT_IO_FAIL (backend dead or sense issue): $msg"
    }
    throw
}

L "FORMAT_OK ${letter}: NTFS on disk $($d2.Number) serial=$serial"
Write-Host "format_exact_ramshared_lun=1"
Get-Volume -DriveLetter $letter | Format-List DriveLetter, FileSystemLabel, FileSystem, Size, SizeRemaining
exit 0
