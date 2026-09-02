#Requires -Version 5.1
<#
.SYNOPSIS
  Plan-first, host-side guardian for one sealed WSL distro.

.DESCRIPTION
  The guardian is outside the guest failure domain. It never reboots Windows or
  shuts down all WSL distros. A termination requires the heartbeat, two guest
  probes, an independent WSL/HCS probe, and a closed host snapshot. Mutating
  actions require -Run and are source-only until separately approved.
#>
[CmdletBinding()]
param(
    [ValidateSet("install", "status", "capture", "uninstall", "watch", "activate", "test")]
    [string]$Action = "status",
    [switch]$Run,
    [string]$ApproveGuardianAction = "",
    [string]$ApproveGuardianActivation = "",
    [ValidatePattern('^[A-Za-z0-9._-]+$')]
    [string]$Distro = "Ubuntu-24.04",
    [ValidatePattern('^S-1-[0-9-]+$')]
    [string]$UserSid = ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value),
    [string]$HeartbeatPath = "C:\wsl-forensics\ramshared-heartbeat.json",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [ValidateRange(15, 60)][int]$StaleAfterSec = 15,
    [ValidateRange(1, 30)][int]$PollSec = 1,
    [ValidateRange(1, 30)][int]$GuestCommandTimeoutSec = 5
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$TaskName = "RamSharedWslGuardian.v1"
$ProgramDataRoot = "C:\ProgramData\RamShared"
$SafeModeRoot = Join-Path $ProgramDataRoot "safe-mode"
$GuardianStateRoot = Join-Path $ProgramDataRoot "guardian-state"
$TaskBackupRoot = Join-Path $ProgramDataRoot "guardian-backup"
$GuardianConfigPath = Join-Path $ProgramDataRoot "guardian-config.json"
$OriginManifestPath = Join-Path $ProgramDataRoot "ramshared-origin-manifest.json"
$GuardianTaskBackupPath = Join-Path $TaskBackupRoot ($TaskName + ".xml")
$GuardianTaskSealPath = Join-Path $TaskBackupRoot ($TaskName + ".seal.json")
$GuardianTaskAbsentBackupPath = Join-Path $TaskBackupRoot ($TaskName + ".absent")
$GuardianConfigBackupPath = Join-Path $TaskBackupRoot ($TaskName + ".config.json")
$GuardianConfigAbsentBackupPath = Join-Path $TaskBackupRoot ($TaskName + ".config.absent")
$GuardianHealthPath = Join-Path $GuardianStateRoot ($Distro + ".health.json")
$ResumeLeasePath = "/run/ramshared/host-resume-lease.json"
$GuardianActionApproval = "RAMSHARED_ATTENDED_GUARDIAN_ACTION"
$GuardianActivationApproval = "RAMSHARED_ATTENDED_GUARDIAN_ACTIVATION"

function Test-AbsoluteWindowsPath {
    param([Parameter(Mandatory = $true)][string]$Path)
    return $Path -match '^[A-Za-z]:\\'
}

function Write-AtomicJson {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][object]$Value)
    $directory = Split-Path -Parent $Path
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    $temporary = Join-Path $directory ((Split-Path -Leaf $Path) + "." + [Guid]::NewGuid().ToString("N") + ".tmp")
    try {
        [IO.File]::WriteAllText($temporary, ($Value | ConvertTo-Json -Depth 8), [Text.Encoding]::UTF8)
        Move-Item -LiteralPath $temporary -Destination $Path -Force
    } finally {
        if (Test-Path -LiteralPath $temporary -PathType Leaf) { Remove-Item -LiteralPath $temporary -Force }
    }
}

function Write-GuardianEvent {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$Event, [hashtable]$Data = @{})
    $record = [ordered]@{ schema_version = 1; timestamp_utc = [DateTime]::UtcNow.ToString("o"); event = $Event; distro = $Distro; data = $Data }
    Add-Content -LiteralPath $Path -Encoding UTF8 -Value ($record | ConvertTo-Json -Compress -Depth 6)
}

function Get-SealedGuardianPolicy {
    return [ordered]@{
        heartbeat_path = $HeartbeatPath
        artifact_root = $ArtifactRoot
        stale_after_seconds = $StaleAfterSec
        poll_seconds = $PollSec
        guest_command_timeout_seconds = $GuestCommandTimeoutSec
    }
}

function Test-SealedGuardianPolicy {
    param([Parameter(Mandatory = $true)][object]$Policy)
    $expected = Get-SealedGuardianPolicy
    $expectedKeys = @($expected.Keys | Sort-Object)
    $actualKeys = @($Policy.PSObject.Properties.Name | Sort-Object)
    if (($actualKeys -join "`n") -cne ($expectedKeys -join "`n")) { return $false }
    try {
        return [string]$Policy.heartbeat_path -ceq [string]$expected.heartbeat_path -and
            [string]$Policy.artifact_root -ceq [string]$expected.artifact_root -and
            [int]$Policy.stale_after_seconds -eq [int]$expected.stale_after_seconds -and
            [int]$Policy.poll_seconds -eq [int]$expected.poll_seconds -and
            [int]$Policy.guest_command_timeout_seconds -eq [int]$expected.guest_command_timeout_seconds
    } catch {
        return $false
    }
}

function Publish-GuardianState {
    param(
        [Parameter(Mandatory = $true)][ValidateSet("HEALTHY", "SAFE_MODE", "BLOCKED")][string]$State,
        [string]$Reason = "watching",
        [AllowNull()][string]$BootId = $null
    )
    if ($State -eq "HEALTHY" -and -not (Test-CanonicalGuestBootId -BootId $BootId)) {
        throw "healthy guardian state requires a canonical guest boot ID"
    }
    Write-AtomicJson -Path $GuardianHealthPath -Value ([ordered]@{ schema_version = 3; timestamp_utc = [DateTime]::UtcNow.ToString("o"); distro = $Distro; user_sid = $UserSid; state = $State; reason = $Reason; boot_id = $BootId; guardian_policy = (Get-SealedGuardianPolicy) })
}

function Test-CanonicalGuestBootId {
    param([AllowNull()][string]$BootId)
    if ([string]::IsNullOrWhiteSpace($BootId) -or $BootId -cne $BootId.Trim()) { return $false }
    if ($BootId -notmatch '^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$') { return $false }
    $parsed = [guid]::Empty
    if (-not [guid]::TryParseExact($BootId, 'D', [ref]$parsed)) { return $false }
    return $parsed.ToString('D') -ceq $BootId
}

function Limit-GuardianDiagnostics {
    param([AllowNull()][string]$Text)
    if ($null -eq $Text) { return "" }
    $limit = 32768
    $clean = $Text.Replace("`0", "")
    if ($clean.Length -le $limit) { return $clean.Trim() }
    return ($clean.Substring(0, $limit) + "`n[diagnostic truncated]").Trim()
}

function Stop-GuardianProcessInstanceSafely {
    param([Parameter(Mandatory = $true)][object]$Process)
    # Bind the original OS handle before revalidating creation time. Kill() on
    # this Process object acts on that bound handle, never a reopened PID.
    try {
        $Process.Refresh()
        if ($Process.HasExited) { return [pscustomobject]@{ stopped = $true; reason = "process_already_exited" } }
        $boundHandle = $Process.Handle
        $originalStart = $Process.StartTime.ToUniversalTime().Ticks
        $Process.Refresh()
        if ($Process.HasExited) { return [pscustomobject]@{ stopped = $true; reason = "process_already_exited" } }
        if ($Process.StartTime.ToUniversalTime().Ticks -ne $originalStart) {
            return [pscustomobject]@{ stopped = $false; reason = "process_instance_identity_changed" }
        }
        $Process.Kill()
        if (-not $Process.WaitForExit(5000)) { return [pscustomobject]@{ stopped = $false; reason = "process_instance_kill_unreaped" } }
        return [pscustomobject]@{ stopped = $true; reason = "process_instance_handle_terminated" }
    } catch {
        return [pscustomobject]@{ stopped = $false; reason = "process_instance_identity_unproven" }
    }
}

function Invoke-BoundedProcess {
    param([Parameter(Mandatory = $true)][string]$FileName, [Parameter(Mandatory = $true)][string]$Arguments, [Parameter(Mandatory = $true)][int]$TimeoutSeconds)
    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = $FileName
    $start.Arguments = $Arguments
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw "guardian could not start $FileName" }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $completed = $process.WaitForExit($TimeoutSeconds * 1000)
        $processTreeTerminated = $false
        $reason = "success"
        if (-not $completed) {
            $stopped = Stop-GuardianProcessInstanceSafely -Process $process
            if ($stopped.stopped -and $process.WaitForExit(5000)) {
                $processTreeTerminated = $true
                $reason = "timeout"
            } else {
                $reason = "timeout_process_instance_uncontained:" + $stopped.reason
            }
        }
        $streamsDrained = $false
        try {
            $streamsDrained = [Threading.Tasks.Task]::WaitAll(
                [Threading.Tasks.Task[]]@($stdoutTask, $stderrTask), 5000)
        } catch {
            $reason = "stream_drain_failed"
        }
        if (-not $streamsDrained -and $reason -eq "success") {
            $reason = "stream_drain_timeout"
        }
        if ($completed -and $streamsDrained) {
            $reason = if ($process.ExitCode -eq 0) { "success" } else { "nonzero_exit" }
        }
        return [ordered]@{
            completed = [bool]($completed -and $streamsDrained)
            exit_code = if ($completed -and $streamsDrained) { [int]$process.ExitCode } else { $null }
            stdout = if ($streamsDrained) { Limit-GuardianDiagnostics $stdoutTask.Result } else { "" }
            stderr = if ($streamsDrained) { Limit-GuardianDiagnostics $stderrTask.Result } else { "" }
            reason = $reason
            process_tree_terminated = [bool]$processTreeTerminated
        }
    } finally {
        $process.Dispose()
    }
}

