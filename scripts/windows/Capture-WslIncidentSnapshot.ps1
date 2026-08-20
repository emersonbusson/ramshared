#Requires -Version 5.1
<#
.SYNOPSIS
  Capture a bounded, read-only WSL incident snapshot for an upstream report.

.DESCRIPTION
  The default path emits a plan and makes no host change. With -Run, this
  script writes a local artifact containing bounded wsl.exe queries, relevant
  Windows service state, and a limited event-log snapshot. It does not start
  pressure, alter WSL lifecycle, or download or execute the Microsoft log
  collector. Run the official collector manually from an elevated console only
  after reviewing its current source.
#>
[CmdletBinding()]
param(
    [switch]$Run,
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [ValidateRange(1, 60)][int]$CommandTimeoutSec = 15,
    [ValidateRange(1, 168)][int]$EventLookbackHours = 24,
    [ValidateRange(1, 1000)][int]$EventLimit = 200
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$officialCollectorUrl = "https://raw.githubusercontent.com/microsoft/WSL/master/diagnostics/collect-wsl-logs.ps1"
$expectedArtifactRoot = [IO.Path]::GetFullPath("C:\ramshared\artifacts").TrimEnd([char]92, [char]47)

function Write-JsonFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][object]$Value
    )

    $json = $Value | ConvertTo-Json -Depth 10
    [IO.File]::WriteAllText($Path, $json, [Text.UTF8Encoding]::new($false))
}

function Assert-ArtifactRoot {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not [IO.Path]::IsPathRooted($Path) -or
        [regex]::IsMatch($Path, '(^|[\\/])\.\.([\\/]|$)')) {
        throw "ArtifactRoot must be the exact absolute RamShared artifact root"
    }
    $canonical = [IO.Path]::GetFullPath($Path).TrimEnd([char]92, [char]47)
    if (-not $canonical.Equals($expectedArtifactRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw "ArtifactRoot must equal $expectedArtifactRoot"
    }
    if (-not (Test-Path -LiteralPath $canonical)) {
        New-Item -ItemType Directory -Path $canonical -Force -ErrorAction Stop | Out-Null
    }
    $item = Get-Item -LiteralPath $canonical -Force -ErrorAction Stop
    if (-not $item.PSIsContainer -or
        (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "ArtifactRoot must be a non-reparse directory"
    }
    return $item.FullName
}

function Invoke-BoundedNativeCommand {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Arguments,
        [Parameter(Mandatory = $true)][ValidateRange(1, 60)][int]$TimeoutSec
    )

    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = "wsl.exe"
    $start.Arguments = $Arguments
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    # wsl.exe emits UTF-16LE when stdout/stderr are redirected on this host.
    # Decode it before writing JSON so the evidence remains portable and readable.
    $start.StandardOutputEncoding = [Text.Encoding]::Unicode
    $start.StandardErrorEncoding = [Text.Encoding]::Unicode
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) {
            $process.Dispose()
            return [ordered]@{
                name = $Name
                arguments = $Arguments
                completed = $false
                reason = "snapshot_command_start_failed"
                exit_code = $null
                stdout = ""
                stderr = ""
            }
        }
    }
    catch {
        $process.Dispose()
        return [ordered]@{
            name = $Name
            arguments = $Arguments
            completed = $false
            reason = "snapshot_command_start_failed"
            exit_code = $null
            stdout = ""
            stderr = $_.Exception.Message
        }
    }
    if (-not $process.WaitForExit($TimeoutSec * 1000)) {
        try { $process.Kill() } catch {}
        $process.WaitForExit(2000) | Out-Null
        $process.Dispose()
        return [ordered]@{
            name = $Name
            arguments = $Arguments
            completed = $false
            reason = "snapshot_command_timeout"
            exit_code = $null
            stdout = ""
            stderr = ""
        }
    }
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $exitCode = $process.ExitCode
    $process.Dispose()
    return [ordered]@{
        name = $Name
        arguments = $Arguments
        completed = $true
        reason = if ($exitCode -eq 0) { "completed" } else { "command_failed" }
        exit_code = $exitCode
        stdout = $stdout
        stderr = $stderr
    }
}

