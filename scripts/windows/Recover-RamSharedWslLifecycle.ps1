#Requires -Version 5.1
<#
.SYNOPSIS
  Plan-first host recovery for a pending RamShared swapoff lifecycle.

.DESCRIPTION
  The host never terminates WSL and never touches a block device. With an exact
  attended approval it asks the existing controller to stop, or invokes that
  controller's recovery-only path when the unit is already inactive. The guest
  controller remains the sole owner of swapoff, detach, and daemon shutdown.
#>
[CmdletBinding()]
param(
    [ValidateSet("plan", "status", "recover", "test")]
    [string]$Action = "plan",
    [switch]$Run,
    [string]$ApproveRecovery = "",
    [ValidatePattern('^[A-Za-z0-9._-]+$')]
    [string]$Distro = "Ubuntu-24.04",
    [ValidateRange(15, 900)][int]$RecoveryTimeoutSec = 300
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ProgramDataRoot = "C:\ProgramData\RamShared"
$MarkerPath = Join-Path (Join-Path $ProgramDataRoot "lifecycle-recovery") ($Distro + ".pending")
$GuestStatusScript = "/opt/ramshared/current/scripts/safety/lifecycle-recovery-status.sh"
$ControllerUnit = "ramshared-cascade.service"

function ConvertFrom-LifecycleMarkerLines {
    param(
        [Parameter(Mandatory = $true)][string[]]$Lines,
        [Parameter(Mandatory = $true)][string]$ExpectedDistro
    )
    $values = @{}
    foreach ($line in $Lines) {
        if ($line -notmatch '^([a-z_]+)=(.*)$') { throw "lifecycle_marker_format_invalid" }
        $key = [string]$Matches[1]
        if ($values.ContainsKey($key)) { throw "lifecycle_marker_duplicate_key" }
        $values[$key] = [string]$Matches[2]
    }
    $expected = @("schema_version", "distro", "release_version", "boot_id", "phase", "managed_device")
    if ((@($values.Keys | Sort-Object) -join "`n") -cne (@($expected | Sort-Object) -join "`n")) {
        throw "lifecycle_marker_keys_invalid"
    }
    if ([string]$values.schema_version -cne "1") { throw "lifecycle_marker_schema_invalid" }
    if ([string]$values.distro -cne $ExpectedDistro) { throw "lifecycle_marker_distro_mismatch" }
    if ([string]$values.release_version -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$') { throw "lifecycle_marker_version_invalid" }
    if ([string]$values.boot_id -notmatch '^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$') { throw "lifecycle_marker_boot_id_invalid" }
    if ([string]$values.phase -notmatch '^(starting|active|stopping|startup_failed|startup_identity_missing)$') { throw "lifecycle_marker_phase_invalid" }
    if ([string]$values.managed_device -and [string]$values.managed_device -notmatch '^/dev/nbd[0-9]+$') { throw "lifecycle_marker_device_invalid" }
    return [pscustomobject][ordered]@{
        schema_version = 1
        distro = [string]$values.distro
        release_version = [string]$values.release_version
        boot_id = [string]$values.boot_id
        phase = [string]$values.phase
        managed_device = [string]$values.managed_device
    }
}

function Read-LifecycleMarker {
    if (-not (Test-Path -LiteralPath $MarkerPath -PathType Leaf)) { return $null }
    $item = Get-Item -LiteralPath $MarkerPath -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or $item.Length -gt 8192) {
        throw "lifecycle_marker_file_invalid"
    }
    return ConvertFrom-LifecycleMarkerLines -Lines ([IO.File]::ReadAllLines($MarkerPath)) -ExpectedDistro $Distro
}

function Stop-BoundProcessInstance {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Process)
    try {
        $Process.Refresh()
        if ($Process.HasExited) { return $true }
        $boundHandle = $Process.Handle
        $started = $Process.StartTime.ToUniversalTime().Ticks
        $Process.Refresh()
        if ($Process.HasExited -or $Process.StartTime.ToUniversalTime().Ticks -ne $started) { return $Process.HasExited }
        $Process.Kill()
        return $Process.WaitForExit(5000)
    } catch {
        return $false
    }
}

function Invoke-BoundedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FileName,
        [Parameter(Mandatory = $true)][string]$Arguments,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds
    )
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $FileName
    $start.Arguments = $Arguments
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw "bounded_process_start_failed" }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $completed = $process.WaitForExit($TimeoutSeconds * 1000)
        $clientStopped = $false
        if (-not $completed) { $clientStopped = Stop-BoundProcessInstance -Process $process }
        $drained = [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdoutTask, $stderrTask), 5000)
        return [pscustomobject][ordered]@{
            completed = [bool]($completed -and $drained)
            exit_code = if ($completed -and $drained) { [int]$process.ExitCode } else { $null }
            stdout = if ($drained) { ([string]$stdoutTask.Result).Replace("`0", "").Trim() } else { "" }
            stderr = if ($drained) { ([string]$stderrTask.Result).Replace("`0", "").Trim() } else { "" }
            host_client_stopped = [bool]$clientStopped
        }
    } finally {
        $process.Dispose()
    }
}

