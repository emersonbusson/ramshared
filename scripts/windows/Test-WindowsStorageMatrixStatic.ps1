#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath,
    [string]$WindowsHostPath,
    [string]$ProductOnlinePath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "Invoke-WindowsStorageMatrix.ps1"
}
if ([string]::IsNullOrWhiteSpace($WindowsHostPath)) {
    $WindowsHostPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "..\..\crates\ramshared-winsvc\src\windows_host.rs"
}
if ([string]::IsNullOrWhiteSpace($ProductOnlinePath)) {
    $ProductOnlinePath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "..\..\crates\ramshared-winsvc\src\product_online.rs"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw
$windowsHost = Get-Content -LiteralPath $WindowsHostPath -Raw
$productOnline = Get-Content -LiteralPath $ProductOnlinePath -Raw

if ($text -match 'Join-Path\s+\$PSHOME\s+["'']powershell\.exe["'']' -or
    $text -notmatch 'function Get-CurrentPowerShellExecutable[\s\S]*?Get-Process -Id \$PID[\s\S]*?Test-Path -LiteralPath \$path -PathType Leaf') {
    throw "static_child_uses_current_host_executable failed: production harness uses a fixed PowerShell filename"
}

$cells = @(
    'minimum", 67108864, 4096, 1, 1048576',
    'small", 268435456, 4096, 4, 1048576',
    'operator", 1073741824, 4096, 4, 1048576',
    'compat", 1073741824, 512, 4, 1048576',
    'concurrency", 2147483648, 4096, 16, 262144'
)
foreach ($cell in $cells) {
    if ($text -notmatch [regex]::Escape($cell)) {
        throw "matrix_cells_exact failed: missing $cell"
    }
}
Write-Output "PASS matrix_cells_exact"

$contracts = [ordered]@{
    three_runs_per_cell = '\[ValidateRange\(3,\s*3\)\]\s*\[int\]\$Runs\s*=\s*3'
    gpu_reserve_refusal = 'insufficient GPU reserve'
    gpu_numeric_sample_is_authoritative = 'samples\.Length -ne 1[\s\S]*\^\\s\*\(\\d\+\)\\s\*\$'
    foreign_volume_refusal = 'foreign volume occupies target letter'
    watchdog_required = 'watchdog\.armed'
    watchdog_tracks_stale_progress = 'watchdogStaleSeconds\s*=\s*600[\s\S]*Update-WatchdogHeartbeat'
    watchdog_shutdown_requires_separate_approval = '\[switch\]\$AllowWatchdogShutdown[\s\S]*if\(\$allowShutdown\)\s*\{\s*shutdown\.exe'
    consumer_first_stop_required = 'sc\.exe stop RamSharedWinSvc'
    regression_thresholds_exact = 'throughputRegressionPct -gt 20|throughput_regression_red_pct\s*=\s*20'
    binary_match_required = 'BINARY_MATCH'
    live_binary_match_required = 'Assert-LiveBinaryMatch'
    immutable_packages_required = 'immutable matrix package already exists'
    package_revision_required = 'PackageRevision is required and must be alphanumeric with optional hyphens'
    package_revision_in_version = '0\.1\.2-matrix-\$name-\$PackageRevision'
    workload_footprint_is_bounded = 'AvailableBytes \* 0\.25 / \$QueueDepth[\s\S]*worker_file_bytes'
    disk_enumeration_is_bounded = 'AddSeconds\(30\)[\s\S]*Start-Sleep -Milliseconds 250[\s\S]*enumeration_ms'
    cell_config_is_asserted_before_seal = 'prepared winsvc config mismatch[\s\S]*prepared broker capacity mismatch[\s\S]*Get-FileHash'
    inflight_limit_is_asserted = 'qd \* \[UInt64\]\$maxIo -gt 4MB'
    corrected_winsvc_packaged = 'Copy-Item \$WinsvcBinary.*ramshared-winsvc\.exe'
    corrected_broker_packaged = 'Copy-Item \$BrokerBinary.*ramshared-winbroker\.exe'
    consumer_stop_is_bounded = 'consumer-first stop timeout state='
    success_keeps_operator = 'operator\\product-manifest\.json'
    primary_error_not_masked = 'primary_error\s*=\s*if \(\$primaryError\)'
    exact_identity_required = 'BusType.*Virtual.*MediaType.*SSD'
    workloads_exact = '"seq1m",\s*"rand4k",\s*"mixed70r30w",\s*"flush",\s*"integrity"'
    concurrent_workers_match_qd = 'effective queue depth mismatch'
    latency_percentiles_recorded = 'latency_p50_ms[\s\S]*latency_p99_ms'
    baseline_is_applied = 'throughput_regression_pct[\s\S]*latency_ratio'
    canonical_direct_counter_probe = 'Measure-RamSharedDiskIo\.ps1[\s\S]*ChecksumRounds 3[\s\S]*ExpectedSerial[\s\S]*ExpectedSizeBytes[\s\S]*storage\.size'
    powershell51_probe_path_resolved_after_param = 'MyInvocation\.MyCommand\.Path[\s\S]*Measure-RamSharedDiskIo\.ps1'
    run_switch_has_no_case_insensitive_counter_collision = 'for \(\$repetition = 1;[\s\S]*\$repetition-\$workload'
    plan_only_default = 'PLAN_ONLY=1'
    context_manifest_complete = 'schema_version[\s\S]*platform_fingerprint[\s\S]*artifact_inventory'
    live_binary_match_is_persisted = 'loaded_driver_sha256[\s\S]*loaded_winsvc_sha256[\s\S]*loaded_broker_sha256'
    summary_includes_deviation = 'stddev_mib_per_sec[\s\S]*min_mib_per_sec[\s\S]*max_mib_per_sec'
    all_expected_matrix_rows_present = 'expectedRows\s*=\s*75[\s\S]*expected_rows[\s\S]*observed_rows'
    red_regression_returns_nonzero_and_restores_lkg = 'RED[\s\S]*RollbackManifest[\s\S]*exit 1'
    yellow_never_reports_pass_or_promotes = 'YELLOW[\s\S]*selected_final[\s\S]*rollback'
    baseline_fingerprint_mismatch_is_incomparable = 'INCOMPARABLE[\s\S]*platform_fingerprint'
    unqualified_baseline_is_not_regression_pass = 'BASELINE[\s\S]*qualified'
    storage_provider_timeout_is_bounded = 'storageProviderTimeoutSeconds\s*=\s*60[\s\S]*storage_provider_timeout'
    watchdog_stop_is_nonblocking = 'sc\.exe[\s\S]*stop[\s\S]*RamSharedWinSvc'
    timeout_overrides_results_to_red = 'watchdog\.timeout[\s\S]*RED'
    partial_format_recovery_clears_only_exact_scratch_lun = 'IsBoot[\s\S]*IsSystem[\s\S]*Clear-Disk[\s\S]*storage_provider_recovery'
    explicit_source_context_is_persisted_when_windows_git_absent = 'SourceCommit[\s\S]*SourceTreeState[\s\S]*SourceDirtyEntryCount[\s\S]*explicit invocation'
    preflight_context_precedes_watchdog = 'Get-RepositoryContext[\s\S]*Get-HostContext[\s\S]*Arm-Watchdog'
    top_level_invocation_is_persisted = 'script:InvocationParameters[\s\S]*script:InvocationParameters\.GetEnumerator'
    physical_matrix_rejects_event_153 = 'Get-WinEvent[\s\S]*Id\s*=\s*153[\s\S]*disk retry events'
    event_153_window_is_current_cell_only = 'StartTime\s*=\s*\$CellStartUtc[\s\S]*EndTime\s*=\s*\$CellEndUtc'
    missing_required_cell_artifact_is_red = 'Assert-RequiredArtifactInventory[\s\S]*missing required cell artifact'
    artifact_inventory_hashes_every_required_file = 'requiredCellArtifacts[\s\S]*bytes[\s\S]*sha256'
    empty_counter_jsonl_is_red = 'counter-direct\.jsonl[\s\S]*empty required cell artifact'
    selected_final_manifest_is_persisted = 'selected_final_manifest[\s\S]*loaded_driver_sha256'
    counter_probe_timeout_is_bounded = 'counterProbeTimeoutSeconds\s*=\s*45[\s\S]*Start-BoundedPowerShellChild[\s\S]*counter probe timeout'
    bounded_child_terminates_process_tree = 'taskkill\.exe[\s\S]*/PID[\s\S]*/T[\s\S]*/F[\s\S]*bounded child process tree termination failed'
    bounded_child_drains_redirected_streams = 'ReadToEndAsync[\s\S]*ReadToEndAsync[\s\S]*Task\]::WaitAll'
    storage_observations_are_bounded = 'StorageProviderObservationWorker[\s\S]*Invoke-BoundedStorageObservation[\s\S]*StorageProviderObservationWorker'
    worker_binds_current_online_serial_before_mutation = 'WorkerExpectedSerial[\s\S]*RAMSHARE VRAMDISK[\s\S]*Initialize-Disk'
    current_online_evidence_precedes_storage_worker = 'Get-CurrentRunOnlineEvidence[\s\S]*Invoke-BoundedStorageProvider'
    partial_format_recovery_requires_partition_phase_and_zero_volumes = 'phase[\s\S]*partition_created[\s\S]*volume_published[\s\S]*Get-Volume[\s\S]*Count -ne 0'
    configured_pagefile_refusal_is_required = '(?=[\s\S]*Win32_PageFileUsage)(?=[\s\S]*PagingFiles)(?=[\s\S]*product-volume pagefile refusal)'
    configured_pagefile_union_refusal = 'Get-NormalizedPagefilePreflight[\s\S]*union[\s\S]*Assert-ProductVolumePagefileEntries'
    bounded_controller_and_workload_required = 'Invoke-BoundedController[\s\S]*Invoke-BoundedWorkload[\s\S]*process_tree_terminated'
    terminal_fallback_stops_partial_candidate = 'Stop-Product[\s\S]*candidate[\s\S]*fallback[\s\S]*Stop-Product'
    toml_unique_scalars_are_verified_before_seal = 'ConvertFrom-RamSharedToml[\s\S]*duplicate TOML table[\s\S]*duplicate TOML scalar[\s\S]*prepared winsvc config mismatch'
    baseline_schema_cardinality_and_metrics_are_validated = 'Assert-BaselineDocument[\s\S]*expected_summaries[\s\S]*positive finite'
    baseline_key_domain_is_validated = 'Assert-BaselineDocument[\s\S]*expected baseline key'
    fresh_outdir_is_required = 'Assert-FreshOutDir[\s\S]*output directory already exists'
    counter_jsonl_semantics_are_required = 'Assert-CounterJsonlSemantics[\s\S]*direct checksum[\s\S]*expected serial'
    counter_jsonl_all_metrics_are_finite = 'Assert-CounterJsonlSemantics[\s\S]*disk_read_bytes_per_sec_avg[\s\S]*current_disk_queue_length_avg'
    manufactured_guards_execute_real_paths = 'Invoke-ManufacturedGuardCase[\s\S]*Assert-RecoveryJournal[\s\S]*Assert-BaselineDocument'
}
if ($text -match 'for \(\$run\s*=') {
    throw "run_switch_has_no_case_insensitive_counter_collision failed"
}
foreach ($entry in $contracts.GetEnumerator()) {
    if ($text -notmatch $entry.Value) {
        throw "$($entry.Key) failed"
    }
    Write-Output "PASS $($entry.Key)"
}

if ($text -match 'Assert-LiveProcessMatch' -or
    $text -notmatch 'rollback_binary_match_includes_loaded_driver' -or
    $text -notmatch 'Start-ManifestProduct\s+\$RollbackManifest' -or
    $text -notmatch 'Assert-LiveBinaryMatch\s+\$manifest' -or
    $text -notmatch 'Assert-FinalProductState\s+\$online') {
    throw "rollback_binary_match_includes_loaded_driver failed"
}
Write-Output "PASS rollback_binary_match_includes_loaded_driver"

if ($windowsHost -match 'Select-Object\s+-First\s+1' -or
    $windowsHost -match 'serde_json::from_str::<DiskInfo>\(&text\)\.unwrap_or' -or
    $windowsHost -notmatch 'find_lun_zero_is_retryable' -or
    $windowsHost -notmatch 'find_lun_exact_singleton_succeeds' -or
    $windowsHost -notmatch 'find_lun_ambiguity_is_refused' -or
    $windowsHost -notmatch 'find_lun_malformed_output_is_refused') {
    throw "find_lun_exact_cardinality failed"
}
Write-Output "PASS find_lun_zero_is_retryable"
Write-Output "PASS find_lun_exact_singleton_succeeds"
Write-Output "PASS find_lun_ambiguity_is_refused"
Write-Output "PASS find_lun_malformed_output_is_refused"

if ($windowsHost -notmatch 'PartitionStyle.*RAW' -or
    $windowsHost -notmatch 'NumberOfPartitions.*-eq 0' -or
    $windowsHost -notmatch '@\(\$d\)\.Length -ne 1' -or
    $windowsHost -notmatch 'raw_letter_occupied' -or
    $productOnline -notmatch 'VolumeLock N/A \(exact RAW disk\)' -or
    $productOnline -notmatch 'if gates\.has_mounted_volume\(\)' -or
    $productOnline -notmatch 'scm_refusal_ack_schedules_same_stop_retry') {
    throw "raw_exact_stop_supported failed"
}
Write-Output "PASS raw_exact_stop_supported"
Write-Output "PASS mounted_stop_still_locks"

function Quote-ProcessArgument([string]$Value) {
    return '"' + $Value.Replace('"', '\"') + '"'
}

function Get-CurrentPowerShellExecutable {
    $path = (Get-Process -Id $PID -ErrorAction Stop).Path
    if ([string]::IsNullOrWhiteSpace($path) -or
        -not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "static_child_uses_current_host_executable failed: current PowerShell path is unavailable"
    }
    $path
}

function Invoke-EvidenceCase([string]$Case, [int]$ExpectedExit,
    [string]$ExpectedVerdict, [string]$ExpectedFinal) {
    $caseRoot = Join-Path $testRoot $Case
    New-Item -ItemType Directory -Force $caseRoot | Out-Null
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Get-CurrentPowerShellExecutable
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.Arguments = @(
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-File", (Quote-ProcessArgument $HarnessPath),
        "-EvidenceSelfTestCase", $Case,
        "-OutDir", (Quote-ProcessArgument $caseRoot)
    ) -join " "
    $process = [Diagnostics.Process]::Start($start)
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -ne $ExpectedExit) {
        throw "$Case exit expected=$ExpectedExit observed=$($process.ExitCode) stdout=$stdout stderr=$stderr"
    }
    $artifactPath = Join-Path $caseRoot "evidence-self-test.json"
    if (-not (Test-Path $artifactPath -PathType Leaf)) {
        throw "$Case missing evidence-self-test.json stdout=$stdout stderr=$stderr"
    }
    $artifact = Get-Content $artifactPath -Raw | ConvertFrom-Json
    if ($artifact.verdict -ne $ExpectedVerdict -or
        $artifact.selected_final -ne $ExpectedFinal) {
        throw "$Case verdict/final mismatch"
    }
    if (-not $artifact.context_complete -or
        -not $artifact.binary_match_persisted -or
        -not $artifact.deviation_present) {
        throw "$Case evidence completeness mismatch"
    }
    if ($Case -eq "missing_rows" -and
        ($artifact.expected_rows -ne 75 -or $artifact.observed_rows -ne 74)) {
        throw "all_expected_matrix_rows_present failed"
    }
    if ($Case -eq "timeout" -and
        ($artifact.failure_phase -ne "storage_provider_timeout" -or
            -not $artifact.cleanup_attempted -or
            -not $artifact.orchestration_child_terminated)) {
        throw "storage_provider_timeout_is_bounded failed"
    }
    if ($Case -eq "event_153" -and
        ($artifact.event_153_count -ne 1 -or $artifact.qualified)) {
        throw "physical_matrix_rejects_event_153 failed"
    }
    if ($Case -eq "missing_artifact" -and
        ($artifact.artifact_inventory_complete -or $artifact.qualified)) {
        throw "missing_required_cell_artifact_is_red failed"
    }
    if ($Case -eq "rollback_driver_mismatch" -and
        ($artifact.rollback_binary_match -or $artifact.qualified)) {
        throw "rollback_loaded_driver_mismatch_is_red failed"
    }
    if ($Case -eq "counter_timeout" -and
        ($artifact.failure_phase -ne "counter_probe_timeout" -or
            -not $artifact.orchestration_child_terminated -or
            -not $artifact.process_tree_terminated)) {
        throw "counter_probe_timeout_is_bounded failed"
    }
    if ($Case -in @("recovery_phase", "recovery_volume", "baseline_invalid", "baseline_key_domain",
            "counter_semantics", "counter_metric_semantics", "fresh_outdir", "toml_duplicate", "toml_duplicate_table",
            "online_identity", "pagefile_configured")) {
        if (-not $artifact.guard_executed -or $artifact.guard_accepted -or
            $artifact.failure_phase -ne "manufactured_guard_refusal") {
            throw "manufactured_guards_execute_real_paths failed case=$Case"
        }
    }
    if ($Case -eq "pipe_flood" -and
        (-not $artifact.guard_executed -or -not $artifact.stdout_drained -or
            -not $artifact.stderr_drained)) {
        throw "bounded_child_drains_redirected_streams failed"
    }
    Write-Output "PASS $($artifact.test_name)"
}

