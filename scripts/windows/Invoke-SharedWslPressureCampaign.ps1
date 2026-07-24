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
    [ValidateRange(0, 120)][int]$ExternalWorkloadDelaySec = 4,
    [ValidateRange(0, 600)][int]$PostCampaignObserveSec = 120,
    [string[]]$HostDiskLetters = @("C", "I")
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

function Normalize-HostDiskLetters {
    param([string[]]$Letters)
    return @($Letters | ForEach-Object {
        $raw = $_
        if ([string]::IsNullOrWhiteSpace($raw)) { return }
        $raw -split ','
    } | ForEach-Object {
        if ([string]::IsNullOrWhiteSpace($_)) { return }
        ($_.Trim().TrimEnd(":").ToUpperInvariant() + ":")
    } | Where-Object { $_ } | Select-Object -Unique)
}

function Start-HostDiskTelemetry {
    param(
        [string[]]$Letters,
        [string]$JsonlPath,
        [string]$VolumePath,
        [int]$IntervalSec = 1
    )
    $normalized = Normalize-HostDiskLetters -Letters $Letters
    $volumeRows = @()
    foreach ($letter in $normalized) {
        try {
            $disk = Get-CimInstance -ClassName Win32_LogicalDisk -Filter ("DeviceID='{0}'" -f $letter) -ErrorAction Stop
            if ($disk) {
                $volumeRows += [ordered]@{
                    name = $disk.DeviceID
                    volume_name = $disk.VolumeName
                    file_system = $disk.FileSystem
                    size_bytes = [uint64]$disk.Size
                    free_bytes = [uint64]$disk.FreeSpace
                    drive_type = [int]$disk.DriveType
                }
            }
        } catch {
            $volumeRows += [ordered]@{
                name = $letter
                error = $_.Exception.Message
            }
        }
    }
    @($volumeRows) | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -LiteralPath $VolumePath
    $normalizedCsv = $normalized -join ','
    return Start-Job -ArgumentList $normalizedCsv, $JsonlPath, $IntervalSec -ScriptBlock {
        param($LettersCsv, $OutPath, $Interval)
        $ErrorActionPreference = "Continue"
        $Letters = @($LettersCsv -split ',' | Where-Object { $_ })
        function U64OrZero($Value) {
            if ($null -eq $Value) { return [uint64]0 }
            return [uint64]$Value
        }
        function F64OrZero($Value) {
            if ($null -eq $Value) { return [double]0 }
            return [double]$Value
        }
        while ($true) {
            $epoch = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
            $timestamp = Get-Date -Format "o"
            $rows = @()
            try {
                $perf = @(Get-CimInstance -ClassName Win32_PerfFormattedData_PerfDisk_LogicalDisk -ErrorAction Stop)
                foreach ($letter in $Letters) {
                    $row = $perf | Where-Object { $_.Name -eq $letter } | Select-Object -First 1
                    $logical = Get-CimInstance -ClassName Win32_LogicalDisk -Filter ("DeviceID='{0}'" -f $letter) -ErrorAction SilentlyContinue
                    if ($null -eq $row) {
                        $rows += [ordered]@{
                            ts = $timestamp
                            epoch = $epoch
                            name = $letter
                            error = "perf_disk_identity_missing"
                        }
                        continue
                    }
                    $rows += [ordered]@{
                        ts = $timestamp
                        epoch = $epoch
                        name = $letter
                        disk_bytes_per_sec = U64OrZero $row.DiskBytesPersec
                        read_bytes_per_sec = U64OrZero $row.DiskReadBytesPersec
                        write_bytes_per_sec = U64OrZero $row.DiskWriteBytesPersec
                        avg_disk_sec_per_read = F64OrZero $row.AvgDisksecPerRead
                        avg_disk_sec_per_write = F64OrZero $row.AvgDisksecPerWrite
                        current_disk_queue_length = U64OrZero $row.CurrentDiskQueueLength
                        percent_disk_time = U64OrZero $row.PercentDiskTime
                        free_bytes = if ($logical) { [uint64]$logical.FreeSpace } else { $null }
                        size_bytes = if ($logical) { [uint64]$logical.Size } else { $null }
                    }
                }
            } catch {
                $rows += [ordered]@{
                    ts = $timestamp
                    epoch = $epoch
                    error = $_.Exception.Message
                }
            }
            foreach ($entry in $rows) {
                ($entry | ConvertTo-Json -Compress -Depth 5) | Add-Content -Encoding UTF8 -LiteralPath $OutPath
            }
            Start-Sleep -Seconds ([Math]::Max(1, [int]$Interval))
        }
    }
}

