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
    '"$bin" stress --full-three-tier',
    'Start-Process -FilePath "wsl.exe"'
)) {
    if (-not $text.Contains($needle)) {
        throw "missing three-tier safety token: $needle"
    }
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