function Test-DistroRunning {
    $result = Invoke-BoundedProcess -FileName "wsl.exe" -Arguments "--list --running --quiet" -TimeoutSeconds 10
    if (-not $result.completed -or $result.exit_code -ne 0) { throw "running_distro_query_failed" }
    return @($result.stdout -split "`r?`n" | ForEach-Object { $_.Trim() }) -ccontains $Distro
}

function ConvertFrom-GuestStatus {
    param([Parameter(Mandatory = $true)][string]$Text)
    $values = @{}
    foreach ($line in @($Text -split "`r?`n")) {
        if ($line -notmatch '^LIFECYCLE_RECOVERY_([A-Z_]+)=(.*)$') { continue }
        $key = [string]$Matches[1]
        if ($values.ContainsKey($key)) { throw "guest_recovery_status_duplicate" }
        $values[$key] = [string]$Matches[2]
    }
    foreach ($required in @("STATE", "MARKER", "PHASE", "MANAGED_SWAP_COUNT", "DAEMON_RUNNING", "DEVICE_ATTACHED")) {
        if (-not $values.ContainsKey($required)) { throw "guest_recovery_status_incomplete" }
    }
    if ([string]$values.STATE -notin @("CLEAN", "PENDING")) { throw "guest_recovery_status_invalid" }
    return [pscustomobject][ordered]@{
        state = [string]$values.STATE
        marker_present = [int]$values.MARKER
        phase = [string]$values.PHASE
        managed_swap_count = [int]$values.MANAGED_SWAP_COUNT
        daemon_running = [int]$values.DAEMON_RUNNING
        device_attached = [int]$values.DEVICE_ATTACHED
    }
}

function Get-GuestRecoveryStatus {
    $arguments = "-d `"$Distro`" -u root -- /usr/bin/env RAMSHARED_WSL_DISTRO=$Distro $GuestStatusScript"
    $result = Invoke-BoundedProcess -FileName "wsl.exe" -Arguments $arguments -TimeoutSeconds 15
    if (-not $result.completed -or $result.exit_code -notin @(0, 2)) {
        throw "guest_recovery_status_unavailable"
    }
    return ConvertFrom-GuestStatus -Text $result.stdout
}

function Get-ControllerUnitState {
    $arguments = "-d `"$Distro`" -u root -- /bin/systemctl is-active $ControllerUnit"
    $result = Invoke-BoundedProcess -FileName "wsl.exe" -Arguments $arguments -TimeoutSeconds 15
    if (-not $result.completed -or $result.exit_code -notin @(0, 3)) { throw "controller_state_unavailable" }
    return [string]$result.stdout.Trim()
}

function Request-ControllerStop {
    $arguments = "-d `"$Distro`" -u root -- /bin/systemctl stop --no-block $ControllerUnit"
    $result = Invoke-BoundedProcess -FileName "wsl.exe" -Arguments $arguments -TimeoutSeconds 15
    if (-not $result.completed -or $result.exit_code -ne 0) { throw "controller_stop_request_failed" }
}

function Invoke-RecoveryController {
    param([Parameter(Mandatory = $true)][object]$Marker)
    $version = [string]$Marker.release_version
    $controller = "/opt/ramshared/releases/$version/scripts/safety/cascade-controller.sh"
    $arguments = "-d `"$Distro`" -u root -- /usr/bin/env RAMSHARED_WSL_DISTRO=$Distro RAMSHARED_NBD_CONTROLLER_APPROVAL=recover:$version $controller --recover"
    return Invoke-BoundedProcess -FileName "wsl.exe" -Arguments $arguments -TimeoutSeconds $RecoveryTimeoutSec
}

function Get-RecoveryDecision {
    param([bool]$DistroRunning, [AllowNull()][object]$Marker, [AllowNull()][object]$GuestStatus, [string]$UnitState)
    if ($null -eq $Marker) { return "NO_PENDING_MARKER" }
    if (-not $DistroRunning) { return "BLOCKED_DISTRO_NOT_RUNNING" }
    if ($null -ne $GuestStatus -and [string]$GuestStatus.state -ceq "CLEAN") { return "CLEAN" }
    if ($UnitState -in @("active", "activating", "deactivating")) { return "WAIT_FOR_CONTROLLER" }
    return "RUN_RECOVERY_CONTROLLER"
}

