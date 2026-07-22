#Requires -Version 5.1
<#
.SYNOPSIS
  App-agnostic GPU workload gate for VRAM reclaim evidence.

.DESCRIPTION
  Runs three aggregate measurement windows around a user-selected GPU workload:
  idle baseline, loaded workload, and recovery. The workload may be launched by
  this script with -WorkloadCommand or started externally with -AttachOnly.

  The gate is deliberately process-name agnostic. It compares aggregate NVIDIA
  VRAM pressure only and does not claim which application caused pressure.
#>
[CmdletBinding(DefaultParameterSetName = "Attach")]
param(
    [Parameter(ParameterSetName = "Launch", Mandatory = $true)]
    [string]$WorkloadCommand,

    [Parameter(ParameterSetName = "Attach")]
    [switch]$AttachOnly,

    [int]$IdleDurationSec = 15,
    [int]$LoadedDurationSec = 60,
    [int]$RecoveryDurationSec = 15,
    [int]$IntervalMs = 500,
    [int]$GpuIndex = 0,
    [int]$MinDeltaMib = 256,
    [string]$WorkloadLabel = "external-gpu-workload",
    [string]$OutDir = "ramshared-gpu-workload-gate-$(Get-Date -Format yyyyMMdd-HHmmss)"
)

$ErrorActionPreference = "Stop"

function Log([string]$Message) {
    Write-Host "[gpu-workload-gate] $Message"
}

function Require-Positive([string]$Name, [int]$Value) {
    if ($Value -lt 1) {
        throw "$Name must be >= 1."
    }
}

function Run-Sampler([string]$Tag, [int]$DurationSec, [string]$Dir) {
    $script = Join-Path $PSScriptRoot "measure-gpu-workload-vram.ps1"
    & $script `
        -Runs 1 `
        -DurationSec $DurationSec `
        -IntervalMs $IntervalMs `
        -Tag $Tag `
        -GpuIndex $GpuIndex `
        -WorkloadLabel $WorkloadLabel `
        -OutDir $Dir
}

function Read-Peak([string]$Dir) {
    $results = Join-Path $Dir "results.jsonl"
    if (-not (Test-Path $results)) {
        throw "missing sampler results: $results"
    }
    $line = Get-Content -Path $results | Where-Object { $_.Trim().Length -gt 0 } | Select-Object -Last 1
    if (-not $line) {
        throw "empty sampler results: $results"
    }
    return [int](($line | ConvertFrom-Json).peak_vram_used_mib)
}

Require-Positive "IdleDurationSec" $IdleDurationSec
Require-Positive "LoadedDurationSec" $LoadedDurationSec
Require-Positive "RecoveryDurationSec" $RecoveryDurationSec
Require-Positive "IntervalMs" $IntervalMs
Require-Positive "MinDeltaMib" $MinDeltaMib

if ($PSCmdlet.ParameterSetName -eq "Attach" -and -not $AttachOnly) {
    throw "Use -AttachOnly when the workload is started externally, or pass -WorkloadCommand."
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$context = [ordered]@{
    tool = "Invoke-GpuWorkloadGate.ps1"
    workload_label = $WorkloadLabel
    workload_mode = $PSCmdlet.ParameterSetName.ToLowerInvariant()
    process_attribution = $false
    gpu_index = $GpuIndex
    min_delta_mib = $MinDeltaMib
    idle_duration_sec = $IdleDurationSec
    loaded_duration_sec = $LoadedDurationSec
    recovery_duration_sec = $RecoveryDurationSec
    interval_ms = $IntervalMs
}
($context | ConvertTo-Json) | Out-File -Encoding utf8 (Join-Path $OutDir "gate-context.json")

Log "idle baseline"
$idleDir = Join-Path $OutDir "idle"
Run-Sampler -Tag "idle" -DurationSec $IdleDurationSec -Dir $idleDir
$idlePeak = Read-Peak $idleDir

$proc = $null
try {
    if ($PSCmdlet.ParameterSetName -eq "Launch") {
        Log "launching workload command"
        $proc = Start-Process -FilePath "powershell.exe" `
            -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", $WorkloadCommand `
            -PassThru
        Start-Sleep -Seconds 2
    } else {
        Log "attach-only mode; start or focus the workload externally now"
    }

    $loadedDir = Join-Path $OutDir "loaded"
    Run-Sampler -Tag "loaded" -DurationSec $LoadedDurationSec -Dir $loadedDir
    $loadedPeak = Read-Peak $loadedDir
} finally {
    if ($proc -ne $null -and -not $proc.HasExited) {
        Log "stopping launched workload command"
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    }
}

Log "recovery window"
$recoveryDir = Join-Path $OutDir "recovery"
Run-Sampler -Tag "idle" -DurationSec $RecoveryDurationSec -Dir $recoveryDir
$recoveryPeak = Read-Peak $recoveryDir

$deltaLoaded = $loadedPeak - $idlePeak
$deltaRecovery = $recoveryPeak - $idlePeak
$pressureObserved = $deltaLoaded -ge $MinDeltaMib
$recoveredNearIdle = $deltaRecovery -lt $MinDeltaMib

$gate = [ordered]@{
    gate = "gpu_workload_vram_pressure"
    ok = ($pressureObserved -and $recoveredNearIdle)
    pressure_observed = $pressureObserved
    recovered_near_idle = $recoveredNearIdle
    process_attribution = $false
    idle_peak_mib = $idlePeak
    loaded_peak_mib = $loadedPeak
    recovery_peak_mib = $recoveryPeak
    loaded_delta_mib = $deltaLoaded
    recovery_delta_mib = $deltaRecovery
    min_delta_mib = $MinDeltaMib
    note = "Aggregate VRAM pressure only; no process attribution is claimed."
}

($gate | ConvertTo-Json) | Out-File -Encoding utf8 (Join-Path $OutDir "gate.json")
if ($gate.ok) {
    Log "PASS: aggregate VRAM pressure observed and recovery returned near idle"
    exit 0
}

Log "PARTIAL: no qualifying aggregate VRAM pressure/recovery proof in this window"
$gate | ConvertTo-Json | Write-Host
exit 1