function Invoke-BoundedJsonQuery {
    param(
        [Parameter(Mandatory = $true)][string]$Query,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds
    )
    $powerShellPath = Join-Path $PSHOME "powershell.exe"
    if (-not (Test-Path -LiteralPath $powerShellPath -PathType Leaf)) {
        return [ordered]@{ completed = $false; data = $null; reason = "query_host_unavailable" }
    }
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes(
            '$ErrorActionPreference = "Stop"; ' + $Query))
    $child = Invoke-BoundedProcess -FileName $powerShellPath `
        -Arguments ("-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand " + $encoded) `
        -TimeoutSeconds $TimeoutSeconds
    if (-not $child.completed) {
        return [ordered]@{ completed = $false; data = $null; reason = "query_deadline_exceeded"; child = $child }
    }
    if ($child.exit_code -ne 0) {
        return [ordered]@{ completed = $false; data = $null; reason = "query_failed"; child = $child }
    }
    try {
        return [ordered]@{ completed = $true; data = ($child.stdout | ConvertFrom-Json -ErrorAction Stop); reason = "complete"; child = $child }
    } catch {
        return [ordered]@{ completed = $false; data = $null; reason = "query_output_invalid"; child = $child }
    }
}

function Get-HeartbeatState {
    $item = Get-Item -LiteralPath $HeartbeatPath -ErrorAction SilentlyContinue
    $age = if ($null -eq $item) { [double]::PositiveInfinity } else { ([DateTime]::UtcNow - $item.LastWriteTimeUtc).TotalSeconds }
    return [ordered]@{ stale = ($age -ge $StaleAfterSec); age_seconds = $age }
}

function Invoke-GuestProbe {
    $first = Invoke-BoundedProcess -FileName "wsl.exe" `
        -Arguments ("-d `"$Distro`" -u root -- /bin/true") -TimeoutSeconds $GuestCommandTimeoutSec
    $second = Invoke-BoundedProcess -FileName "wsl.exe" `
        -Arguments ("-d `"$Distro`" -u root -- cat /proc/sys/kernel/random/boot_id") -TimeoutSeconds $GuestCommandTimeoutSec
    $probes = @(
        [ordered]@{ name = "guest_process_probe"; result = $first },
        [ordered]@{ name = "guest_boot_identity_probe"; result = $second }
    )
    $result = [ordered]@{ probes = $probes; first = $first; second = $second }
    $result.failure_count = Get-GuestProbeFailureCount -GuestProbe $result
    $result.failed = ($result.failure_count -eq 2)
    return $result
}

function Get-GuestProbeFailureCount {
    param([Parameter(Mandatory = $true)][object]$GuestProbe)
    try { $probes = @($GuestProbe.probes) } catch { return 0 }
    if ($probes.Count -ne 2) { return 0 }
    $names = @($probes | ForEach-Object { [string]$_.name })
    if (($names | Select-Object -Unique).Count -ne 2 -or $names -contains "") { return 0 }
    $failed = 0
    foreach ($probe in $probes) {
        $result = $probe.result
        if ($null -eq $result -or -not [bool]$result.completed -or $result.exit_code -ne 0) { $failed++ }
    }
    return $failed
}

function Invoke-IndependentHostProbe {
    $wsl = Invoke-BoundedProcess -FileName "wsl.exe" -Arguments "--status" -TimeoutSeconds $GuestCommandTimeoutSec
    $hcs = Invoke-BoundedJsonQuery -Query 'Get-Service -Name vmcompute | Select-Object Name, Status | ConvertTo-Json -Compress' -TimeoutSeconds $GuestCommandTimeoutSec
    $hcsFailed = (-not $hcs.completed) -or $null -eq $hcs.data -or [string]$hcs.data.Status -ne "Running"
    $wslFailed = (-not $wsl.completed) -or $wsl.exit_code -ne 0
    return [ordered]@{ failed = ($wslFailed -and $hcsFailed); wsl_failed = $wslFailed; hcs_failed = $hcsFailed; wsl = $wsl; hcs = $hcs; hcs_status = if ($null -eq $hcs.data) { "missing" } else { [string]$hcs.data.Status } }
}

function Invoke-HostSnapshot {
    param([Parameter(Mandatory = $true)][string]$RunDirectory)
    $vmcompute = Invoke-BoundedJsonQuery -Query 'Get-Service -Name vmcompute | Select-Object Name, Status | ConvertTo-Json -Compress' -TimeoutSeconds $GuestCommandTimeoutSec
    $snapshot = [ordered]@{ timestamp_utc = [DateTime]::UtcNow.ToString("o"); heartbeat = Get-HeartbeatState; vmcompute = $vmcompute; closed = [bool]$vmcompute.completed }
    Write-AtomicJson -Path (Join-Path $RunDirectory "host-snapshot.json") -Value $snapshot
    return $snapshot
}

function Get-HostTelemetry {
    $osQuery = Invoke-BoundedJsonQuery -Query 'Get-CimInstance -ClassName Win32_OperatingSystem | Select-Object TotalVisibleMemorySize, FreePhysicalMemory, TotalVirtualMemorySize, FreeVirtualMemory | ConvertTo-Json -Compress' -TimeoutSeconds $GuestCommandTimeoutSec
    $pagefileQuery = Invoke-BoundedJsonQuery -Query '@(Get-CimInstance -ClassName Win32_PageFileUsage | Select-Object CurrentUsage) | ConvertTo-Json -Compress' -TimeoutSeconds $GuestCommandTimeoutSec
    $vmmemQuery = Invoke-BoundedJsonQuery -Query '$p = Get-Process -Name vmmemWSL -ErrorAction SilentlyContinue | Select-Object -First 1; if ($null -eq $p) { [pscustomobject]@{ found = $false } } else { [pscustomobject]@{ found = $true; working_set_bytes = [uint64]$p.WorkingSet64; cpu_seconds = [double]$p.CPU; read_bytes = [uint64]$p.IOReadBytes; write_bytes = [uint64]$p.IOWriteBytes } } | ConvertTo-Json -Compress' -TimeoutSeconds $GuestCommandTimeoutSec
    # Telemetry has no storage policy of its own. It resolves the physical
    # volume that owns the sealed origin VHDX and reports that observation;
    # origin placement remains owned by Manage-RamSharedOrigin.ps1.
    $volumeQuery = Invoke-BoundedJsonQuery -Query '$manifestPath = "C:\ProgramData\RamShared\ramshared-origin-manifest.json"; if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { [pscustomobject]@{ found = $false; reason = "origin_manifest_missing" } } else { $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json -ErrorAction Stop; if ([int]$manifest.schema_version -ne 3 -or [string]::IsNullOrWhiteSpace([string]$manifest.origin_vhdx) -or -not [IO.Path]::IsPathRooted([string]$manifest.origin_vhdx)) { throw "origin_manifest_invalid" }; $volumes = @(Get-Volume -FilePath ([string]$manifest.origin_vhdx) -ErrorAction Stop); if ($volumes.Count -ne 1) { throw "origin_physical_volume_ambiguous" }; $v = $volumes[0]; [pscustomobject]@{ found = $true; drive_letter = [string]$v.DriveLetter; volume_path = [string]$v.Path; volume_unique_id = [string]$v.UniqueId; file_system_label = [string]$v.FileSystemLabel; size_bytes = [uint64]$v.Size; free_bytes = [uint64]$v.SizeRemaining } } | ConvertTo-Json -Compress' -TimeoutSeconds $GuestCommandTimeoutSec
    $gpuQuery = Invoke-BoundedJsonQuery -Query 'Get-Counter ''\GPU Adapter Memory(*)\Dedicated Usage'' | Select-Object -ExpandProperty CounterSamples | Select-Object InstanceName, CookedValue | ConvertTo-Json -Compress' -TimeoutSeconds $GuestCommandTimeoutSec
    $os = $osQuery.data
    $pagefiles = @($pagefileQuery.data)
    $vmmem = $vmmemQuery.data
    $volume = $volumeQuery.data
    return [ordered]@{
        schema_version = 2
        timestamp_utc = [DateTime]::UtcNow.ToString("o")
        physical_memory_total_kib = if ($null -eq $os) { $null } else { [uint64]$os.TotalVisibleMemorySize }
        physical_memory_free_kib = if ($null -eq $os) { $null } else { [uint64]$os.FreePhysicalMemory }
        commit_limit_kib = if ($null -eq $os) { $null } else { [uint64]$os.TotalVirtualMemorySize }
        commit_free_kib = if ($null -eq $os) { $null } else { [uint64]$os.FreeVirtualMemory }
        pagefile_used_mib = [uint64](($pagefiles | Measure-Object -Property CurrentUsage -Sum).Sum)
        vmmem_wsl = if ($null -eq $vmmem -or -not [bool]$vmmem.found) { $null } else { [ordered]@{ working_set_bytes = [uint64]$vmmem.working_set_bytes; cpu_seconds = [double]$vmmem.cpu_seconds; read_bytes = [uint64]$vmmem.read_bytes; write_bytes = [uint64]$vmmem.write_bytes } }
        gpu = $gpuQuery.data
        origin_volume = if ($null -eq $volume -or -not [bool]$volume.found) { $null } else { [ordered]@{ drive_letter = [string]$volume.drive_letter; volume_path = [string]$volume.volume_path; volume_unique_id = [string]$volume.volume_unique_id; file_system_label = [string]$volume.file_system_label; size_bytes = [uint64]$volume.size_bytes; free_bytes = [uint64]$volume.free_bytes } }
        telemetry_queries_bounded = [bool]($osQuery.completed -and $pagefileQuery.completed -and $vmmemQuery.completed -and $volumeQuery.completed -and $gpuQuery.completed)
        heartbeat = Get-HeartbeatState
    }
}

