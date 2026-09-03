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
    [ValidatePattern('^[A-Za-z0-9._-]+$')]
    [string]$Distro = "Ubuntu-24.04",
    [string]$WslRepo = $env:RAMSHARED_WSL_REPO,
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [int]$VramMiB = 1024,
    [int]$ZramMiB = 256,
    [int]$Rounds = 2,
    [int]$WatchdogSec = 120,
    [int]$OuterTimeoutSec = 420,
    [ValidateRange(5, 120)][int]$ActionCleanupGraceSec = 45,
    [ValidateRange(0.0, 16.0)][double]$PressureAllocGiB = 0,
    [ValidateRange(0, 4096)][int]$ExternalWorkloadMiB = 0,
    [ValidateRange(1, 3600)][int]$ExternalWorkloadHoldSec = 60,
    [ValidateRange(0, 120)][int]$ExternalWorkloadDelaySec = 4,
    [ValidateRange(0, 600)][int]$PostCampaignObserveSec = 120,
    [ValidateRange(4096, 2147483647)][int]$HostCommitReserveMiB = 4096,
    [string[]]$HostDiskLetters = @()
)

$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "SharedWslHostMemoryGate.psm1") -Force
$hostMemoryGateOk = $false
$hostCommitHeadroomMiB = $null
$hostCommitRequiredMiB = $null
$hostMemoryGuardianFired = $false
$hostMemoryTelemetryOk = $false
$SealedDistro = "Ubuntu-24.04"

if ([string]::IsNullOrWhiteSpace($WslRepo)) {
    throw "Set -WslRepo or RAMSHARED_WSL_REPO to the repository path inside the selected distro."
}
if ($Distro -cne $SealedDistro) {
    throw "Distro does not match the sealed target"
}

if ($PressureAllocGiB -eq 0) {
    # Allocate enough to exceed the 1200 MiB cgroup resident ceiling and fill
    # the configured zram/VRAM tiers, without the old fixed 6.5 GiB overdrive.
    $PressureAllocGiB = [Math]::Round(
        [Math]::Max(1.5, ($VramMiB + $ZramMiB + 1712) / 1024.0), 2
    )
}
$minimumOuterTimeoutSec = (($WatchdogSec + $ActionCleanupGraceSec) * $Rounds) + 60
if ($OuterTimeoutSec -lt $minimumOuterTimeoutSec) {
    throw "OuterTimeoutSec must be at least $minimumOuterTimeoutSec seconds for the configured rounds and cleanup grace."
}

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

function Get-CampaignDriveLetterFromPath {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$Name)
    $expanded = [Environment]::ExpandEnvironmentVariables($Path.Trim())
    if ($expanded -notmatch '^([A-Za-z]):[\\/]') {
        throw "$Name is not on a local drive-letter volume"
    }
    return ($Matches[1].ToUpperInvariant() + ":")
}

function Resolve-CampaignHostDiskLetters {
    param(
        [Parameter(Mandatory = $true)][string]$SelectedDistro,
        [string[]]$AdditionalLetters = @(),
        [AllowNull()][object[]]$DistroRecords = $null,
        [AllowNull()][object]$OriginManifest = $null,
        [string]$ManifestPath = "C:\ProgramData\RamShared\ramshared-origin-manifest.json"
    )
    if ($null -eq $OriginManifest) {
        if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
            throw "sealed origin manifest is unavailable for disk discovery"
        }
        try { $OriginManifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json -ErrorAction Stop }
        catch { throw "sealed origin manifest is malformed for disk discovery" }
    }
    if ([int]$OriginManifest.schema_version -ne 3 -or
        [string]::IsNullOrWhiteSpace([string]$OriginManifest.origin_vhdx)) {
        throw "sealed origin manifest lacks an exact origin volume"
    }

    if ($null -eq $DistroRecords) {
        $DistroRecords = @()
        $registry = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss"
        if (Test-Path -LiteralPath $registry) {
            foreach ($entry in Get-ChildItem -LiteralPath $registry -ErrorAction Stop) {
                $DistroRecords += Get-ItemProperty -LiteralPath $entry.PSPath -ErrorAction Stop
            }
        }
    }
    $matches = @($DistroRecords | Where-Object {
        [string]$_.DistributionName -ceq $SelectedDistro -and
        -not [string]::IsNullOrWhiteSpace([string]$_.BasePath)
    })
    if ($matches.Count -ne 1) {
        throw "selected WSL distro storage identity is missing or ambiguous"
    }

    $letters = @(
        Get-CampaignDriveLetterFromPath -Path ([string]$matches[0].BasePath) -Name "WSL distro BasePath"
        Get-CampaignDriveLetterFromPath -Path ([string]$OriginManifest.origin_vhdx) -Name "origin VHDX"
    )
    if (-not [string]::IsNullOrWhiteSpace([string]$OriginManifest.existing_wsl_swap_vhdx)) {
        $letters += Get-CampaignDriveLetterFromPath `
            -Path ([string]$OriginManifest.existing_wsl_swap_vhdx) -Name "WSL swap VHDX"
    }
    $letters += Normalize-HostDiskLetters -Letters $AdditionalLetters
    $resolved = @(Normalize-HostDiskLetters -Letters $letters)
    if ($resolved.Count -lt 1) {
        throw "no host volume was derived from distro and origin identities"
    }
    return $resolved
}

function Stop-CampaignProcessInstanceSafely {
    param(
        [Parameter(Mandatory = $true)][object]$Process,
        [Parameter(Mandatory = $true)][string]$Operation
    )
    # Bind the Process handle before revalidating creation time. Kill() acts on
    # that specific handle rather than reopening a numeric PID after a race.
    try {
        $Process.Refresh()
        if ($Process.HasExited) { return [pscustomobject]@{ stopped = $true; reason = "$Operation`_already_exited" } }
        $boundHandle = $Process.Handle
        $originalStart = $Process.StartTime.ToUniversalTime().Ticks
        $Process.Refresh()
        if ($Process.HasExited) { return [pscustomobject]@{ stopped = $true; reason = "$Operation`_already_exited" } }
        if ($Process.StartTime.ToUniversalTime().Ticks -ne $originalStart) {
            return [pscustomobject]@{ stopped = $false; reason = "$Operation`_process_instance_identity_changed" }
        }
        try {
            $Process.CloseMainWindow() | Out-Null
        } catch {}
        if (-not $Process.WaitForExit(10000)) {
            Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
            if (-not $Process.WaitForExit(5000)) { return [pscustomobject]@{ stopped = $false; reason = "$Operation`_process_instance_kill_unreaped" } }
            return [pscustomobject]@{ stopped = $true; reason = "$Operation`_process_instance_force_terminated" }
        }
        return [pscustomobject]@{ stopped = $true; reason = "$Operation`_process_instance_handle_terminated" }
    } catch {
        return [pscustomobject]@{ stopped = $false; reason = "$Operation`_process_instance_identity_unproven" }
    }
}