function Test-HostDiskTelemetryArtifacts {
    param(
        [string[]]$Letters,
        [string]$JsonlPath,
        [string]$VolumePath
    )
    $expected = @(Normalize-HostDiskLetters -Letters $Letters)
    if ($expected.Count -eq 0) {
        return [pscustomobject]@{ Ok = $false; Reason = "no_expected_disk_identity" }
    }
    if (-not (Test-Path -LiteralPath $JsonlPath) -or
        -not (Test-Path -LiteralPath $VolumePath)) {
        return [pscustomobject]@{ Ok = $false; Reason = "telemetry_artifact_missing" }
    }
    try {
        $volumes = @(Get-Content -LiteralPath $VolumePath -Raw | ConvertFrom-Json)
        $validVolumes = @($volumes | Where-Object {
            $null -eq $_.error -and $_.name -and $null -ne $_.size_bytes
        } | ForEach-Object { [string]$_.name })
        foreach ($letter in $expected) {
            if ($validVolumes -notcontains $letter) {
                return [pscustomobject]@{ Ok = $false; Reason = "volume_identity_missing:$letter" }
            }
        }

        $validSamples = @{}
        foreach ($line in @(Get-Content -LiteralPath $JsonlPath)) {
            if ([string]::IsNullOrWhiteSpace($line)) { continue }
            $row = $line | ConvertFrom-Json
            if ($null -eq $row.error -and $row.name -and $null -ne $row.epoch) {
                $validSamples[[string]$row.name] = $true
            }
        }
        foreach ($letter in $expected) {
            if (-not $validSamples.ContainsKey($letter)) {
                return [pscustomobject]@{ Ok = $false; Reason = "sample_identity_missing:$letter" }
            }
        }
        return [pscustomobject]@{ Ok = $true; Reason = "complete" }
    } catch {
        return [pscustomobject]@{ Ok = $false; Reason = "telemetry_parse_failed" }
    }
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
        POST_CAMPAIGN_OBSERVE_SEC = $PostCampaignObserveSec
        HOST_DISK_LETTERS = @(Normalize-HostDiskLetters -Letters $HostDiskLetters)
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
$hostDiskJsonl = Join-Path $artifactDir "host-disk-logical.jsonl"
$hostDiskVolumes = Join-Path $artifactDir "host-disk-volumes.json"
$hostDiskJob = Start-HostDiskTelemetry -Letters $HostDiskLetters -JsonlPath $hostDiskJsonl -VolumePath $hostDiskVolumes

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
export RAMSHARED_ALLOW_RECENT_OOM_MARKER=1
export RAMSHARED_FREEZE_WATCHDOG_SEC="$WatchdogSec"
./scripts/safety/wsl2-freeze-campaign.sh \
  --approve-shared-daily-host \
  --run-shared-daily-host \
  --artifact-dir "`$artifact/campaign" \
  --rounds "$Rounds" \
  --watchdog-sec "$WatchdogSec" \
  --json >"`$artifact/campaign.out" 2>"`$artifact/campaign.err"
export RAMSHARED_FREEZE_REQUIRED_ROUNDS="$Rounds"
validation_rc=0
./scripts/safety/validate-wsl2-freeze-campaign-artifact.sh "`$artifact/campaign" >"`$artifact/validation.out" 2>"`$artifact/validation.err" || validation_rc=`$?
printf 'validation_rc=%s\n' "`$validation_rc" >"`$artifact/validation-rc.txt"
if [ "$ExternalWorkloadMiB" -gt 0 ]; then
  printf 'campaign_validation_complete\n' >"`$artifact/external-phase-ready.txt"
  external_deadline=`$((SECONDS + $ExternalWorkloadHoldSec + $PostCampaignObserveSec + 90))
  while [ ! -f "`$artifact/external-phase-complete.txt" ]; do
    if [ "`$SECONDS" -ge "`$external_deadline" ]; then
      echo "external phase completion timed out" >&2
      exit 1
    fi
    sleep 1
  done
  if [ "$PostCampaignObserveSec" -gt 0 ]; then
    sleep "$PostCampaignObserveSec"
  fi
fi
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
$campaignStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$proc = Start-Process -FilePath "wsl.exe" -ArgumentList $argList -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru -WindowStyle Hidden
$externalProc = $null
$externalExitCode = $null
$externalReady = Join-Path $artifactDir "external-phase-ready.txt"
$externalComplete = Join-Path $artifactDir "external-phase-complete.txt"
if ($ExternalWorkloadMiB -gt 0) {
    while (-not (Test-Path -LiteralPath $externalReady) -and -not $proc.HasExited -and
        $campaignStopwatch.Elapsed.TotalSeconds -lt $OuterTimeoutSec) {
        Start-Sleep -Seconds 1
        $proc.Refresh()
    }
    if (Test-Path -LiteralPath $externalReady) {
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
}

if ($externalProc -ne $null) {
    if (-not $externalProc.WaitForExit(($ExternalWorkloadHoldSec + 20) * 1000)) {
        Stop-Process -Id $externalProc.Id -Force -ErrorAction SilentlyContinue
    } else {
        $externalProc.Refresh()
        $externalExitCode = [int]$externalProc.ExitCode
    }
    New-Item -ItemType File -Force -Path $externalComplete | Out-Null
}

$remainingTimeoutMs = [Math]::Max(0, [int](($OuterTimeoutSec - $campaignStopwatch.Elapsed.TotalSeconds) * 1000))
if (-not $proc.WaitForExit($remainingTimeoutMs)) {
    "outer watchdog fired after ${OuterTimeoutSec}s" | Set-Content -Encoding UTF8 (Join-Path $artifactDir "windows-watchdog-fired.txt")
    try {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    } catch {
    }
    if ($externalProc -ne $null -and -not $externalProc.HasExited) {
        Stop-Process -Id $externalProc.Id -Force -ErrorAction SilentlyContinue
    }
    if ($hostDiskJob -ne $null) {
        Stop-Job $hostDiskJob -ErrorAction SilentlyContinue
        Remove-Job $hostDiskJob -Force -ErrorAction SilentlyContinue
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
if ($hostDiskJob -ne $null) {
    Stop-Job $hostDiskJob -ErrorAction SilentlyContinue
    Remove-Job $hostDiskJob -Force -ErrorAction SilentlyContinue
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
$hostDiskAudit = Test-HostDiskTelemetryArtifacts -Letters $HostDiskLetters `
    -JsonlPath $hostDiskJsonl -VolumePath $hostDiskVolumes
$host_disk_telemetry_ok = [bool]$hostDiskAudit.Ok
$validationPass = (Test-Path -LiteralPath $validation) -and
    ((Get-Content -LiteralPath $validation -Raw) -match '(?m)^WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS(?:\s|$)')
$diagnosePath = Join-Path $artifactDir "diagnose.json"
$diagnoseOk = $false
$demoteTotal = 0
$demoteReason = $null
if (Test-Path -LiteralPath $diagnosePath) {
    try {
        $diagnose = Get-Content -LiteralPath $diagnosePath -Raw | ConvertFrom-Json
        $demoteTotal = [int]$diagnose.demotes
        $demoteReason = $diagnose.last_reason
        $diagnoseOk = $true
    } catch {
        $diagnoseOk = $false
    }
}
$finalHealthPath = Join-Path $artifactDir "final-health.json"
$finalClean = $false
if (Test-Path -LiteralPath $finalHealthPath) {
    try {
        $finalHealth = Get-Content -LiteralPath $finalHealthPath -Raw | ConvertFrom-Json
        $finalClean = [bool]$finalHealth.ok -and
            -not [bool]$finalHealth.flags.ghost -and
            -not [bool]$finalHealth.flags.has_vram -and
            -not [bool]$finalHealth.daemon.alive
        if ($null -eq $demoteReason -and $null -ne $finalHealth.demote) {
            $demoteReason = $finalHealth.demote.last_reason
        }
    } catch {
        $finalClean = $false
    }
}
$external_demote_ok = $ExternalWorkloadMiB -gt 0 -and
    -not $watchdogFired -and
    $exitCode -eq 0 -and
    $externalWorkloadOk -and
    $diagnoseOk -and
    $demoteTotal -gt 0 -and
    $finalClean
$matrixRowClose = $validationPass -and $external_demote_ok
$campaignPass = -not $watchdogFired -and
    $exitCode -eq 0 -and
    $externalWorkloadOk -and
    $validationPass -and
    $finalClean -and
    $host_disk_telemetry_ok

if ($campaignPass) {
    Write-Summary -Dir $artifactDir -Status "PASS" -Reason "validated_shared_daily_host_campaign" -Extra @{
        wsl_exit_code = $exitCode
        vram_mib = $VramMiB
        zram_mib = $ZramMiB
        rounds = $Rounds
        external_workload_exit_code = $externalExitCode
        external_workload_ok = [bool]$externalWorkloadOk
        external_demote_ok = [bool]$external_demote_ok
        host_disk_telemetry_ok = [bool]$host_disk_telemetry_ok
        host_disk_telemetry_reason = $hostDiskAudit.Reason
        diagnose_ok = [bool]$diagnoseOk
        demote_total = $demoteTotal
        demote_reason = $demoteReason
        final_clean = [bool]$finalClean
        freeze_campaign_validated = $true
        matrix_row_close = [bool]$matrixRowClose
    }
    exit 0
}

if ($external_demote_ok) {
    Write-Summary -Dir $artifactDir -Status "PASS" -Reason "validated_external_global_gpu_demote" -Extra @{
        wsl_exit_code = $exitCode
        vram_mib = $VramMiB
        zram_mib = $ZramMiB
        rounds = $Rounds
        external_workload_exit_code = $externalExitCode
        external_workload_ok = [bool]$externalWorkloadOk
        external_demote_ok = [bool]$external_demote_ok
        host_disk_telemetry_ok = [bool]$host_disk_telemetry_ok
        host_disk_telemetry_reason = $hostDiskAudit.Reason
        diagnose_ok = [bool]$diagnoseOk
        demote_total = $demoteTotal
        demote_reason = $demoteReason
        final_clean = [bool]$finalClean
        validation_pass = [bool]$validationPass
        freeze_campaign_validated = $false
        matrix_row_close = [bool]$matrixRowClose
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
    external_demote_ok = [bool]$external_demote_ok
    host_disk_telemetry_ok = [bool]$host_disk_telemetry_ok
    host_disk_telemetry_reason = $hostDiskAudit.Reason
    diagnose_ok = [bool]$diagnoseOk
    demote_total = $demoteTotal
    demote_reason = $demoteReason
    final_clean = [bool]$finalClean
    validation_pass = [bool]$validationPass
    freeze_campaign_validated = [bool]$validationPass
    matrix_row_close = [bool]$matrixRowClose
}
exit 2
