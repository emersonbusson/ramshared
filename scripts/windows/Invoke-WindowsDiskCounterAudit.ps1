#Requires -Version 5.1
<#
.SYNOPSIS
  Plan or run the Windows virtual disk counter audit.

.DESCRIPTION
  This harness does not initialize or format disks directly. Approved live mode
  delegates LUN lifecycle to Run-HostExhaustive.ps1, then parses the generated
  artifact for DISK_IO_MEASURE_OK, direct checksum I/O, and non-zero PerfDisk
  activity evidence.
#>
[CmdletBinding()]
param(
    [switch]$Run,
    [switch]$ApprovePhysicalHost,
    [UInt64]$SizeBytes = 67108864,
    [string]$OutDir = "C:\ramshared\artifacts\disk-counter-audit-$(Get-Date -Format yyyyMMdd-HHmmss)"
)

$ErrorActionPreference = "Stop"

function L([string]$Message) {
    Write-Host "[windows-disk-counter-audit] $Message"
}

function Read-FirstRegex([string]$Text, [string]$Pattern) {
    $m = [regex]::Match($Text, $Pattern)
    if ($m.Success) { return $m.Groups[1].Value }
    return $null
}

New-Item -Force -ItemType Directory -Path $OutDir | Out-Null

$plan = [ordered]@{
    tool = "Invoke-WindowsDiskCounterAudit.ps1"
    run = [bool]$Run
    approve_physical_host = [bool]$ApprovePhysicalHost
    size_bytes = $SizeBytes
    stages = @(
        "storage-only preflight",
        "delegated RAMSHARE VRAMDISK lifecycle",
        "formatted mounted write/read sampling",
        "checksum direct I/O",
        "teardown LUN/Win32/PnP gone"
    )
    note = "Task Manager UI parity is not claimed; CIM/direct metrics are authoritative for this audit."
}
$plan | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $OutDir "audit-plan.json")

if (-not $Run) {
    L "PLAN_ONLY=1"
    L ("OUT_DIR=" + $OutDir)
    exit 0
}

if (-not $ApprovePhysicalHost) {
    throw "Refusing live disk counter audit: -ApprovePhysicalHost is required"
}

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$preflight = Join-Path $scriptRoot "Get-WinDrivePreflight.ps1"
$runner = Join-Path $scriptRoot "Run-HostExhaustive.ps1"
if (-not (Test-Path -LiteralPath $preflight)) { throw "Missing preflight script: $preflight" }
if (-not (Test-Path -LiteralPath $runner)) { throw "Missing exhaustive runner: $runner" }

L "RUN preflight"
$preflightOutput = & $preflight -StorageOnly 2>&1 | ForEach-Object { $_.ToString() }
$preflightExit = $LASTEXITCODE
$preflightOutput | Set-Content -Encoding utf8 (Join-Path $OutDir "preflight.out")
if ($preflightExit -ne 0) {
    throw "Preflight failed exit=$preflightExit"
}

L "RUN delegated exhaustive host smoke"
$runnerOutput = & $runner -SizeBytes $SizeBytes -ExternalWorkloadMiB 0 -MinFreeAfterPlanMiB 512 2>&1 |
    ForEach-Object { $_.ToString() }
$runnerExit = $LASTEXITCODE
$runnerText = ($runnerOutput -join "`n")
$runnerText | Set-Content -Encoding utf8 (Join-Path $OutDir "runner.out")
if ($runnerExit -ne 0) {
    throw "Delegated exhaustive runner failed exit=$runnerExit"
}

$artifact = Read-FirstRegex $runnerText 'ARTIFACT=(C:\\[^\r\n]+)'
if ([string]::IsNullOrWhiteSpace($artifact)) {
    $latest = Get-ChildItem -LiteralPath "C:\ramshared\artifacts" -Directory -Filter "exhaustive-*" -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if ($latest) {
        $artifact = $latest.FullName
        L ("delegated ARTIFACT recovered from latest exhaustive directory: " + $artifact)
    }
}
if ([string]::IsNullOrWhiteSpace($artifact)) {
    throw "Could not find delegated ARTIFACT path"
}
$summaryPath = Join-Path $artifact "summary.json"
$diskIoPath = Join-Path $artifact "disk-io.out"
if (-not (Test-Path -LiteralPath $summaryPath)) { throw "Missing delegated summary: $summaryPath" }
if (-not (Test-Path -LiteralPath $diskIoPath)) { throw "Missing delegated disk I/O evidence: $diskIoPath" }

$summary = Get-Content -LiteralPath $summaryPath -Raw | ConvertFrom-Json
$diskIo = Get-Content -LiteralPath $diskIoPath -Raw
$directLoad = $diskIo -match 'Direct load during sampling iterations=(\d+) written=([0-9,.]+) MiB read=([0-9,.]+) MiB match=True'
$directProbe = $diskIo -match 'Direct \d+ MiB write=.* read=.* match=True'
$perfMatch = $diskIo -match 'PerfDisk match:'
$busyMax = Read-FirstRegex $diskIo 'Busy pct DiskTime\s+avg=[0-9,.]+ pct max=([0-9,.]+) pct'
$writeMax = Read-FirstRegex $diskIo 'Write\s+avg=[0-9,.]+ MB/s max=([0-9,.]+) MB/s'
$queueMax = Read-FirstRegex $diskIo 'Queue depth\s+avg=[0-9,.]+ max=([0-9,.]+)'
$activity = $false
foreach ($v in @($busyMax, $writeMax, $queueMax)) {
    if ($null -ne $v) {
        $n = [double]::Parse(($v -replace ',', '.'), [Globalization.CultureInfo]::InvariantCulture)
        if ($n -gt 0) { $activity = $true }
    }
}

$pass = [bool]$summary.DISK_IO_MEASURE_OK -and $directLoad -and $directProbe -and $perfMatch -and $activity
$audit = [ordered]@{
    PASS = [bool]$pass
    ARTIFACT = $OutDir
    DELEGATED_ARTIFACT = $artifact
    DISK_IO_MEASURE_OK = [bool]$summary.DISK_IO_MEASURE_OK
    DIRECT_LOAD_MATCH = [bool]$directLoad
    DIRECT_PROBE_MATCH = [bool]$directProbe
    PERFDISK_MATCH = [bool]$perfMatch
    NONZERO_ACTIVITY = [bool]$activity
    LUN_GONE = [bool]$summary.LUN_GONE
    WIN32_GONE = [bool]$summary.WIN32_GONE
    PNP_GONE = [bool]$summary.PNP_GONE
}
$audit | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $OutDir "audit-summary.json")
L ("SUMMARY " + ($audit | ConvertTo-Json -Compress))
if ($pass) { exit 0 }
exit 2