if ($Action -eq "test") {
    $valid = @(
        "schema_version=1", "distro=Manufactured-Ubuntu", "release_version=test-v1",
        "boot_id=11111111-2222-4333-8444-555555555555", "phase=stopping", "managed_device=/dev/nbd7"
    )
    $marker = ConvertFrom-LifecycleMarkerLines -Lines $valid -ExpectedDistro "Manufactured-Ubuntu"
    if ($marker.managed_device -cne "/dev/nbd7") { throw "manufactured_marker_legitimate_path_failed" }
    try { ConvertFrom-LifecycleMarkerLines -Lines ($valid + "phase=active") -ExpectedDistro "Manufactured-Ubuntu" | Out-Null; throw "manufactured_duplicate_marker_accepted" } catch { if ($_.Exception.Message -eq "manufactured_duplicate_marker_accepted") { throw } }
    try { ConvertFrom-LifecycleMarkerLines -Lines $valid -ExpectedDistro "Foreign-Ubuntu" | Out-Null; throw "manufactured_foreign_marker_accepted" } catch { if ($_.Exception.Message -eq "manufactured_foreign_marker_accepted") { throw } }
    if ((Get-RecoveryDecision -DistroRunning $false -Marker $marker -GuestStatus $null -UnitState "unknown") -cne "BLOCKED_DISTRO_NOT_RUNNING") { throw "manufactured_stopped_distro_was_not_blocked" }
    if ((Get-RecoveryDecision -DistroRunning $true -Marker $marker -GuestStatus ([pscustomobject]@{ state = "PENDING" }) -UnitState "deactivating") -cne "WAIT_FOR_CONTROLLER") { throw "manufactured_parallel_recovery_was_not_blocked" }
    Write-Output "PASS lifecycle_recovery_accepts_only_exact_sealed_marker"
    Write-Output "PASS lifecycle_recovery_never_starts_or_terminates_stopped_distro"
    Write-Output "PASS lifecycle_recovery_serializes_behind_active_controller"
    exit 0
}

if ($Action -eq "plan") {
    [ordered]@{
        state = "PLAN"
        distro = $Distro
        marker_path = $MarkerPath
        approval = "recover:$Distro"
        mutates_only_when_run = $true
        never_terminates_wsl = $true
        guest_controller_owns_swapoff_detach_and_daemon_stop = $true
    } | ConvertTo-Json -Depth 4
    exit 0
}

$marker = Read-LifecycleMarker
$running = Test-DistroRunning
if (-not $running) {
    [ordered]@{ state = if ($null -eq $marker) { "OFF" } else { "BLOCKED" }; reason = if ($null -eq $marker) { "distro_not_running_no_pending_marker" } else { "pending_marker_but_distro_not_running" }; distro = $Distro } | ConvertTo-Json -Depth 4
    if ($null -eq $marker) { exit 0 } else { exit 2 }
}

$guestStatus = Get-GuestRecoveryStatus
if ($Action -eq "status") {
    [ordered]@{ state = $guestStatus.state; distro = $Distro; marker = $marker; guest = $guestStatus } | ConvertTo-Json -Depth 6
    if ($guestStatus.state -ceq "CLEAN") { exit 0 } else { exit 2 }
}

if (-not $Run -or $ApproveRecovery -cne "recover:$Distro") {
    throw "recovery requires -Run and exact -ApproveRecovery recover:$Distro"
}
if ($null -eq $marker) {
    if ($guestStatus.state -ceq "CLEAN") { Write-Output '{"state":"CLEAN","reason":"no_pending_marker"}'; exit 0 }
    throw "recovery_without_sealed_marker_refused"
}

Request-ControllerStop
for ($attempt = 0; $attempt -lt 3; $attempt++) {
    Start-Sleep -Seconds 1
    $guestStatus = Get-GuestRecoveryStatus
    if ($guestStatus.state -ceq "CLEAN") {
        [ordered]@{ state = "CLEAN"; path = "systemd_controller"; distro = $Distro; guest = $guestStatus } | ConvertTo-Json -Depth 6
        exit 0
    }
}

$unitState = Get-ControllerUnitState
$decision = Get-RecoveryDecision -DistroRunning $true -Marker $marker -GuestStatus $guestStatus -UnitState $unitState
if ($decision -ceq "WAIT_FOR_CONTROLLER") {
    [ordered]@{ state = "PENDING"; reason = "controller_is_still_owning_swapoff"; unit_state = $unitState; guest = $guestStatus } | ConvertTo-Json -Depth 6
    exit 2
}
if ($decision -cne "RUN_RECOVERY_CONTROLLER") { throw "unexpected_recovery_decision" }

$recovery = Invoke-RecoveryController -Marker $marker
$guestStatus = Get-GuestRecoveryStatus
if ($recovery.completed -and $recovery.exit_code -eq 0 -and $guestStatus.state -ceq "CLEAN") {
    [ordered]@{ state = "CLEAN"; path = "recovery_controller"; distro = $Distro; guest = $guestStatus } | ConvertTo-Json -Depth 6
    exit 0
}
[ordered]@{ state = "PENDING"; reason = "swapoff_or_detach_not_proven"; distro = $Distro; recovery = $recovery; guest = $guestStatus } | ConvertTo-Json -Depth 7
exit 2
