#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ScriptPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Measure-RamSharedDiskIo.ps1"
}
$text = Get-Content -LiteralPath $ScriptPath -Raw

foreach ($needle in @(
    "Win32_PerfFormattedData_PerfDisk_PhysicalDisk",
    "DiskReadBytesPersec",
    "DiskWriteBytesPersec",
    "PercentDiskTime",
    "Get-FileHash -Algorithm SHA256",
    "three_rounds_emit_p50_p95_p99",
    "matching_checksum_exits_0",
    "checksum_mismatch_exits_6"
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("ramshared_disk_io_static: missing " + $needle)
    }
}

$checksum = $text.IndexOf('if ($ChecksumRounds -gt 0)')
$defaultExit = $text.IndexOf('# Exit 0 if we have disk')
if ($checksum -lt 0 -or $defaultExit -lt 0 -or $defaultExit -lt $checksum) {
    throw "ramshared_disk_io_static: default exit must remain after checksum mode"
}

if ($text -match '\$match\s*=\s*\(\$got\.Length\s*-eq\s*\$bytes\.Length\)') {
    throw "ramshared_disk_io_static: length-only direct match is forbidden"
}

Write-Output "PASS Test-RamSharedDiskIoStatic"
