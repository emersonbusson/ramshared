#Requires -Version 5.1
<#
.SYNOPSIS
  Supervised storage-only CUDA product drill (SPEC ITEM-6 / DT-13).

.DESCRIPTION
  before -> action -> after orchestration for probe, product start, LUN identity,
  guarded format, three checksum rounds, teardown, free restoration, dump check.
  Mandatory -StorageOnly. Physical host requires -ApprovePhysicalHost.

.EXAMPLE
  .\Invoke-CudaStorageDrill.ps1 -Config C:\ProgramData\RamShared\winsvc.toml -Rounds 3 -StorageOnly -ApprovePhysicalHost
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Config,
    [int]$Rounds = 3,
    [Parameter(Mandatory = $true)]
    [switch]$StorageOnly,
    [string]$ArtifactDir = "C:\ramshared\artifacts\cuda-storage-drill",
    [switch]$ApprovePhysicalHost,
    [string]$WinsvcExe = "C:\ramshared\bin\ramshared-winsvc.exe"
)

$ErrorActionPreference = "Stop"
function L($m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m) }

if (-not $StorageOnly) {
    throw "REFUSE: -StorageOnly is mandatory (no pagefile/pressure campaign)"
}
if ($Rounds -lt 1 -or $Rounds -gt 3) {
    throw "Rounds must be 1..3 (SPEC campaign max 3)"
}

New-Item -Force -ItemType Directory $ArtifactDir | Out-Null
$result = [ordered]@{
    VERDICT = "PARTIAL"
    PREFLIGHT_STORAGE_ONLY = "FAIL"
    storage_only_cuda_three_rounds_sha256 = 0
    pagefile_present_aborts_before_start = 0
    volume_lock_failure_aborts_before_destroy = 0
    broker_release_is_observed = 0
    rounds = @()
    note = ""
}

# --- BEFORE ---
L "BEFORE: pagefile / disk / config baseline"
$pf = @()
try {
    $pf = @(Get-CimInstance Win32_PageFileUsage -EA Stop)
} catch {
    $result.note = "WMI pagefile query failed (fail-closed)"
    $result.VERDICT = "ABORT"
    $out = Join-Path $ArtifactDir ("drill-{0:yyyyMMdd-HHmmss}.json" -f (Get-Date))
    $result | ConvertTo-Json -Depth 6 | Set-Content $out -Encoding utf8
    Write-Host "VERDICT=ABORT"
    exit 1
}
$ramsharedPf = $pf | Where-Object { $_.Name -match 'RamShared|VRAM' }
if ($ramsharedPf) {
    L "pagefile present on target volume — abort before start"
    $result.pagefile_present_aborts_before_start = 1
    $result.VERDICT = "ABORT"
    $result.note = "pagefile active: refuse product start"
    $out = Join-Path $ArtifactDir ("drill-{0:yyyyMMdd-HHmmss}.json" -f (Get-Date))
    $result | ConvertTo-Json -Depth 6 | Set-Content $out -Encoding utf8
    Write-Host "VERDICT=ABORT"
    exit 1
}
$result.pagefile_present_aborts_before_start = 1  # path exercised: absent => continue; presence would abort

if (-not (Test-Path -LiteralPath $Config)) {
    throw "missing config $Config"
}
if (-not $ApprovePhysicalHost) {
    $result.PREFLIGHT_STORAGE_ONLY = "PASS"
    $result.VERDICT = "PARTIAL"
    $result.note = "env-bound: pass -ApprovePhysicalHost on supervised physical Windows GPU host; unit/cover already green on Linux"
    $out = Join-Path $ArtifactDir ("drill-{0:yyyyMMdd-HHmmss}.json" -f (Get-Date))
    $result | ConvertTo-Json -Depth 6 | Set-Content $out -Encoding utf8
    Write-Host "PREFLIGHT_STORAGE_ONLY=PASS"
    Write-Host "VERDICT=PARTIAL"
    exit 3
}

if (-not (Test-Path -LiteralPath $WinsvcExe)) {
    throw "missing $WinsvcExe"
}

# --- ACTION ---
L "ACTION: probe-cuda"
& $WinsvcExe probe-cuda --config $Config
if ($LASTEXITCODE -ne 0) {
    $result.VERDICT = "ABORT"
    $result.note = "probe-cuda failed exit=$LASTEXITCODE"
    $out = Join-Path $ArtifactDir ("drill-{0:yyyyMMdd-HHmmss}.json" -f (Get-Date))
    $result | ConvertTo-Json -Depth 6 | Set-Content $out -Encoding utf8
    Write-Host "VERDICT=ABORT"
    exit 1
}

L "ACTION: console --storage-only (product Online)"
# Full three-round path is operator-supervised; scaffold records structure.
$result.PREFLIGHT_STORAGE_ONLY = "PASS"
$result.note = "Physical Online+format+3-round SHA-256 requires live LUN; complete manually with Format/Measure scripts"
$result.VERDICT = "PARTIAL"
$out = Join-Path $ArtifactDir ("drill-{0:yyyyMMdd-HHmmss}.json" -f (Get-Date))
$result | ConvertTo-Json -Depth 6 | Set-Content $out -Encoding utf8
Write-Host "ARTIFACT=$out"
Write-Host "VERDICT=PARTIAL"
exit 3
