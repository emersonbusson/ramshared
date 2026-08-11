#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ScriptPath,
    [string]$MeasurementPath,
    [string]$DriverPath,
    [string]$ControlPath,
    [string]$VirtualDiskPath,
    [string]$QueuePath,
    [string]$InfPath,
    [string]$IoctlValidationPath,
    [string]$GuestLifecyclePath,
    [string]$GuestPackagePath,
    [string]$GuestPsDirectPath,
    [string]$GuestPsDirectDeadlineTestPath,
    [string]$ProtectedEvidencePath,
    [switch]$SkipDriverIdentity
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Invoke-WindowsDiskCounterAudit.ps1"
}
$text = Get-Content -LiteralPath $ScriptPath -Raw
if ([string]::IsNullOrWhiteSpace($MeasurementPath)) {
    $MeasurementPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "Measure-RamSharedDiskIo.ps1"
}
if ([string]::IsNullOrWhiteSpace($DriverPath)) {
    $DriverPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "..\..\drivers\windows\ramshared\driver.c"
}
if ([string]::IsNullOrWhiteSpace($InfPath)) {
    $InfPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "..\..\drivers\windows\ramshared\ramshared.inf"
}
if ([string]::IsNullOrWhiteSpace($ControlPath)) {
    $ControlPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "..\..\drivers\windows\ramshared\control.c"
}
if ([string]::IsNullOrWhiteSpace($VirtualDiskPath)) {
    $VirtualDiskPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "..\..\drivers\windows\ramshared\virtdisk.c"
}
if ([string]::IsNullOrWhiteSpace($QueuePath)) {
    $QueuePath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "..\..\drivers\windows\ramshared\queue.c"
}
if ([string]::IsNullOrWhiteSpace($IoctlValidationPath)) {
    $IoctlValidationPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "Invoke-WinDriveIoctlValidation.ps1"
}
if ([string]::IsNullOrWhiteSpace($GuestLifecyclePath)) {
    $GuestLifecyclePath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "Run-GuestAutonomousLifecycle.ps1"
}
if ([string]::IsNullOrWhiteSpace($GuestPackagePath)) {
    $GuestPackagePath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "Run-GuestProductPackage.ps1"
}
if ([string]::IsNullOrWhiteSpace($GuestPsDirectPath)) {
    $GuestPsDirectPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "Invoke-GuestPsDirectBounded.ps1"
}
if ([string]::IsNullOrWhiteSpace($GuestPsDirectDeadlineTestPath)) {
    $GuestPsDirectDeadlineTestPath = Join-Path `
        (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "Test-GuestPsDirectDeadlineStatic.ps1"
}
if ([string]::IsNullOrWhiteSpace($ProtectedEvidencePath)) {
    $ProtectedEvidencePath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "Copy-RamSharedProtectedEvidence.ps1"
}
$measurement = Get-Content -LiteralPath $MeasurementPath -Raw
$driver = Get-Content -LiteralPath $DriverPath -Raw
$control = Get-Content -LiteralPath $ControlPath -Raw
$virtualDisk = Get-Content -LiteralPath $VirtualDiskPath -Raw
$queue = Get-Content -LiteralPath $QueuePath -Raw
$inf = Get-Content -LiteralPath $InfPath -Raw
$ioctlValidation = Get-Content -LiteralPath $IoctlValidationPath -Raw
$guestLifecycle = Get-Content -LiteralPath $GuestLifecyclePath -Raw
$guestPackage = Get-Content -LiteralPath $GuestPackagePath -Raw
$guestPsDirect = Get-Content -LiteralPath $GuestPsDirectPath -Raw
$protectedEvidence = Get-Content -LiteralPath $ProtectedEvidencePath -Raw

foreach ($needle in @(
    "PLAN_ONLY=1",
    "LEGACY_RETIRED=1",
    "Invoke-WindowsStorageMatrix.ps1",
    "Invoke-WindowsDiskCounterAudit.ps1 is retired"
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("windows_disk_counter_audit_static: missing " + $needle)
    }
}

foreach ($forbiddenLegacy in @(
    "Get-WinDrivePreflight.ps1",
    "Run-HostExhaustive.ps1",
    "delegated ARTIFACT recovered from latest exhaustive directory",
    "exhaustive-*",
    "DISK_IO_MEASURE_OK",
    "LUN_GONE",
    "WIN32_GONE",
    "PNP_GONE"
)) {
    if ($text -match [regex]::Escape($forbiddenLegacy)) {
        throw ("legacy_counter_audit_run_is_retired: forbidden live token " + $forbiddenLegacy)
    }
}
Write-Output "PASS legacy_counter_audit_run_is_retired"

foreach ($forbidden in @(
    "Initialize-Disk",
    "Format-Volume",
    "New-Partition",
    "Clear-Disk",
    "Remove-Partition"
)) {
    if ($text -match [regex]::Escape($forbidden)) {
        throw ("windows_disk_counter_audit_static: forbidden token " + $forbidden)
    }
}

if ($measurement -match "Get-Date\s+-UFormat\s+%s" -or
    $measurement -notmatch "\[DateTime\]::UtcNow\.Ticks" -or
    $measurement -notmatch "measurement_error_exits_7" -or
    $measurement -notmatch '\$ErrorActionPreference\s*=\s*"Stop"') {
    throw "seed_is_powershell51_safe failed"
}
Write-Output "PASS seed_is_powershell51_safe"
Write-Output "PASS measurement_errors_fail_nonzero"

if (-not $SkipDriverIdentity) {
    if ($driver -notmatch "StorPortSetAdapterBusType" -or
        $driver -notmatch "BusTypeVirtual") {
        throw "virtual_bus_is_explicit failed"
    }
    if ($driver -match "BusTypeNvme|BusTypeSata|BusTypeSas") {
        throw "no_false_physical_bus failed"
    }
    if ($inf -notmatch 'HKR,\s*"Parameters",\s*"BusType",\s*0x00010001,\s*0x0000000E') {
        throw "inf_virtual_bus_fallback failed"
    }
    if ($inf -match "0x0000000A") {
        throw "no_false_physical_bus failed"
    }
    Write-Output "PASS virtual_bus_is_explicit"
    Write-Output "PASS inf_virtual_bus_fallback"
    Write-Output "PASS no_false_physical_bus"
    if ($virtualDisk -notmatch 'STATUS_INSUFFICIENT_RESOURCES[\s\S]*StorPortDeviceBusy' -or
        $virtualDisk -notmatch 'Srb->ScsiStatus\s*=\s*SCSISTAT_BUSY' -or
        $virtualDisk -notmatch 'Srb->SrbStatus\s*=\s*SRB_STATUS_BUSY') {
        throw "queue_full_returns_storport_busy failed"
    }
    Write-Output "PASS queue_full_returns_storport_busy"
    if ($driver -notmatch 'MaximumTransferLength\s*=\s*RAMSHARED_MATRIX_MAX_IO' -or
        $driver -notmatch 'MaxNumberOfIO\s*=\s*RAMSHARED_MAX_QD' -or
        $driver -notmatch 'MaxIOsPerLun\s*=\s*RAMSHARED_MAX_QD' -or
        $driver -notmatch 'InitialLunQueueDepth\s*=\s*1') {
        throw "adapter_transfer_limit_matches_smallest_matrix_io failed"
    }
    Write-Output "PASS adapter_transfer_limit_matches_smallest_matrix_io"
    Write-Output "PASS initial_lun_queue_depth_is_one"
    if ($virtualDisk -notmatch 'InquirySeen' -or
        $virtualDisk -notmatch 'StorPortSetDeviceQueueDepth' -or
        $virtualDisk -notmatch 'AppliedQueueDepth') {
        throw "registered_depth_applied_after_inquiry failed"
    }
    Write-Output "PASS registered_depth_applied_after_inquiry"
    if ($control -notmatch 'VdApplyRegisteredQueueDepth' -or
        $control -notmatch 'QUnregister[\s\S]*STATUS_DEVICE_CONFIGURATION_ERROR') {
        throw "queue_depth_apply_failure_refuses_registration failed"
    }
    Write-Output "PASS queue_depth_apply_failure_refuses_registration"
}

if ($ioctlValidation -notmatch 'FriendlyName\s+-like\s+"RAMSHARE\*"' -or
    $ioctlValidation -notmatch 'PROPERTY_IDENTITY_MISSING' -or
    $ioctlValidation -notmatch '\$script:EarlyPropertyDisk\s*=\s*\$d') {
    throw "property_identity_uses_exact_lun failed"
}
Write-Output "PASS property_identity_uses_exact_lun"

if ($guestPackage -notmatch '\[uint32\]\$QueueDepth' -or
    $guestPackage -notmatch 'queue_depth = \$QueueDepth' -or
    $guestLifecycle -notmatch 'qd1_mixed_flush_has_zero_retry_events' -or
    $guestLifecycle -notmatch 'Id\s*=\s*153' -or
    $guestLifecycle -notmatch 'mixed70r30w' -or
    $guestLifecycle -notmatch 'Flush\(\$true\)') {
    throw "qd1_mixed_flush_has_zero_retry_events failed"
}
Write-Output "PASS qd1_mixed_flush_has_zero_retry_events"

if ($queue -notmatch 'REENTRANT_REQUEST_COMPLETE_SLOT_FREE' -or
    $queue -notmatch 'RamSlotFree;[\s\S]{0,500}StorPortNotification\(RequestComplete') {
    throw "queue_slot_freed_before_request_complete failed"
}
Write-Output "PASS queue_slot_freed_before_request_complete"

if (-not $SkipDriverIdentity) {
    if ($inf -notmatch 'DriverVer\s*=\s*08/09/2026,10\.0\.26200\.8') {
        throw "driverver_increments_for_queue_depth_fix failed"
    }
    Write-Output "PASS driverver_increments_for_queue_depth_fix"
}

if ($guestLifecycle -notmatch 'ManufacturedWorkloadFailure' -or
    $guestLifecycle -notmatch 'failure-cleanup\.json' -or
    $guestLifecycle -notmatch 'sc\.exe stop RamSharedWinSvc' -or
    $guestLifecycle -notmatch 'consumer_state.+Stopped[\s\S]{0,800}sc\.exe stop RamSharedBroker') {
    throw "qd1_failure_runs_supported_cleanup failed"
}
Write-Output "PASS qd1_failure_runs_supported_cleanup"

if ($guestLifecycle -notmatch 'format-timeout-recovery\.json' -or
    $guestLifecycle -notmatch 'exact recovery identity count' -or
    $guestLifecycle -notmatch 'recovery partition mismatch' -or
    $guestLifecycle -notmatch 'refuse recovery: volume published' -or
    $guestLifecycle -notmatch 'Clear-Disk[\s\S]{0,200}-RemoveData' -or
    $guestLifecycle -notmatch 'Wait-Job\s+\$recoveryJob\s+-Timeout\s+60') {
    throw "vm_partial_format_timeout_recovers_exact_scratch_lun failed"
}
Write-Output "PASS vm_partial_format_timeout_recovers_exact_scratch_lun"

if ($guestLifecycle -notmatch 'all_registered_depths_have_zero_disk_retries' -or
    $guestLifecycle -notmatch 'registered QD emitted disk Event 153' -or
    $guestLifecycle -notmatch 'Get-WinEvent[\s\S]{0,250}-ErrorAction Stop') {
    throw "all_registered_depths_have_zero_disk_retries failed"
}
Write-Output "PASS all_registered_depths_have_zero_disk_retries"

if ($guestLifecycle -notmatch 'harness_waits_for_current_run_online' -or
    $guestLifecycle -notmatch 'run-\$ServicePid-\*\.jsonl' -or
    $guestLifecycle -notmatch 'current winsvc run did not reach Online' -or
    $guestLifecycle -notmatch 'phase\s*-ceq\s*"FailedSafe"' -or
    $guestLifecycle -notmatch 'lun_serial' -or
    $guestLifecycle -notmatch 'current winsvc Online identity is invalid' -or
    $guestLifecycle -match 'Get-ChildItem[\s\S]{0,300}-ErrorAction SilentlyContinue') {
    throw "harness_waits_for_current_run_online failed"
}
Write-Output "PASS harness_waits_for_current_run_online"

if ($guestLifecycle -match 'Get-WinEvent[\s\S]{0,250}-ErrorAction SilentlyContinue' -or
    $guestLifecycle -notmatch 'ProviderName\s*=\s*"disk"' -or
    $guestLifecycle -notmatch 'NoMatchingEventsFound' -or
    $guestLifecycle -notmatch 'event153_query_failure_is_red') {
    throw "event153_query_failure_is_red failed"
}
Write-Output "PASS event153_query_failure_is_red"

if ($guestLifecycle -match 'Get-Volume[\s\S]{0,120}-ErrorAction SilentlyContinue' -or
    $guestLifecycle -notmatch 'recovery_volume_query_failure_is_red') {
    throw "recovery_volume_query_failure_is_red failed"
}
Write-Output "PASS recovery_volume_query_failure_is_red"

if ($guestLifecycle -notmatch 'current_online_evidence_failure_is_red' -or
    $guestLifecycle -notmatch 'Get-ChildItem[\s\S]{0,250}-ErrorAction Stop' -or
    $guestLifecycle -notmatch 'run_id[\s\S]{0,300}lun_serial') {
    throw "current_online_evidence_failure_is_red failed"
}
Write-Output "PASS current_online_evidence_failure_is_red"

if ($protectedEvidence -notmatch 'function\s+Resolve-CanonicalArtifactDestination' -or
    $protectedEvidence -notmatch 'raw destination contains \.\. path segment' -or
    $protectedEvidence -notmatch '\[IO\.Path\]::GetFullPath' -or
    $protectedEvidence -notmatch 'function\s+Assert-PathHasNoReparsePoint') {
    throw "protected_evidence_dotdot_is_refused failed"
}
Write-Output "PASS protected_evidence_dotdot_is_refused"

if ($protectedEvidence -notmatch 'ReparsePoint' -or
    $protectedEvidence -notmatch 'Get-ChildItem[\s\S]{0,250}-Force' -or
    $protectedEvidence -notmatch 'source evidence path is a reparse point') {
    throw "protected_evidence_reparse_is_refused failed"
}
Write-Output "PASS protected_evidence_reparse_is_refused"

if ($protectedEvidence -notmatch '\[int\]\$MaxFiles\s*=\s*4096' -or
    $protectedEvidence -notmatch '\[UInt64\]\$MaxBytes\s*=\s*268435456' -or
    $protectedEvidence -notmatch 'evidence-copy-inventory\.json' -or
    $protectedEvidence -notmatch 'Get-FileHash' -or
    $protectedEvidence -notmatch 'source_sha256' -or
    $protectedEvidence -notmatch 'destination_sha256' -or
    $protectedEvidence -notmatch '\.staging-' -or
    $protectedEvidence -notmatch 'source/destination SHA256 mismatch') {
    throw "protected_evidence_copy_is_bounded_and_hashed failed"
}
Write-Output "PASS protected_evidence_copy_is_bounded_and_hashed"

if ($protectedEvidence -notmatch '\[IO\.Path\]::GetDirectoryName\(\$target\)' -or
    $protectedEvidence -match 'Split-Path\s+-LiteralPath\s+\$\w+\s+-Parent' -or
    $protectedEvidence -notmatch 'finally\s*\{' -or
    $protectedEvidence -notmatch 'Remove-SafeStaging\s+\$staging' -or
    $protectedEvidence -notmatch '\[IO\.Directory\]::Delete\(\$canonicalPath,\s*\$true\)') {
    throw "protected_evidence_positive_copy_is_ps51_safe_and_cleans_staging failed"
}
Write-Output "PASS protected_evidence_positive_copy_is_ps51_safe_and_cleans_staging"

if ($guestLifecycle -notmatch '\$pendingDeadline\s*=\s*\(Get-Date\)\.AddSeconds\(5\)' -or
    $guestLifecycle -notmatch 'Get-Service\s+RamSharedWinSvc[\s\S]{0,250}Status\s*-eq\s*"StopPending"' -or
    $guestLifecycle -notmatch 'refusal_state_observation_is_bounded' -or
    $guestLifecycle -notmatch 'refused STOP did not remain StopPending; observed=') {
    throw "refusal_state_observation_is_bounded failed"
}
Write-Output "PASS refusal_state_observation_is_bounded"

if ($guestPackage -notmatch 'Invoke-GuestPsDirectBounded' -or
    $guestLifecycle -notmatch 'Invoke-GuestPsDirectBounded' -or
    $guestPsDirect -notmatch 'New-PSSession' -or
    $guestPsDirect -notmatch 'AddSeconds\(\[int\]\$payload\.connect_timeout_seconds\)' -or
    $guestPsDirect -notmatch 'Start-Sleep\s+-Seconds\s+3' -or
    $guestPsDirect -notmatch 'PowerShell Direct unavailable after') {
    throw "guest_product_package_retries_psdirect_readiness failed"
}
Write-Output "PASS guest_product_package_retries_psdirect_readiness"

& $GuestPsDirectDeadlineTestPath -HelperPath $GuestPsDirectPath `
    -GuestLifecyclePath $GuestLifecyclePath -GuestPackagePath $GuestPackagePath

if ($guestLifecycle -notmatch '\[string\]\$HostBinDir' -or
    $guestLifecycle -notmatch '\$packageHarness[\s\S]{0,500}-HostBinDir\s+\$HostBinDir') {
    throw "guest_lifecycle_forwards_explicit_host_bin_dir failed"
}
Write-Output "PASS guest_lifecycle_forwards_explicit_host_bin_dir"

Write-Output "PASS Test-WindowsDiskCounterAuditStatic"
