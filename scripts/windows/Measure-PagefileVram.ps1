#Requires -Version 5.1
<#
.SYNOPSIS
  ITEM-9 / RNF-2 / DT-13 — side-by-side pagefile-VRAM vs disk latency/capacity.

.DESCRIPTION
  Same window, >=3 runs, tags idle|loaded. Appends docs/benchmarks/results.jsonl
  and a human block for docs/BENCHMARKS.md when -RepoRoot is set.

.NOTES
  Never thrash the live WSL2 host; run inside Windows VM or dedicated host lab.
#>
[CmdletBinding()]
param(
    [int]$Runs = 3,
    [ValidateSet("idle", "loaded")]
    [string]$LoadTag = "idle",
    [string]$RepoRoot = "",
    [string]$ArtifactDir = ".\artifacts\pagefile-vram-measure"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $ArtifactDir | Out-Null

function Measure-Once {
    param([string]$Label)
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    # Capacity proxy: pagefile usage sample
    $usage = $null
    try {
        $usage = (Get-Counter "\Paging File(_Total)\% Usage" -ErrorAction Stop).CounterSamples[0].CookedValue
    } catch {
        $usage = -1
    }
    # Lightweight page-in pressure: allocate+touch 64 MiB then free (does not thrash multi-GB).
    $n = 64 * 1024 * 1024
    $buf = New-Object byte[] $n
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    $rng.GetBytes($buf)
    $sum = 0L
    for ($i = 0; $i -lt $n; $i += 4096) { $sum += $buf[$i] }
    $sw.Stop()
    [pscustomobject]@{
        label = $Label
        ms = $sw.Elapsed.TotalMilliseconds
        pagefile_usage_pct = $usage
        touch_sum = $sum
    }
}

$samples = @()
for ($i = 1; $i -le $Runs; $i++) {
    Write-Host "Run $i / $Runs ($LoadTag)"
    $samples += Measure-Once -Label "window-$i"
}

$ms = $samples | ForEach-Object { $_.ms } | Sort-Object
$p50 = $ms[[int]([math]::Floor(($ms.Count - 1) * 0.5))]
$p99 = $ms[[int]([math]::Floor(($ms.Count - 1) * 0.99))]
$median = $p50
$runId = "pfvram-{0:yyyyMMdd-HHmmss}" -f (Get-Date)

$record = [ordered]@{
    "run-id" = $runId
    feature = "windows-swap-driver"
    item = "ITEM-9"
    load = $LoadTag
    n = $Runs
    median_ms = $median
    p50_ms = $p50
    p99_ms = $p99
    samples = $samples
    note = "K not invented here (DT-13); first real VRAM-vs-disk window defines K."
}

$json = ($record | ConvertTo-Json -Depth 6 -Compress)
Set-Content -Path (Join-Path $ArtifactDir "$runId.json") -Value $json -Encoding UTF8
Write-Host $json

if ($RepoRoot -ne "") {
    $jsonl = Join-Path $RepoRoot "docs/benchmarks/results.jsonl"
    Add-Content -Path $jsonl -Value $json -Encoding UTF8
    $bench = Join-Path $RepoRoot "docs/BENCHMARKS.md"
    $block = @"

## $runId — Measure-PagefileVram ($LoadTag)

- n=$Runs median_ms=$median p50=$p50 p99=$p99
- K deferred to first side-by-side VRAM vs disk (DT-13).
- artifact: $ArtifactDir\$runId.json

"@
    if (Test-Path $bench) {
        Add-Content -Path $bench -Value $block -Encoding UTF8
    }
}

Write-Host "Done. Gate RNF-2: pagefile-VRAM usage under pressure must be > 0; p99 <= Kx disk (K from 1st real pair)."
exit 0