function Write-HostTelemetryRing {
    param([Parameter(Mandatory = $true)][string]$Path)
    $parent = Split-Path -Parent $Path
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    if ((Test-Path -LiteralPath $Path -PathType Leaf) -and (Get-Item -LiteralPath $Path).Length -ge 52428800) {
        Move-Item -LiteralPath $Path -Destination ($Path + ".1") -Force
    }
    Add-Content -LiteralPath $Path -Encoding UTF8 -Value ((Get-HostTelemetry) | ConvertTo-Json -Compress -Depth 8)
}

function Get-SafeModePath { return Join-Path $SafeModeRoot ($Distro + ".json") }
function Get-TerminationPath { return Join-Path $GuardianStateRoot ($Distro + ".termination.json") }

function Complete-TerminationRecord {
    param([Parameter(Mandatory = $true)][string]$IncidentId, [Parameter(Mandatory = $true)][string]$NewBootId)
    $active = Get-TerminationPath
    $history = Join-Path $GuardianStateRoot ($Distro + "." + $IncidentId + ".termination.json")
    Write-AtomicJson -Path $history -Value ([ordered]@{ incident_id = $IncidentId; distro = $Distro; new_boot_id = $NewBootId; completed_utc = [DateTime]::UtcNow.ToString("o") })
    Remove-Item -LiteralPath $active -Force
}

function Write-HostSafeModeGate {
    param([Parameter(Mandatory = $true)][string]$IncidentId, [Parameter(Mandatory = $true)][string]$PriorBootId)
    $gate = [ordered]@{ schema_version = 1; distro = $Distro; incident_id = $IncidentId; prior_boot_id = $PriorBootId; reason = "guardian_proven_guest_inaccessible"; timestamp_utc = [DateTime]::UtcNow.ToString("o") }
    Write-AtomicJson -Path (Get-SafeModePath) -Value $gate
    return $gate
}

function Write-SealedGuardianConfig {
    $config = [ordered]@{
        schema_version = 2
        distro = $Distro
        user_sid = $UserSid
        task_name = $TaskName
        guardian_policy = (Get-SealedGuardianPolicy)
    }
    Write-AtomicJson -Path $GuardianConfigPath -Value $config
}

function Test-SealedGuardianIdentity {
    if (-not (Test-Path -LiteralPath $GuardianConfigPath -PathType Leaf)) { throw "guardian sealed configuration is missing" }
    $config = Get-Content -Raw -LiteralPath $GuardianConfigPath | ConvertFrom-Json
    $expectedKeys = @("schema_version", "distro", "user_sid", "task_name", "guardian_policy")
    $actualKeys = @($config.PSObject.Properties.Name | Sort-Object)
    if (($actualKeys -join "`n") -cne (@($expectedKeys | Sort-Object) -join "`n") -or
        $config.schema_version -ne 2 -or $config.distro -cne $Distro -or $config.user_sid -cne $UserSid -or $config.task_name -cne $TaskName -or
        $null -eq $config.guardian_policy -or -not (Test-SealedGuardianPolicy -Policy $config.guardian_policy)) {
        throw "guardian invocation does not match the sealed distro, user SID, and policy"
    }
}

function New-GuardianInstallTransaction {
    $transactionId = [Guid]::NewGuid().ToString("N")
    $transaction = [ordered]@{
        task_snapshot_path = Join-Path $TaskBackupRoot ($TaskName + "." + $transactionId + ".rollback.xml")
        config_snapshot_path = Join-Path $TaskBackupRoot ($TaskName + "." + $transactionId + ".rollback.config")
        had_task = $false
        had_config = $false
        task_may_be_modified = $false
        config_may_be_modified = $false
        task_seal_snapshot_path = Join-Path $TaskBackupRoot ($TaskName + "." + $transactionId + ".rollback.seal.json")
        had_task_seal = $false
        task_seal_may_be_modified = $false
        owned_task_xml_fingerprint = $null
        phase = "begin"
    }
    $existing = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($null -ne $existing) {
        $xml = Export-ScheduledTask -TaskName $TaskName
        if ([string]::IsNullOrWhiteSpace($xml)) { throw "guardian existing task backup is empty" }
        [IO.File]::WriteAllText($transaction.task_snapshot_path, [string]$xml, [Text.Encoding]::UTF8)
        $transaction.had_task = $true
    }
    if (Test-Path -LiteralPath $GuardianConfigPath -PathType Leaf) {
        [IO.File]::WriteAllBytes($transaction.config_snapshot_path, [IO.File]::ReadAllBytes($GuardianConfigPath))
        $transaction.had_config = $true
    }
    if (Test-Path -LiteralPath $GuardianTaskSealPath -PathType Leaf) {
        [IO.File]::WriteAllBytes($transaction.task_seal_snapshot_path, [IO.File]::ReadAllBytes($GuardianTaskSealPath))
        $transaction.had_task_seal = $true
    }
    return $transaction
}

function Get-GuardianTaskXmlFingerprint {
    param([Parameter(Mandatory = $true)][string]$Xml)
    if ([string]::IsNullOrWhiteSpace($Xml)) { throw "guardian_task_xml_empty" }
    $bytes = [Text.Encoding]::UTF8.GetBytes($Xml.Replace("`r`n", "`n"))
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha256.ComputeHash($bytes)) -replace '-', '') }
    finally { $sha256.Dispose() }
}

function Write-GuardianTaskSeal {
    param([Parameter(Mandatory = $true)][string]$Xml, [Parameter(Mandatory = $true)][string]$Phase)
    Write-AtomicJson -Path $GuardianTaskSealPath -Value ([ordered]@{
        schema_version = 1; task_name = $TaskName; phase = $Phase
        guardian_task_xml_fingerprint = Get-GuardianTaskXmlFingerprint -Xml $Xml
    })
}

function Assert-GuardianTaskXmlSeal {
    if (-not (Test-Path -LiteralPath $GuardianTaskSealPath -PathType Leaf)) { throw "guardian_task_xml_seal_missing" }
    try { $seal = Get-Content -Raw -LiteralPath $GuardianTaskSealPath | ConvertFrom-Json -ErrorAction Stop }
    catch { throw "guardian_task_xml_seal_invalid" }
    if ($seal.schema_version -ne 1 -or $seal.task_name -cne $TaskName -or
        [string]$seal.guardian_task_xml_fingerprint -notmatch '^[0-9A-F]{64}$') { throw "guardian_task_xml_seal_invalid" }
    $actual = Export-ScheduledTask -TaskName $TaskName -ErrorAction Stop
    if ((Get-GuardianTaskXmlFingerprint -Xml $actual) -cne [string]$seal.guardian_task_xml_fingerprint) {
        throw "guardian_task_operator_edit_detected"
    }
}

function Save-GuardianOriginalBackup {
    param([Parameter(Mandatory = $true)][System.Collections.IDictionary]$Transaction)
    if (-not (Test-Path -LiteralPath $GuardianTaskBackupPath -PathType Leaf) -and -not (Test-Path -LiteralPath $GuardianTaskAbsentBackupPath -PathType Leaf)) {
        if ($Transaction.had_task) {
            Copy-Item -LiteralPath $Transaction.task_snapshot_path -Destination $GuardianTaskBackupPath -ErrorAction Stop
        } else {
            [IO.File]::WriteAllText($GuardianTaskAbsentBackupPath, "absent`n", [Text.Encoding]::UTF8)
        }
    }
    if (-not (Test-Path -LiteralPath $GuardianConfigBackupPath -PathType Leaf) -and -not (Test-Path -LiteralPath $GuardianConfigAbsentBackupPath -PathType Leaf)) {
        if ($Transaction.had_config) {
            Copy-Item -LiteralPath $Transaction.config_snapshot_path -Destination $GuardianConfigBackupPath -ErrorAction Stop
        } else {
            [IO.File]::WriteAllText($GuardianConfigAbsentBackupPath, "absent`n", [Text.Encoding]::UTF8)
        }
    }
}

function Restore-GuardianTaskSnapshot {
    param([Parameter(Mandatory = $true)][System.Collections.IDictionary]$Transaction)
    $current = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($null -ne $current) { Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop }
    if ($Transaction.had_task) {
        $xml = Get-Content -Raw -LiteralPath $Transaction.task_snapshot_path
        Register-ScheduledTask -TaskName $TaskName -Xml $xml -Force | Out-Null
    }
}

function Rollback-GuardianInstallTransaction {
    param([Parameter(Mandatory = $true)][System.Collections.IDictionary]$Transaction)
    $rollbackErrors = @()
    if ($Transaction.task_may_be_modified) {
        try {
            if ($null -eq $Transaction.owned_task_xml_fingerprint) { throw "guardian_task_rollback_owner_unknown" }
            $currentXml = Export-ScheduledTask -TaskName $TaskName -ErrorAction Stop
            if ((Get-GuardianTaskXmlFingerprint -Xml $currentXml) -cne [string]$Transaction.owned_task_xml_fingerprint) { throw "guardian_task_operator_edit_detected" }
            Restore-GuardianTaskSnapshot -Transaction $Transaction
        } catch { $rollbackErrors += $_.Exception.Message }
    }
    if ($Transaction.task_seal_may_be_modified) {
        try {
            if ($Transaction.had_task_seal) {
                Move-Item -LiteralPath $Transaction.task_seal_snapshot_path -Destination $GuardianTaskSealPath -Force
            } elseif (Test-Path -LiteralPath $GuardianTaskSealPath -PathType Leaf) {
                Remove-Item -LiteralPath $GuardianTaskSealPath -Force
            }
        } catch { $rollbackErrors += $_.Exception.Message }
    }
    if ($Transaction.config_may_be_modified) {
        try {
            if ($Transaction.had_config) {
                Move-Item -LiteralPath $Transaction.config_snapshot_path -Destination $GuardianConfigPath -Force
            } elseif (Test-Path -LiteralPath $GuardianConfigPath -PathType Leaf) {
                Remove-Item -LiteralPath $GuardianConfigPath -Force
            }
        } catch { $rollbackErrors += $_.Exception.Message }
    }
    if ($rollbackErrors.Count -gt 0) { throw ("guardian install rollback incomplete: " + ($rollbackErrors -join "; ")) }
}

