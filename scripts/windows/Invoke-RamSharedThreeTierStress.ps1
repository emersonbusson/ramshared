#Requires -Version 5.1
<#
.SYNOPSIS
  Supervise the exact physical-cache and three-tier stress profile from Windows.
.DESCRIPTION
  The Windows process owns the timeout and host commit guard. The guest performs
  swapoff-first cleanup. A stalled guest is contained by terminating only the
  selected WSL distribution; all evidence is retained under ArtifactRoot.
#>
[CmdletBinding()]
param(
    [switch]$Run,
    [string]$Distro = "Ubuntu-24.04",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [ValidateRange(120, 7200)][int]$TimeoutSec = 1800,
    [ValidateRange(4096, 32768)][int]$HostCommitReserveMiB = 4096,
    [ValidateRange(4096, 32768)][int]$HostPhysicalReserveMiB = 4096
)

$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "SharedWslHostMemoryGate.psm1") -Force
if ($Distro -cne "Ubuntu-24.04") { throw "sealed distro mismatch" }
if ($ArtifactRoot -notmatch '^[A-Za-z]:\\') { throw "ArtifactRoot must be an absolute Windows drive path" }

$manifestPath = "C:\ProgramData\RamShared\ramshared-origin-manifest.json"
$guardianPath = "C:\ProgramData\RamShared\guardian-state\$Distro.health.json"
$releasePath = "/opt/ramshared/current"
$requiredCommitMiB = Get-SharedWslHostRequiredHeadroomMiB -PressureAllocGiB 16 -ReserveMiB $HostCommitReserveMiB
$requiredPhysicalMiB = Get-SharedWslHostRequiredHeadroomMiB -PressureAllocGiB 16 -ReserveMiB $HostPhysicalReserveMiB
$guestMemAvailableReserveMiB = 1024
$guestSwapFreeReserveMiB = 1024
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
if ([int]$manifest.schema_version -ne 3 -or [int]$manifest.logical_capacity_mib -ne 4096 -or
    [int]$manifest.physical_cache_cap_mib -ne 4096) { throw "exact 4096 MiB sealed origin is unavailable" }
$guardian = Get-Content -Raw -LiteralPath $guardianPath | ConvertFrom-Json
if (-not (Test-SharedWslGuardianFresh -Guardian $guardian -MaximumAgeSeconds 15)) {
    throw "Windows WSL guardian is unavailable or stale"
}

$samples = @()
for ($i = 0; $i -lt 3; $i++) {
    $samples += Get-SharedWslHostMemorySample
    if ($i -lt 2) { Start-Sleep -Seconds 1 }
}
$admission = Test-SharedWslHostMemoryAdmission -Samples $samples `
    -RequiredCommitMiB $requiredCommitMiB -RequiredPhysicalMiB $requiredPhysicalMiB
$plan = [ordered]@{
    profile = "full-three-tier"
    distro = $Distro
    release = $releasePath
    physical_cache_target_mib = 4096
    zram_target_pct = 100
    nbd_logical_target_pct = 100
    ssd_target_pct = 99
    timeout_sec = $TimeoutSec
    host_commit_required_mib = $requiredCommitMiB
    host_commit_headroom_mib = $admission.commit_headroom_mib
    host_commit_reserve_mib = $HostCommitReserveMiB
    host_physical_required_mib = $requiredPhysicalMiB
    host_physical_headroom_mib = $admission.physical_headroom_mib
    host_physical_reserve_mib = $HostPhysicalReserveMiB
    guest_mem_available_reserve_mib = $guestMemAvailableReserveMiB
    guest_swap_free_reserve_mib = $guestSwapFreeReserveMiB
    host_memory_gate_ok = [bool]$admission.ok
    host_memory_gate_reason = [string]$admission.reason
    host_memory_samples = $samples
}
if (-not $Run) { $plan | ConvertTo-Json -Depth 6; return }
if (-not $admission.ok) { throw "host memory admission failed: $($admission.reason)" }

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$dir = Join-Path $ArtifactRoot "three-tier-stress-$stamp"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$plan | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $dir "admission.json") -Encoding UTF8
$guestDir = "/mnt/" + $dir.Substring(0, 1).ToLowerInvariant() + ($dir.Substring(2) -replace '\\', '/')
$scriptPath = Join-Path $dir "guest-stress.sh"
$guestAdmissionSource = Join-Path $PSScriptRoot "..\safety\ramshared-guest-memory-admission.sh"
Copy-Item -LiteralPath $guestAdmissionSource -Destination (Join-Path $dir "guest-memory-admission.sh") -ErrorAction Stop
$guestScript = @'
#!/usr/bin/env bash
set -euo pipefail
artifact=$1
guest_mem_available_reserve_mib=$2
guest_swap_free_reserve_mib=$3
release=/opt/ramshared/current
bin="$release/bin/ramshared"
daemon="$release/bin/ramsharedd"
monitor_pid=""
bash "$artifact/guest-memory-admission.sh" /proc/meminfo "$guest_mem_available_reserve_mib" "$guest_swap_free_reserve_mib" >"$artifact/guest-memory-admission.json"
cleanup() {
  rc=$?
  "$bin" down >"$artifact/down.out" 2>"$artifact/down.err" || rc=1
  systemctl stop ramshared-supervisor.service >"$artifact/supervisor-stop.out" 2>"$artifact/supervisor-stop.err" || rc=1
  if test -n "$monitor_pid"; then
    kill "$monitor_pid" 2>/dev/null || true
    wait "$monitor_pid" 2>/dev/null || true
  fi
  cat /proc/swaps >"$artifact/final-swaps.txt"
  dmesg | tail -n 300 >"$artifact/final-dmesg.txt" || true
  exit "$rc"
}
trap cleanup EXIT INT TERM
test -x "$bin" && test -x "$daemon"
"$bin" check >"$artifact/check.out" 2>"$artifact/check.err"
"$bin" monitor --jsonl --interval-ms 1000 --heartbeat /mnt/c/wsl-forensics/ramshared-heartbeat.json --output "$artifact/monitor.jsonl" >"$artifact/monitor.out" 2>"$artifact/monitor.err" &
monitor_pid=$!
cat /proc/swaps >"$artifact/before-swaps.txt"
"$bin" up --vram 4096 --zram 1024 --daemon "$daemon" >"$artifact/up.out" 2>"$artifact/up.err"
systemctl start ramshared-supervisor.service
ready=0
for _ in $(seq 1 30); do
  "$bin" status --json >"$artifact/armed-status.json"
  if python3 - "$artifact/armed-status.json" <<'PY'
import json, sys
s = json.load(open(sys.argv[1], encoding='utf-8'))
sys.exit(0 if s.get('ok') and s.get('control_state') == 'HEALTHY' and
         s.get('cache_state') == 'ACTIVE' and s.get('origin_state') == 'READY' and
         not s.get('measurement_errors') else 1)
PY
  then ready=1; break; fi
  sleep 1
done
test "$ready" = 1
cat /proc/swaps >"$artifact/armed-swaps.txt"
pid=$(pgrep -n -x ramsharedd)
test "$(sha256sum "/proc/$pid/exe" | cut -d ' ' -f 1)" = "$(sha256sum "$daemon" | cut -d ' ' -f 1)"
printf 'BINARY_MATCH=true\n' >"$artifact/binary-match.txt"
"$bin" stress --full-three-tier --step 5 --interval-ms 500 --hold-sec 10 --json --log "$artifact/telemetry.jsonl" >"$artifact/stress.json" 2>"$artifact/stress.err"
'@
[IO.File]::WriteAllText($scriptPath, ($guestScript -replace "`r`n", "`n"), [Text.Encoding]::ASCII)

