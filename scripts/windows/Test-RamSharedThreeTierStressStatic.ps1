#Requires -Version 5.1
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$scriptPath = Join-Path $root "scripts\windows\Invoke-RamSharedThreeTierStress.ps1"
$guestGatePath = Join-Path $root "scripts\safety\ramshared-guest-memory-admission.sh"
$text = Get-Content -LiteralPath $scriptPath -Raw
$guestGate = Get-Content -LiteralPath $guestGatePath -Raw

$tokens = $null
$parseErrors = $null
[Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref]$tokens, [ref]$parseErrors) | Out-Null
if ($parseErrors.Count -ne 0) {
    throw "three-tier stress supervisor has PowerShell parser errors"
}

$stressAst = [Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref]$tokens, [ref]$parseErrors)
$resultFunction = $stressAst.Find({
    param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -ceq "Test-ThreeTierStressPhysicalCacheResult"
}, $true)
if ($null -eq $resultFunction) {
    throw "three-tier supervisor must validate worker-reported dynamic physical cache targets"
}
. ([scriptblock]::Create($resultFunction.Extent.Text))

$validDynamicCache = [pscustomobject]@{
    status = "PASS_ZERO_PANIC"
    metric_version = 2
    tier1_target_pct = 100
    tier2_target_pct = 100
    tier3_target_pct = 99
    physical_cache_samples = 12
    simultaneous_full_tiers = $true
    physical_cache_required_mib = 2560
    tier2_physical_cache_target_mb = 3072
    tier2_vram_mb = 3072
    simultaneous_physical_cache_target_mib = 2816
    simultaneous_physical_cache_mib = 2880
}
if (-not (Test-ThreeTierStressPhysicalCacheResult -Stress $validDynamicCache -PhysicalCacheCapMiB 4096)) {
    throw "a worker-qualified safe target below the 4096 MiB cap must pass"
}
Write-Host "PASS dynamic_worker_physical_cache_target_below_cap"

$lowerTierReport = $validDynamicCache.PSObject.Copy()
$lowerTierReport.tier1_target_pct = 95
if (Test-ThreeTierStressPhysicalCacheResult -Stress $lowerTierReport -PhysicalCacheCapMiB 4096) {
    throw "lower-than-profile tier targets must not qualify the full campaign"
}
Write-Host "PASS non_full_tier_target_report_refused"

$overCapCache = [pscustomobject]@{
    status = "PASS_ZERO_PANIC"
    metric_version = 2
    tier1_target_pct = 100
    tier2_target_pct = 100
    tier3_target_pct = 99
    physical_cache_samples = 12
    simultaneous_full_tiers = $true
    physical_cache_required_mib = 4097
    tier2_physical_cache_target_mb = 4097
    tier2_vram_mb = 4097
    simultaneous_physical_cache_target_mib = 4097
    simultaneous_physical_cache_mib = 4097
}
if (Test-ThreeTierStressPhysicalCacheResult -Stress $overCapCache -PhysicalCacheCapMiB 4096) {
    throw "worker target above the sealed physical cache cap must refuse"
}
Write-Host "PASS dynamic_worker_target_above_cap_refused"

$incoherentPeakCache = [pscustomobject]@{
    status = "PASS_ZERO_PANIC"
    metric_version = 2
    tier1_target_pct = 100
    tier2_target_pct = 100
    tier3_target_pct = 99
    physical_cache_samples = 12
    simultaneous_full_tiers = $true
    physical_cache_required_mib = 2560
    tier2_physical_cache_target_mb = 4096
    tier2_vram_mb = 4096
    simultaneous_physical_cache_target_mib = 3072
    simultaneous_physical_cache_mib = 2560
}
if (Test-ThreeTierStressPhysicalCacheResult -Stress $incoherentPeakCache -PhysicalCacheCapMiB 4096) {
    throw "separate peak cache values must not substitute for a short simultaneous cache sample"
}
Write-Host "PASS simultaneous_cache_shortfall_refused_despite_peaks"

