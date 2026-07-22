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
    "Start-Job",
    "probe_during_sampling",
    "Direct load during sampling",
    "three_rounds_emit_p50_p95_p99",
    "matching_checksum_exits_0",
    "checksum_mismatch_exits_6",
    "FILE_FLAG_NO_BUFFERING",
    "FILE_FLAG_WRITE_THROUGH",
    "UNCACHED_READ_BYTES",
    "UNCACHED_WRITE_BYTES"
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

$uncached = [regex]::Match($text, "(?s)`\$uncachedSource = @'\r?\n(.*?)\r?\n'@")
if (-not $uncached.Success) {
    throw "ramshared_disk_io_static: uncached C# source block missing"
}
Add-Type -TypeDefinition $uncached.Groups[1].Value -ErrorAction Stop

Write-Output "PASS Test-RamSharedDiskIoStatic"