function Invoke-BoundedCampaignCimQuery {
    param([Parameter(Mandatory = $true)][string]$Query, [ValidateRange(1, 30)][int]$TimeoutSeconds = 5)
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes('$ErrorActionPreference = "Stop"; ' + $Query))
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = (Join-Path $PSHOME "powershell.exe")
    $info.Arguments = "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded"
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $child = New-Object System.Diagnostics.Process
    $child.StartInfo = $info
    try {
        if (-not $child.Start()) { throw "campaign_cim_child_start_failed" }
        $stdout = $child.StandardOutput.ReadToEndAsync()
        $stderr = $child.StandardError.ReadToEndAsync()
        if (-not $child.WaitForExit($TimeoutSeconds * 1000)) {
            $stopped = Stop-CampaignProcessInstanceSafely -Process $child -Operation "campaign_cim_child"
            if (-not $stopped.stopped -or -not $child.WaitForExit(5000)) { throw "campaign_cim_child_termination_unresolved:$($stopped.reason)" }
            throw "campaign_cim_query_deadline_exceeded"
        }
        if (-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdout, $stderr), 5000)) { throw "campaign_cim_stream_drain_failed" }
        if ($child.ExitCode -ne 0) { throw "campaign_cim_query_failed" }
        try { return ($stdout.Result | ConvertFrom-Json -ErrorAction Stop) }
        catch { throw "campaign_cim_query_output_invalid" }
    } finally { $child.Dispose() }
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
            $disk = Invoke-BoundedCampaignCimQuery -TimeoutSeconds 5 -Query ("Get-CimInstance -ClassName Win32_LogicalDisk -Filter `"DeviceID='{0}'`" -ErrorAction Stop | Select-Object DeviceID, VolumeName, FileSystem, Size, FreeSpace, DriveType | ConvertTo-Json -Compress" -f $letter)
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
    $boundedFunction = ${function:Invoke-BoundedCampaignCimQuery}.ToString()
    return Start-Job -ArgumentList $normalizedCsv, $JsonlPath, $IntervalSec, $boundedFunction -ScriptBlock {
        param($LettersCsv, $OutPath, $Interval, $BoundedFunction)
        $ErrorActionPreference = "Continue"
        Set-Item -Path Function:\script:Invoke-BoundedCampaignCimQuery -Value ([scriptblock]::Create($BoundedFunction))
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
                $perf = @(Invoke-BoundedCampaignCimQuery -TimeoutSeconds 5 -Query 'Get-CimInstance -ClassName Win32_PerfFormattedData_PerfDisk_LogicalDisk -ErrorAction Stop | Select-Object Name, DiskBytesPersec, DiskReadBytesPersec, DiskWriteBytesPersec, AvgDisksecPerRead, AvgDisksecPerWrite, CurrentDiskQueueLength, PercentDiskTime | ConvertTo-Json -Compress')
                foreach ($letter in $Letters) {
                    $row = $perf | Where-Object { $_.Name -eq $letter } | Select-Object -First 1
                    $logical = Invoke-BoundedCampaignCimQuery -TimeoutSeconds 5 -Query ("Get-CimInstance -ClassName Win32_LogicalDisk -Filter `"DeviceID='{0}'`" -ErrorAction Stop | Select-Object Size, FreeSpace | ConvertTo-Json -Compress" -f $letter)
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
        # Windows PowerShell 5.1 emits a JSON array as one pipeline item when
        # ConvertFrom-Json is wrapped directly in @(...). Split assignment from
        # array coercion so each volume remains independently enumerable.
        $decodedVolumes = Get-Content -LiteralPath $VolumePath -Raw | ConvertFrom-Json
        $volumes = @($decodedVolumes)
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

function Write-HostMemorySample {
    param(
        [string]$JsonlPath,
        [Parameter(Mandatory = $true)]$Sample
    )
    ($Sample | ConvertTo-Json -Compress -Depth 5) | Add-Content -Encoding UTF8 -LiteralPath $JsonlPath
}

function Write-BestEffortArtifact {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Content
    )
    try {
        $Content | Set-Content -Encoding UTF8 -LiteralPath $Path -ErrorAction Stop
    } catch {
    }
}