$missingSimultaneousCache = [pscustomobject]@{
    status = "PASS_ZERO_PANIC"
    metric_version = 2
    tier1_target_pct = 100
    tier2_target_pct = 100
    tier3_target_pct = 99
    physical_cache_samples = 12
    simultaneous_full_tiers = $true
    physical_cache_required_mib = 2560
    tier2_physical_cache_target_mb = 2560
    tier2_vram_mb = 2560
    simultaneous_physical_cache_target_mib = $null
    simultaneous_physical_cache_mib = $null
}
if (Test-ThreeTierStressPhysicalCacheResult -Stress $missingSimultaneousCache -PhysicalCacheCapMiB 4096) {
    throw "a full-tier result without a paired physical-cache sample must refuse"
}
Write-Host "PASS missing_simultaneous_cache_sample_refused"

$malformedDynamicCache = [pscustomobject]@{
    status = "PASS_ZERO_PANIC"
    metric_version = 2
    tier1_target_pct = 100
    tier2_target_pct = 100
    tier3_target_pct = 99
    physical_cache_samples = 12
    simultaneous_full_tiers = $true
    physical_cache_required_mib = 2560.5
    tier2_physical_cache_target_mb = 3072
    tier2_vram_mb = 3072
    simultaneous_physical_cache_target_mib = 2816
    simultaneous_physical_cache_mib = 2880
}
if (Test-ThreeTierStressPhysicalCacheResult -Stress $malformedDynamicCache -PhysicalCacheCapMiB 4096) {
    throw "fractional cache telemetry must refuse"
}
Write-Host "PASS malformed_dynamic_worker_target_refused"

$belowTargetCache = [pscustomobject]@{
    status = "PASS_ZERO_PANIC"
    metric_version = 2
    tier1_target_pct = 100
    tier2_target_pct = 100
    tier3_target_pct = 99
    physical_cache_samples = 12
    simultaneous_full_tiers = $true
    physical_cache_required_mib = 2560
    tier2_physical_cache_target_mb = 3072
    tier2_vram_mb = 3072
    simultaneous_physical_cache_target_mib = 2816
    simultaneous_physical_cache_mib = 2815
}
if (Test-ThreeTierStressPhysicalCacheResult -Stress $belowTargetCache -PhysicalCacheCapMiB 4096) {
    throw "simultaneous physical bytes below the admitted target must refuse"
}
Write-Host "PASS simultaneous_cache_below_worker_target_refused"

$shrunkWorkerTarget = [pscustomobject]@{
    status = "PASS_ZERO_PANIC"
    metric_version = 2
    tier1_target_pct = 100
    tier2_target_pct = 100
    tier3_target_pct = 99
    physical_cache_samples = 12
    simultaneous_full_tiers = $true
    physical_cache_required_mib = 3072
    tier2_physical_cache_target_mb = 4096
    tier2_vram_mb = 4096
    simultaneous_physical_cache_target_mib = 2560
    simultaneous_physical_cache_mib = 3072
}
if (Test-ThreeTierStressPhysicalCacheResult -Stress $shrunkWorkerTarget -PhysicalCacheCapMiB 4096) {
    throw "a worker target below the startup-admitted requirement must refuse"
}
Write-Host "PASS worker_target_drop_below_startup_target_refused"

foreach ($needle in @(
    "HostCommitReserveMiB",
    "HostPhysicalReserveMiB",
    "Get-SharedWslHostRequiredHeadroomMiB",
    "RequiredCommitMiB `$requiredCommitMiB",
    "RequiredPhysicalMiB `$requiredPhysicalMiB",
    "host_physical_required_mib",
    "host_physical_headroom_mib",
    "guestMemAvailableReserveMiB = 1024",
    "guestSwapFreeReserveMiB = 1024",
    "guest-memory-admission.sh",
    "guest-memory-admission.json",
    "host_memory_gate_reason",
    "physical_cache_cap_mib",
    "physical_cache_target_policy",
    "metric_version",
    "tier1_target_pct",
    "tier2_target_pct",
    "tier3_target_pct",
    "physical_cache_samples",
    "physical_cache_required_mib",
    "tier2_physical_cache_target_mb",
    "simultaneous_physical_cache_target_mib",
    "simultaneous_physical_cache_mib",
    "Test-ThreeTierStressPhysicalCacheResult",
    '"$bin" stress --full-three-tier',
    'Start-Process -FilePath "wsl.exe"'
)) {
    if (-not $text.Contains($needle)) {
        throw "missing three-tier safety token: $needle"
    }
}

