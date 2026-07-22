#Requires -Version 5.1
<#
.SYNOPSIS
  Run the supervised shared-host WSL2 pressure campaign.

.DESCRIPTION
  This is the only approved daily/shared WSL2 pressure path. Windows owns the
  outer watchdog, so a WSL-side hang still has an external process that can call
  `wsl.exe --terminate`. The script does not create, resize, initialize, or
  format disks.
#>
[CmdletBinding()]
param(
    [switch]$ApproveSharedDailyHost,
    [string]$Distro = "Ubuntu-24.04",
    [string]$WslRepo = "/home/emdev/codespace/ramshared",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [int]$VramMiB = 1024,
    [int]$ZramMiB = 256,
    [int]$Rounds = 2,
    [int]$WatchdogSec = 120,
    [int]$OuterTimeoutSec = 420,
    [switch]$PreallocateVram,
    [ValidateRange(0, 4096)][int]$ExternalWorkloadMiB = 0,
    [ValidateRange(1, 3600)][int]$ExternalWorkloadHoldSec = 60,
    [ValidateRange(0, 120)][int]$ExternalWorkloadDelaySec = 4
)

$ErrorActionPreference = "Stop"

if ($ArtifactRoot -notmatch '^[A-Za-z]:\\') {
    throw "ArtifactRoot must be an absolute Windows path such as C:\ramshared\artifacts. Quote backslashes when invoking from WSL."
}

function New-ArtifactDir {
    param([string]$Root)
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $dir = Join-Path $Root "shared-wsl-pressure-$stamp"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    return $dir
}

function Convert-ToWslPath {
    param([string]$Path)
    if ($Path -match '^([A-Za-z]):\\(.*)$') {
        $drive = $Matches[1].ToLowerInvariant()
        $rest = $Matches[2] -replace '\\','/'
        return "/mnt/$drive/$rest"
    }
    return $Path
}

function Write-Summary {
    param(
        [string]$Dir,
        [string]$Status,
        [string]$Reason,
        [hashtable]$Extra = @{}
    )
    $summary = [ordered]@{
        STATUS = $Status
        PASS = ($Status -eq "PASS")
        REASON = $Reason
        DISTRO = $Distro
        WSL_REPO = $WslRepo
        ARTIFACT = $Dir
        APPROVED_SHARED_DAILY_HOST = [bool]$ApproveSharedDailyHost
        OUTER_TIMEOUT_SEC = $OuterTimeoutSec
        DISK_MUTATION = $false
        PREALLOCATE_VRAM = [bool]$PreallocateVram
        EXTERNAL_WORKLOAD_MIB = $ExternalWorkloadMiB
    }
    foreach ($k in $Extra.Keys) { $summary[$k] = $Extra[$k] }
    $summary | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $Dir "summary.json")
    Write-Host "STATUS=$Status"
    Write-Host "REASON=$Reason"
    Write-Host "ARTIFACT_DIR=$Dir"
}

if (-not $ApproveSharedDailyHost) {
    $artifactDir = New-ArtifactDir -Root $ArtifactRoot
    Write-Summary -Dir $artifactDir -Status "REFUSED" -Reason "missing_ApproveSharedDailyHost"
    exit 2
}

$artifactDir = New-ArtifactDir -Root $ArtifactRoot
$artifactWsl = Convert-ToWslPath -Path $artifactDir
$stdout = Join-Path $artifactDir "wsl-campaign.out"
$stderr = Join-Path $artifactDir "wsl-campaign.err"
$guestScriptWin = Join-Path $artifactDir "run-shared-wsl-pressure.sh"
$guestScriptWsl = Convert-ToWslPath -Path $guestScriptWin