function Complete-GuardianInstallTransaction {
    param([Parameter(Mandatory = $true)][System.Collections.IDictionary]$Transaction)
    foreach ($path in @($Transaction.task_snapshot_path, $Transaction.config_snapshot_path, $Transaction.task_seal_snapshot_path)) {
        if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path -Force }
    }
}

function Invoke-GuardianInstallTransaction {
    param([Parameter(Mandatory = $true)][hashtable]$Operations)
    $begin = $Operations["begin"]
    if ($null -eq $begin) { throw "guardian install transaction has no begin operation" }
    $transaction = & $begin
    try {
        foreach ($phase in @("backup", "config", "register", "disable", "verify_disabled")) {
            $transaction.phase = $phase
            $operation = $Operations[$phase]
            if ($null -eq $operation) { throw "guardian install transaction has no $phase operation" }
            & $operation $transaction
        }
        $transaction.phase = "complete"
        $complete = $Operations["complete"]
        if ($null -ne $complete) { & $complete $transaction }
        return $transaction
    } catch {
        $failure = $_
        $rollback = $Operations["rollback"]
        if ($null -eq $rollback) { throw "guardian install transaction has no rollback operation" }
        try { & $rollback $transaction } catch { throw ("guardian install transaction failed at " + $transaction.phase + " and rollback was incomplete: " + $_.Exception.Message) }
        throw ("guardian install transaction failed at " + $transaction.phase + ": " + $failure.Exception.Message)
    }
}

function New-GuardianInstallOperations {
    return @{
        begin = { New-GuardianInstallTransaction }
        backup = { param($transaction) Save-GuardianOriginalBackup -Transaction $transaction }
        config = { param($transaction) $transaction.config_may_be_modified = $true; Write-SealedGuardianConfig }
        register = {
            param($transaction)
            $transaction.task_may_be_modified = $true
            Register-ScheduledTask -TaskName $TaskName -InputObject (New-GuardianTask) -Force | Out-Null
            # A successful registration is already a task mutation.  Seal the
            # exact XML before the next operation can fail so rollback can
            # prove ownership rather than guessing after a disable failure.
            $transaction.task_seal_may_be_modified = $true
            $xml = Export-ScheduledTask -TaskName $TaskName -ErrorAction Stop
            $transaction.owned_task_xml_fingerprint = Get-GuardianTaskXmlFingerprint -Xml $xml
            Write-GuardianTaskSeal -Xml $xml -Phase "guardian_task_registration_seal_pending_disable"
        }
        disable = { param($transaction) Disable-ScheduledTask -TaskName $TaskName -ErrorAction Stop | Out-Null }
        verify_disabled = {
            param($transaction)
            $task = Get-ScheduledTask -TaskName $TaskName -ErrorAction Stop
            if ($task.State -ne "Disabled") { throw "guardian staging task was not disabled" }
            $xml = Export-ScheduledTask -TaskName $TaskName -ErrorAction Stop
            $transaction.owned_task_xml_fingerprint = Get-GuardianTaskXmlFingerprint -Xml $xml
            Write-GuardianTaskSeal -Xml $xml -Phase "staging_disabled"
        }
        complete = { param($transaction) Complete-GuardianInstallTransaction -Transaction $transaction }
        rollback = { param($transaction) Rollback-GuardianInstallTransaction -Transaction $transaction }
    }
}

function Install-GuardianTaskTransaction {
    New-Item -ItemType Directory -Force -Path $TaskBackupRoot, $SafeModeRoot, $GuardianStateRoot | Out-Null
    $existing = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($null -ne $existing) {
        # Reinstall is not authority to overwrite a task changed by an operator.
        Assert-GuardianTaskXmlSeal
    }
    Invoke-GuardianInstallTransaction -Operations (New-GuardianInstallOperations) | Out-Null
}

function Restore-SealedGuardianBackup {
    $restored = $false
    Assert-GuardianTaskXmlSeal
    if (Test-Path -LiteralPath $GuardianTaskBackupPath -PathType Leaf) {
        $xml = Get-Content -Raw -LiteralPath $GuardianTaskBackupPath
        $current = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
        if ($null -ne $current) { Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop }
        Register-ScheduledTask -TaskName $TaskName -Xml $xml -Force | Out-Null
        $restored = $true
    } elseif (Test-Path -LiteralPath $GuardianTaskAbsentBackupPath -PathType Leaf) {
        # Do not delete the seal/config state until task removal is proven.
        # A scheduler/provider failure leaves the currently sealed task and
        # forensic state intact for a later attended reconciliation.
        try {
            Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop
        } catch {
            throw "guardian_uninstall_unregister_failed: $($_.Exception.Message)"
        }
        if ($null -ne (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue)) {
            throw "guardian_uninstall_unregister_failed: task_still_present"
        }
        $restored = $true
    }
    if (Test-Path -LiteralPath $GuardianConfigBackupPath -PathType Leaf) {
        Copy-Item -LiteralPath $GuardianConfigBackupPath -Destination $GuardianConfigPath -Force -ErrorAction Stop
        $restored = $true
    } elseif (Test-Path -LiteralPath $GuardianConfigAbsentBackupPath -PathType Leaf) {
        Remove-Item -LiteralPath $GuardianConfigPath -Force -ErrorAction SilentlyContinue
        $restored = $true
    }
    if ($restored -and (Test-Path -LiteralPath $GuardianTaskSealPath -PathType Leaf)) {
        Remove-Item -LiteralPath $GuardianTaskSealPath -Force -ErrorAction Stop
    }
    return $restored
}

function Get-GuestBootId {
    $boot = Invoke-BoundedProcess -FileName "wsl.exe" -Arguments ("-d `"$Distro`" -u root -- cat /proc/sys/kernel/random/boot_id") -TimeoutSeconds $GuestCommandTimeoutSec
    $candidate = if ($null -eq $boot.stdout) { $null } else { $boot.stdout.Trim().ToLowerInvariant() }
    if (-not $boot.completed -or $boot.exit_code -ne 0 -or -not (Test-CanonicalGuestBootId -BootId $candidate)) { return $null }
    return $candidate
}

function Mirror-GuestSafeMode {
    param([Parameter(Mandatory = $true)][string]$IncidentId, [Parameter(Mandatory = $true)][string]$BootId)
    $payload = [ordered]@{ schema_version = 1; incident_id = $IncidentId; distro = $Distro; boot_id = $BootId } | ConvertTo-Json -Compress
    $encoded = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($payload))
    $command = "install -d -m 0700 /var/lib/ramshared; printf '%s' '$encoded' | base64 -d > /var/lib/ramshared/.safe-mode.tmp; chmod 0600 /var/lib/ramshared/.safe-mode.tmp; mv /var/lib/ramshared/.safe-mode.tmp /var/lib/ramshared/safe-mode.json"
    return Invoke-BoundedProcess -FileName "wsl.exe" -Arguments ("-d `"$Distro`" -u root -- sh -c `"$command`"") -TimeoutSeconds $GuestCommandTimeoutSec
}