function Stop-HostDiskTelemetryBounded {
    param($Job)
    if ($null -eq $Job) { return [pscustomobject]@{ stopped = $true; reason = "telemetry_not_started" } }
    $telemetry_stop_deadline_started = [System.Diagnostics.Stopwatch]::StartNew()
    $stopController = [powershell]::Create()
    try {
        [void]$stopController.AddScript('param($targetJob) Stop-Job -Job $targetJob -ErrorAction Stop').AddArgument($Job)
        $stopAsync = $stopController.BeginInvoke()
        $remainingMilliseconds = [Math]::Max(0, 5000 - [int]$telemetry_stop_deadline_started.ElapsedMilliseconds)
        if (-not $stopAsync.AsyncWaitHandle.WaitOne($remainingMilliseconds)) {
            try { $stopController.Stop() } catch {}
            return [pscustomobject]@{ stopped = $false; reason = "telemetry_job_stop_deadline_exceeded" }
        }
        $stopController.EndInvoke($stopAsync) | Out-Null
        $remainingMilliseconds = [Math]::Max(0, 5000 - [int]$telemetry_stop_deadline_started.ElapsedMilliseconds)
        if ($remainingMilliseconds -le 0) {
            return [pscustomobject]@{ stopped = $false; reason = "telemetry_job_stop_deadline_exceeded" }
        }
        $settled = Wait-Job -Job $Job -Timeout ([Math]::Max(1, [int][Math]::Ceiling($remainingMilliseconds / 1000.0))) -ErrorAction SilentlyContinue
        $state = [string]$Job.State
        if ($null -eq $settled -and $state -notin @("Completed", "Failed", "Stopped")) {
            return [pscustomobject]@{ stopped = $false; reason = "telemetry_job_stop_deadline_exceeded" }
        }
        if ($state -notin @("Completed", "Failed", "Stopped")) {
            return [pscustomobject]@{ stopped = $false; reason = "telemetry_job_stop_unproven" }
        }
        Remove-Job -Job $Job -Force -ErrorAction Stop
        return [pscustomobject]@{ stopped = $true; reason = "telemetry_job_stopped" }
    } catch {
        return [pscustomobject]@{ stopped = $false; reason = "telemetry_job_stop_failed" }
    } finally {
        $stopController.Dispose()
    }
}

function Stop-OptionalExternalWorkload {
    param($Process)
    if ($null -eq $Process) { return [pscustomobject]@{ stopped = $true; reason = "external_workload_not_started" } }
    try {
        $Process.Refresh()
        if (-not $Process.HasExited) {
            $stopped = Stop-CampaignProcessInstanceSafely -Process $Process -Operation "external_workload"
            if (-not $stopped.stopped -or -not $Process.WaitForExit(5000)) {
                return [pscustomobject]@{ stopped = $false; reason = "external_workload_containment_unproven" }
            }
        }
        $Process.Refresh()
        return [pscustomobject]@{ stopped = [bool]$Process.HasExited; reason = if ($Process.HasExited) { "external_workload_stopped" } else { "external_workload_containment_unproven" } }
    } catch {
        return [pscustomobject]@{ stopped = $false; reason = "external_workload_containment_unproven" }
    }
}

function Stop-LauncherProcessBounded {
    param($Process)
    if ($null -eq $Process) { return [pscustomobject]@{ stopped = $true; reason = "launcher_not_started" } }
    try {
        $Process.Refresh()
        if (-not $Process.HasExited) {
            $stopped = Stop-CampaignProcessInstanceSafely -Process $Process -Operation "launcher"
            if (-not $stopped.stopped -or -not $Process.WaitForExit(5000)) { return [pscustomobject]@{ stopped = $false; reason = "launcher_containment_unproven" } }
        }
        $Process.Refresh()
        return [pscustomobject]@{ stopped = [bool]$Process.HasExited; reason = if ($Process.HasExited) { "launcher_stopped" } else { "launcher_containment_unproven" } }
    } catch { return [pscustomobject]@{ stopped = $false; reason = "launcher_containment_unproven" } }
}