$guestScript = @"
set -euo pipefail
cd "$WslRepo"
artifact="$artifactWsl"
mkdir -p "`$artifact"
health_pid=""
cleanup() {
  rc=`$?
  if [ -n "`$health_pid" ]; then
    kill "`$health_pid" 2>/dev/null || true
    wait "`$health_pid" 2>/dev/null || true
  fi
  if [ -x ./target/release/ramshared ]; then
    sudo -n ./target/release/ramshared down >"`$artifact/ramshared-down.out" 2>"`$artifact/ramshared-down.err" || true
  fi
  ./scripts/safety/cascade-health.sh --once >"`$artifact/final-health.json" 2>/dev/null || true
  cat /proc/swaps >"`$artifact/final-swaps.txt" 2>/dev/null || true
  dmesg | tail -n 240 >"`$artifact/final-dmesg-tail.txt" 2>/dev/null || true
  exit `$rc
}
trap cleanup EXIT INT TERM

./scripts/safety/cascade-health.sh --loop --interval 1 --out "`$artifact/cascade-health.jsonl" >"`$artifact/cascade-health.stdout" 2>"`$artifact/cascade-health.stderr" &
health_pid=`$!

sudo -n ./target/release/ramshared down >"`$artifact/pre-down.out" 2>"`$artifact/pre-down.err" || true
daemon_wrapper="`$artifact/ramsharedd-logged.sh"
printf '%s\n' '#!/usr/bin/env bash' 'exec "$WslRepo/target/release/ramsharedd" "`$@" >>"$artifactWsl/daemon.out" 2>&1' >"`$daemon_wrapper"
chmod 0700 "`$daemon_wrapper"
if [ "$([int][bool]$PreallocateVram)" -eq 1 ]; then
  export RAMSHARED_VRAM_PREALLOC=1
fi
sudo -n env RAMSHARED_TRACE_PROBE=1 ./target/release/ramshared up --vram "$VramMiB" --zram "$ZramMiB" --daemon "`$daemon_wrapper" >"`$artifact/ramshared-up.out" 2>"`$artifact/ramshared-up.err"
./scripts/safety/cascade-health.sh --once >"`$artifact/after-up-health.json"

export RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_WSL_TERMINATION
export RAMSHARED_WINDOWS_WATCHDOG_ARMED=1
export RAMSHARED_FREEZE_WATCHDOG_SEC="$WatchdogSec"
./scripts/safety/wsl2-freeze-campaign.sh \
  --approve-shared-daily-host \
  --run-shared-daily-host \
  --artifact-dir "`$artifact/campaign" \
  --rounds "$Rounds" \
  --watchdog-sec "$WatchdogSec" \
  --json >"`$artifact/campaign.out" 2>"`$artifact/campaign.err"
export RAMSHARED_FREEZE_REQUIRED_ROUNDS="$Rounds"
./scripts/safety/validate-wsl2-freeze-campaign-artifact.sh "`$artifact/campaign" >"`$artifact/validation.out" 2>"`$artifact/validation.err"
kill "`$health_pid" 2>/dev/null || true
wait "`$health_pid" 2>/dev/null || true
health_pid=""
python3 - "`$artifact/cascade-health.jsonl" "`$artifact/events.jsonl" <<'PY'
import json, sys

with open(sys.argv[1], encoding="utf-8") as source, open(sys.argv[2], "w", encoding="utf-8") as out:
    for line in source:
        if not line.strip():
            continue
        sample = json.loads(line)
        demote = sample.get("demote") or {}
        event = {
            "t": sample.get("epoch"),
            "swap_used": (sample.get("used_kib") or {}).get("vram", 0) * 1024,
            "canario_demotes": demote.get("total", 0),
            "demote_reason": demote.get("last_reason"),
            "flag": "none" if sample.get("ok") else "partial",
        }
        out.write(json.dumps(event, separators=(",", ":")) + "\n")
PY
./target/release/ramshared diagnose --events "`$artifact/events.jsonl" --json >"`$artifact/diagnose.json"
"@

$ascii = [System.Text.Encoding]::ASCII
[System.IO.File]::WriteAllText($guestScriptWin, ($guestScript -replace "`r`n", "`n"), $ascii)

$argList = @("-d", $Distro, "-u", "root", "--", "bash", $guestScriptWsl)
$proc = Start-Process -FilePath "wsl.exe" -ArgumentList $argList -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru -WindowStyle Hidden
$externalProc = $null
if ($ExternalWorkloadMiB -gt 0) {
    Start-Sleep -Seconds $ExternalWorkloadDelaySec
    $externalSource = Resolve-Path (Join-Path $PSScriptRoot "..\p0\Start-CudaVramWorkload.ps1")
    $externalScript = Join-Path $artifactDir "external-workload.ps1"
    Get-Content -LiteralPath $externalSource.Path -Raw | Set-Content -LiteralPath $externalScript -Encoding UTF8
    $externalOut = Join-Path $artifactDir "external-workload.out"
    $externalErr = Join-Path $artifactDir "external-workload.err"
    $externalArgs = @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $externalScript,
        "-MiB", $ExternalWorkloadMiB, "-HoldSec", $ExternalWorkloadHoldSec
    )
    $externalProc = Start-Process -FilePath "powershell.exe" -ArgumentList $externalArgs `
        -RedirectStandardOutput $externalOut -RedirectStandardError $externalErr -PassThru -WindowStyle Hidden
}

if (-not $proc.WaitForExit($OuterTimeoutSec * 1000)) {
    "outer watchdog fired after ${OuterTimeoutSec}s" | Set-Content -Encoding UTF8 (Join-Path $artifactDir "windows-watchdog-fired.txt")
    try {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    } catch {
    }
    if ($externalProc -ne $null -and -not $externalProc.HasExited) {
        Stop-Process -Id $externalProc.Id -Force -ErrorAction SilentlyContinue
    }
    $term = Start-Process -FilePath "wsl.exe" -ArgumentList @("--terminate", $Distro) -PassThru -WindowStyle Hidden
    if (-not $term.WaitForExit(60000)) {
        "wsl --terminate timed out after 60s; manual Windows reboot may be required" | Set-Content -Encoding UTF8 (Join-Path $artifactDir "wsl-terminate-timeout.txt")
        try { Stop-Process -Id $term.Id -Force -ErrorAction SilentlyContinue } catch {}
    }
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "outer_watchdog_fired" -Extra @{
        wsl_exit_code = $null
    }
    exit 2
}

$proc.WaitForExit()
$proc.Refresh()
$externalExitCode = $null
if ($externalProc -ne $null) {
    if (-not $externalProc.WaitForExit(($ExternalWorkloadHoldSec + 20) * 1000)) {
        Stop-Process -Id $externalProc.Id -Force -ErrorAction SilentlyContinue
    } else {
        $externalProc.Refresh()
        $externalExitCode = [int]$externalProc.ExitCode
    }
}
$exitCode = if ($proc.HasExited) { [int]$proc.ExitCode } else { $null }
$validation = Join-Path $artifactDir "validation.out"
$watchdogFired = Test-Path -LiteralPath (Join-Path $artifactDir "windows-watchdog-fired.txt")
$externalWorkloadOk = $ExternalWorkloadMiB -eq 0
if ($ExternalWorkloadMiB -gt 0) {
    $externalWorkloadOutput = Join-Path $artifactDir "external-workload.out"
    $externalWorkloadOk = $externalExitCode -eq 0 -and
        (Test-Path -LiteralPath $externalWorkloadOutput) -and
        (Get-Content -LiteralPath $externalWorkloadOutput -Raw).Contains("[cuda-vram-workload] released")
}
if (-not $watchdogFired -and $externalWorkloadOk -and (Test-Path -LiteralPath $validation) -and ((Get-Content -LiteralPath $validation -Raw) -match "WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS")) {
    Write-Summary -Dir $artifactDir -Status "PASS" -Reason "validated_shared_daily_host_campaign" -Extra @{
        wsl_exit_code = $exitCode
        vram_mib = $VramMiB
        zram_mib = $ZramMiB
        rounds = $Rounds
        external_workload_exit_code = $externalExitCode
        external_workload_ok = [bool]$externalWorkloadOk
    }
    exit 0
}

Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "shared_campaign_failed_or_unvalidated" -Extra @{
    wsl_exit_code = $exitCode
    vram_mib = $VramMiB
    zram_mib = $ZramMiB
    rounds = $Rounds
    external_workload_exit_code = $externalExitCode
    external_workload_ok = [bool]$externalWorkloadOk
}
exit 2