if ($text.Contains("physical_cache_target_mib = 4096") -or
    $text.Contains("tier2_vram_mb -ge 4096")) {
    throw "three-tier stress must use the active worker target beneath the sealed cache cap"
}

$planGuard = $text.IndexOf('if (-not $Run) { $plan | ConvertTo-Json -Depth 6; return }')
$artifactCreate = $text.IndexOf('New-Item -ItemType Directory -Force -Path $dir')
$wslLaunch = $text.IndexOf('Start-Process -FilePath "wsl.exe"')
$hostAdmission = $text.IndexOf('Test-SharedWslHostMemoryAdmission')
if ($planGuard -lt 0 -or $artifactCreate -lt 0 -or $wslLaunch -lt 0 -or $hostAdmission -lt 0 -or
    $hostAdmission -gt $planGuard -or $planGuard -gt $artifactCreate -or $artifactCreate -gt $wslLaunch) {
    throw "plan-only mode or host admission can reach WSL pressure setup"
}

$guestScriptMatch = [regex]::Match($text, '(?s)\$guestScript\s*=\s*@''\r?\n(.*?)\r?\n''@')
if (-not $guestScriptMatch.Success) {
    throw "three-tier guest script here-string is missing"
}
$guestBody = $guestScriptMatch.Groups[1].Value
$guestAdmission = $guestBody.IndexOf('bash "$artifact/guest-memory-admission.sh" /proc/meminfo')
$ramsharedUp = $guestBody.IndexOf('"$bin" up --vram')
$guestStress = $guestBody.IndexOf('"$bin" stress --full-three-tier')
if ($guestAdmission -lt 0 -or $ramsharedUp -lt 0 -or $guestStress -lt 0 -or
    $guestAdmission -gt $ramsharedUp -or $guestAdmission -gt $guestStress) {
    throw "guest memory refusal must occur before RamShared activation and stress"
}
if (-not $guestBody.Contains('"$bin" up --vram "$physical_cache_cap_mib"') -or
    -not $text.Contains('[string]$physicalCacheCapMiB')) {
    throw "worker cache request must use the sealed manifest cap as its maximum"
}

$launchTargets = [regex]::Matches($text, 'Start-Process\s+-FilePath\s+"([^"]+)"')
if ($launchTargets.Count -lt 2) {
    throw "three-tier supervisor must launch the guest and have a targeted containment path"
}
foreach ($launchTarget in $launchTargets) {
    if ($launchTarget.Groups[1].Value -cne "wsl.exe") {
        throw "three-tier supervisor may only launch WSL processes: $($launchTarget.Groups[1].Value)"
    }
}

if (-not $guestGate.Contains('read_meminfo_kib MemAvailable') -or
    -not $guestGate.Contains('read_meminfo_kib SwapFree') -or
    -not $guestGate.Contains('guest_mem_available_below_reserve') -or
    -not $guestGate.Contains('guest_swap_free_below_reserve')) {
    throw "guest admission must enforce both MemAvailable and SwapFree"
}

foreach ($forbidden in @(
    "Start-CudaVramWorkload.ps1",
    "external-workload.ps1",
    "VirtualAlloc",
    "New-ProcessMemory"
)) {
    if ($text.Contains($forbidden)) {
        throw "three-tier WSL stress wrapper must not allocate pressure in Windows: $forbidden"
    }
}

Write-Host "RAMSHARED_THREE_TIER_STRESS_STATIC=PASS"