$stdout = Join-Path $dir "wsl.out"
$stderr = Join-Path $dir "wsl.err"
$proc = $null
$reason = $null
$invalidSamples = 0
$start = [Diagnostics.Stopwatch]::StartNew()
$memoryLog = Join-Path $dir "host-memory.jsonl"
try {
    $guestScriptPath = "$guestDir/guest-stress.sh"
    $proc = Start-Process -FilePath "wsl.exe" -ArgumentList @(
        "-d", $Distro, "-u", "root", "--", "bash", $guestScriptPath, $guestDir,
        [string]$guestMemAvailableReserveMiB, [string]$guestSwapFreeReserveMiB
    ) `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru -WindowStyle Hidden
    while ($true) {
        $proc.Refresh()
        if ($proc.HasExited) { break }
        $sample = Get-SharedWslHostMemorySample
        ($sample | ConvertTo-Json -Compress) | Add-Content -LiteralPath $memoryLog -Encoding UTF8
        $guard = Test-SharedWslHostMemoryGuardian -Sample $sample `
            -HostCommitReserveMiB $HostCommitReserveMiB -HostPhysicalReserveMiB $HostPhysicalReserveMiB `
            -InvalidSampleCount $invalidSamples
        $invalidSamples = $guard.invalid_sample_count
        if ($guard.trip) { $reason = $guard.reason; break }
        $guardian = Get-Content -Raw -LiteralPath $guardianPath | ConvertFrom-Json
        if (-not (Test-SharedWslGuardianFresh -Guardian $guardian -MaximumAgeSeconds 15)) {
            $reason = "windows_guardian_unhealthy"; break
        }
        if ($start.Elapsed.TotalSeconds -ge $TimeoutSec) { $reason = "outer_timeout"; break }
        Start-Sleep -Seconds 1
    }
} catch {
    $reason = "controller_error: $($_.Exception.Message)"
} finally {
    if ($null -ne $proc) {
        $proc.Refresh()
        if (-not $proc.HasExited) {
            $termination = Start-Process -FilePath "wsl.exe" -ArgumentList @("--terminate", $Distro) -Wait -PassThru -WindowStyle Hidden
            if ($termination.ExitCode -ne 0) { $reason = "targeted_termination_failed" }
            if (-not $proc.WaitForExit(30000)) { $reason = "launcher_containment_unproven" }
        }
    }
}
$exitCode = if ($null -ne $proc -and $proc.HasExited) { [int]$proc.ExitCode } else { $null }
$stressPath = Join-Path $dir "stress.json"
$stress = $null
if (Test-Path -LiteralPath $stressPath) {
    try { $stress = Get-Content -Raw -LiteralPath $stressPath | ConvertFrom-Json } catch {}
}
$pass = $null -eq $reason -and $exitCode -eq 0 -and $null -ne $stress -and
    $stress.status -ceq "PASS_ZERO_PANIC" -and [bool]$stress.simultaneous_full_tiers -and
    [int]$stress.tier2_vram_mb -ge 4096 -and
    (Test-Path -LiteralPath (Join-Path $dir "binary-match.txt"))
$summary = [ordered]@{
    status = if ($pass) { "PASS" } else { "FAIL" }
    reason = $reason
    wsl_exit_code = $exitCode
    stress_verdict = if ($null -ne $stress) { $stress.status } else { $null }
    simultaneous_full_tiers = if ($null -ne $stress) { $stress.simultaneous_full_tiers } else { $false }
    physical_cache_mib = if ($null -ne $stress) { $stress.tier2_vram_mb } else { $null }
    artifact = $dir
}
$summary | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $dir "summary.json") -Encoding UTF8
$summary | ConvertTo-Json -Depth 5
if (-not $pass) { exit 2 }
