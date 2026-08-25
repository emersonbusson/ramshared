#Requires -Version 5.1
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$target = Join-Path $PSScriptRoot "Watch-RamSharedWsl.ps1"
if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
    throw "ramshared_wsl_guardian: target script is missing"
}
$source = Get-Content -Raw -LiteralPath $target

function Require-GuardianContract {
    param([Parameter(Mandatory = $true)][string]$Text)
    if (-not $source.Contains($Text)) {
        throw "ramshared_wsl_guardian: missing contract $Text"
    }
}

foreach ($required in @(
    'ValidateSet("install", "status", "capture", "uninstall", "watch", "activate", "test")',
    'Register-ScheduledTask',
    'Unregister-ScheduledTask',
    'Get-ScheduledTask',
    'PT0S',
    'ramshared-heartbeat.json',
    'ValidateRange(15, 60)',
    'Invoke-GuestProbe',
    'Invoke-IndependentHostProbe',
    'Invoke-HostSnapshot',
    'safe-mode',
    'host-resume-lease.json',
    'guardian-config.json',
    'Publish-GuardianState',
    '.health.json',
    'Test-SealedGuardianIdentity',
    'Restore-SealedGuardianBackup',
    'New-GuardianInstallTransaction',
    'Invoke-GuardianInstallTransaction',
    'Rollback-GuardianInstallTransaction',
    'Register-ScheduledTask -TaskName $TaskName -Xml',
    '--terminate',
    'Ubuntu-24.04',
    'WaitForExit',
    'guardian-events.jsonl',
    'windows-telemetry.jsonl',
    'physical_memory_total_kib',
    'pagefile_used_mib',
    'vmmem_wsl',
    'origin_volume',
    'ramshared-origin-manifest.json',
    'Get-Volume -FilePath',
    'volume_unique_id',
    '[Convert]::ToBase64String',
    'termination_already_recorded',
    'Complete-TerminationRecord',
    'new_boot_id',
    'Get-GuardianTerminationDecision',
    'guardian_policy',
    '-HeartbeatPath',
    '-ArtifactRoot',
    '-StaleAfterSec',
    '-PollSec',
    '-GuestCommandTimeoutSec',
    '-Action watch -Run'
)) {
    Require-GuardianContract -Text $required
}
if ($source.Contains('Get-Volume -DriveLetter I') -or $source.Contains('drive_letter = "I"')) {
    throw 'ramshared_wsl_guardian: origin telemetry must discover the manifest physical volume'
}

foreach ($forbidden in @(
    'Ensure-OriginAttached',
    '--mount --vhd',
    'Get-DiskImage',
    '--shutdown',
    'Restart-Computer',
    'Stop-Computer',
    'shutdown.exe',
    'wsl.exe --terminate',
    'wsl.exe --shutdown'
)) {
    if ($source.Contains($forbidden)) {
        throw "ramshared_wsl_guardian: forbidden host or broad WSL action $forbidden"
    }
}

foreach ($required in @(
    'RAMSHARED_ATTENDED_GUARDIAN_ACTION',
    'guardian action requires exact attended approval',
    'Disable-ScheduledTask -TaskName $TaskName',
    '-Action watch -Run'
)) {
    Require-GuardianContract -Text $required
}

# A timed child-process call must not turn into an unbounded cleanup wait after
# its deadline. The guardian has to return to its independent control loop even
# when a WSL client does not exit promptly after termination is requested.
if ($source -match '\$process\.WaitForExit\(\)') {
    throw "ramshared_wsl_guardian: timeout cleanup must not wait without a bound"
}
if ($source -notmatch '\$process\.Dispose\(\)') {
    throw "ramshared_wsl_guardian: timeout cleanup must release its process handle"
}
if ($source -notmatch '\.StandardOutput\.ReadToEndAsync\(\)' -or
    $source -notmatch '\.StandardError\.ReadToEndAsync\(\)' -or
    $source -notmatch 'Stop-GuardianProcessInstanceSafely\s+-Process\s+\$process' -or
    $source -notmatch 'process_instance_handle_terminated' -or
    $source -notmatch 'process_tree_terminated') {
    throw "ramshared_wsl_guardian: bounded clients must drain pipes and fail closed without numeric PID termination"
}
if ($source -match '(?m)^\s*(?:Stop-Process|taskkill\.exe)') {
    throw 'ramshared_wsl_guardian: watchdog may not race a numeric PID force-kill'
}

