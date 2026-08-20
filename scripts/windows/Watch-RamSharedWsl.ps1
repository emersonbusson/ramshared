#Requires -Version 5.1
<#
.SYNOPSIS
  Monitor the sealed RamShared guest heartbeat from Windows.

.DESCRIPTION
  Windows remains outside the WSL failure domain. If the heartbeat becomes
  stale, this watchdog records evidence and signals only the managed workload
  slice. It never performs a WSL VM lifecycle action.
#>
[CmdletBinding()]
param(
    [switch]$Run,
    [ValidatePattern('^[A-Za-z0-9._-]+$')]
    [string]$Distro = "Ubuntu-24.04",
    [string]$HeartbeatPath = "C:\wsl-forensics\ramshared-heartbeat.json",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [ValidateRange(3, 60)][int]$StaleAfterSec = 5,
    [ValidateRange(1, 30)][int]$PollSec = 2,
    [ValidateRange(1, 60)][int]$TerminationGraceSec = 10,
    [ValidateRange(1, 30)][int]$GuestCommandTimeoutSec = 5
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-WatchdogEvent {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Event,
        [System.Collections.IDictionary]$Data = @{}
    )
    $record = [ordered]@{
        schema_version = 1
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        event = $Event
        distro = $Distro
        data = $Data
    }
    Add-Content -LiteralPath $Path -Encoding UTF8 -Value ($record | ConvertTo-Json -Compress -Depth 5)
}

function Invoke-BoundedWslCommand {
    param(
        [Parameter(Mandatory = $true)][ValidateSet("TERM", "KILL")][string]$Signal
    )
    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = "wsl.exe"
    $start.Arguments = "-d $Distro -u root -- systemctl kill --kill-who=all --signal=$Signal ramshared-workloads.slice"
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $start
    if (-not $process.Start()) {
        throw "watchdog could not start the bounded WSL client"
    }
    if (-not $process.WaitForExit($GuestCommandTimeoutSec * 1000)) {
        $process.Kill()
        $process.WaitForExit()
        return [ordered]@{ completed = $false; exit_code = $null; reason = "guest_command_timeout" }
    }
    return [ordered]@{
        completed = $true
        exit_code = $process.ExitCode
        reason = if ($process.ExitCode -eq 0) { "signal_delivered" } else { "guest_command_failed" }
    }
}

if (-not $Run) {
    [ordered]@{
        state = "PLAN"
        distro = $Distro
        heartbeat = $HeartbeatPath
        stale_after_seconds = $StaleAfterSec
        managed_target = "ramshared-workloads.slice"
    } | ConvertTo-Json -Depth 3
    exit 0
}

if ($ArtifactRoot -notmatch '^[A-Za-z]:\\') {
    throw "ArtifactRoot must be an absolute Windows path"
}
New-Item -ItemType Directory -Force -Path $ArtifactRoot | Out-Null
$runDirectory = Join-Path $ArtifactRoot ("ramshared-watchdog-" + (Get-Date -Format "yyyyMMdd-HHmmss"))
New-Item -ItemType Directory -Path $runDirectory | Out-Null
$eventPath = Join-Path $runDirectory "watchdog-events.jsonl"
Write-WatchdogEvent -Path $eventPath -Event "watchdog_started" -Data @{ heartbeat = $HeartbeatPath }

while ($true) {
    $heartbeat = Get-Item -LiteralPath $HeartbeatPath -ErrorAction SilentlyContinue
    $age = if ($null -eq $heartbeat) {
        [double]::PositiveInfinity
    } else {
        ([DateTime]::UtcNow - $heartbeat.LastWriteTimeUtc).TotalSeconds
    }
    if ($age -gt $StaleAfterSec) {
        Write-WatchdogEvent -Path $eventPath -Event "heartbeat_stale" -Data @{ age_seconds = $age }
        $term = Invoke-BoundedWslCommand -Signal "TERM"
        Write-WatchdogEvent -Path $eventPath -Event "managed_scope_term" -Data $term
        Start-Sleep -Seconds $TerminationGraceSec
        $kill = Invoke-BoundedWslCommand -Signal "KILL"
        Write-WatchdogEvent -Path $eventPath -Event "managed_scope_kill" -Data $kill
        Write-Error "RamShared heartbeat became stale; only the managed workload slice was targeted. Evidence: $eventPath"
        exit 2
    }
    Start-Sleep -Seconds $PollSec
}