function Get-WslServiceSnapshot {
    $serviceNames = @("WslService", "LxssManager", "vmcompute", "hns")
    try {
        return @(Get-CimInstance Win32_Service -ErrorAction Stop |
            Where-Object { $serviceNames -contains $_.Name } |
            Sort-Object -Property Name |
            ForEach-Object {
                [ordered]@{
                    name = $_.Name
                    state = $_.State
                    start_mode = $_.StartMode
                    process_id = $_.ProcessId
                    exit_code = $_.ExitCode
                    path_name = $_.PathName
                }
            })
    }
    catch {
        return @([ordered]@{ query_error = $_.Exception.Message })
    }
}

function Get-WslConfigSnapshot {
    $snapshot = [ordered]@{
        schema_version = 1
        source = ".wslconfig"
        present = $false
        path_omitted = $true
        custom_kernel_configured = $false
        values = [ordered]@{}
    }
    $configPath = Join-Path $env:USERPROFILE ".wslconfig"
    if (-not (Test-Path -LiteralPath $configPath -PathType Leaf)) {
        return $snapshot
    }

    $snapshot.present = $true
    $safeValueKeys = @(
        "memory",
        "processors",
        "swap",
        "pageReporting",
        "autoMemoryReclaim",
        "networkingMode",
        "vmIdleTimeout",
        "debugConsole",
        "nestedVirtualization"
    )
    try {
        foreach ($line in Get-Content -LiteralPath $configPath -ErrorAction Stop) {
            $trimmed = $line.Trim()
            if ($trimmed -notmatch '^(?<key>[A-Za-z][A-Za-z0-9]*)\s*=\s*(?<value>.*)$') {
                continue
            }
            $key = $Matches["key"].ToLowerInvariant()
            $value = $Matches["value"].Trim()
            if ($key -eq "kernel") {
                $snapshot.custom_kernel_configured = -not [string]::IsNullOrWhiteSpace($value)
                $snapshot.values["kernel"] = if ($snapshot.custom_kernel_configured) { "configured" } else { "empty" }
                continue
            }
            if ($key -eq "swapfile") {
                $snapshot.values["swapFile"] = if ([string]::IsNullOrWhiteSpace($value)) { "empty" } else { "configured" }
                continue
            }
            if ($safeValueKeys -contains $key) {
                $snapshot.values[$key] = $value
            }
        }
    }
    catch {
        $snapshot.query_error = $_.Exception.Message
    }
    return $snapshot
}

function Get-WslEventSnapshot {
    param(
        [Parameter(Mandatory = $true)][DateTime]$StartTime,
        [Parameter(Mandatory = $true)][int]$Limit
    )

    $providers = @(
        "Microsoft.Windows.Lxss.Manager",
        "Microsoft-Windows-Lxss-Manager",
        "Microsoft-Windows-Hyper-V-Compute",
        "Microsoft-Windows-Host-Network-Service"
    )
    $records = @()
    foreach ($provider in $providers) {
        try {
            $events = @(Get-WinEvent -FilterHashtable @{ ProviderName = $provider; StartTime = $StartTime } -MaxEvents $Limit -ErrorAction Stop)
            foreach ($event in $events) {
                $records += [ordered]@{
                    provider = $event.ProviderName
                    id = $event.Id
                    level = $event.LevelDisplayName
                    timestamp_utc = $event.TimeCreated.ToUniversalTime().ToString("o")
                    message = $event.Message
                }
            }
        }
        catch {
            $records += [ordered]@{
                provider = $provider
                query_error = $_.Exception.Message
            }
        }
    }
    return $records
}