$powershell = Join-Path $PSHOME "powershell.exe"
if (-not (Test-Path -LiteralPath $powershell -PathType Leaf)) { $powershell = "powershell.exe" }
$manufactured = @(& $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $target -Action test -Run -Distro "Manufactured-Ubuntu" -UserSid "S-1-5-21-100" -HeartbeatPath "C:\manufactured\heartbeat.json" -ArtifactRoot "C:\manufactured\artifacts" -StaleAfterSec 17 -PollSec 3 -GuestCommandTimeoutSec 7 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw ("ramshared_wsl_guardian: manufactured decision cases failed: " + ($manufactured -join "`n"))
}
foreach ($required in @(
    'PASS healthy_guest_with_stale_monitor_never_terminates',
    'PASS guardian_refuses_when_guest_probe_is_responsive',
    'PASS guardian_refuses_when_host_probe_is_healthy',
    'PASS guardian_refuses_when_snapshot_is_open',
    'PASS guardian_terminates_only_after_all_gates',
    'PASS guardian_refuses_duplicate_termination',
    'PASS guardian_task_arguments_seal_distro_timeout_and_artifact_policy',
    'PASS guardian_terminates_exactly_once_and_enters_safe_mode',
    'PASS host_safe_mode_gate_survives_guardian_and_guest_restart',
    'PASS guardian_never_uses_wsl_shutdown_or_host_restart',
    'PASS guardian_only_terminates_the_sealed_distro_after_all_gates',
    'PASS guardian_timeout_cleanup_is_bounded',
    'PASS disabled_guardian_task_never_mutates_wsl_or_disk_at_logon',
    'PASS guardian_install_failure_rolls_back_all_phases',
    'PASS guardian_install_preserves_prior_task_config_and_backup',
    'PASS guardian_new_task_is_disabled_or_absent_after_failure'
    'PASS one_timeout_or_single_guest_probe_refuses_termination'
    'PASS guardian_requires_two_distinct_guest_failures_and_dual_host_corroboration'
    'PASS boot_bound_healthy_proof_required_before_publish'
    'PASS guardian_activation_is_explicit_and_staging_remains_disabled'
)) {
    if (-not ($manufactured -join "`n").Contains($required)) {
        throw "ramshared_wsl_guardian: manufactured output missing $required"
    }
}

foreach ($required in @(
    'Get-GuestProbeFailureCount',
    'Test-CanonicalGuestBootId',
    'Invoke-GuardianWatchIteration',
    'guardian_watch_heartbeat_checked_before_healthy_publish',
    'guardian_watch_revalidates_guest_recovery_before_termination',
    'guardian_task_xml_fingerprint',
    'guardian_task_operator_edit_detected',
    'Invoke-GuardianWatchTerminationTail',
    'guardian_tail_refuses_recovered_guest',
    'guardian_task_registration_seal_pending_disable',
    'guardian_uninstall_unregister_failed',
    'Invoke-BoundedJsonQuery',
    'two_distinct_guest_probe_failures_required',
    'dual_wsl_hcs_corroboration_required',
    'schema_version = 3',
    'boot_id = $BootId'
    'guardian activation requires exact attended approval'
    'staging_capture_only = $true'
    'Stop-GuardianProcessInstanceSafely'
)) {
    Require-GuardianContract -Text $required
}
if ($source.Contains('-GuestFailed $true')) {
    throw 'ramshared_wsl_guardian: a precomputed guest-failure Boolean may not bypass probe aggregation'
}

# R4-HOST-01/02/09/10 RED: the source action test must drive the production
# watch iteration with injected observations, not merely manufacture a final
# decision Boolean.  These named outcomes are emitted only by that path.
foreach ($required in @(
    'PASS guardian_watch_heartbeat_checked_before_healthy_publish',
    'PASS guardian_watch_revalidates_guest_recovery_before_termination',
    'PASS guardian_task_xml_seal_refuses_operator_edit_and_preserves_backup',
    'PASS guardian_boot_id_rejects_noncanonical_guid'
    'PASS guardian_tail_refuses_recovered_guest_before_targeted_termination'
    'PASS guardian_register_success_disable_failure_restores_task_config_and_seal'
    'PASS guardian_uninstall_failure_retains_seal_and_state'
    'PASS guardian_activation_export_failure_restores_exact_task_seal_and_operator_config'
    'PASS guardian_pid_reuse_never_signals_foreign_process'
)) {
    if (-not ($manufactured -join "`n").Contains($required)) {
        throw "ramshared_wsl_guardian: production-flow output missing $required"
    }
}
