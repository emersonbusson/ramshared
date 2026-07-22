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
    [int]$OuterTimeoutSec = 420
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
sudo -n ./target/release/ramshared up --vram "$VramMiB" --zram "$ZramMiB" >"`$artifact/ramshared-up.out" 2>"`$artifact/ramshared-up.err"
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
./scripts/safety/validate-wsl2-freeze-campaign-artifact.sh "`$artifact/campaign" >"`$artifact/validation.out" 2>"`$artifact/validation.err"
"@

Set-Content -LiteralPath $guestScriptWin -Encoding ASCII -Value $guestScript

$argList = @("-d", $Distro, "-u", "root", "--", "bash", $guestScriptWsl)
$proc = Start-Process -FilePath "wsl.exe" -ArgumentList $argList -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru -WindowStyle Hidden

if (-not $proc.WaitForExit($OuterTimeoutSec * 1000)) {
    "outer watchdog fired after ${OuterTimeoutSec}s" | Set-Content -Encoding UTF8 (Join-Path $artifactDir "windows-watchdog-fired.txt")
    try {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    } catch {
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
$exitCode = if ($proc.HasExited) { [int]$proc.ExitCode } else { $null }
$validation = Join-Path $artifactDir "validation.out"
$watchdogFired = Test-Path -LiteralPath (Join-Path $artifactDir "windows-watchdog-fired.txt")
if (-not $watchdogFired -and (Test-Path -LiteralPath $validation) -and ((Get-Content -LiteralPath $validation -Raw) -match "WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS")) {
    Write-Summary -Dir $artifactDir -Status "PASS" -Reason "validated_shared_daily_host_campaign" -Extra @{
        wsl_exit_code = $exitCode
        vram_mib = $VramMiB
        zram_mib = $ZramMiB
        rounds = $Rounds
    }
    exit 0
}

Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "shared_campaign_failed_or_unvalidated" -Extra @{
    wsl_exit_code = $exitCode
    vram_mib = $VramMiB
    zram_mib = $ZramMiB
    rounds = $Rounds
}
exit 2
