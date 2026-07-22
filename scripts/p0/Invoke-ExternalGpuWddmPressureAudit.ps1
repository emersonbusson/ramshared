#Requires -Version 5.1
<#
.SYNOPSIS
  Correlate external GPU pressure with RamShared daemon DEMOTE telemetry.

.DESCRIPTION
  This audit is app-agnostic. It can inspect an existing GPU workload gate
  directory, optionally launch Invoke-GpuWorkloadGate.ps1, and always requires a
  daemon JSONL event stream diagnosed by `ramshared diagnose --events --json` for
  PASS. Pressure without DEMOTE telemetry is PARTIAL.
#>
[CmdletBinding()]
param(
    [string]$GpuGateDir = "",
    [string]$EventsPath = "",
    [string]$DiagnoseJsonPath = "",
    [string]$RamsharedExe = "",
    [switch]$RunGpuGate,
    [string]$WorkloadCommand = "",
    [int]$MinDeltaMib = 256,
    [string]$OutDir = "ramshared-external-gpu-wddm-pressure-audit-$(Get-Date -Format yyyyMMdd-HHmmss)"
)

$ErrorActionPreference = "Stop"

function L([string]$Message) {
    Write-Host "[external-gpu-wddm-pressure-audit] $Message"
}

function Resolve-RamsharedExe {
    param([string]$Candidate)
    if (-not [string]::IsNullOrWhiteSpace($Candidate)) { return $Candidate }
    $repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
    foreach ($path in @(
        (Join-Path $repoRoot "target\release\ramshared.exe"),
        (Join-Path $repoRoot "target\debug\ramshared.exe"),
        (Join-Path $repoRoot "target\release\ramshared"),
        (Join-Path $repoRoot "target\debug\ramshared")
    )) {
        if (Test-Path -LiteralPath $path) { return $path }
    }
    return "ramshared"
}

function Resolve-InputPath {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) { return "" }
    if (Test-Path -LiteralPath $Path) {
        return (Resolve-Path -LiteralPath $Path).Path
    }
    return $Path
}

New-Item -Force -ItemType Directory -Path $OutDir | Out-Null
$GpuGateDir = Resolve-InputPath -Path $GpuGateDir
$EventsPath = Resolve-InputPath -Path $EventsPath
$DiagnoseJsonPath = Resolve-InputPath -Path $DiagnoseJsonPath
$context = [ordered]@{
    tool = "Invoke-ExternalGpuWddmPressureAudit.ps1"
    run_gpu_gate = [bool]$RunGpuGate
    process_attribution = $false
    gpu_gate_dir = $GpuGateDir
    events_path = $EventsPath
    diagnose_json_path = $DiagnoseJsonPath
    min_delta_mib = $MinDeltaMib
    note = "Aggregate external GPU pressure only; no process attribution is claimed."
}
$context | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $OutDir "audit-plan.json")

if ($RunGpuGate) {
    if ([string]::IsNullOrWhiteSpace($WorkloadCommand)) {
        throw "-RunGpuGate requires -WorkloadCommand"
    }
    $GpuGateDir = Join-Path $OutDir "gpu-gate"
    L "RUN delegated GPU workload gate"
    & (Join-Path $PSScriptRoot "Invoke-GpuWorkloadGate.ps1") `
        -WorkloadCommand $WorkloadCommand `
        -MinDeltaMib $MinDeltaMib `
        -OutDir $GpuGateDir
    if ($LASTEXITCODE -ne 0) {
        L "delegated GPU workload gate did not pass"
    }
}

$gateOk = $false
$gatePath = ""
if (-not [string]::IsNullOrWhiteSpace($GpuGateDir)) {
    $gatePath = Join-Path $GpuGateDir "gate.json"
    if (Test-Path -LiteralPath $gatePath) {
        $gate = Get-Content -LiteralPath $gatePath -Raw | ConvertFrom-Json
        $gateOk = [bool]$gate.ok
        Copy-Item -LiteralPath $gatePath -Destination (Join-Path $OutDir "gate.json") -Force
    }
}

$diagnoseOk = $false
$demotes = 0
$diagnosePath = Join-Path $OutDir "diagnose.json"
if (-not [string]::IsNullOrWhiteSpace($DiagnoseJsonPath) -and (Test-Path -LiteralPath $DiagnoseJsonPath)) {
    Copy-Item -LiteralPath $DiagnoseJsonPath -Destination $diagnosePath -Force
    $diag = Get-Content -LiteralPath $diagnosePath -Raw | ConvertFrom-Json
    $demotes = [int]$diag.demotes
    $diagnoseOk = $true
} elseif (-not [string]::IsNullOrWhiteSpace($EventsPath) -and (Test-Path -LiteralPath $EventsPath)) {
    $ramshared = Resolve-RamsharedExe -Candidate $RamsharedExe
    $diagText = & $ramshared diagnose --events $EventsPath --json 2>&1 | ForEach-Object { $_.ToString() }
    $diagExit = $LASTEXITCODE
    ($diagText -join "`n") | Set-Content -Encoding utf8 $diagnosePath
    if ($diagExit -eq 0) {
        $diag = ($diagText -join "`n") | ConvertFrom-Json
        $demotes = [int]$diag.demotes
        $diagnoseOk = $true
    }
}

$pass = $gateOk -and $diagnoseOk -and ($demotes -gt 0)
$status = if ($pass) { "PASS" } else { "PARTIAL" }
$summary = [ordered]@{
    STATUS = $status
    PASS = [bool]$pass
    ARTIFACT = $OutDir
    GPU_GATE_OK = [bool]$gateOk
    DIAGNOSE_OK = [bool]$diagnoseOk
    DEMOTES = $demotes
    PROCESS_ATTRIBUTION = $false
    GATE_PATH = $gatePath
    EVENTS_PATH = $EventsPath
    DIAGNOSE_JSON_PATH = $DiagnoseJsonPath
}
$summary | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $OutDir "audit-summary.json")
L ("SUMMARY " + ($summary | ConvertTo-Json -Compress))
if ($pass) { exit 0 }
exit 2