function Invoke-SelectedDistroTermination {
    param(
        [Parameter(Mandatory = $true)][string]$SelectedDistro,
        [Parameter(Mandatory = $true)][ref]$TerminationIssued,
        [Parameter(Mandatory = $true)][string]$Dir
    )
    if ($SelectedDistro -cne $SealedDistro) {
        Write-BestEffortArtifact -Path (Join-Path $Dir "wsl-terminate-unsealed-target.txt") `
            -Content "termination_refused_unsealed_target=$SelectedDistro"
        return [pscustomobject]@{ termination_completed = $false; contained = $false; guest_pressure_stopped = $false; reason = "termination_refused_unsealed_target" }
    }
    if ($TerminationIssued.Value) {
        return [pscustomobject]@{ termination_completed = $false; contained = $false; guest_pressure_stopped = $false; reason = "termination_already_issued" }
    }
    $TerminationIssued.Value = $true
    try {
        $term = Start-Process -FilePath "wsl.exe" -ArgumentList @("--terminate", $SelectedDistro) `
            -PassThru -WindowStyle Hidden -ErrorAction Stop
    } catch {
        Write-BestEffortArtifact -Path (Join-Path $Dir "wsl-terminate-start-failed.txt") `
            -Content ("termination_start_failed: " + $_.Exception.Message)
        return [pscustomobject]@{ termination_completed = $false; contained = $false; guest_pressure_stopped = $false; reason = "termination_start_failed" }
    }
    try {
        if (-not $term.WaitForExit(60000)) {
            Write-BestEffortArtifact -Path (Join-Path $Dir "wsl-terminate-timeout.txt") `
                -Content "wsl --terminate timed out after 60s"
            $stopped = Stop-CampaignProcessInstanceSafely -Process $term -Operation "selected_distro_termination"
            if (-not $term.WaitForExit(5000)) {
                Write-BestEffortArtifact -Path (Join-Path $Dir "wsl-terminate-child-unreaped.txt") `
                    -Content ("wsl --terminate child could not be reaped; " + $stopped.reason)
            }
            return [pscustomobject]@{ termination_completed = $false; contained = $false; guest_pressure_stopped = $false; reason = "termination_deadline_exceeded" }
        }
    } catch {
        Write-BestEffortArtifact -Path (Join-Path $Dir "wsl-terminate-wait-failed.txt") `
            -Content ("termination_wait_failed: " + $_.Exception.Message)
        return [pscustomobject]@{ termination_completed = $false; contained = $false; guest_pressure_stopped = $false; reason = "termination_wait_failed" }
    }
    if ($term.ExitCode -ne 0) {
        Write-BestEffortArtifact -Path (Join-Path $Dir "wsl-terminate-nonzero.txt") `
            -Content ("wsl --terminate exited " + $term.ExitCode)
        return [pscustomobject]@{ termination_completed = $false; contained = $false; guest_pressure_stopped = $false; reason = "termination_nonzero_exit" }
    }
    return [pscustomobject]@{ termination_completed = $true; contained = $false; guest_pressure_stopped = $false; reason = "termination_command_completed" }
}

function Invoke-BoundedWslRunningList {
    param([Parameter(Mandatory = $true)][int]$TimeoutSeconds)
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = "wsl.exe"
    $info.Arguments = "--list --running --quiet"
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $probe = New-Object System.Diagnostics.Process
    $probe.StartInfo = $info
    try {
        if (-not $probe.Start()) { return [pscustomobject]@{ completed = $false; stdout = ""; reason = "running_list_start_failed" } }
        $stdout = $probe.StandardOutput.ReadToEndAsync()
        $stderr = $probe.StandardError.ReadToEndAsync()
        if (-not $probe.WaitForExit($TimeoutSeconds * 1000)) {
            $stopped = Stop-CampaignProcessInstanceSafely -Process $probe -Operation "running_list_probe"
            $probe.WaitForExit(5000) | Out-Null
            return [pscustomobject]@{ completed = $false; stdout = ""; reason = "running_list_deadline_exceeded" }
        }
        if (-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdout, $stderr), 5000)) {
            return [pscustomobject]@{ completed = $false; stdout = ""; reason = "running_list_stream_drain_failed" }
        }
        if ($probe.ExitCode -ne 0) { return [pscustomobject]@{ completed = $false; stdout = ""; reason = "running_list_nonzero_exit" } }
        return [pscustomobject]@{ completed = $true; stdout = [string]$stdout.Result; reason = "complete" }
    } finally {
        $probe.Dispose()
    }
}

function Test-SelectedDistroTerminationContainment {
    param(
        [Parameter(Mandatory = $true)][string]$SelectedDistro,
        [Parameter(Mandatory = $true)][string]$Dir
    )
    $deadline = [System.Diagnostics.Stopwatch]::StartNew()
    $lastProbe = $null
    while ($deadline.Elapsed.TotalSeconds -lt 30) {
        $lastProbe = Invoke-BoundedWslRunningList -TimeoutSeconds 5
        if ($lastProbe.completed) {
            $running = @($lastProbe.stdout -split "`r?`n" | ForEach-Object { $_.Trim() } | Where-Object { $_ })
            if ($running -notcontains $SelectedDistro) {
                $proof = [ordered]@{ selected_distro = $SelectedDistro; contained = $true; guest_pressure_stopped = $true; running_distros = $running; reason = "selected_distro_absent" }
                $proof | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $Dir "wsl-termination-containment.json") -Encoding UTF8
                return [pscustomobject]$proof
            }
        }
        Start-Sleep -Seconds 1
    }
    $proof = [ordered]@{ selected_distro = $SelectedDistro; contained = $false; guest_pressure_stopped = $false; running_distros = if ($null -eq $lastProbe) { @() } else { @($lastProbe.stdout -split "`r?`n" | ForEach-Object { $_.Trim() } | Where-Object { $_ }) }; reason = if ($null -eq $lastProbe) { "containment_probe_missing" } else { $lastProbe.reason } }
    $proof | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $Dir "wsl-termination-containment.json") -Encoding UTF8
    return [pscustomobject]$proof
}

function Invoke-PostLaunchCleanup {
    param(
        $LauncherProcess,
        $ExternalProcess,
        $DiskTelemetryJob,
        [bool]$TerminateSelectedDistro,
        [Parameter(Mandatory = $true)][string]$SelectedDistro,
        [Parameter(Mandatory = $true)][ref]$TerminationIssued,
        [Parameter(Mandatory = $true)][string]$Dir
    )
    $externalContainment = Stop-OptionalExternalWorkload -Process $ExternalProcess
    $launcherContainment = Stop-LauncherProcessBounded -Process $LauncherProcess
    $telemetryContainment = Stop-HostDiskTelemetryBounded -Job $DiskTelemetryJob
    $termination = [pscustomobject]@{ termination_completed = $true; contained = $true; guest_pressure_stopped = $true; reason = "termination_not_required" }
    if ($TerminateSelectedDistro) {
        $termination = Invoke-SelectedDistroTermination -SelectedDistro $SelectedDistro `
            -TerminationIssued $TerminationIssued -Dir $Dir
        # A timeout, nonzero, or failed terminate request proves nothing about
        # the guest. Probe every outcome; unknown containment remains NO_GO.
        $containment = Test-SelectedDistroTerminationContainment -SelectedDistro $SelectedDistro -Dir $Dir
        $termination.contained = [bool]$containment.contained
        $termination.guest_pressure_stopped = [bool]$containment.guest_pressure_stopped
        if (-not $termination.termination_completed -and -not $termination.contained) {
            $termination.reason = "termination_containment_required_after_command_failure"
        } else { $termination.reason = $containment.reason }
    }
    $cleanupProven = [bool]($externalContainment.stopped -and $launcherContainment.stopped -and $telemetryContainment.stopped -and $termination.contained -and $termination.guest_pressure_stopped)
    return [pscustomobject]@{ cleanup_proven = $cleanupProven; termination = $termination; external = $externalContainment; launcher = $launcherContainment; telemetry = $telemetryContainment }
}

function Get-CampaignCleanupFailureReason {
    param([Parameter(Mandatory = $true)][object]$Cleanup)
    if (-not $Cleanup.external.stopped) { return "external_workload_containment_unproven" }
    if (-not $Cleanup.launcher.stopped) { return "launcher_containment_unproven" }
    if (-not $Cleanup.telemetry.stopped) { return "telemetry_job_stop_unproven" }
    return "targeted_termination_unproven"
}

function Invoke-CampaignFinalization {
    param(
        [Parameter(Mandatory = $true)][object]$InitialCleanup,
        $LauncherProcess,
        $ExternalProcess,
        $DiskTelemetryJob,
        [Parameter(Mandatory = $true)][string]$SelectedDistro,
        [Parameter(Mandatory = $true)][ref]$TerminationIssued,
        [Parameter(Mandatory = $true)][string]$Dir
    )
    if ($InitialCleanup.cleanup_proven) {
        return [pscustomobject]@{ cleanup = $InitialCleanup; reason = $null; containment_requested = $false }
    }
    # Set terminal reason and containment intent before the second cleanup.
    # The selected-distro absence/worker proof is mandatory before an outcome
    # can be emitted for any incomplete normal cleanup.
    $reason = Get-CampaignCleanupFailureReason -Cleanup $InitialCleanup
    $finalCleanup = Invoke-PostLaunchCleanup -LauncherProcess $LauncherProcess -ExternalProcess $ExternalProcess `
        -DiskTelemetryJob $DiskTelemetryJob -TerminateSelectedDistro $true `
        -SelectedDistro $SelectedDistro -TerminationIssued $TerminationIssued -Dir $Dir
    if (-not $finalCleanup.cleanup_proven) {
        $reason = Get-CampaignCleanupFailureReason -Cleanup $finalCleanup
    }
    return [pscustomobject]@{ cleanup = $finalCleanup; reason = $reason; containment_requested = $true }
}

function Write-BestEffortSummary {
    param(
        [string]$Dir,
        [string]$Status,
        [string]$Reason,
        [hashtable]$Extra = @{}
    )
    try {
        Write-Summary -Dir $Dir -Status $Status -Reason $Reason -Extra $Extra
    } catch {
    }
}

function Test-HostMemoryTelemetryArtifacts {
    param(
        [Parameter(Mandatory = $true)][string]$JsonlPath,
        [Parameter(Mandatory = $true)][int]$RuntimeSampleCount
    )
    if ($RuntimeSampleCount -lt 1 -or -not (Test-Path -LiteralPath $JsonlPath)) {
        return $false
    }
    try {
        $rows = @(Get-Content -LiteralPath $JsonlPath | Where-Object {
            -not [string]::IsNullOrWhiteSpace($_)
        } | ForEach-Object { $_ | ConvertFrom-Json })
        return $rows.Count -ge (3 + $RuntimeSampleCount)
    } catch {
        return $false
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
        EXTERNAL_WORKLOAD_MIB = $ExternalWorkloadMiB
        POST_CAMPAIGN_OBSERVE_SEC = $PostCampaignObserveSec
        HOST_DISK_LETTERS = @(Normalize-HostDiskLetters -Letters $HostDiskLetters)
        host_memory_gate_ok = [bool]$hostMemoryGateOk
        host_commit_headroom_mib = $hostCommitHeadroomMiB
        host_commit_required_mib = $hostCommitRequiredMiB
        host_commit_reserve_mib = $HostCommitReserveMiB
        host_memory_guardian_fired = [bool]$hostMemoryGuardianFired
        host_memory_telemetry_ok = [bool]$hostMemoryTelemetryOk
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
try {
    $HostDiskLetters = @(Resolve-CampaignHostDiskLetters -SelectedDistro $Distro `
        -AdditionalLetters $HostDiskLetters)
} catch {
    Write-Summary -Dir $artifactDir -Status "REFUSED" -Reason $_.Exception.Message
    exit 2
}
$hostMemoryJsonl = Join-Path $artifactDir "host-memory.jsonl"
$hostMemoryAdmissionPath = Join-Path $artifactDir "host-memory-admission.json"
$hostCommitRequiredMiB = Get-SharedWslHostCommitRequiredMiB `
    -PressureAllocGiB $PressureAllocGiB -HostCommitReserveMiB $HostCommitReserveMiB
$admissionSamples = @()
for ($sampleIndex = 0; $sampleIndex -lt 3; $sampleIndex++) {
    $sample = Get-SharedWslHostMemorySample
    $admissionSamples += $sample
    Write-HostMemorySample -JsonlPath $hostMemoryJsonl -Sample $sample
    if ($sampleIndex -lt 2) {
        Start-Sleep -Seconds 1
    }
}
$hostMemoryAdmission = Test-SharedWslHostMemoryAdmission -Samples $admissionSamples `
    -RequiredMiB $hostCommitRequiredMiB
$hostMemoryGateOk = [bool]$hostMemoryAdmission.ok
$hostCommitHeadroomMiB = $hostMemoryAdmission.commit_headroom_mib
[ordered]@{
    host_memory_gate_ok = [bool]$hostMemoryAdmission.ok
    reason = $hostMemoryAdmission.reason
    host_commit_headroom_mib = $hostMemoryAdmission.commit_headroom_mib
    host_commit_required_mib = $hostCommitRequiredMiB
    host_commit_reserve_mib = $HostCommitReserveMiB
    samples = @($admissionSamples)
} | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 -LiteralPath $hostMemoryAdmissionPath
if (-not $hostMemoryAdmission.ok) {
    Write-Summary -Dir $artifactDir -Status "REFUSED" -Reason $hostMemoryAdmission.reason -Extra @{
        host_memory_gate_ok = $false
        host_commit_headroom_mib = $hostMemoryAdmission.commit_headroom_mib
        host_commit_required_mib = $hostCommitRequiredMiB
        host_commit_reserve_mib = $HostCommitReserveMiB
        host_memory_guardian_fired = $false
    }
    exit 2
}

$artifactWsl = Convert-ToWslPath -Path $artifactDir
$stdout = Join-Path $artifactDir "wsl-campaign.out"
$stderr = Join-Path $artifactDir "wsl-campaign.err"
$guestScriptWin = Join-Path $artifactDir "run-shared-wsl-pressure.sh"
$guestScriptWsl = Convert-ToWslPath -Path $guestScriptWin
$hostDiskJsonl = Join-Path $artifactDir "host-disk-logical.jsonl"
$hostDiskVolumes = Join-Path $artifactDir "host-disk-volumes.json"
$hostDiskJob = $null
$proc = $null
$externalProc = $null
$terminationIssued = $false
$postLaunchReason = $null
$postLaunchCleanupComplete = $false
$terminationContainment = $null
$telemetry_lifecycle_finally_armed_before_start = $true

try {
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
sudo -n env RAMSHARED_TRACE_PROBE=1 ./target/release/ramshared up --vram "$VramMiB" --zram "$ZramMiB" --daemon "`$daemon_wrapper" >"`$artifact/ramshared-up.out" 2>"`$artifact/ramshared-up.err"
./scripts/safety/cascade-health.sh --once >"`$artifact/after-up-health.json"

export RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_WSL_TERMINATION
export RAMSHARED_WINDOWS_WATCHDOG_ARMED=1
export RAMSHARED_FREEZE_WATCHDOG_SEC="$WatchdogSec"
export RAMSHARED_ACTION_CLEANUP_GRACE_SEC="$ActionCleanupGraceSec"
export RAMSHARED_PRESSURE_ALLOC_GIB="$PressureAllocGiB"
export RAMSHARED_PRESSURE_MEM_MAX=1200M
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
$externalLaunchScheduled = $false
$externalLaunchAfterSec = $null
$externalDeadlineSec = $null
$hostMemoryGuardianReason = $null
$hostMemoryInvalidSampleCount = 0
$hostMemoryRuntimeSampleCount = 0
$hostMemoryTelemetryOk = $true
while ($true) {
    $proc.Refresh()
    if ($proc.HasExited) {
        break
    }

    $runtimeSample = Get-SharedWslHostMemorySample
    try {
        Write-HostMemorySample -JsonlPath $hostMemoryJsonl -Sample $runtimeSample
        $hostMemoryRuntimeSampleCount++
    } catch {
        $hostMemoryTelemetryOk = $false
        throw "host_memory_telemetry_write_failed"
    }
    $runtimeGuard = Test-SharedWslHostMemoryGuardian -Sample $runtimeSample `
        -HostCommitReserveMiB $HostCommitReserveMiB `
        -InvalidSampleCount $hostMemoryInvalidSampleCount
    $hostMemoryInvalidSampleCount = $runtimeGuard.invalid_sample_count
    if ($null -ne $runtimeGuard.commit_headroom_mib) {
        $hostCommitHeadroomMiB = [int]$runtimeGuard.commit_headroom_mib
    }
    if ($runtimeGuard.trip) {
        $hostMemoryGuardianFired = $true
        $hostMemoryGuardianReason = $runtimeGuard.reason
        $postLaunchReason = $hostMemoryGuardianReason
        break
    }

    if ($campaignStopwatch.Elapsed.TotalSeconds -ge $OuterTimeoutSec) {
        $postLaunchReason = "outer_watchdog_fired"
        break
    }

    if ($ExternalWorkloadMiB -gt 0 -and -not $externalLaunchScheduled -and
        (Test-Path -LiteralPath $externalReady)) {
        $externalLaunchScheduled = $true
        $externalLaunchAfterSec = $campaignStopwatch.Elapsed.TotalSeconds + $ExternalWorkloadDelaySec
    }
    if ($ExternalWorkloadMiB -gt 0 -and $externalLaunchScheduled -and
        $null -eq $externalProc -and $campaignStopwatch.Elapsed.TotalSeconds -ge $externalLaunchAfterSec) {
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
        $externalDeadlineSec = $campaignStopwatch.Elapsed.TotalSeconds + $ExternalWorkloadHoldSec + 20
    }
    if ($null -ne $externalProc) {
        $externalProc.Refresh()
        if (-not $externalProc.HasExited -and $campaignStopwatch.Elapsed.TotalSeconds -ge $externalDeadlineSec) {
            Stop-OptionalExternalWorkload -Process $externalProc
            $externalProc.Refresh()
        }
        if ($externalProc.HasExited -and -not (Test-Path -LiteralPath $externalComplete)) {
            $externalExitCode = [int]$externalProc.ExitCode
            New-Item -ItemType File -Force -Path $externalComplete | Out-Null
        }
    }

    Start-Sleep -Seconds 1
}

if ($null -ne $postLaunchReason) {
    throw $postLaunchReason
}

$proc.Refresh()
$normalCleanup = Invoke-PostLaunchCleanup -LauncherProcess $proc -ExternalProcess $externalProc `
    -DiskTelemetryJob $hostDiskJob -TerminateSelectedDistro $false `
    -SelectedDistro $Distro -TerminationIssued ([ref]$terminationIssued) -Dir $artifactDir
$normalFinalization = Invoke-CampaignFinalization -InitialCleanup $normalCleanup -LauncherProcess $proc `
    -ExternalProcess $externalProc -DiskTelemetryJob $hostDiskJob -SelectedDistro $Distro `
    -TerminationIssued ([ref]$terminationIssued) -Dir $artifactDir
$postLaunchCleanupComplete = $true
$terminationContainment = $normalFinalization.cleanup.termination
if ($null -ne $normalFinalization.reason) { $postLaunchReason = $normalFinalization.reason }
$exitCode = if ($proc.HasExited) { [int]$proc.ExitCode } else { $null }
$hostMemoryTelemetryOk = Test-HostMemoryTelemetryArtifacts -JsonlPath $hostMemoryJsonl `
    -RuntimeSampleCount $hostMemoryRuntimeSampleCount
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
    $finalClean -and
    $hostMemoryTelemetryOk
$matrixRowClose = $validationPass -and $external_demote_ok
$campaignPass = -not $watchdogFired -and
    $exitCode -eq 0 -and
    $externalWorkloadOk -and
    $validationPass -and
    $finalClean -and
    $host_disk_telemetry_ok -and
    $hostMemoryTelemetryOk
} catch {
    if ($null -eq $postLaunchReason) {
        $postLaunchReason = if (-not $hostMemoryTelemetryOk) {
            "host_memory_telemetry_stale"
        } else {
            "post_launch_cleanup_required"
        }
    }
} finally {
    if (-not $postLaunchCleanupComplete) {
        $finalCleanup = Invoke-PostLaunchCleanup -LauncherProcess $proc -ExternalProcess $externalProc `
            -DiskTelemetryJob $hostDiskJob -TerminateSelectedDistro ($null -ne $postLaunchReason) `
            -SelectedDistro $Distro -TerminationIssued ([ref]$terminationIssued) -Dir $artifactDir
        $terminationContainment = $finalCleanup.termination
        if (-not $finalCleanup.cleanup_proven -and $null -eq $postLaunchReason) {
            $postLaunchReason = if (-not $finalCleanup.external.stopped) { "external_workload_containment_unproven" } elseif (-not $finalCleanup.launcher.stopped) { "launcher_containment_unproven" } elseif (-not $finalCleanup.telemetry.stopped) { "telemetry_job_stop_unproven" } else { "targeted_termination_unproven" }
        }
    }
}

if ($terminationIssued -and ($null -eq $terminationContainment -or
        -not [bool]$terminationContainment.contained -or
        -not [bool]$terminationContainment.guest_pressure_stopped)) {
    $postLaunchReason = "targeted_termination_unproven"
}

if ($null -ne $postLaunchReason) {
    if ($postLaunchReason -eq "outer_watchdog_fired") {
        Write-BestEffortArtifact -Path (Join-Path $artifactDir "windows-watchdog-fired.txt") `
            -Content "outer watchdog fired after ${OuterTimeoutSec}s"
    } elseif ($hostMemoryGuardianFired) {
        Write-BestEffortArtifact -Path (Join-Path $artifactDir "host-memory-guardian-fired.txt") `
            -Content $postLaunchReason
    } elseif ($postLaunchReason -eq "host_memory_telemetry_stale") {
        Write-BestEffortArtifact -Path (Join-Path $artifactDir "host-memory-telemetry-failed.txt") `
            -Content $postLaunchReason
    }
    $terminalStatus = if ($postLaunchReason -in @("targeted_termination_unproven", "external_workload_containment_unproven", "launcher_containment_unproven", "telemetry_job_stop_unproven")) { "NO_GO" } else { "PARTIAL" }
    Write-BestEffortSummary -Dir $artifactDir -Status $terminalStatus -Reason $postLaunchReason -Extra @{
        wsl_exit_code = $null
        host_memory_gate_ok = $true
        host_commit_headroom_mib = $hostCommitHeadroomMiB
        host_commit_required_mib = $hostCommitRequiredMiB
        host_commit_reserve_mib = $HostCommitReserveMiB
        host_memory_guardian_fired = [bool]$hostMemoryGuardianFired
        host_memory_telemetry_ok = [bool]$hostMemoryTelemetryOk
        termination_proven = [bool]($null -ne $terminationContainment -and $terminationContainment.contained)
        guest_pressure_stopped = [bool]($null -ne $terminationContainment -and $terminationContainment.guest_pressure_stopped)
    }
    exit 2
}

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
        host_memory_telemetry_ok = [bool]$hostMemoryTelemetryOk
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
        host_memory_telemetry_ok = [bool]$hostMemoryTelemetryOk
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
    host_memory_telemetry_ok = [bool]$hostMemoryTelemetryOk
    diagnose_ok = [bool]$diagnoseOk
    demote_total = $demoteTotal
    demote_reason = $demoteReason
    final_clean = [bool]$finalClean
    validation_pass = [bool]$validationPass
    freeze_campaign_validated = [bool]$validationPass
    matrix_row_close = [bool]$matrixRowClose
}
exit 2