Write-Output "PASS static_child_uses_current_host_executable"

$testRoot = Join-Path ([IO.Path]::GetTempPath()) `
    ("ramshared-matrix-evidence-test-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $testRoot | Out-Null
try {
    Invoke-EvidenceCase "pass" 0 "PASS" "operator"
    Write-Output "PASS event153_zero_case_is_cross_version_empty"
    Invoke-EvidenceCase "baseline" 3 "BASELINE" "rollback"
    Invoke-EvidenceCase "unqualified" 4 "INCOMPARABLE" "rollback"
    Invoke-EvidenceCase "incomparable" 4 "INCOMPARABLE" "rollback"
    Invoke-EvidenceCase "yellow" 2 "YELLOW" "rollback"
    Invoke-EvidenceCase "red" 1 "RED" "rollback"
    Invoke-EvidenceCase "missing_rows" 1 "RED" "rollback"
    Invoke-EvidenceCase "timeout" 1 "RED" "rollback"
    Invoke-EvidenceCase "event_153" 1 "RED" "rollback"
    Invoke-EvidenceCase "missing_artifact" 1 "RED" "rollback"
    Invoke-EvidenceCase "rollback_driver_mismatch" 1 "RED" "rollback"
    Invoke-EvidenceCase "counter_timeout" 1 "RED" "rollback"
    Invoke-EvidenceCase "recovery_phase" 1 "RED" "rollback"
    Invoke-EvidenceCase "recovery_volume" 1 "RED" "rollback"
    Invoke-EvidenceCase "baseline_invalid" 1 "RED" "rollback"
    Invoke-EvidenceCase "baseline_key_domain" 1 "RED" "rollback"
    Invoke-EvidenceCase "counter_semantics" 1 "RED" "rollback"
    Invoke-EvidenceCase "counter_metric_semantics" 1 "RED" "rollback"
    Invoke-EvidenceCase "fresh_outdir" 1 "RED" "rollback"
    Invoke-EvidenceCase "toml_duplicate" 1 "RED" "rollback"
    Invoke-EvidenceCase "toml_duplicate_table" 1 "RED" "rollback"
    Invoke-EvidenceCase "pagefile_configured" 1 "RED" "rollback"
    Invoke-EvidenceCase "online_identity" 1 "RED" "rollback"
    Invoke-EvidenceCase "pipe_flood" 0 "PASS" "operator"
} finally {
    Remove-Item $testRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output "PASS Test-WindowsStorageMatrixStatic"
