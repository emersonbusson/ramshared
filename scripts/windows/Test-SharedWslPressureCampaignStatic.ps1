#Requires -Version 5.1
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$script = Join-Path $root "scripts\windows\Invoke-SharedWslPressureCampaign.ps1"
$module = Join-Path $root "scripts\windows\SharedWslHostMemoryGate.psm1"
if (-not (Test-Path -LiteralPath $script)) {
    throw "missing script: $script"
}
if (-not (Test-Path -LiteralPath $module)) {
    throw "missing module: $module"
}

$text = Get-Content -LiteralPath $script -Raw
$moduleText = Get-Content -LiteralPath $module -Raw

function Import-CampaignFunction([string]$Name) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) { throw "campaign fixture source has parser errors" }
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $Name }, $true)
    if ($null -eq $definition) { throw "campaign fixture missing production function $Name" }
    $body = $definition.Body.Extent.Text
    Set-Item -Path ("Function:\script:{0}" -f $Name) -Value ([scriptblock]::Create($body.Substring(1, $body.Length - 2)))
}
$required = @(
    "ApproveSharedDailyHost",
    '[ValidatePattern(''^[A-Za-z0-9._-]+$'')]',
    '$SealedDistro = "Ubuntu-24.04"',
    'Distro does not match the sealed target',
    'wsl-terminate-unsealed-target.txt',
    "ExternalWorkloadMiB",
    "PostCampaignObserveSec",
    "HostCommitReserveMiB",
    "HostDiskLetters",
    "Resolve-CampaignHostDiskLetters",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss",
    "ramshared-origin-manifest.json",
    "origin_vhdx",
    "existing_wsl_swap_vhdx",
    "SharedWslHostMemoryGate.psm1",
    "host-memory-admission.json",
    "host-memory.jsonl",
    "Get-SharedWslHostCommitRequiredMiB",
    "Get-SharedWslHostMemorySample",
    "Test-SharedWslHostMemoryAdmission",
    "Test-SharedWslHostMemoryGuardian",
    "host_memory_gate_ok",
    "host_commit_headroom_mib",
    "host_commit_required_mib",
    "host_commit_reserve_mib",
    "host_memory_guardian_fired",
    "Invoke-SelectedDistroTermination",
    "Test-SelectedDistroTerminationContainment",
    "targeted_termination_unproven",
    "guest_pressure_stopped",
    "wsl-termination-containment.json",
    "NO_GO",
    "-SelectedDistro `$Distro",
    "host-disk-logical.jsonl",
    "host-disk-volumes.json",
    "Win32_PerfFormattedData_PerfDisk_LogicalDisk",
    "Win32_LogicalDisk",
    "host_disk_telemetry_ok",
    "Test-HostDiskTelemetryArtifacts",
    "volume_identity_missing",
    "sample_identity_missing",
    '$decodedVolumes',
    '$campaignPass = -not $watchdogFired',
    '$exitCode -eq 0',
    '$finalClean',
    '$host_disk_telemetry_ok',
    "Start-CudaVramWorkload.ps1",
    "external-workload.ps1",
    "external-workload.out",
    "external-workload.err",
    "external-phase-ready.txt",
    "external-phase-complete.txt",
    "campaign_validation_complete",
    "external phase completion timed out",
    "campaignStopwatch",
    "[cuda-vram-workload] released",
    "external_workload_ok",
    "RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_WSL_TERMINATION",
    "RAMSHARED_WINDOWS_WATCHDOG_ARMED=1",
    "RAMSHARED_ACTION_CLEANUP_GRACE_SEC",
    "RAMSHARED_PRESSURE_ALLOC_GIB",
    "PressureAllocGiB",
    "--approve-shared-daily-host",
    "--run-shared-daily-host",
    "cascade-health.sh --loop",
    "ramsharedd-logged.sh",
    "daemon.out",
    '>>"$artifactWsl/daemon.out"',
    "diagnose.json",
    "validation-rc.txt",
    "external_demote_ok",
    "matrixRowClose",
    "validated_external_global_gpu_demote",
    "matrix_row_close",
    "freeze_campaign_validated",
    "canario_demotes",
    "WriteAllText",
    '-replace "`r`n", "`n"',
    "RAMSHARED_TRACE_PROBE=1",
    'RAMSHARED_FREEZE_REQUIRED_ROUNDS="$Rounds"',
    "validate-wsl2-freeze-campaign-artifact.sh",
    "WaitForExit",
    "wsl.exe",
    "--terminate",
    "Stop-OptionalExternalWorkload",
    "ramshared down",
    "DISK_MUTATION = `$false"
)