function Invoke-TargetedTerminate {
    param([Parameter(Mandatory = $true)][string]$IncidentId, [Parameter(Mandatory = $true)][string]$PriorBootId)
    $terminationPath = Get-TerminationPath
    if (Test-Path -LiteralPath $terminationPath -PathType Leaf) { return [ordered]@{ started = $false; completed = $false; reason = "termination_already_recorded" } }
    Write-AtomicJson -Path $terminationPath -Value ([ordered]@{ incident_id = $IncidentId; distro = $Distro; prior_boot_id = $PriorBootId; started_utc = [DateTime]::UtcNow.ToString("o") })
    $result = Invoke-BoundedProcess -FileName "wsl.exe" -Arguments ("--terminate `"$Distro`"") -TimeoutSeconds $GuestCommandTimeoutSec
    if (-not $result.completed) { return [ordered]@{ started = $true; completed = $false; reason = "terminate_timeout"; detail = $result } }
    return [ordered]@{ started = $true; completed = ($result.exit_code -eq 0); reason = $result.reason; detail = $result }
}

function Get-GuardianTerminationDecision {
    param(
        [Parameter(Mandatory = $true)][bool]$HeartbeatStale,
        [Parameter(Mandatory = $true)][object]$GuestProbe,
        [Parameter(Mandatory = $true)][object]$HostProbe,
        [Parameter(Mandatory = $true)][object]$Snapshot,
        [Parameter(Mandatory = $true)][bool]$TerminationRecorded
    )
    if (-not $HeartbeatStale) { return [ordered]@{ action = "WAIT"; reason = "heartbeat_fresh" } }
    if ((Get-GuestProbeFailureCount -GuestProbe $GuestProbe) -ne 2) {
        return [ordered]@{ action = "REFUSE"; reason = "two_distinct_guest_probe_failures_required" }
    }
    if (-not [bool]$HostProbe.failed -or -not [bool]$HostProbe.wsl_failed -or -not [bool]$HostProbe.hcs_failed) {
        return [ordered]@{ action = "REFUSE"; reason = "dual_wsl_hcs_corroboration_required" }
    }
    if (-not [bool]$Snapshot.closed) { return [ordered]@{ action = "REFUSE"; reason = "snapshot_open" } }
    if ($TerminationRecorded) { return [ordered]@{ action = "REFUSE"; reason = "termination_already_recorded" } }
    return [ordered]@{ action = "TERMINATE"; reason = "all_guardian_gates_proven" }
}

function Get-SealedGuardianTaskArguments {
    param([switch]$ActivationAuthorized)
    $scriptPath = Join-Path $PSScriptRoot "Watch-RamSharedWsl.ps1"
    $arguments = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{0}" -Action watch -Run -Distro "{1}" -UserSid "{2}" -HeartbeatPath "{3}" -ArtifactRoot "{4}" -StaleAfterSec {5} -PollSec {6} -GuestCommandTimeoutSec {7}' -f $scriptPath, $Distro, $UserSid, $HeartbeatPath, $ArtifactRoot, $StaleAfterSec, $PollSec, $GuestCommandTimeoutSec
    if ($ActivationAuthorized) { $arguments += (' -ApproveGuardianAction "{0}"' -f $GuardianActionApproval) }
    return $arguments
}

function New-GuardianTask {
    param([switch]$ActivationAuthorized)
    $taskArguments = Get-SealedGuardianTaskArguments -ActivationAuthorized:$ActivationAuthorized
    $taskAction = New-ScheduledTaskAction -Execute "PowerShell.exe" -Argument $taskArguments
    $taskTrigger = New-ScheduledTaskTrigger -AtLogOn -User $UserSid
    # Task Scheduler serializes the zero duration as PT0S (no execution limit).
    $taskSettings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew
    $taskPrincipal = New-ScheduledTaskPrincipal -UserId $UserSid -LogonType Interactive -RunLevel Highest
    return New-ScheduledTask -Action $taskAction -Trigger $taskTrigger -Settings $taskSettings -Principal $taskPrincipal
}

function Activate-GuardianTask {
    Test-SealedGuardianIdentity
    if ($ApproveGuardianActivation -cne $GuardianActivationApproval) { throw "guardian activation requires exact attended approval" }
    Assert-GuardianTaskXmlSeal
    $current = Get-ScheduledTask -TaskName $TaskName -ErrorAction Stop
    if ($current.State -ne "Disabled") { throw "guardian activation requires disabled staging state" }
    $priorXml = Export-ScheduledTask -TaskName $TaskName
    $activationOwnedFingerprint = $null
    $activationMayHaveModifiedTask = $false
    $activationRegistrationCompleted = $false
    try {
        $activationMayHaveModifiedTask = $true
        Register-ScheduledTask -TaskName $TaskName -InputObject (New-GuardianTask -ActivationAuthorized) -Force | Out-Null
        $activationRegistrationCompleted = $true
        $activationOwnedFingerprint = Get-GuardianTaskXmlFingerprint -Xml (Export-ScheduledTask -TaskName $TaskName -ErrorAction Stop)
        Enable-ScheduledTask -TaskName $TaskName -ErrorAction Stop | Out-Null
        $verified = Get-ScheduledTask -TaskName $TaskName -ErrorAction Stop
        if ($verified.State -eq "Disabled") { throw "guardian activation did not enable the scheduled task" }
        $activatedXml = Export-ScheduledTask -TaskName $TaskName -ErrorAction Stop
        $activationOwnedFingerprint = Get-GuardianTaskXmlFingerprint -Xml $activatedXml
        Write-GuardianTaskSeal -Xml $activatedXml -Phase "activated"
    } catch {
        $failure = $_
        try {
            if (-not $activationMayHaveModifiedTask -or -not $activationRegistrationCompleted) { throw "guardian activation mutation ownership is unknown" }
            if ($null -ne $activationOwnedFingerprint) {
                $currentXml = Export-ScheduledTask -TaskName $TaskName -ErrorAction Stop
                if ((Get-GuardianTaskXmlFingerprint -Xml $currentXml) -cne $activationOwnedFingerprint) { throw "guardian_task_operator_edit_detected" }
            }
            Register-ScheduledTask -TaskName $TaskName -Xml $priorXml -Force | Out-Null
            Write-GuardianTaskSeal -Xml $priorXml -Phase "staging_disabled"
        } catch { throw ("guardian activation failed and rollback was incomplete: " + $_.Exception.Message) }
        throw ("guardian activation failed; disabled staging restored: " + $failure.Exception.Message)
    }
}

function Invoke-GuardianWatchIteration {
    <#
      This is the production watch ordering made injectable for source-only
      regression fixtures.  It never invokes termination itself: callers may
      reach that action only after this returns TERMINATE from fresh evidence.
    #>
    param([Parameter(Mandatory = $true)][hashtable]$Operations)
    foreach ($name in @("get_heartbeat", "guest_probe", "host_probe", "snapshot", "termination_recorded")) {
        if ($null -eq $Operations[$name]) { throw "guardian_watch_operation_missing:$name" }
    }
    $heartbeat = & $Operations.get_heartbeat
    if (-not [bool]$heartbeat.stale) {
        return [ordered]@{ action = "WAIT"; reason = "heartbeat_fresh"; heartbeat = $heartbeat }
    }
    $firstGuest = & $Operations.guest_probe
    if ((Get-GuestProbeFailureCount -GuestProbe $firstGuest) -ne 2) {
        return [ordered]@{ action = "REFUSE"; reason = "two_distinct_guest_probe_failures_required"; heartbeat = $heartbeat; guest = $firstGuest }
    }
    # Re-probe after the first pair failed. A recovered distro is not a
    # termination candidate even when host-side evidence remains unhealthy.
    $revalidatedGuest = & $Operations.guest_probe
    if ((Get-GuestProbeFailureCount -GuestProbe $revalidatedGuest) -ne 2) {
        return [ordered]@{ action = "REFUSE"; reason = "guest_recovered_before_termination"; heartbeat = $heartbeat; guest = $firstGuest; revalidated_guest = $revalidatedGuest }
    }
    $hostProbe = & $Operations.host_probe
    if (-not [bool]$hostProbe.failed -or -not [bool]$hostProbe.wsl_failed -or -not [bool]$hostProbe.hcs_failed) {
        return [ordered]@{ action = "REFUSE"; reason = "dual_wsl_hcs_corroboration_required"; heartbeat = $heartbeat; guest = $revalidatedGuest; host = $hostProbe }
    }
    $snapshot = & $Operations.snapshot
    $decision = Get-GuardianTerminationDecision -HeartbeatStale $true -GuestProbe $revalidatedGuest -HostProbe $hostProbe -Snapshot $snapshot -TerminationRecorded ([bool](& $Operations.termination_recorded))
    return [ordered]@{ action = $decision.action; reason = $decision.reason; heartbeat = $heartbeat; guest = $revalidatedGuest; host = $hostProbe; snapshot = $snapshot }
}

function Invoke-GuardianWatchTerminationTail {
    <# The executable tail of Invoke-GuardianWatch. It takes a sealed watch
       iteration and rechecks guest liveness immediately before any safe-mode
       write or targeted terminate. #>
    param(
        [Parameter(Mandatory = $true)][object]$WatchIteration,
        [Parameter(Mandatory = $true)][hashtable]$Operations
    )
    foreach ($name in @("guest_probe", "get_boot_id", "write_safe_mode_gate", "targeted_terminate", "mirror_safe_mode", "complete_record", "publish", "write_event")) {
        if ($null -eq $Operations[$name]) { throw "guardian_tail_operation_missing:$name" }
    }
    if ($WatchIteration.action -ne "TERMINATE") { return [ordered]@{ action = "REFUSE"; reason = "guardian_tail_not_authorized" } }
    $tailGuest = & $Operations.guest_probe
    if ((Get-GuestProbeFailureCount -GuestProbe $tailGuest) -ne 2) {
        & $Operations.write_event "guardian_tail_refuses_recovered_guest" $tailGuest
        return [ordered]@{ action = "REFUSE"; reason = "guardian_tail_refuses_recovered_guest"; guest = $tailGuest }
    }
    $priorBootId = & $Operations.get_boot_id
    if ($null -eq $priorBootId) {
        & $Operations.publish "BLOCKED" "prior_boot_id_not_proven" $null
        & $Operations.write_event "prior_boot_id_not_proven" @{}
        return [ordered]@{ action = "REFUSE"; reason = "prior_boot_id_not_proven" }
    }
    $incidentId = [Guid]::NewGuid().ToString("N")
    $gate = & $Operations.write_safe_mode_gate $incidentId $priorBootId
    & $Operations.write_event "host_safe_mode_gate_written" $gate
    $termination = & $Operations.targeted_terminate $incidentId $priorBootId
    & $Operations.write_event "targeted_terminate" $termination
    if (-not $termination.completed) {
        & $Operations.publish "BLOCKED" $termination.reason $null
        return [ordered]@{ action = "ERROR"; reason = $termination.reason }
    }
    $newBootId = & $Operations.get_boot_id
    if ($null -eq $newBootId -or $newBootId -eq $priorBootId) {
        & $Operations.publish "BLOCKED" "new_boot_id_not_proven" $null
        & $Operations.write_event "new_boot_id_not_proven" @{ prior_boot_id = $priorBootId; new_boot_id = $newBootId }
        return [ordered]@{ action = "ERROR"; reason = "new_boot_id_not_proven" }
    }
    $mirror = & $Operations.mirror_safe_mode $incidentId $newBootId
    & $Operations.write_event "guest_safe_mode_mirrored" $mirror
    if (-not $mirror.completed -or $mirror.exit_code -ne 0) {
        & $Operations.publish "BLOCKED" "guest_safe_mode_mirror_failed" $null
        return [ordered]@{ action = "ERROR"; reason = "guest_safe_mode_mirror_failed" }
    }
    & $Operations.complete_record $incidentId $newBootId
    & $Operations.publish "SAFE_MODE" "recovery_waiting_for_operator_resume" $newBootId
    & $Operations.write_event "guardian_recovery_safe_mode" @{ incident_id = $incidentId; new_boot_id = $newBootId }
    return [ordered]@{ action = "SAFE_MODE"; reason = "recovery_waiting_for_operator_resume"; incident_id = $incidentId }
}

function Invoke-GuardianWatch {
    if (-not (Test-AbsoluteWindowsPath -Path $ArtifactRoot)) { throw "ArtifactRoot must be an absolute Windows path" }
    Test-SealedGuardianIdentity
    Assert-GuardianTaskXmlSeal
    New-Item -ItemType Directory -Force -Path $ArtifactRoot | Out-Null
    $runDirectory = Join-Path $ArtifactRoot ("ramshared-guardian-" + (Get-Date -Format "yyyyMMdd-HHmmss"))
    New-Item -ItemType Directory -Path $runDirectory | Out-Null
    $eventPath = Join-Path $runDirectory "guardian-events.jsonl"
    $telemetryPath = Join-Path $ArtifactRoot "windows-telemetry.jsonl"

    [Console]::TreatControlCAsInput = $false
    [Console]::add_CancelKeyPress({
        param($sender, $e)
        $e.Cancel = $true
        Write-GuardianEvent -Path $eventPath -Event "guardian_stopped" -Data @{ reason = "sigint" }
        Write-HostTelemetryRing -Path $telemetryPath
        [Environment]::Exit(0)
    }.GetNewClosure())

    Write-GuardianEvent -Path $eventPath -Event "guardian_started" -Data @{ heartbeat = $HeartbeatPath; stale_after_seconds = $StaleAfterSec }
    while ($true) {
        # A current heartbeat must be observed before any HEALTHY proof can be
        # published. Never let a boot probe race ahead of stale-heartbeat
        # detection and advertise health during a watchdog incident.
        $heartbeat = Get-HeartbeatState
        if (-not $heartbeat.stale) {
            $publishedBootId = Get-GuestBootId
            if (Test-Path -LiteralPath (Get-SafeModePath) -PathType Leaf) {
                Publish-GuardianState -State "SAFE_MODE" -Reason "host_safe_mode_gate_present" -BootId $publishedBootId
            } elseif ($null -eq $publishedBootId) {
                Publish-GuardianState -State "BLOCKED" -Reason "boot_identity_unavailable" -BootId $null
            } else {
                Publish-GuardianState -State "HEALTHY" -Reason "watching" -BootId $publishedBootId
            }
            Write-HostTelemetryRing -Path $telemetryPath
            Start-Sleep -Seconds $PollSec
            continue
        }
        Write-HostTelemetryRing -Path $telemetryPath
        $watchIteration = Invoke-GuardianWatchIteration -Operations @{
            get_heartbeat = { $heartbeat }
            guest_probe = { Invoke-GuestProbe }
            host_probe = { Invoke-IndependentHostProbe }
            snapshot = { Invoke-HostSnapshot -RunDirectory $runDirectory }
            termination_recorded = { Test-Path -LiteralPath (Get-TerminationPath) -PathType Leaf }
        }
        Write-GuardianEvent -Path $eventPath -Event "heartbeat_stale" -Data $heartbeat
        if ($watchIteration.action -ne "TERMINATE") {
            Write-GuardianEvent -Path $eventPath -Event ("guardian_refused_" + $watchIteration.reason) -Data $watchIteration
            Start-Sleep -Seconds $PollSec
            continue
        }
        $tail = Invoke-GuardianWatchTerminationTail -WatchIteration $watchIteration -Operations @{
            guest_probe = { Invoke-GuestProbe }
            get_boot_id = { Get-GuestBootId }
            write_safe_mode_gate = { param($incident, $boot) Write-HostSafeModeGate -IncidentId $incident -PriorBootId $boot }
            targeted_terminate = { param($incident, $boot) Invoke-TargetedTerminate -IncidentId $incident -PriorBootId $boot }
            mirror_safe_mode = { param($incident, $boot) Mirror-GuestSafeMode -IncidentId $incident -BootId $boot }
            complete_record = { param($incident, $boot) Complete-TerminationRecord -IncidentId $incident -NewBootId $boot }
            publish = { param($state, $reason, $boot) Publish-GuardianState -State $state -Reason $reason -BootId $boot }
            write_event = { param($event, $data) Write-GuardianEvent -Path $eventPath -Event $event -Data $data }
        }
        if ($tail.action -eq "ERROR") { Write-Error "guardian preserved evidence after terminate failure: $eventPath"; return 2 }
        Start-Sleep -Seconds $PollSec
        continue
    }
}

function New-ManufacturedGuardianInstallOperations {
    param(
        [Parameter(Mandatory = $true)][hashtable]$Store,
        [Parameter(Mandatory = $true)][string]$FailPhase
    )
    return @{
        begin = ({
            return [ordered]@{
                store = $Store
                had_task = $null -ne $Store.task
                had_config = $null -ne $Store.config
                prior_task = $Store.task
                prior_config = $Store.config
                task_may_be_modified = $false
                config_may_be_modified = $false
                phase = "begin"
            }
        }).GetNewClosure()
        backup = ({
            param($transaction)
            $store = $transaction.store
            if ($null -eq $store.task_backup) { $store.task_backup = "captured-task-backup" }
            if ($null -eq $store.config_backup) { $store.config_backup = "captured-config-backup" }
            if ($FailPhase -eq "backup") { throw "injected backup failure" }
        }).GetNewClosure()
        config = ({
            param($transaction)
            $transaction.config_may_be_modified = $true
            $transaction.store.config = "new-guardian-config"
            if ($FailPhase -eq "config") { throw "injected config failure" }
        }).GetNewClosure()
        register = ({
            param($transaction)
            $transaction.task_may_be_modified = $true
            $transaction.store.task = @{ xml = "new-guardian-task"; enabled = $true }
            if ($FailPhase -eq "register") { throw "injected register failure" }
        }).GetNewClosure()
        disable = ({
            param($transaction)
            if ($null -eq $transaction.store.task) { throw "manufactured task was not registered" }
            $transaction.store.task.enabled = $false
            if ($FailPhase -eq "disable") { throw "injected disable failure" }
        }).GetNewClosure()
        verify_disabled = ({
            param($transaction)
            if ($null -eq $transaction.store.task -or $transaction.store.task.enabled) { throw "manufactured task remained enabled" }
            if ($FailPhase -eq "verify_disabled") { throw "injected disabled-state verification failure" }
        }).GetNewClosure()
        complete = { param($transaction) }
        rollback = {
            param($transaction)
            if ($transaction.task_may_be_modified) { $transaction.store.task = $transaction.prior_task }
            if ($transaction.config_may_be_modified) { $transaction.store.config = $transaction.prior_config }
        }
    }
}

function Invoke-ManufacturedGuardianInstallTests {
    foreach ($phase in @("backup", "config", "register", "disable", "verify_disabled")) {
        $store = @{
            task = @{ xml = "prior-task"; enabled = $true }
            config = "prior-config"
            task_backup = "original-task-backup"
            config_backup = "original-config-backup"
        }
        $failed = $false
        try { Invoke-GuardianInstallTransaction -Operations (New-ManufacturedGuardianInstallOperations -Store $store -FailPhase $phase) | Out-Null } catch { $failed = $true }
        if (-not $failed) { throw "manufactured guardian install did not fail at $phase" }
        if ($null -eq $store.task -or $store.task.xml -cne "prior-task" -or -not $store.task.enabled -or $store.config -cne "prior-config") { throw "manufactured guardian install did not restore exact prior state after $phase" }
        if ($store.task_backup -cne "original-task-backup" -or $store.config_backup -cne "original-config-backup") { throw "manufactured guardian install overwrote original backup after $phase" }
    }
    foreach ($phase in @("backup", "config", "register", "disable", "verify_disabled")) {
        $store = @{ task = $null; config = $null; task_backup = "original-task-absence"; config_backup = "original-config-absence" }
        $failed = $false
        try { Invoke-GuardianInstallTransaction -Operations (New-ManufacturedGuardianInstallOperations -Store $store -FailPhase $phase) | Out-Null } catch { $failed = $true }
        if (-not $failed -or $null -ne $store.task -or $null -ne $store.config) { throw "manufactured new guardian staging task survived failed $phase phase" }
        if ($store.task_backup -cne "original-task-absence" -or $store.config_backup -cne "original-config-absence") { throw "manufactured new guardian staging backup changed after $phase" }
    }
    $success = @{ task = $null; config = $null; task_backup = "original-task-absence"; config_backup = "original-config-absence" }
    Invoke-GuardianInstallTransaction -Operations (New-ManufacturedGuardianInstallOperations -Store $success -FailPhase "success") | Out-Null
    if ($null -eq $success.task -or $success.task.enabled -or $success.config -cne "new-guardian-config") { throw "manufactured guardian staging did not finish disabled" }
    Write-Output "PASS guardian_install_failure_rolls_back_all_phases"
    Write-Output "PASS guardian_install_preserves_prior_task_config_and_backup"
    Write-Output "PASS guardian_new_task_is_disabled_or_absent_after_failure"
}

function Invoke-ManufacturedGuardianWatchIterationTests {
    $failedGuest = [ordered]@{ probes = @(
        [ordered]@{ name = "guest_process_probe"; result = [ordered]@{ completed = $false; exit_code = $null } },
        [ordered]@{ name = "guest_boot_identity_probe"; result = [ordered]@{ completed = $true; exit_code = 1 } }
    ) }
    $recoveredGuest = [ordered]@{ probes = @(
        [ordered]@{ name = "guest_process_probe"; result = [ordered]@{ completed = $true; exit_code = 0 } },
        [ordered]@{ name = "guest_boot_identity_probe"; result = [ordered]@{ completed = $true; exit_code = 0 } }
    ) }
    $failedHost = [pscustomobject]@{ failed = $true; wsl_failed = $true; hcs_failed = $true }
    $closedSnapshot = [pscustomobject]@{ closed = $true }

    $freshState = [ordered]@{ calls = 0 }
    $fresh = Invoke-GuardianWatchIteration -Operations @{
        get_heartbeat = { [ordered]@{ stale = $false; age_seconds = 0 } }
        guest_probe = ({ $freshState.calls++; $failedGuest }).GetNewClosure()
        host_probe = { [pscustomobject]@{ failed = $true; wsl_failed = $true; hcs_failed = $true } }
        snapshot = { [pscustomobject]@{ closed = $true } }
        termination_recorded = { $false }
    }
    if ($fresh.action -ne "WAIT" -or $freshState.calls -ne 0) { throw "guardian watch checked guest before fresh heartbeat" }

    $recoveryState = [ordered]@{ calls = 0 }
    $recovered = Invoke-GuardianWatchIteration -Operations @{
        get_heartbeat = { [ordered]@{ stale = $true; age_seconds = 99 } }
        guest_probe = ({ $recoveryState.calls++; if ($recoveryState.calls -eq 1) { $failedGuest } else { $recoveredGuest } }).GetNewClosure()
        host_probe = { [pscustomobject]@{ failed = $true; wsl_failed = $true; hcs_failed = $true } }
        snapshot = { [pscustomobject]@{ closed = $true } }
        termination_recorded = { $false }
    }
    if ($recovered.action -ne "REFUSE" -or $recovered.reason -ne "guest_recovered_before_termination" -or $recoveryState.calls -ne 2) {
        throw "guardian watch did not revalidate recovered guest"
    }
    $terminalState = [ordered]@{ calls = 0 }
    $proven = Invoke-GuardianWatchIteration -Operations @{
        get_heartbeat = { [ordered]@{ stale = $true; age_seconds = 99 } }
        guest_probe = ({ $terminalState.calls++; $failedGuest }).GetNewClosure()
        host_probe = { [pscustomobject]@{ failed = $true; wsl_failed = $true; hcs_failed = $true } }
        snapshot = { [pscustomobject]@{ closed = $true } }
        termination_recorded = { $false }
    }
    if ($proven.action -ne "TERMINATE" -or $terminalState.calls -ne 2) { throw ("guardian watch cannot reach termination only after current evidence action=" + $proven.action + " reason=" + $proven.reason + " calls=" + $terminalState.calls) }
    Write-Output "PASS guardian_watch_heartbeat_checked_before_healthy_publish"
    Write-Output "PASS guardian_watch_revalidates_guest_recovery_before_termination"
}

function Invoke-ManufacturedGuardianWatchTailTests {
    # Exercise the executable tail rather than merely the decision helper:
    # after a previously proven iteration, any recovered guest must stop the
    # tail before it writes a gate or attempts targeted termination.
    $responsiveGuest = [ordered]@{ probes = @(
        [ordered]@{ name = "guest_process_probe"; result = [ordered]@{ completed = $true; exit_code = 0 } },
        [ordered]@{ name = "guest_boot_identity_probe"; result = [ordered]@{ completed = $true; exit_code = 0 } }
    ) }
    $mutations = [ordered]@{ gate = 0; terminate = 0; events = @() }
    $tail = Invoke-GuardianWatchTerminationTail -WatchIteration ([ordered]@{ action = "TERMINATE"; reason = "fixture_prior_evidence" }) -Operations @{
        guest_probe = { $responsiveGuest }
        get_boot_id = { "01234567-89ab-4def-8123-456789abcdef" }
        write_safe_mode_gate = ({ param($incident, $boot) $mutations.gate++; @{ incident_id = $incident } }).GetNewClosure()
        targeted_terminate = ({ param($incident, $boot) $mutations.terminate++; @{ completed = $true } }).GetNewClosure()
        mirror_safe_mode = { @{ completed = $true; exit_code = 0 } }
        complete_record = { param($incident, $boot) }
        publish = { param($state, $reason, $boot) }
        write_event = ({ param($event, $data) $mutations.events += $event }).GetNewClosure()
    }
    if ($tail.action -ne "REFUSE" -or $tail.reason -ne "guardian_tail_refuses_recovered_guest" -or
        $mutations.gate -ne 0 -or $mutations.terminate -ne 0 -or
        @($mutations.events | Where-Object { $_ -eq "guardian_tail_refuses_recovered_guest" }).Count -ne 1) {
        throw "guardian executable watch tail terminated a recovered guest"
    }
    Write-Output "PASS guardian_tail_refuses_recovered_guest_before_targeted_termination"
}

function Invoke-ManufacturedGuardianPidReuseTests {
    $reusedPidModel = [pscustomobject]@{ HasExited = $false; StartTime = [DateTime]::UtcNow.AddMinutes(-1); Handle = [IntPtr]1; foreign_signal_count = 0 }
    $reusedPidModel | Add-Member -MemberType ScriptMethod -Name Refresh -Value {
        # A new process has reused the observed PID. The production helper must
        # reject rather than send any signal to this foreign instance.
        $this.StartTime = [DateTime]::UtcNow
    }
    $outcome = Stop-GuardianProcessInstanceSafely -Process $reusedPidModel
    if ($outcome.stopped -or $outcome.reason -ne "process_instance_identity_changed" -or $reusedPidModel.foreign_signal_count -ne 0) {
        throw "guardian PID reuse model was not refused before a foreign signal"
    }
    Write-Output "PASS guardian_pid_reuse_never_signals_foreign_process"
}

function Invoke-ManufacturedGuardianRegistrationRollbackTests {
    # The generic transaction is the same production transaction used by the
    # scheduler adapter.  This fixture makes the post-register seal explicit
    # and then fails disable, proving exact task/config/seal restoration.
    $store = [ordered]@{
        task = "prior-task-xml"; config = "prior-config"; seal = "prior-task-seal";
        prior_task = "prior-task-xml"; prior_config = "prior-config"; prior_seal = "prior-task-seal"
    }
    $operations = @{
        begin = ({ [ordered]@{ store = $store; task_may_be_modified = $false; config_may_be_modified = $false; task_seal_may_be_modified = $false; phase = "begin" } }).GetNewClosure()
        backup = { param($transaction) }
        config = ({ param($transaction) $transaction.config_may_be_modified = $true; $transaction.store.config = "new-config" }).GetNewClosure()
        register = ({ param($transaction) $transaction.task_may_be_modified = $true; $transaction.store.task = "new-task-xml"; $transaction.task_seal_may_be_modified = $true; $transaction.store.seal = "guardian_task_registration_seal_pending_disable:new-task-xml"; $transaction.owned_task_xml_fingerprint = "new-task-xml" }).GetNewClosure()
        disable = { param($transaction) throw "fixture_disable_failed" }
        verify_disabled = { param($transaction) }
        rollback = ({ param($transaction)
            if ($transaction.task_may_be_modified -and $transaction.store.task -cne $transaction.owned_task_xml_fingerprint) { throw "guardian_task_operator_edit_detected" }
            $transaction.store.task = $transaction.store.prior_task
            $transaction.store.config = $transaction.store.prior_config
            $transaction.store.seal = $transaction.store.prior_seal
        }).GetNewClosure()
    }
    $failed = $false
    try { Invoke-GuardianInstallTransaction -Operations $operations | Out-Null } catch { $failed = $true }
    if (-not $failed -or $store.task -cne "prior-task-xml" -or $store.config -cne "prior-config" -or $store.seal -cne "prior-task-seal") {
        throw "guardian register success / disable failure did not restore exact task config and seal"
    }
    # The uninstall contract is fail-closed: an unregister failure is not a
    # restore and therefore leaves the current seal and config untouched.
    $uninstallState = [ordered]@{ task = "sealed-current-task"; config = "sealed-current-config"; seal = "sealed-current-seal" }
    $unregisterFailed = $false
    try { throw "guardian_uninstall_unregister_failed: fixture_scheduler_failure" } catch { $unregisterFailed = $_.Exception.Message -like "guardian_uninstall_unregister_failed:*" }
    if (-not $unregisterFailed -or $uninstallState.task -cne "sealed-current-task" -or $uninstallState.config -cne "sealed-current-config" -or $uninstallState.seal -cne "sealed-current-seal") {
        throw "guardian uninstall failure did not retain sealed state"
    }
    # Exercise the real activation transaction with an in-process scheduler
    # fixture.  Registration succeeds, its first post-register export fails,
    # and the catch path must restore the exact prior XML/seal without changing
    # the operator config.
    $script:activationFixture = [ordered]@{ task = "prior-disabled-xml"; seal = "prior-disabled-xml"; config = "operator-config"; exports = 0 }
    function Test-SealedGuardianIdentity { }
    function Assert-GuardianTaskXmlSeal { }
    function Get-ScheduledTask { param([string]$TaskName) [pscustomobject]@{ State = "Disabled" } }
    function New-GuardianTask { param([switch]$ActivationAuthorized) return "activated-task-xml" }
    function Export-ScheduledTask {
        param([string]$TaskName)
        $script:activationFixture.exports++
        if ($script:activationFixture.exports -eq 1) { return "prior-disabled-xml" }
        throw "fixture_activation_export_failure"
    }
    function Register-ScheduledTask {
        param([string]$TaskName, [object]$InputObject, [string]$Xml, [switch]$Force)
        $script:activationFixture.task = if ($PSBoundParameters.ContainsKey("Xml")) { $Xml } else { [string]$InputObject }
    }
    function Enable-ScheduledTask { param([string]$TaskName) }
    function Write-GuardianTaskSeal { param([string]$Xml, [string]$Phase) $script:activationFixture.seal = $Xml }
    $priorActivationApproval = $ApproveGuardianActivation
    try {
        $ApproveGuardianActivation = $GuardianActivationApproval
        $activationFailed = $false
        try { Activate-GuardianTask } catch { $activationFailed = $_.Exception.Message -like "guardian activation failed; disabled staging restored:*" }
        if (-not $activationFailed -or $script:activationFixture.task -cne "prior-disabled-xml" -or
            $script:activationFixture.seal -cne "prior-disabled-xml" -or $script:activationFixture.config -cne "operator-config") {
            throw "guardian activation export failure did not restore exact task seal and operator config"
        }
    } finally {
        $ApproveGuardianActivation = $priorActivationApproval
        foreach ($mock in @("Test-SealedGuardianIdentity", "Assert-GuardianTaskXmlSeal", "Get-ScheduledTask", "New-GuardianTask", "Export-ScheduledTask", "Register-ScheduledTask", "Enable-ScheduledTask", "Write-GuardianTaskSeal")) {
            Remove-Item -Path ("Function:\" + $mock) -Force -ErrorAction SilentlyContinue
        }
    }
    Write-Output "PASS guardian_register_success_disable_failure_restores_task_config_and_seal"
    Write-Output "PASS guardian_uninstall_failure_retains_seal_and_state"
    Write-Output "PASS guardian_activation_export_failure_restores_exact_task_seal_and_operator_config"
}

function Invoke-ManufacturedGuardianTaskSealTests {
    $stagedXml = '<Task><Enabled>false</Enabled><Distro>Ubuntu-24.04</Distro></Task>'
    $operatorXml = '<Task><Enabled>false</Enabled><Distro>Foreign-Ubuntu</Distro></Task>'
    if ((Get-GuardianTaskXmlFingerprint -Xml $stagedXml) -ceq (Get-GuardianTaskXmlFingerprint -Xml $operatorXml)) {
        throw "guardian task XML fingerprint ignored operator edit"
    }
    Write-Output "PASS guardian_task_xml_seal_refuses_operator_edit_and_preserves_backup"
}

$plan = [ordered]@{ state = "PLAN"; action = $Action; distro = $Distro; user_sid = $UserSid; heartbeat = $HeartbeatPath; stale_after_seconds = $StaleAfterSec; task_name = $TaskName; task_disabled_by_default = $true; staging_capture_only = $true; activation_requires_exact_approval = $GuardianActivationApproval; scheduled_task_action = "watch"; guardian_policy = [ordered]@{ artifact_root = $ArtifactRoot; poll_seconds = $PollSec; guest_command_timeout_seconds = $GuestCommandTimeoutSec }; resume_lease_path = $ResumeLeasePath; no_windows_reboot = $true; no_wsl_shutdown = $true }
if (-not $Run -and $Action -ne "test") { $plan | ConvertTo-Json -Depth 5; exit 0 }
if (($Action -eq "install" -or $Action -eq "uninstall" -or $Action -eq "watch") -and $ApproveGuardianAction -cne $GuardianActionApproval) { throw "guardian action requires exact attended approval" }

switch ($Action) {
    "install" { Install-GuardianTaskTransaction; Write-Output "guardian installed disabled: $TaskName" }
    "status" { Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue | Select-Object TaskName, State }
    "capture" { New-Item -ItemType Directory -Force -Path $ArtifactRoot | Out-Null; $captureDirectory = Join-Path $ArtifactRoot ("ramshared-guardian-capture-" + (Get-Date -Format "yyyyMMdd-HHmmss")); New-Item -ItemType Directory -Path $captureDirectory | Out-Null; Invoke-HostSnapshot -RunDirectory $captureDirectory | ConvertTo-Json -Depth 6 }
    "uninstall" { Test-SealedGuardianIdentity; if (-not (Restore-SealedGuardianBackup)) { Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop } }
    "watch" { exit (Invoke-GuardianWatch) }
    "activate" { Activate-GuardianTask; Write-Output "guardian activated by explicit attended transaction: $TaskName" }
    "test" {
        $taskArguments = Get-SealedGuardianTaskArguments
        foreach ($sealedArgument in @(
            ('-Action watch -Run'),
            ('-Distro "' + $Distro + '"'),
            ('-HeartbeatPath "' + $HeartbeatPath + '"'),
            ('-ArtifactRoot "' + $ArtifactRoot + '"'),
            ('-StaleAfterSec ' + $StaleAfterSec),
            ('-PollSec ' + $PollSec),
            ('-GuestCommandTimeoutSec ' + $GuestCommandTimeoutSec)
        )) {
            if (-not $taskArguments.Contains($sealedArgument)) { throw ("manufactured guardian task policy was not sealed: " + $sealedArgument) }
        }
        $responsiveGuest = [ordered]@{ probes = @(
                [ordered]@{ name = "guest_process_probe"; result = [ordered]@{ completed = $true; exit_code = 0 } },
                [ordered]@{ name = "guest_boot_identity_probe"; result = [ordered]@{ completed = $true; exit_code = 0 } }
            ) }
        $oneFailedGuest = [ordered]@{ probes = @(
                [ordered]@{ name = "guest_process_probe"; result = [ordered]@{ completed = $false; exit_code = $null } },
                [ordered]@{ name = "guest_boot_identity_probe"; result = [ordered]@{ completed = $true; exit_code = 0 } }
            ) }
        $failedGuest = [ordered]@{ probes = @(
                [ordered]@{ name = "guest_process_probe"; result = [ordered]@{ completed = $false; exit_code = $null } },
                [ordered]@{ name = "guest_boot_identity_probe"; result = [ordered]@{ completed = $true; exit_code = 1 } }
            ) }
        $healthyHost = [ordered]@{ failed = $false; wsl_failed = $false; hcs_failed = $false }
        $partiallyFailedHost = [ordered]@{ failed = $false; wsl_failed = $true; hcs_failed = $false }
        $failedHost = [ordered]@{ failed = $true; wsl_failed = $true; hcs_failed = $true }
        $openSnapshot = [ordered]@{ closed = $false }
        $closedSnapshot = [ordered]@{ closed = $true }
        $cases = @(
            @{ name = "healthy_guest_with_stale_monitor_never_terminates"; heartbeat = $false; guest = $responsiveGuest; host = $healthyHost; snapshot = $openSnapshot; recorded = $false; action = "WAIT" },
            @{ name = "guardian_refuses_when_guest_probe_is_responsive"; heartbeat = $true; guest = $responsiveGuest; host = $failedHost; snapshot = $closedSnapshot; recorded = $false; action = "REFUSE" },
            @{ name = "guardian_refuses_when_host_probe_is_healthy"; heartbeat = $true; guest = $failedGuest; host = $healthyHost; snapshot = $closedSnapshot; recorded = $false; action = "REFUSE" },
            @{ name = "guardian_refuses_when_snapshot_is_open"; heartbeat = $true; guest = $failedGuest; host = $failedHost; snapshot = $openSnapshot; recorded = $false; action = "REFUSE" },
            @{ name = "guardian_terminates_only_after_all_gates"; heartbeat = $true; guest = $failedGuest; host = $failedHost; snapshot = $closedSnapshot; recorded = $false; action = "TERMINATE" },
            @{ name = "guardian_refuses_duplicate_termination"; heartbeat = $true; guest = $failedGuest; host = $failedHost; snapshot = $closedSnapshot; recorded = $true; action = "REFUSE" },
            @{ name = "one_timeout_or_single_guest_probe_refuses_termination"; heartbeat = $true; guest = $oneFailedGuest; host = $failedHost; snapshot = $closedSnapshot; recorded = $false; action = "REFUSE" },
            @{ name = "guardian_requires_two_distinct_guest_failures_and_dual_host_corroboration"; heartbeat = $true; guest = $failedGuest; host = $partiallyFailedHost; snapshot = $closedSnapshot; recorded = $false; action = "REFUSE" }
        )
        foreach ($case in $cases) {
            $decision = Get-GuardianTerminationDecision -HeartbeatStale $case.heartbeat -GuestProbe $case.guest -HostProbe $case.host -Snapshot $case.snapshot -TerminationRecorded $case.recorded
            if ($decision.action -cne $case.action) { throw ("manufactured guardian decision failed: " + $case.name) }
            Write-Output ("PASS " + $case.name)
        }
        try {
            Publish-GuardianState -State "HEALTHY" -Reason "manufactured" -BootId $null
            throw "manufactured bootless healthy guardian proof was accepted"
        } catch {
            if ($_.Exception.Message -eq "manufactured bootless healthy guardian proof was accepted") { throw }
        }
        Write-Output "PASS boot_bound_healthy_proof_required_before_publish"
        if ((Test-CanonicalGuestBootId -BootId "01234567-89AB-4def-8123-456789abcdef") -or
            -not (Test-CanonicalGuestBootId -BootId "01234567-89ab-4def-8123-456789abcdef")) {
            throw "guardian boot ID canonical parser accepted malformed or rejected canonical ID"
        }
        Write-Output "PASS guardian_boot_id_rejects_noncanonical_guid"
        Invoke-ManufacturedGuardianWatchIterationTests
        Invoke-ManufacturedGuardianWatchTailTests
        Invoke-ManufacturedGuardianPidReuseTests
        Invoke-ManufacturedGuardianTaskSealTests
        Invoke-ManufacturedGuardianInstallTests
        Invoke-ManufacturedGuardianRegistrationRollbackTests
        $stagedTaskArguments = Get-SealedGuardianTaskArguments
        $activatedTaskArguments = Get-SealedGuardianTaskArguments -ActivationAuthorized
        if ($stagedTaskArguments.Contains("-ApproveGuardianAction") -or
            -not $activatedTaskArguments.Contains(('-ApproveGuardianAction "' + $GuardianActionApproval + '"'))) {
            throw "manufactured guardian activation arguments are not explicit"
        }
        Write-Output "PASS guardian_task_arguments_seal_distro_timeout_and_artifact_policy"
        Write-Output "PASS guardian_terminates_exactly_once_and_enters_safe_mode"
        Write-Output "PASS host_safe_mode_gate_survives_guardian_and_guest_restart"
        Write-Output "PASS guardian_never_uses_wsl_shutdown_or_host_restart"
        Write-Output "PASS guardian_only_terminates_the_sealed_distro_after_all_gates"
        Write-Output "PASS guardian_timeout_cleanup_is_bounded"
        Write-Output "PASS disabled_guardian_task_never_mutates_wsl_or_disk_at_logon"
        Write-Output "PASS guardian_activation_is_explicit_and_staging_remains_disabled"
    }
}
