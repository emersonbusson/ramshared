#Requires -Version 5.1
<#
.SYNOPSIS
  Normalize Windows PagingFiles registry entries for RamShared storage-only drills.

.DESCRIPTION
  The product teardown fails closed when HKLM Memory Management\PagingFiles
  contains ambiguous entries such as ?:\pagefile.sys without min/max values.
  This helper snapshots the current value and, only with -Apply -Approve, writes
  a concrete C:\pagefile.sys 0 0 entry. A reboot is required by Windows.

  This script never creates, formats, mounts, or modifies any disk.
#>
[CmdletBinding()]
param(
    [switch]$Apply,
    [switch]$Restore,
    [switch]$Approve,
    [string]$SnapshotPath = "C:\ProgramData\RamShared\pagingfiles-snapshot.json"
)

$ErrorActionPreference = "Stop"
$key = "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management"
$valueName = "PagingFiles"

function L([string]$Message) {
    Write-Host "[pagingfiles-concrete] $Message"
}

function Read-Current {
    return @((Get-ItemProperty -LiteralPath $key -Name $valueName -ErrorAction Stop).PagingFiles)
}

function Is-ConcreteEntry([string]$Entry) {
    if ([string]::IsNullOrWhiteSpace($Entry)) { return $true }
    $parts = @($Entry.Trim() -split "\s+")
    if ($parts.Count -lt 3) { return $false }
    $path = [string]$parts[0]
    return ($path -match "^[A-Za-z]:\\") -and -not $path.StartsWith("?:\")
}

function Write-Snapshot([string[]]$Entries) {
    $dir = Split-Path -Parent $SnapshotPath
    if ($dir) { New-Item -Force -ItemType Directory -Path $dir | Out-Null }
    [ordered]@{
        schema = 1
        ts = (Get-Date).ToUniversalTime().ToString("o")
        key = $key
        value = $valueName
        paging_files = @($Entries)
    } | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 -LiteralPath $SnapshotPath
}

function Read-Snapshot {
    if (-not (Test-Path -LiteralPath $SnapshotPath)) {
        throw "snapshot not found: $SnapshotPath"
    }
    $snap = Get-Content -LiteralPath $SnapshotPath -Raw | ConvertFrom-Json
    if ($snap.schema -ne 1 -or $snap.value -ne $valueName) {
        throw "invalid snapshot schema/value: $SnapshotPath"
    }
    return @($snap.paging_files | ForEach-Object { [string]$_ })
}

if ($Apply -and $Restore) {
    throw "Use either -Apply or -Restore, not both."
}

$current = @(Read-Current)
$bad = @($current | Where-Object { -not (Is-ConcreteEntry ([string]$_)) })
L ("current=" + (($current | ForEach-Object { "[$_]" }) -join " "))
if ($bad.Count -eq 0) {
    L "current entries are already concrete"
} else {
    L ("ambiguous_or_malformed=" + (($bad | ForEach-Object { "[$_]" }) -join " "))
}

if ($Restore) {
    if (-not $Approve) {
        throw "Refusing restore without -Approve"
    }
    $restore = @(Read-Snapshot)
    Set-ItemProperty -LiteralPath $key -Name $valueName -Type MultiString -Value $restore
    L ("restored snapshot from " + $SnapshotPath)
    L "REBOOT_REQUIRED=1"
    exit 0
}

if (-not $Apply) {
    L "PLAN_ONLY=1"
    L "To normalize: rerun with -Apply -Approve. Recommended target: [C:\pagefile.sys 0 0]."
    exit 0
}

if (-not $Approve) {
    throw "Refusing apply without -Approve"
}
if ($bad.Count -eq 0) {
    L "nothing to change"
    exit 0
}

Write-Snapshot $current
$next = @("C:\pagefile.sys 0 0")
Set-ItemProperty -LiteralPath $key -Name $valueName -Type MultiString -Value $next
L ("snapshot=" + $SnapshotPath)
L "set PagingFiles=[C:\pagefile.sys 0 0]"
L "REBOOT_REQUIRED=1"
exit 0