foreach ($needle in $required) {
    if (-not $text.Contains($needle)) {
        throw "missing token: $needle"
    }
}

if ($text.Contains('>>"`$artifact/daemon.out"')) {
    throw "daemon wrapper must not depend on an unset runtime artifact variable"
}
if ($text.Contains('$volumes = @(Get-Content -LiteralPath $VolumePath -Raw | ConvertFrom-Json)')) {
    throw "PowerShell 5.1 must not wrap a ConvertFrom-Json array inside one pipeline item"
}
if ($text.Contains('[string[]]$HostDiskLetters = @("C", "I")')) {
    throw "pressure campaign must not default telemetry to C:/I:"
}

Import-CampaignFunction 'Normalize-HostDiskLetters'
Import-CampaignFunction 'Get-CampaignDriveLetterFromPath'
Import-CampaignFunction 'Resolve-CampaignHostDiskLetters'
$fixtureManifest = [pscustomobject]@{
    schema_version = 3
    origin_vhdx = 'R:\RamShared\origin.vhdx'
    existing_wsl_swap_vhdx = 'E:\WSL\swap.vhdx'
}
$fixtureDistros = @(
    [pscustomobject]@{ DistributionName = 'Fixture-Ubuntu'; BasePath = 'Q:\WSL\Ubuntu' }
)
$fixtureLetters = @(Resolve-CampaignHostDiskLetters -SelectedDistro 'Fixture-Ubuntu' `
    -OriginManifest $fixtureManifest -DistroRecords $fixtureDistros)
if (($fixtureLetters -join ',') -cne 'Q:,R:,E:') {
    throw "dynamic distro/origin volume discovery returned: $($fixtureLetters -join ',')"
}
$ambiguousRefused = $false
try {
    Resolve-CampaignHostDiskLetters -SelectedDistro 'Fixture-Ubuntu' `
        -OriginManifest $fixtureManifest -DistroRecords @($fixtureDistros[0], $fixtureDistros[0]) | Out-Null
} catch { $ambiguousRefused = $true }
if (-not $ambiguousRefused) {
    throw "ambiguous distro storage identity was accepted"
}

if ($text.IndexOf('Test-SharedWslHostMemoryAdmission') -gt
    $text.IndexOf('Start-HostDiskTelemetry -Letters')) {
    throw "host memory admission must happen before host disk telemetry"
}
if (-not $text.Contains('ArgumentList @("--terminate", $SelectedDistro)')) {
    throw "only the selected distro may be targeted for termination"
}
if (-not $text.Contains('Invoke-SelectedDistroTermination -SelectedDistro $SelectedDistro')) {
    throw 'termination containment must invoke the sealed selected distro termination path'
}
$terminateFunctionStart = $text.IndexOf('function Invoke-SelectedDistroTermination')
$terminateStart = $text.IndexOf('Start-Process -FilePath "wsl.exe"', $terminateFunctionStart)
$sealedTargetGuard = $text.IndexOf('if ($SelectedDistro -cne $SealedDistro)', $terminateFunctionStart)
if ($terminateFunctionStart -lt 0 -or $terminateStart -lt 0 -or $sealedTargetGuard -lt 0 -or
    $sealedTargetGuard -gt $terminateStart) {
    throw "an unsealed or foreign distro can reach the termination path"
}
foreach ($needle in @(
    "Invoke-PostLaunchCleanup",
    "Invoke-BoundedCampaignCimQuery",
    "Stop-HostDiskTelemetryBounded",
    "external_workload_containment_unproven",
    "termination_containment_required_after_command_failure",
    "BeginInvoke",
    "telemetry_stop_deadline_started",
    "Write-BestEffortArtifact",
    "host_memory_telemetry_ok",
    "host_memory_telemetry_write_failed"
    "Stop-CampaignProcessInstanceSafely"
    "Invoke-CampaignFinalization"
    "telemetry_lifecycle_finally_armed_before_start"
)) {
    if (-not $text.Contains($needle)) {
        throw "missing containment proof: $needle"
    }
}