function Get-ArtifactManifest {
    param([Parameter(Mandatory = $true)][string]$Directory)

    return @(
        Get-ChildItem -LiteralPath $Directory -File -ErrorAction Stop |
            Sort-Object -Property Name |
            ForEach-Object {
                $hash = Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256 -ErrorAction Stop
                [ordered]@{
                    relative_path = $_.Name
                    bytes = [UInt64]$_.Length
                    sha256 = $hash.Hash.ToUpperInvariant()
                }
            }
    )
}

$plan = [ordered]@{
    schema_version = 1
    state = "PLAN"
    collector = "Capture-WslIncidentSnapshot"
    artifact_root = $ArtifactRoot
    command_timeout_seconds = $CommandTimeoutSec
    event_lookback_hours = $EventLookbackHours
    event_limit = $EventLimit
    official_wsl_log_collector_url = $officialCollectorUrl
    official_wsl_log_collector_execution = "manual_elevated_after_source_review"
    changes_wsl_lifecycle = $false
    starts_pressure = $false
}
if (-not $Run) {
    $plan | ConvertTo-Json -Depth 6
    exit 0
}

$canonicalRoot = Assert-ArtifactRoot -Path $ArtifactRoot
$runName = "wsl-incident-" + (Get-Date -Format "yyyyMMdd-HHmmss") + "-" + [Guid]::NewGuid().ToString("N")
$artifactDirectory = Join-Path $canonicalRoot $runName
New-Item -ItemType Directory -Path $artifactDirectory -ErrorAction Stop | Out-Null

$commands = @(
    Invoke-BoundedNativeCommand -Name "wsl-version" -Arguments "--version" -TimeoutSec $CommandTimeoutSec
    Invoke-BoundedNativeCommand -Name "wsl-status" -Arguments "--status" -TimeoutSec $CommandTimeoutSec
    Invoke-BoundedNativeCommand -Name "wsl-list" -Arguments "-l -v" -TimeoutSec $CommandTimeoutSec
)
$services = Get-WslServiceSnapshot
$wslConfig = Get-WslConfigSnapshot
$startTime = [DateTime]::UtcNow.AddHours(-1 * $EventLookbackHours)
$events = Get-WslEventSnapshot -StartTime $startTime -Limit $EventLimit
$timeoutCount = @($commands | Where-Object { $_.reason -eq "snapshot_command_timeout" }).Count

Write-JsonFile -Path (Join-Path $artifactDirectory "wsl-commands.json") -Value $commands
Write-JsonFile -Path (Join-Path $artifactDirectory "wsl-services.json") -Value $services
Write-JsonFile -Path (Join-Path $artifactDirectory "wsl-config.json") -Value $wslConfig
Write-JsonFile -Path (Join-Path $artifactDirectory "wsl-events.json") -Value $events
$summary = [ordered]@{
    schema_version = 1
    state = if ($timeoutCount -eq 0) { "CAPTURED" } else { "PARTIAL" }
    artifact_root = $artifactDirectory
    captured_at_utc = [DateTime]::UtcNow.ToString("o")
    command_timeout_count = $timeoutCount
    custom_kernel_configured = [bool]$wslConfig.custom_kernel_configured
    official_wsl_log_collector_url = $officialCollectorUrl
    official_wsl_log_collector_execution = "manual_elevated_after_source_review"
    changes_wsl_lifecycle = $false
    starts_pressure = $false
}
Write-JsonFile -Path (Join-Path $artifactDirectory "wsl-incident-summary.json") -Value $summary
$manifest = [ordered]@{
    schema_version = 1
    artifact_root = $artifactDirectory
    files = Get-ArtifactManifest -Directory $artifactDirectory
}
Write-JsonFile -Path (Join-Path $artifactDirectory "wsl-incident-manifest.json") -Value $manifest

Write-Output "WSL_INCIDENT_SNAPSHOT=$artifactDirectory"
Write-Output "WSL_INCIDENT_STATE=$($summary.state)"
if ($timeoutCount -gt 0) {
    exit 2
}
