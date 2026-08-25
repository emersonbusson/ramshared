#Requires -Version 5.1
<#
.SYNOPSIS
  Show the last RamShared guest observation without entering the guest.

.DESCRIPTION
  Reads only the host-visible heartbeat written by the RamShared monitor. The
  default is a plan preview. Use -Run to read the file and -Json for machine
  output. This viewer starts no process and performs no lifecycle operation.
#>
[CmdletBinding()]
param(
    [switch]$Run,
    [switch]$Json,
    [string]$HeartbeatPath = "C:\wsl-forensics\ramshared-heartbeat.json",
    [ValidateRange(3, 300)][int]$StaleAfterSec = 5
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-ObservationProperty {
    param(
        [Parameter(Mandatory = $true)]$Object,
        [Parameter(Mandatory = $true)][string]$Name,
        $Default = $null
    )
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $Default
    }
    return $property.Value
}

if (-not $Run) {
    [ordered]@{
        state = "PLAN"
        heartbeat = $HeartbeatPath
        stale_after_seconds = $StaleAfterSec
        action = "read_host_heartbeat_only"
    } | ConvertTo-Json -Depth 3
    exit 0
}

$heartbeat = Get-Item -LiteralPath $HeartbeatPath -ErrorAction Stop
$record = Get-Content -Raw -LiteralPath $HeartbeatPath | ConvertFrom-Json
$schemaVersion = Get-ObservationProperty -Object $record -Name "schema_version" -Default 0
if ([int]$schemaVersion -ne 3) {
    throw "Unsupported RamShared heartbeat schema: $schemaVersion"
}

$fileAgeMs = [Math]::Max(0, ([DateTime]::UtcNow - $heartbeat.LastWriteTimeUtc).TotalMilliseconds)
$reportedAgeMs = [double](Get-ObservationProperty -Object $record -Name "sample_age_ms" -Default 0)
$sampleAgeMs = [Math]::Round($fileAgeMs + $reportedAgeMs)
$isStale = $sampleAgeMs -gt ($StaleAfterSec * 1000)

$memory = Get-ObservationProperty -Object $record -Name "mem" -Default ([pscustomobject]@{})
$pressure = Get-ObservationProperty -Object $record -Name "control_plane" -Default ([pscustomobject]@{})
$gpu = Get-ObservationProperty -Object $record -Name "gpu"
$activation = Get-ObservationProperty -Object $record -Name "activation" -Default ([pscustomobject]@{})
$tiers = Get-ObservationProperty -Object $record -Name "tiers" -Default ([pscustomobject]@{})
$vramTier = Get-ObservationProperty -Object $tiers -Name "vram" -Default ([pscustomobject]@{})
$diskTier = Get-ObservationProperty -Object $tiers -Name "disk" -Default ([pscustomobject]@{})

$view = [ordered]@{
    state = if ($isStale) { "STALE" } else { "LIVE" }
    sample_age_ms = [int64]$sampleAgeMs
    protection_state = Get-ObservationProperty -Object $record -Name "protection_state" -Default "UNKNOWN"
    protection_reason = Get-ObservationProperty -Object $record -Name "protection_reason" -Default "unknown"
    phase = Get-ObservationProperty -Object $record -Name "phase" -Default "Unknown"
    product_active = [bool](Get-ObservationProperty -Object $activation -Name "active" -Default $false)
    binary_version = Get-ObservationProperty -Object $activation -Name "binary_version" -Default "unknown"
    ram_available_mib = [Math]::Round(([double](Get-ObservationProperty -Object $memory -Name "available_kib" -Default 0)) / 1024, 1)
    swap_free_mib = [Math]::Round(([double](Get-ObservationProperty -Object $memory -Name "swap_free_kib" -Default 0)) / 1024, 1)
    memory_psi_full_avg10 = [double](Get-ObservationProperty -Object $pressure -Name "memory_psi_full_avg10" -Default 0)
    gpu_used_mib = if ($null -eq $gpu) { $null } else { Get-ObservationProperty -Object $gpu -Name "used_mib" }
    gpu_free_mib = if ($null -eq $gpu) { $null } else { Get-ObservationProperty -Object $gpu -Name "free_mib" }
    guaranteed_vram_kib = Get-ObservationProperty -Object (Get-ObservationProperty -Object $record -Name "capacity" -Default ([pscustomobject]@{})) -Name "guaranteed_kib"
    vram_swap_used_kib = Get-ObservationProperty -Object $vramTier -Name "used_kib" -Default 0
    disk_swap_used_kib = Get-ObservationProperty -Object $diskTier -Name "used_kib" -Default 0
    disk_baseline_kib = Get-ObservationProperty -Object $activation -Name "disk_baseline_kib"
    disk_growth_kib = Get-ObservationProperty -Object $activation -Name "disk_growth_kib"
    heartbeat = $HeartbeatPath
}

if ($Json) {
    $view | ConvertTo-Json -Depth 4
} else {
    [pscustomobject]$view
}