if (-not $moduleText.Contains('Invoke-SharedWslBoundedPowerShellQuery') -or
    -not $moduleText.Contains('host_memory_query_deadline_exceeded')) {
    throw 'host memory CIM telemetry must be queried by a bounded child deadline'
}
# runtime_telemetry_write_failure_requests_cleanup
# guardian_marker_write_failure_cannot_precede_cleanup
# termination_start_failure_is_bounded_and_not_retried
# launcher_exit_stops_external_workload
if ([regex]::IsMatch($text, 'Write-BestEffortSummary\s+-Dir\s+\$artifactDir\s+-Status\s+"PASS"')) {
    throw "normal PASS paths must not swallow summary write failures"
}
foreach ($needle in @(
    "wsl-terminate-start-failed.txt",
    "wsl-terminate-wait-failed.txt"
)) {
    if (-not $text.Contains($needle)) {
        throw "missing bounded termination artifact: $needle"
    }
}
$cleanupCall = $text.LastIndexOf('Invoke-PostLaunchCleanup -LauncherProcess')
$summaryCall = $text.IndexOf('Write-BestEffortSummary -Dir $artifactDir -Status $terminalStatus')
if ($cleanupCall -lt 0 -or $summaryCall -lt 0 -or $cleanupCall -gt $summaryCall) {
    throw "containment must precede partial summary writes"
}
$cleanupStart = $text.IndexOf('function Invoke-PostLaunchCleanup')
$cleanupEnd = $text.IndexOf('function Write-BestEffortSummary')
$cleanupBlock = $text.Substring($cleanupStart, $cleanupEnd - $cleanupStart)
if ($cleanupBlock.IndexOf('Stop-OptionalExternalWorkload') -gt $cleanupBlock.IndexOf('Stop-HostDiskTelemetry')) {
    throw "external workload cleanup must precede disk telemetry cleanup"
}
foreach ($needle in @(
    "host_commit_headroom_insufficient",
    "host_memory_query_failed",
    "host_commit_reserve_breached",
    "host_memory_telemetry_stale"
)) {
    if (-not $moduleText.Contains($needle)) {
        throw "missing memory gate reason: $needle"
    }
}

$forbidden = @(
    "RAMSHARED_ALLOW_RECENT_OOM_MARKER",
    "Initialize-Disk",
    "Format-Volume",
    "Resize-VHD",
    "New-VHD",
    "New-VM",
    "Start-VM",
    "Stop-VM",
    "Remove-VM",
    "Clear-Disk",
    "diskpart",
    "wsl.exe --shutdown",
    "Restart-Computer",
    "Stop-Computer",
    "shutdown.exe"
)

foreach ($needle in $forbidden) {
    if ($text -match [regex]::Escape($needle)) {
        throw "forbidden token: $needle"
    }
}

