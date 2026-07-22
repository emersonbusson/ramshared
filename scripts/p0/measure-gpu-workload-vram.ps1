#Requires -Version 5.1
<#
.SYNOPSIS
  Generic GPU workload VRAM/RAM sampler for Windows.

.DESCRIPTION
  Samples NVIDIA VRAM usage and system free RAM while any user-selected GPU
  workload runs. The workload is intentionally external to this script: a game,
  DCC tool, video editor, browser workload, or synthetic test can be measured
  without encoding an application name into RamShared.

  This script does not launch, modify, terminate, or identify the workload. It
  records aggregate host/GPU pressure only.
#>
[CmdletBinding()]
param(
    [int]$Runs = 3,
    [int]$DurationSec = 600,
    [int]$IntervalMs = 500,
    [ValidateSet("idle", "loaded")][string]$Tag = "loaded",
    [int]$GpuIndex = 0,
    [string]$WorkloadLabel = "external-gpu-workload",
    [string]$OutDir = "ramshared-gpu-workload-$(Get-Date -Format yyyyMMdd-HHmmss)"
)

$ErrorActionPreference = "Stop"

function Log([string]$Message) {
    Write-Host "[gpu-workload] $Message"
}

function Percentile($Values, [int]$Percentile) {
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) { return $null }
    $rank = [int]([math]::Ceiling($Percentile / 100.0 * $sorted.Count)) - 1
    if ($rank -lt 0) { $rank = 0 }
    if ($rank -ge $sorted.Count) { $rank = $sorted.Count - 1 }
    return $sorted[$rank]
}

function Stddev($Values) {
    $items = @($Values)
    if ($items.Count -lt 2) { return 0 }
    $mean = ($items | Measure-Object -Average).Average
    $variance = ($items | ForEach-Object { ($_ - $mean) * ($_ - $mean) } |
        Measure-Object -Sum).Sum / ($items.Count - 1)
    return [math]::Round([math]::Sqrt($variance), 1)
}

function Sample-VramUsed([int]$Index) {
    $line = & nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits -i $Index 2>$null
    if ($line) { return [int]($line.Trim()) }
    return $null
}

function Sample-RamFreeMib {
    return [int]((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1024)
}

if (-not (Get-Command nvidia-smi -ErrorAction SilentlyContinue)) {
    throw "nvidia-smi not found; install the NVIDIA driver before measuring VRAM."
}
if ($Runs -lt 1) {
    throw "Runs must be >= 1."
}
if ($DurationSec -lt 1) {
    throw "DurationSec must be >= 1."
}
if ($IntervalMs -lt 100) {
    throw "IntervalMs must be >= 100."
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$os = Get-CimInstance Win32_OperatingSystem
$ramTotalMib = [int]($os.TotalVisibleMemorySize / 1024)
$gpuCsv = (& nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv,noheader,nounits -i $GpuIndex) -split ','
$gpuName = $gpuCsv[0].Trim()
$gpuVramTotal = [int]$gpuCsv[1].Trim()
$driver = $gpuCsv[2].Trim()

$context = [ordered]@{
    tool = "measure-gpu-workload-vram.ps1"
    mode = "external"
    tag = $Tag
    workload_label = $WorkloadLabel
    os = $os.Caption
    os_version = $os.Version
    ram_total_mib = $ramTotalMib
    gpu = $gpuName
    gpu_vram_mib = $gpuVramTotal
    driver = $driver
    runs = $Runs
    duration_sec = $DurationSec
    interval_ms = $IntervalMs
}
($context | ConvertTo-Json) | Out-File -Encoding utf8 (Join-Path $OutDir "context.json")

Log "context: $gpuName ($gpuVramTotal MiB) | $($os.Caption) | RAM $ramTotalMib MiB"
Log "start the workload externally; sampling aggregate pressure only"

$records = @()
for ($run = 1; $run -le $Runs; $run++) {
    $csv = Join-Path $OutDir ("run{0}.csv" -f $run)
    "ts_ms,vram_used_mib,ram_free_mib" | Out-File -Encoding ascii $csv
    $peak = 0
    $minRam = [int]::MaxValue
    $samples = 0
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    Log ("run {0}/{1}: sampling for {2}s" -f $run, $Runs, $DurationSec)

    while ($watch.Elapsed.TotalSeconds -lt $DurationSec) {
        $vram = Sample-VramUsed $GpuIndex
        $ramFree = Sample-RamFreeMib
        $ts = [int64]([datetimeoffset]::UtcNow.ToUnixTimeMilliseconds())
        "$ts,$vram,$ramFree" | Out-File -Encoding ascii -Append $csv
        if ($vram -ne $null -and $vram -gt $peak) { $peak = $vram }
        if ($ramFree -lt $minRam) { $minRam = $ramFree }
        $samples++
        Start-Sleep -Milliseconds $IntervalMs
    }

    if ($minRam -eq [int]::MaxValue) { $minRam = $null }
    $ramUsedPeak = $null
    if ($minRam -ne $null) { $ramUsedPeak = $ramTotalMib - $minRam }

    $record = [ordered]@{
        run = $run
        tag = $Tag
        workload_label = $WorkloadLabel
        peak_vram_used_mib = $peak
        vram_total_mib = $gpuVramTotal
        min_ram_free_mib = $minRam
        ram_used_peak_mib = $ramUsedPeak
        samples = $samples
        duration_s = [math]::Round($watch.Elapsed.TotalSeconds, 1)
    }
    ($record | ConvertTo-Json -Compress) | Add-Content -Encoding ascii (Join-Path $OutDir "results.jsonl")
    $records += $record
}

$peaks = @($records | ForEach-Object { $_.peak_vram_used_mib })
$median = Percentile $peaks 50
$p99 = Percentile $peaks 99
$sd = Stddev $peaks

$summary = @()
$summary += "## GPU workload VRAM/RAM - $gpuName ($gpuVramTotal MiB) - tag=$Tag - $(Get-Date -Format s)"
$summary += ""
$summary += "- **Context:** $($os.Caption) | RAM $ramTotalMib MiB | driver $driver"
$summary += "- **Workload label:** $WorkloadLabel"
$summary += "- **Peak VRAM used (MiB):** median **$median** | p99 $p99 | deviation $sd (values: $($peaks -join ', '))"
$summary += "- **Minimum free RAM (MiB):** $(($records | ForEach-Object { $_.min_ram_free_mib }) -join ', ')"
$summary += "- **Files:** ``$OutDir\{context.json, results.jsonl, run*.csv}``"
$summary | Out-File -Encoding utf8 (Join-Path $OutDir "summary.md")

Log "=== SUMMARY ==="
$summary | ForEach-Object { Write-Host $_ }
Log "DONE. Archive the folder with: Compress-Archive -Path '$OutDir\*' -DestinationPath '$OutDir.zip'"