# R4-HOST-05: execute the real cleanup orchestration with contained fixture
# operations.  A terminate client outcome (success, timeout, or nonzero) is
# never enough: each branch must invoke the selected-distro absence probe and
# report unproven containment as terminally false.
Import-CampaignFunction 'Invoke-PostLaunchCleanup'
$script:containmentProbeCount = 0
$script:terminationCase = $null
function Stop-OptionalExternalWorkload { param($Process) [pscustomobject]@{ stopped = $true; reason = 'fixture_external_stopped' } }
function Stop-LauncherProcessBounded { param($Process) [pscustomobject]@{ stopped = $true; reason = 'fixture_launcher_stopped' } }
function Stop-HostDiskTelemetryBounded { param($Job) [pscustomobject]@{ stopped = $true; reason = 'fixture_telemetry_stopped' } }
function Invoke-SelectedDistroTermination {
    param([string]$SelectedDistro, [ref]$TerminationIssued, [string]$Dir)
    $TerminationIssued.Value = $true
    [pscustomobject]@{ termination_completed = [bool]$script:terminationCase.completed; contained = $false; guest_pressure_stopped = $false; reason = [string]$script:terminationCase.reason }
}
function Test-SelectedDistroTerminationContainment {
    param([string]$SelectedDistro, [string]$Dir)
    $script:containmentProbeCount++
    [pscustomobject]@{ contained = $false; guest_pressure_stopped = $false; reason = 'fixture_selected_distro_still_unproven' }
}
foreach ($case in @(
        [pscustomobject]@{ name = 'success'; completed = $true; reason = 'termination_command_completed' },
        [pscustomobject]@{ name = 'timeout'; completed = $false; reason = 'termination_deadline_exceeded' },
        [pscustomobject]@{ name = 'nonzero'; completed = $false; reason = 'termination_nonzero_exit' })) {
    $script:terminationCase = $case
    $script:containmentProbeCount = 0
    $issued = $false
    $cleanup = Invoke-PostLaunchCleanup -LauncherProcess $null -ExternalProcess $null -DiskTelemetryJob $null `
        -TerminateSelectedDistro $true -SelectedDistro 'Ubuntu-24.04' -TerminationIssued ([ref]$issued) -Dir $env:TEMP
    if ($cleanup.cleanup_proven -or $script:containmentProbeCount -ne 1 -or $cleanup.termination.contained -or $cleanup.termination.guest_pressure_stopped) {
        throw "campaign cleanup did not fail closed after terminate $($case.name)"
    }
}

# R4-HOST-03: the production outer try/finally is armed before telemetry job
# creation, guest-script serialization, and launcher start. Its real cleanup
# orchestration must stop a created telemetry job on the early-failure route.
$telemetryTryOffset = $text.IndexOf('try {', $text.IndexOf('$telemetry_lifecycle_finally_armed_before_start = $true'))
$telemetryStartOffset = $text.IndexOf('$hostDiskJob = Start-HostDiskTelemetry', $telemetryTryOffset)
$guestWriteOffset = $text.IndexOf('[System.IO.File]::WriteAllText($guestScriptWin', $telemetryStartOffset)
$launcherStartOffset = $text.IndexOf('$proc = Start-Process -FilePath "wsl.exe"', $guestWriteOffset)
$telemetryFinallyOffset = $text.IndexOf('} finally {', $launcherStartOffset)
if ($telemetryTryOffset -lt 0 -or $telemetryStartOffset -lt 0 -or $guestWriteOffset -lt 0 -or
    $launcherStartOffset -lt 0 -or $telemetryFinallyOffset -lt 0 -or
    -not ($telemetryTryOffset -lt $telemetryStartOffset -and $telemetryStartOffset -lt $guestWriteOffset -and
          $guestWriteOffset -lt $launcherStartOffset -and $launcherStartOffset -lt $telemetryFinallyOffset)) {
    throw 'campaign_telemetry_lifecycle_is_not_armed_before_job_write_and_launcher'
}
$script:earlyFailureTelemetryStops = 0
function Stop-HostDiskTelemetryBounded { param($Job) $script:earlyFailureTelemetryStops++; [pscustomobject]@{ stopped = $true; reason = 'fixture_telemetry_stopped' } }
$earlyIssued = $false
$earlyFailureCleanup = Invoke-PostLaunchCleanup -LauncherProcess $null -ExternalProcess $null -DiskTelemetryJob ([pscustomobject]@{ State = 'Running' }) `
    -TerminateSelectedDistro $true -SelectedDistro 'Ubuntu-24.04' -TerminationIssued ([ref]$earlyIssued) -Dir $env:TEMP
if ($script:earlyFailureTelemetryStops -ne 1 -or $earlyFailureCleanup.cleanup_proven) {
    throw 'campaign_early_failure_did_not_stop_created_telemetry_before_terminal_containment'
}

# R4-HOST-02: execute the real finalization branch. An incomplete ordinary
# cleanup first records its terminal reason, then requests selected-distro
# containment before a caller can publish an outcome.
Import-CampaignFunction 'Get-CampaignCleanupFailureReason'
Import-CampaignFunction 'Invoke-CampaignFinalization'
$script:finalizationOrder = @()
function Get-CampaignCleanupFailureReason {
    param($Cleanup)
    $script:finalizationOrder += 'reason'
    'external_workload_containment_unproven'
}
function Invoke-PostLaunchCleanup {
    param($LauncherProcess, $ExternalProcess, $DiskTelemetryJob, $TerminateSelectedDistro, $SelectedDistro, $TerminationIssued, $Dir)
    $script:finalizationOrder += ('cleanup:' + [string]$TerminateSelectedDistro)
    [pscustomobject]@{
        cleanup_proven = $false
        termination = [pscustomobject]@{ contained = $false; guest_pressure_stopped = $false }
        external = [pscustomobject]@{ stopped = $true }
        launcher = [pscustomobject]@{ stopped = $true }
        telemetry = [pscustomobject]@{ stopped = $true }
    }
}
$normalUnproven = [pscustomobject]@{
    cleanup_proven = $false
    termination = [pscustomobject]@{ contained = $true; guest_pressure_stopped = $true }
    external = [pscustomobject]@{ stopped = $false }
    launcher = [pscustomobject]@{ stopped = $true }
    telemetry = [pscustomobject]@{ stopped = $true }
}
$issued = $false
$finalization = Invoke-CampaignFinalization -InitialCleanup $normalUnproven -LauncherProcess $null -ExternalProcess $null `
    -DiskTelemetryJob $null -SelectedDistro 'Ubuntu-24.04' -TerminationIssued ([ref]$issued) -Dir $env:TEMP
if (-not $finalization.containment_requested -or $script:finalizationOrder.Count -lt 2 -or
    $script:finalizationOrder[0] -ne 'reason' -or $script:finalizationOrder[1] -ne 'cleanup:True') {
    throw 'campaign_normal_cleanup_unproven_did_not_set_reason_then_request_selected_distro_containment'
}

# Execute the production telemetry-stop function itself (without replacing it
# with a stub): null means no job was ever created and must return immediately.
Import-CampaignFunction 'Stop-HostDiskTelemetryBounded'
$directTelemetryStop = Stop-HostDiskTelemetryBounded -Job $null
if (-not $directTelemetryStop.stopped -or $directTelemetryStop.reason -ne 'telemetry_not_started') {
    throw 'campaign telemetry stop production function did not preserve the no-job bounded path'
}

Import-CampaignFunction 'Stop-CampaignProcessInstanceSafely'
$campaignPidReuseModel = [pscustomobject]@{ HasExited = $false; StartTime = [DateTime]::UtcNow.AddMinutes(-1); Handle = [IntPtr]1; foreign_signal_count = 0 }
$campaignPidReuseModel | Add-Member -MemberType ScriptMethod -Name Refresh -Value { $this.StartTime = [DateTime]::UtcNow }
$campaignPidReuse = Stop-CampaignProcessInstanceSafely -Process $campaignPidReuseModel -Operation 'fixture'
if ($campaignPidReuse.stopped -or $campaignPidReuse.reason -ne 'fixture_process_instance_identity_changed' -or $campaignPidReuseModel.foreign_signal_count -ne 0) {
    throw 'campaign_pid_reuse_never_signals_foreign_process failed: reused PID was not refused'
}
if ($text -match '(?m)^\s*Stop-Process\s+-Id') {
    throw 'campaign_pid_reuse_never_signals_foreign_process failed: numeric PID Stop-Process remains in campaign source'
}

Write-Host "STATIC_SHARED_WSL_PRESSURE_CAMPAIGN=PASS"
Write-Host "PASS unsealed_or_foreign_target_cannot_reach_termination"
Write-Host "PASS targeted_termination_requires_distro_absence_and_guest_pressure_stop_proof"
Write-Host "PASS campaign_disk_telemetry_and_job_cleanup_are_deadline_bounded"
Write-Host "PASS all_terminate_outcomes_require_external_and_guest_containment_proof"
Write-Host "PASS telemetry_stop_deadline_precedes_nonblocking_stop_invocation"
Write-Host "PASS campaign_pid_reuse_never_signals_foreign_process"
Write-Host "PASS normal_cleanup_unproven_requests_selected_distro_containment_before_outcome"
Write-Host "PASS campaign_early_failure_stops_created_telemetry_before_terminal_containment"
Write-Host "PASS legacy_full_capacity_selector_is_absent"
