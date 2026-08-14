#Requires -Version 5.1
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$script = Join-Path $root "scripts\windows\Invoke-NbdBenchmarkMatrix.ps1"
if (-not (Test-Path -LiteralPath $script -PathType Leaf)) {
    throw "nbd benchmark matrix script is missing"
}

$text = Get-Content -LiteralPath $script -Raw
$required = @(
    "ApproveSharedDailyHost",
    "PlanOnly",
    "unobserved_plan_only",
    "ExpectedSourceCommit",
    "BaselineFile",
    "1024, 2048, 4096",
    "disk-only",
    '"idle", "bounded"',
    "external_workload_mib = 512",
    "reserve_mib = 512",
    "free_vram_mib",
    "allocation_to_hold_ms",
    "allocation_chunk_bytes",
    "worker_threads",
    "anonymous_memory_sequential_write",
    "Start-CudaVramWorkload.ps1",
    "cuda_allocation_ready",
    "cuda-ready.txt",
    "cuda_handshake_path_already_exists",
    "--sealed-release-root",
    "--release-version",
    "--expected-source-commit",
    "--expected-manifest-sha256",
    "--pair-id",
    "Complete-CudaWorkload",
    "injected_cuda_post_start_failure",
    "Invoke-BoundedProcess",
    "ReadToEndAsync",
    "ConvertTo-WindowsCommandLine",
    "Resolve-SelectedRelease",
    "Assert-PinnedReleaseIdentity",
    "New-PinnedNbdProductPreflightArguments",
    "New-PinnedNbdDeactivationArguments",
    "Assert-PinnedCampaignPreflightOutput",
    "Invoke-PinnedCampaignProductPreflight",
    "selected_release_discovery_direct_argv",
    "Invoke-CampaignProductOffPreflight",
    "campaign_preflight_ready_binary_match_required",
    "pinned_ramshared_down",
    "selector_flip_deactivation=PINNED",
    "selected_release_source_mismatch",
    "source_tree_state",
    "expected_source_commit",
    "selected_release_timeout",
    "unverified_unknown",
    "immediately_before_cell",
    "gpu_after_cuda_ready",
    "Get-GpuIdentity",
    "gpu_model",
    "gpu_driver",
    "utilization_percent",
    "temperature_celsius",
    "gpu_before_pair",
    "gpu_after_cuda_ready",
    "cuda_containment",
    "cuda_completion",
    "pair_contexts",
    "Invoke-CellPair",
    "Apply-BaselineVerdictToPair",
    "Assert-CellEvidence",
    "Get-CellEvidenceCustodyFingerprint",
    "Assert-PublicPairEvidenceCustodyCurrent",
    "Assert-PublicPairArtifactBinding",
    "Get-NbdIdentity",
    "ConvertTo-StrictInt64",
    "comparison_nbd_identity_invalid",
    "comparison_nbd_identity_lower_mismatch",
    "comparison_nbd_identity_sink_alias",
    "comparison_second_tier_identity_not_distinct",
    "public_pair_evidence_nbd_identity_invalid",
    "public_pair_evidence_second_tier_identity_not_distinct",
    "context_sha256",
    "cell_result_directory",
    "custody_fingerprint",
    "artifact-inventory.json",
    "evidence-envelope.json",
    "ramshared-nbd-cell-evidence/v1",
    "ramshared-evidence/v1",
    "public-pair-evidence.json",
    "installed_manifest_sha256",
    "input_bundle_manifest_sha256",
    "candidate/noncanonical",
    "size_kib",
    "identity_sha256",
    "cell_evidence_inventory_unlisted_file",
    "cell_evidence_inventory_path_invalid",
    "matrix-artifact-inventory.json",
    "nbd_vs_disk_median_ratio",
    "nbd_vs_disk_p99_ratio",
    "BASELINE_CANDIDATE",
    "NOT_COMPARABLE",
    "baseline_regression",
    "environment_fingerprint",
    "comparison_release_identity_mismatch",
    "comparison_script_hash_mismatch",
    "comparison_windows_script_hash_invalid",
    "comparison_zram_topology_mismatch",
    "comparison_lower_topology_mismatch",
    "comparison_lower_sink_binding_mismatch",
    "public_pair_evidence_nbd_binary_match_required",
    "public_pair_evidence_comparison_required",
    "public_pair_evidence_custody_stale",
    "public_pair_evidence_noncanonical",
    "raw_measurement_status",
    "pair_decision",
    "windows_script_sha256",
    "cuda_pair_hold_too_short",
    "Get-CellTimeoutBudget",
    "Get-PairTimeoutBudget",
    "Get-StrictCellTimeoutBudget",
    "Assert-CellTimeoutBudgetMatch",
    "cell_timeout_budget_property_order_is_semantic",
    "cell_evidence_timeout_budget_mismatch",
    "timeout_budget",
    "sample_timeout_sec",
    "integrity_finalization_timeout_sec",
    "cell_outer_timeout_sec",
    "Assert-CellFailureReceipt",
    "New-CellControllerFailureExecution",
    "failure-receipt.json",
    "ramshared-nbd-cell-failure/v1",
    "cell_failure_receipt_invalid",
    "cell_failure_receipt_product_off",
    "wsl_controller_failed",
    "unverified_unknown",
    "ManufacturedSelfTestCase",
    "manufactured_self_test_live_approval_conflict",
    "RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_BOUNDED_SHARED_HOST_PRESSURE",
    "RAMSHARED_WINDOWS_WATCHDOG_ARMED=1",
    "wsl.exe",
    "watchdog_timeout_red",
    "finally",
    "Apply-CudaCompletionToPairResult",
    "cuda_cleanup_secondary",
    "watchdog_cuda_serialization_sanitized",
    "C:\secret\cuda.err",
    "promotion_stopped",
    "PRODUCT_OFF",
    "NBD_BENCHMARK_MATRIX"
)
foreach ($needle in $required) {
    if (-not $text.Contains($needle)) { throw "missing token: $needle" }
}

$forbidden = @("Restart-Computer", "Stop-Computer", "shutdown.exe", "Initialize-Disk", "Format-Volume", "Clear-Disk", "WslRepo")
foreach ($needle in $forbidden) {
    if ($text.Contains($needle)) { throw "forbidden token: $needle" }
}
foreach ($needle in @("--terminate", "I_ACCEPT_WSL_TERMINATION", "wsl --shutdown", "Restart-Computer", "Stop-Computer")) {
    if ($text.Contains($needle)) { throw "forbidden watchdog lifecycle token: $needle" }
}
$getRequiredPropertySource = [regex]::Match(
    $text,
    '(?ms)^function Get-RequiredProperty \{.*?(?=^function ConvertTo-FiniteNumber \{)'
).Value
if ([string]::IsNullOrWhiteSpace($getRequiredPropertySource) -or
    [regex]::Matches($getRequiredPropertySource, '\$property\.Value').Count -ne 1 -or
    $getRequiredPropertySource -notmatch '\[Collections\.IDictionary\]' -or
    $getRequiredPropertySource -notmatch 'return \$Object\[\$Name\]') {
    throw "Get-RequiredProperty must return one scalar property or dictionary value"
}
if ($text -match '(?m)^\s*&\s*wsl\.exe\b' -or $text -match 'Start-Process\s+wsl\.exe') {
    throw "all WSL execution must use the bounded ProcessStartInfo runner"
}
if ($text.Contains("RAMSHARED_NBD_LOWER_SINK")) {
    throw "approved live WSL arguments must not inject a lower-tier sink seam"
}
if ($text -match '\$OuterTimeoutSec') {
    throw "cell timeout must be tier-derived rather than a global OuterTimeoutSec"
}
if ($text -match '\$summary\.timeout_budget\s*\|\s*ConvertTo-Json' -or
    $text -notmatch 'Assert-CellTimeoutBudgetMatch\s+-Budget') {
    throw "cell timeout budget comparison must use strict semantic custody"
}
if ($text -notmatch '\$inventory\.schema -ne 2') {
    throw "cell evidence must require the sealed schema-2 inventory"
}
if ($text -notmatch 'Get-RequiredProperty -Object \$zram -Name "size_kib"' -or
    $text -notmatch 'Get-RequiredProperty -Object \$zram -Name "device"' -or
    $text -notmatch 'Get-RequiredProperty -Object \$zram -Name "identity_sha256"') {
    throw "comparison must require the sealed zram topology fields"
}

$cudaScript = Join-Path $root "scripts\p0\Start-CudaVramWorkload.ps1"
if (-not (Test-Path -LiteralPath $cudaScript -PathType Leaf)) {
    throw "cuda workload script is missing"
}
$cudaText = Get-Content -LiteralPath $cudaScript -Raw
foreach ($needle in @(
    "FileMode.CreateNew",
    "FileShare.None",
    "cuda_allocation_ready",
    "finally",
    "cuMemFree_v2 rc=",
    "cuCtxDestroy_v2 rc=",
    "cuda_cleanup_failure",
    "CleanupSelfTest",
    "cleanup_self_test_live_campaign_conflict"
)) {
    if (-not $cudaText.Contains($needle)) {
        throw "cuda handshake lifecycle missing token: $needle"
    }
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("ramshared-nbd-matrix-static-" + [guid]::NewGuid().ToString("N"))
$cleanupTmp = Join-Path ([IO.Path]::GetTempPath()) ("ramshared-nbd-matrix-static-" + [guid]::NewGuid().ToString("N"))
$ownedTemporaryRoots = @(
    ([IO.Path]::GetFullPath($tmp)).TrimEnd([char[]]('/\')),
    ([IO.Path]::GetFullPath($cleanupTmp)).TrimEnd([char[]]('/\'))
)

function Remove-TestOwnedTemporaryRoot {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [ValidateRange(1, 20)][int]$MaxAttempts = 20,
        [ValidateRange(1, 1000)][int]$RetryDelayMs = 100,
        [scriptblock]$OnRetry = $null
    )
    $resolved = ([IO.Path]::GetFullPath($Path)).TrimEnd([char[]]('/\'))
    $tempRoot = ([IO.Path]::GetFullPath([IO.Path]::GetTempPath())).TrimEnd([char[]]('/\'))
    $leaf = [IO.Path]::GetFileName($resolved)
    $parent = ([IO.Path]::GetDirectoryName($resolved)).TrimEnd([char[]]('/\'))
    if ($parent -ine $tempRoot -or $leaf -notmatch '^ramshared-nbd-matrix-static-[0-9a-f]{32}$' -or
        -not ($ownedTemporaryRoots -contains $resolved)) {
        throw "temporary_root_ownership_invalid: $resolved"
    }
    if (-not (Test-Path -LiteralPath $resolved -PathType Any)) { return }
    $item = Get-Item -LiteralPath $resolved -Force -ErrorAction Stop
    if (-not $item.PSIsContainer -or (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "temporary_root_ownership_invalid: $resolved"
    }
    $lastError = ""
    for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
        try {
            Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction Stop
            $lastError = ""
        } catch {
            $lastError = $_.Exception.Message
        }
        if (-not (Test-Path -LiteralPath $resolved -PathType Any)) { return }
        if ($attempt -lt $MaxAttempts) {
            if ($null -ne $OnRetry) { & $OnRetry $attempt }
            Start-Sleep -Milliseconds $RetryDelayMs
        }
    }
    throw "temporary_root_cleanup_failed: $resolved attempts=$MaxAttempts last_error=$lastError"
}

New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Output ("PLAN_TEMP_ROOT=" + $tmp)
    New-Item -ItemType Directory -Path $cleanupTmp | Out-Null
    $cleanupNestedParent = Join-Path $cleanupTmp "nested"
    New-Item -ItemType Directory -Path $cleanupNestedParent | Out-Null
    $cleanupNested = Join-Path $cleanupNestedParent "deeper"
    New-Item -ItemType Directory -Path $cleanupNested | Out-Null
    [IO.File]::WriteAllText((Join-Path $cleanupNested "payload.txt"), "manufactured cleanup payload`n")
    $heldPath = Join-Path $cleanupNested "held.txt"
    [IO.File]::WriteAllText($heldPath, "transient open handle`n")
    $heldStream = [IO.File]::Open($heldPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $retryState = @{ invoked = $false }
    try {
        Remove-TestOwnedTemporaryRoot -Path $cleanupTmp -MaxAttempts 20 -RetryDelayMs 100 -OnRetry {
            param($attempt)
            if ($attempt -eq 1) {
                $heldStream.Dispose()
                $retryState.invoked = $true
            }
        }
    } finally {
        if ($null -ne $heldStream) { $heldStream.Dispose() }
    }
    if (Test-Path -LiteralPath $cleanupTmp -PathType Any) {
        throw "temporary_root_cleanup_postcondition_failed: $cleanupTmp"
    }
    if ($retryState.invoked) {
        Write-Output "PASS temporary_root_cleanup_retries_transient_open_handle"
    } else {
        Write-Output "PASS temporary_root_cleanup_handles_platform_unlink_semantics"
    }
    $fakeNvidia = Join-Path $tmp "nvidia-smi.ps1"
    "Write-Output '6144, 5900, 17, 53'" | Set-Content -LiteralPath $fakeNvidia -Encoding Ascii
    & $script -PlanOnly -ArtifactRoot $tmp -NvidiaSmiPath $fakeNvidia
    $plan = Get-Content -LiteralPath (Join-Path $tmp "nbd-benchmark-plan.json") -Raw | ConvertFrom-Json
    if (@($plan.cells).Count -ne 12) { throw "valid plan must have 12 disk/NBD cells" }
    if ($plan.terminal_state -ne "unobserved_plan_only") {
        throw "PlanOnly must not claim a live PRODUCT_OFF terminal state"
    }
    foreach ($cell in @($plan.cells)) {
        if ($cell.measurement -ne "allocation_to_hold_ms" -or
            $cell.allocation_chunk_bytes -ne 67108864 -or $cell.worker_threads -ne 1 -or
            $cell.workload -ne "anonymous_memory_sequential_write" -or
            $null -ne $cell.PSObject.Properties["block_size_bytes"] -or
            $null -ne $cell.PSObject.Properties["queue_depth"]) {
            throw "plan must use allocation rather than I/O labels"
        }
        $expectedSampleTimeout = switch ($cell.tier_mib) { 1024 { 240 }; 2048 { 240 }; 4096 { 600 } }
        $expectedFinalizationTimeout = switch ($cell.tier_mib) { 1024 { 120 }; 2048 { 240 }; 4096 { 600 } }
        $expectedOuterTimeout = switch ($cell.tier_mib) { 1024 { 1380 }; 2048 { 1740 }; 4096 { 3900 } }
        if ($cell.sample_timeout_sec -ne $expectedSampleTimeout -or
            $cell.integrity_finalization_timeout_sec -ne $expectedFinalizationTimeout -or
            $cell.cell_outer_timeout_sec -ne $expectedOuterTimeout -or
            $cell.setup_cleanup_timeout_sec -ne 300) {
            throw "plan tier-derived timeout budget mismatch"
        }
    }
    Write-Output "PASS plan_uses_allocation_contract_labels"
    Write-Output "PASS plan_uses_tier_derived_timeout_budgets"
    $cellBudgetFunctionSource = [regex]::Match(
        $text,
        '(?ms)^function Get-CellTimeoutBudget \{.*?(?=^function Get-PairTimeoutBudget \{)'
    ).Value
    $pairBudgetFunctionSource = [regex]::Match(
        $text,
        '(?ms)^function Get-PairTimeoutBudget \{.*?(?=^function Get-StrictCellTimeoutBudget \{)'
    ).Value
    if ([string]::IsNullOrWhiteSpace($cellBudgetFunctionSource) -or
        [string]::IsNullOrWhiteSpace($pairBudgetFunctionSource)) {
        throw "pair timeout budget functions are missing"
    }
    Invoke-Expression $cellBudgetFunctionSource
    Invoke-Expression $pairBudgetFunctionSource
    foreach ($cudaTuple in @(
        @{ tier = 1024; outer = 1380; hold = 2880 },
        @{ tier = 2048; outer = 1740; hold = 3600 },
        @{ tier = 4096; outer = 3900; hold = 7920 }
    )) {
        $budget = Get-PairTimeoutBudget -TierMiB $cudaTuple.tier
        if ($budget.cell.cell_outer_timeout_sec -ne $cudaTuple.outer -or
            $budget.cuda_hold_min_sec -ne $cudaTuple.hold) {
            throw "pair CUDA hold policy mismatch for tier $($cudaTuple.tier)"
        }
        if (($cudaTuple.hold - 1) -ge $budget.cuda_hold_min_sec -or
            ($cudaTuple.hold + 1) -le $budget.cuda_hold_min_sec) {
            throw "pair CUDA hold tamper boundary was accepted for tier $($cudaTuple.tier)"
        }
    }
    Write-Output "PASS pair_cuda_hold_policy_and_tamper_boundaries"
    $keys = @($plan.cells | ForEach-Object { "{0}:{1}:{2}" -f $_.tier_mib,$_.condition,$_.mode })
    $expected = @(
        "1024:idle:disk-only", "1024:idle:nbd", "1024:bounded:disk-only", "1024:bounded:nbd",
        "2048:idle:disk-only", "2048:idle:nbd", "2048:bounded:disk-only", "2048:bounded:nbd",
        "4096:idle:disk-only", "4096:idle:nbd", "4096:bounded:disk-only", "4096:bounded:nbd"
    )
    if (($keys -join "|") -ne ($expected -join "|")) { throw "promotion order mismatch" }
    Write-Output "PASS matrix_promotes_only_after_complete_prior_pair"

    $shortNvidia = Join-Path $tmp "nvidia-short.ps1"
    "Write-Output '6144, 5000, 17, 53'" | Set-Content -LiteralPath $shortNvidia -Encoding Ascii
    $shortRefused = $false
    try {
        & $script -PlanOnly -ArtifactRoot $tmp -NvidiaSmiPath $shortNvidia -PlanFileName "short.json"
    } catch {
        $shortRefused = $_.Exception.Message -like "gpu_headroom_shortfall*"
    }
    if (-not $shortRefused) { throw "short GPU headroom plan must refuse" }
    $short = Get-Content -LiteralPath (Join-Path $tmp "short.json") -Raw | ConvertFrom-Json
    if ($short.reason -ne "gpu_headroom_shortfall" -or $short.blocked_tier_mib -ne 4096) {
        throw "short GPU headroom refusal mismatch"
    }
    Write-Output "PASS bounded_cell_requires_numeric_gpu_headroom"

    $source = Get-Content -LiteralPath $script -Raw
    $functionSourceStart = $source.IndexOf("function Write-JsonNoBom")
    $functionSourceEndMatch = [regex]::Match(
        $source,
        '(?m)^if \(-not \[string\]::IsNullOrWhiteSpace\(\$ManufacturedSelfTestCase\)\) \{'
    )
    if ($functionSourceStart -lt 0 -or -not $functionSourceEndMatch.Success -or
        $functionSourceEndMatch.Index -le $functionSourceStart) {
        throw "matrix function source extraction failed"
    }
    . ([scriptblock]::Create($source.Substring($functionSourceStart, $functionSourceEndMatch.Index - $functionSourceStart)))
    $lowerIdentity = ("c" * 64) -join ""
    $sinkIdentity = ("d" * 64) -join ""
    $daemonManifestSha256 = ("e" * 64) -join ""
    $newNbdCapacityContext = {
        param([hashtable]$Overrides)
        $nbd = [ordered]@{
            device = "/dev/nbd0"; block_major_minor = "43:0"; size_kib = 1048576
            capacity_sectors = 2097152; usable_size_kib = 1048572; priority = 100
            server_pid = 4242; daemon_executable_relative_path = "bin/ramsharedd"
            daemon_manifest_sha256 = $daemonManifestSha256; identity_sha256 = $lowerIdentity
        }
        foreach ($key in $Overrides.Keys) {
            if ($null -eq $Overrides[$key]) { $null = $nbd.Remove($key) } else { $nbd[$key] = $Overrides[$key] }
        }
        [pscustomobject]@{
            schema = 2; pair_id = "1024-idle"; mode = "nbd"; condition = "idle"; tier_mib = 1024
            release = [pscustomobject]@{ version = "manufactured-v1"; source_commit = (("a" * 40) -join ""); source_tree_state = "clean"; manifest_sha256 = (("b" * 64) -join "") }
            binary_match = "PASS"; watchdog = [pscustomobject]@{ armed = $true; outcome = "not_fired" }
            timeout_budget = [pscustomobject]@{ sample_timeout_sec = 240; integrity_finalization_timeout_sec = 120; samples = 3; setup_cleanup_timeout_sec = 300; cell_outer_timeout_sec = 1380 }
            lower = [pscustomobject]@{
                type = "nbd"; identity_sha256 = $lowerIdentity
                sink_type = "directory"; sink_identity_sha256 = $sinkIdentity
            }
            nbd = [pscustomobject]$nbd
        }
    }
    $capacityCustodyFailures = New-Object 'System.Collections.Generic.List[string]'
    $validCapacityContext = & $newNbdCapacityContext @{}
    $validCapacityLower = Get-ModeBoundLowerTopology -Context $validCapacityContext -ExpectedMode "nbd" -FailurePrefix "manufactured"
    try {
        $validCapacityIdentity = Get-NbdIdentity -Context $validCapacityContext -ExpectedTierMiB 1024 -LowerTopology $validCapacityLower -FailurePrefix "manufactured"
        $capacityProperty = $validCapacityIdentity.PSObject.Properties["capacity_sectors"]
        $usableProperty = $validCapacityIdentity.PSObject.Properties["usable_size_kib"]
        if ($null -eq $capacityProperty -or $null -eq $usableProperty -or
            $capacityProperty.Value -ne 2097152 -or $usableProperty.Value -ne 1048572) {
            [void]$capacityCustodyFailures.Add("identity_valid_values_not_retained")
        }
    } catch {
        [void]$capacityCustodyFailures.Add("identity_valid_context_refused")
    }
    $identityRefusalMutations = @(
        @{ name = "missing_capacity"; overrides = @{ capacity_sectors = $null } },
        @{ name = "missing_usable"; overrides = @{ usable_size_kib = $null } },
        @{ name = "malformed_capacity"; overrides = @{ capacity_sectors = "2097152.0" } },
        @{ name = "malformed_usable"; overrides = @{ usable_size_kib = "1048572.0" } },
        @{ name = "wrong_capacity"; overrides = @{ capacity_sectors = 2097151 } },
        @{ name = "capacity_overflow"; overrides = @{ capacity_sectors = "18446744073709551616" } },
        @{ name = "usable_overflow"; overrides = @{ usable_size_kib = "18446744073710600192" } },
        @{ name = "excess_usable_loss"; overrides = @{ usable_size_kib = 1048567 } }
    )
    foreach ($mutation in $identityRefusalMutations) {
        $context = & $newNbdCapacityContext $mutation.overrides
        $lower = Get-ModeBoundLowerTopology -Context $context -ExpectedMode "nbd" -FailurePrefix "manufactured"
        $accepted = $false
        try { Get-NbdIdentity -Context $context -ExpectedTierMiB 1024 -LowerTopology $lower -FailurePrefix "manufactured" | Out-Null; $accepted = $true } catch {}
        if ($accepted) { [void]$capacityCustodyFailures.Add("identity_" + $mutation.name + "_accepted") }
    }
    $newCapacityEvidence = {
        param([hashtable]$Overrides, [string]$Name)
        $dir = Join-Path $tmp ("nbd-capacity-custody-" + $Name)
        New-Item -ItemType Directory -Path $dir | Out-Null
        $contextPath = Join-Path $dir "context.json"
        $samplesPath = Join-Path $dir "samples.jsonl"
        $summaryPath = Join-Path $dir "summary.json"
        $inventoryPath = Join-Path $dir "artifact-inventory.json"
        $envelopePath = Join-Path $dir "evidence-envelope.json"
        $beforePath = Join-Path $dir "before.txt"
        $actionPath = Join-Path $dir "action.txt"
        $afterPath = Join-Path $dir "after.txt"
        $context = & $newNbdCapacityContext $Overrides
        Write-JsonNoBom -Value $context -Path $contextPath
        [IO.File]::WriteAllText($samplesPath, '{"sample":"manufactured"}' + "`n", [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText($beforePath, "before=manufactured`n", [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText($actionPath, "action=manufactured`n", [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText($afterPath, "after=manufactured`n", [Text.UTF8Encoding]::new($false))
        $summary = [ordered]@{
            status = "PASS"; terminal_state = "PRODUCT_OFF"; mode = "nbd"; binary_match = "PASS"
            context_sha256 = Get-Sha256File -Path $contextPath
            timeout_budget = $context.timeout_budget
        }
        Write-JsonNoBom -Value $summary -Path $summaryPath
        $inventory = [ordered]@{ schema = 2; files = @(
            [ordered]@{ name = "before.txt"; bytes = (Get-Item -LiteralPath $beforePath).Length; sha256 = Get-Sha256File -Path $beforePath },
            [ordered]@{ name = "action.txt"; bytes = (Get-Item -LiteralPath $actionPath).Length; sha256 = Get-Sha256File -Path $actionPath },
            [ordered]@{ name = "after.txt"; bytes = (Get-Item -LiteralPath $afterPath).Length; sha256 = Get-Sha256File -Path $afterPath },
            [ordered]@{ name = "context.json"; bytes = (Get-Item -LiteralPath $contextPath).Length; sha256 = Get-Sha256File -Path $contextPath },
            [ordered]@{ name = "samples.jsonl"; bytes = (Get-Item -LiteralPath $samplesPath).Length; sha256 = Get-Sha256File -Path $samplesPath },
            [ordered]@{ name = "summary.json"; bytes = (Get-Item -LiteralPath $summaryPath).Length; sha256 = Get-Sha256File -Path $summaryPath }
        ) }
        Write-JsonNoBom -Value $inventory -Path $inventoryPath
        $envelope = [ordered]@{
            schema_version = "ramshared-nbd-cell-evidence/v1"; pair_id = "1024-idle"; mode = "nbd"
            release = [ordered]@{ version = "manufactured-v1"; source_commit = (("a" * 40) -join ""); manifest_sha256 = (("b" * 64) -join "") }
            context_sha256 = Get-Sha256File -Path $contextPath; summary_sha256 = Get-Sha256File -Path $summaryPath
            artifact_inventory_sha256 = Get-Sha256File -Path $inventoryPath; binary_match = "PASS"
            watchdog = [ordered]@{ armed = $true; outcome = "not_fired" }; classification = "INCOMPARABLE"
            timeout_budget = $context.timeout_budget
            artifacts = @($inventory.files | ForEach-Object { [ordered]@{ path = $_.name; bytes = $_.bytes; sha256 = $_.sha256 } })
        }
        Write-JsonNoBom -Value $envelope -Path $envelopePath
        [pscustomobject]@{ directory = $dir; summary = [pscustomobject]$summary }
    }
    $validCapacityEvidence = & $newCapacityEvidence @{} "valid"
    try { Assert-CellEvidence -Summary $validCapacityEvidence.summary -CellResultDirectory $validCapacityEvidence.directory | Out-Null } catch {
        [void]$capacityCustodyFailures.Add("evidence_valid_context_refused:" + $_.Exception.Message)
    }
    foreach ($mutation in $identityRefusalMutations) {
        $evidence = & $newCapacityEvidence $mutation.overrides $mutation.name
        $accepted = $false
        try { Assert-CellEvidence -Summary $evidence.summary -CellResultDirectory $evidence.directory | Out-Null; $accepted = $true } catch {}
        if ($accepted) { [void]$capacityCustodyFailures.Add("evidence_" + $mutation.name + "_accepted") }
    }
    if ($capacityCustodyFailures.Count -ne 0) {
        throw ("NBD capacity/usable-size custody accepted invalid state: " + ($capacityCustodyFailures -join ","))
    }
    $newPublicPairCellEvidence = {
        param(
            [Parameter(Mandatory = $true)][string]$Mode,
            [Parameter(Mandatory = $true)][string]$Name
        )
        $dir = Join-Path $tmp ("public-pair-cell-" + $Name + "-" + $Mode)
        New-Item -ItemType Directory -Path $dir | Out-Null
        $binaryMatch = if ($Mode -eq "nbd") { "PASS" } else { "N/A" }
        $lowerType = if ($Mode -eq "nbd") { "nbd" } else { "scratch" }
        $lowerIdentity = if ($Mode -eq "nbd") { ("a" * 64) -join "" } else { ("b" * 64) -join "" }
        $sinkIdentity = ("d" * 64) -join ""
        $contextPath = Join-Path $dir "context.json"
        $samplesPath = Join-Path $dir "samples.jsonl"
        $summaryPath = Join-Path $dir "summary.json"
        $inventoryPath = Join-Path $dir "artifact-inventory.json"
        $envelopePath = Join-Path $dir "evidence-envelope.json"
        $beforePath = Join-Path $dir "before.txt"
        $actionPath = Join-Path $dir "action.txt"
        $afterPath = Join-Path $dir "after.txt"
        $timeoutBudget = [ordered]@{
            sample_timeout_sec = 240; integrity_finalization_timeout_sec = 120; samples = 3; setup_cleanup_timeout_sec = 300; cell_outer_timeout_sec = 1380
        }
        $context = [ordered]@{
            schema = 2; pair_id = "1024-idle"; mode = $Mode; condition = "idle"; tier_mib = 1024
            utc = [ordered]@{ started = "2026-08-12T12:00:00Z" }
            kernel_release = "6.6.0-manufactured"
            release = [ordered]@{
                version = "manufactured-v1"; source_commit = (("a" * 40) -join ""); source_tree_state = "clean"
                manifest_sha256 = (("b" * 64) -join "")
            }
            binary_match = $binaryMatch
            watchdog = [ordered]@{ armed = $true; outcome = "not_fired" }
            timeout_budget = $timeoutBudget
            zram = [ordered]@{
                device = "zram0"; algorithm = "zstd"; size_kib = 1048576; priority = 200
                identity_sha256 = (("c" * 64) -join "")
            }
            lower = [ordered]@{
                type = $lowerType; identity_sha256 = $lowerIdentity; sink_type = "directory"; sink_identity_sha256 = $sinkIdentity
            }
            workload = [ordered]@{
                pattern = "shake256-v1"; allocation_chunk_bytes = 67108864; worker_threads = 1; allocated_mib = 3584
            }
        }
        if ($Mode -eq "nbd") {
            $context.nbd = [ordered]@{
                device = "/dev/nbd0"; block_major_minor = "43:0"; size_kib = 1048576
                capacity_sectors = 2097152; usable_size_kib = 1048572; priority = 100; server_pid = 4242
                daemon_executable_relative_path = "bin/ramsharedd"; daemon_manifest_sha256 = (("e" * 64) -join "")
                identity_sha256 = $lowerIdentity
            }
        }
        Write-JsonNoBom -Value $context -Path $contextPath
        [IO.File]::WriteAllText($samplesPath, '{"sample":"manufactured"}' + "`n", [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText($beforePath, "before=manufactured`n", [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText($actionPath, "action=manufactured`n", [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText($afterPath, "after=manufactured`n", [Text.UTF8Encoding]::new($false))
        $summary = [ordered]@{
            status = "PASS"; terminal_state = "PRODUCT_OFF"; mode = $Mode; binary_match = $binaryMatch
            raw_measurement_status = "PASS"; context_sha256 = Get-Sha256File -Path $contextPath; timeout_budget = $timeoutBudget
            samples = @(
                [ordered]@{ allocation_to_hold_ms = 100.0 },
                [ordered]@{ allocation_to_hold_ms = 110.0 },
                [ordered]@{ allocation_to_hold_ms = 120.0 }
            )
        }
        Write-JsonNoBom -Value $summary -Path $summaryPath
        $inventory = [ordered]@{ schema = 2; files = @(
            [ordered]@{ name = "before.txt"; bytes = (Get-Item -LiteralPath $beforePath).Length; sha256 = Get-Sha256File -Path $beforePath },
            [ordered]@{ name = "action.txt"; bytes = (Get-Item -LiteralPath $actionPath).Length; sha256 = Get-Sha256File -Path $actionPath },
            [ordered]@{ name = "after.txt"; bytes = (Get-Item -LiteralPath $afterPath).Length; sha256 = Get-Sha256File -Path $afterPath },
            [ordered]@{ name = "context.json"; bytes = (Get-Item -LiteralPath $contextPath).Length; sha256 = Get-Sha256File -Path $contextPath },
            [ordered]@{ name = "samples.jsonl"; bytes = (Get-Item -LiteralPath $samplesPath).Length; sha256 = Get-Sha256File -Path $samplesPath },
            [ordered]@{ name = "summary.json"; bytes = (Get-Item -LiteralPath $summaryPath).Length; sha256 = Get-Sha256File -Path $summaryPath }
        ) }
        Write-JsonNoBom -Value $inventory -Path $inventoryPath
        $envelope = [ordered]@{
            schema_version = "ramshared-nbd-cell-evidence/v1"; pair_id = "1024-idle"; mode = $Mode
            release = [ordered]@{
                version = $context.release.version; source_commit = $context.release.source_commit; manifest_sha256 = $context.release.manifest_sha256
            }
            context_sha256 = Get-Sha256File -Path $contextPath; summary_sha256 = Get-Sha256File -Path $summaryPath
            artifact_inventory_sha256 = Get-Sha256File -Path $inventoryPath; binary_match = $binaryMatch
            watchdog = [ordered]@{ armed = $true; outcome = "not_fired" }; timeout_budget = $timeoutBudget; classification = "INCOMPARABLE"
            artifacts = @($inventory.files | ForEach-Object { [ordered]@{ path = $_.name; bytes = $_.bytes; sha256 = $_.sha256 } })
        }
        Write-JsonNoBom -Value $envelope -Path $envelopePath
        [pscustomobject]@{
            directory = $dir; envelope_path = $envelopePath; summary = [pscustomobject]$summary
        }
    }
    $publicPairContext = [pscustomobject]@{
        pair_id = "1024-idle"; timeout_budget = Get-PairTimeoutBudget -TierMiB 1024
        cuda_hold_sec = 0
        gpu_identity = [pscustomobject]@{ gpu_model = "Manufactured GPU"; gpu_driver = "1.2.3" }
        windows_script_sha256 = [pscustomobject]@{
            "Invoke-NbdBenchmarkMatrix.ps1" = Get-Sha256File -Path $script
            "Start-CudaVramWorkload.ps1" = "N/A"
        }
    }
    $publicSelectedRelease = [pscustomobject]@{
        version = "manufactured-v1"; source_commit = (("a" * 40) -join "")
        installed_manifest_sha256 = (("b" * 64) -join ""); input_bundle_manifest_sha256 = "not_exposed"
    }
    $publicComparison = [pscustomobject]@{
        environment_fingerprint = (("f" * 64) -join ""); baseline_verdict = "BASELINE_CANDIDATE"; baseline_reason = "manufactured"
        nbd_vs_disk_median_ratio = 1.0; nbd_vs_disk_p99_ratio = 1.0; nbd_vs_disk_population_stddev_ratio = 1.0
    }
    $newPublicPair = {
        param([Parameter(Mandatory = $true)][string]$Name)
        $disk = & $newPublicPairCellEvidence "disk-only" $Name
        $nbd = & $newPublicPairCellEvidence "nbd" $Name
        $diskEvidence = Assert-CellEvidence -Summary $disk.summary -CellResultDirectory $disk.directory
        $nbdEvidence = Assert-CellEvidence -Summary $nbd.summary -CellResultDirectory $nbd.directory
        [pscustomobject]@{
            disk = $disk; nbd = $nbd
            results = @(
                [pscustomobject]@{ result = $disk.summary; Context = $diskEvidence.context; Evidence = $diskEvidence },
                [pscustomobject]@{ result = $nbd.summary; Context = $nbdEvidence.context; Evidence = $nbdEvidence }
            )
        }
    }
    $initialPublicPair = & $newPublicPair "initial"
    $diskPublicCell = $initialPublicPair.disk
    $nbdPublicCell = $initialPublicPair.nbd
    $publicPairResults = $initialPublicPair.results
    $staleCustodyMutations = @(
        [pscustomobject]@{ name = "disk_envelope_deleted"; cell = "disk"; artifact = "evidence-envelope.json"; remove = $true },
        [pscustomobject]@{ name = "nbd_envelope_replaced"; cell = "nbd"; artifact = "evidence-envelope.json"; remove = $false },
        [pscustomobject]@{ name = "disk_inventory_deleted"; cell = "disk"; artifact = "artifact-inventory.json"; remove = $true },
        [pscustomobject]@{ name = "nbd_inventory_replaced"; cell = "nbd"; artifact = "artifact-inventory.json"; remove = $false },
        [pscustomobject]@{ name = "disk_context_replaced"; cell = "disk"; artifact = "context.json"; remove = $false },
        [pscustomobject]@{ name = "nbd_context_replaced"; cell = "nbd"; artifact = "context.json"; remove = $false },
        [pscustomobject]@{ name = "disk_summary_replaced"; cell = "disk"; artifact = "summary.json"; remove = $false },
        [pscustomobject]@{ name = "nbd_summary_replaced"; cell = "nbd"; artifact = "summary.json"; remove = $false },
        [pscustomobject]@{ name = "nbd_listed_artifact_replaced"; cell = "nbd"; artifact = "before.txt"; remove = $false }
    )
    foreach ($mutation in $staleCustodyMutations) {
        $stalePair = & $newPublicPair $mutation.name
        $cell = if ($mutation.cell -eq "disk") { $stalePair.disk } else { $stalePair.nbd }
        $target = Join-Path $cell.directory $mutation.artifact
        if ($mutation.remove) {
            Remove-Item -LiteralPath $target -Force
        } else {
            [IO.File]::WriteAllText($target, "{`"tampered`":true}`n", [Text.UTF8Encoding]::new($false))
        }
        $stalePairDirectory = Join-Path $tmp ("public-pair-stale-" + $mutation.name)
        New-Item -ItemType Directory -Path $stalePairDirectory | Out-Null
        $staleCustodyRefused = $false
        try {
            Write-PublicPairEvidence -PairResults $stalePair.results -Comparison $publicComparison -PairContext $publicPairContext `
                -SelectedRelease $publicSelectedRelease -PairDir $stalePairDirectory | Out-Null
        } catch {
            $staleCustodyRefused = $_.Exception.Message -eq "public_pair_evidence_custody_stale"
        }
        if (-not $staleCustodyRefused) {
            throw ("public pair writer accepted stale cached custody: " + $mutation.name)
        }
        if (Test-Path -LiteralPath (Join-Path $stalePairDirectory "public-evidence") -PathType Any) {
            throw ("public pair writer emitted stale custody: " + $mutation.name)
        }
    }
    Write-Output "PASS stale_cell_custody_blocks_public_pair_evidence"
    $cachedMetricMutationPair = & $newPublicPair "cached-summary-metric-mutation"
    $cachedMetricMutationPair.results[0].result.samples[0].allocation_to_hold_ms = 999.0
    $cachedMetricMutationDirectory = Join-Path $tmp "public-pair-cached-summary-metric-mutation"
    New-Item -ItemType Directory -Path $cachedMetricMutationDirectory | Out-Null
    $cachedMetricMutationRefused = $false
    try {
        Write-PublicPairEvidence -PairResults $cachedMetricMutationPair.results -Comparison $publicComparison -PairContext $publicPairContext `
            -SelectedRelease $publicSelectedRelease -PairDir $cachedMetricMutationDirectory | Out-Null
    } catch {
        $cachedMetricMutationRefused = $_.Exception.Message -eq "public_pair_evidence_custody_stale"
    }
    if (-not $cachedMetricMutationRefused) {
        throw "public pair writer accepted a cached summary metric mutation after validation"
    }
    if (Test-Path -LiteralPath (Join-Path $cachedMetricMutationDirectory "public-evidence") -PathType Any) {
        throw "public pair writer emitted evidence after a cached summary metric mutation"
    }
    Write-Output "PASS cached_summary_metric_mutation_blocks_public_pair_evidence"
    $artifactMutationPair = & $newPublicPair "published-artifact-mutation"
    $artifactMutationDirectory = Join-Path $tmp "public-pair-published-artifact-mutation"
    New-Item -ItemType Directory -Path $artifactMutationDirectory | Out-Null
    $artifactMutationCandidate = Write-PublicPairEvidence -PairResults $artifactMutationPair.results -Comparison $publicComparison `
        -PairContext $publicPairContext -SelectedRelease $publicSelectedRelease -PairDir $artifactMutationDirectory
    $artifactMutationCustodyPath = Join-Path $artifactMutationCandidate.public_artifact_directory "pair-custody.json"
    $artifactMutationComparisonPath = Join-Path $artifactMutationCandidate.public_artifact_directory "pair-comparison.json"
    [IO.File]::WriteAllText($artifactMutationComparisonPath, "{`"tampered`":true}`n", [Text.UTF8Encoding]::new($false))
    $artifactMutationRefused = $false
    try {
        Assert-PublicPairArtifactBinding -Record $artifactMutationCandidate.record -PairCustodyPath $artifactMutationCustodyPath `
            -ComparisonPath $artifactMutationComparisonPath
    } catch {
        $artifactMutationRefused = $_.Exception.Message -eq "public_pair_evidence_artifact_binding_invalid"
    }
    if (-not $artifactMutationRefused) {
        throw "public pair artifact binding accepted a mutated comparison artifact"
    }
    Write-Output "PASS published_comparison_artifact_mutation_is_refused"
    foreach ($baselineVerdict in @("YELLOW", "RED")) {
        $classifiedPair = & $newPublicPair ("classified-" + $baselineVerdict.ToLowerInvariant())
        $classifiedComparison = [pscustomobject]@{
            environment_fingerprint = (("f") * 64) -join ""; baseline_verdict = $baselineVerdict; baseline_reason = "manufactured"
            nbd_vs_disk_median_ratio = 1.05; nbd_vs_disk_p99_ratio = 1.04; nbd_vs_disk_population_stddev_ratio = 1.0
        }
        Apply-BaselineVerdictToPair -PairResults $classifiedPair.results -Comparison $classifiedComparison
        $classifiedDirectory = Join-Path $tmp ("public-pair-classified-" + $baselineVerdict.ToLowerInvariant())
        New-Item -ItemType Directory -Path $classifiedDirectory | Out-Null
        $classifiedCandidate = Write-PublicPairEvidence -PairResults $classifiedPair.results -Comparison $classifiedComparison `
            -PairContext $publicPairContext -SelectedRelease $publicSelectedRelease -PairDir $classifiedDirectory
        $classifiedDecision = $classifiedCandidate.record.decision
        $classifiedCell = $classifiedPair.results[1].result
        if ($classifiedCandidate.record.comparison.baseline_verdict -ne $baselineVerdict -or
            $classifiedDecision.verdict -ne $baselineVerdict -or [bool]$classifiedDecision.promotable -or
            -not [bool]$classifiedCandidate.record.comparison.qualified -or
            $classifiedCell.status -ne "PASS" -or $classifiedCell.promotion -ne "promotion_stopped" -or
            $null -eq $classifiedCell.PSObject.Properties["pair_decision"] -or
            $classifiedCell.pair_decision.verdict -ne $baselineVerdict -or [bool]$classifiedCell.pair_decision.promotable) {
            throw ("public pair baseline classification did not preserve immutable cell custody: " + $baselineVerdict)
        }
        if (Test-PromotionMayAdvance -PairResults $classifiedPair.results) {
            throw ("public pair baseline classification advanced promotion: " + $baselineVerdict)
        }
    }
    Write-Output "PASS public_pair_evidence_yellow_red_publish_without_mutating_cell_summary"
    Remove-Item -LiteralPath $nbdPublicCell.envelope_path -Force
    $missingCellEnvelopeRefused = $false
    $nbdEvidenceAfterEnvelopeRemoval = $null
    try {
        $nbdEvidenceAfterEnvelopeRemoval = Assert-CellEvidence -Summary $nbdPublicCell.summary -CellResultDirectory $nbdPublicCell.directory
    } catch {
        $missingCellEnvelopeRefused = $_.Exception.Message -eq "cell_evidence_artifact_missing"
    }
    if (-not $missingCellEnvelopeRefused) { throw "missing cell evidence envelope was accepted" }
    $publicPairResults[1].Evidence = $nbdEvidenceAfterEnvelopeRemoval
    $publicWriterRefused = $false
    $publicRefusalDirectory = Join-Path $tmp "public-pair-missing-cell-envelope"
    New-Item -ItemType Directory -Path $publicRefusalDirectory | Out-Null
    try {
        Write-PublicPairEvidence -PairResults $publicPairResults -Comparison $publicComparison -PairContext $publicPairContext `
            -SelectedRelease $publicSelectedRelease -PairDir $publicRefusalDirectory | Out-Null
    } catch {
        $publicWriterRefused = $_.Exception.Message -eq "public_pair_evidence_custody_required"
    }
    if (-not $publicWriterRefused) { throw "public pair writer accepted missing cell evidence envelope" }
    Write-Output "PASS missing_cell_evidence_envelope_blocks_public_pair_evidence"
    Write-Output "PASS windows_nbd_capacity_and_usable_size_are_strict_custody_fields"
    if ($source -notmatch 'watchdog_timeout_red[\s\S]*promotion_stopped') {
        throw "watchdog must be RED and stop promotion"
    }
    $timeoutRun = [pscustomobject]@{ completed = $false; timed_out = $true; exit_code = $null }
    $timeoutExecution = New-CellControllerFailureExecution -Run $timeoutRun -Cell ([pscustomobject]@{
            tier_mib = 1024; condition = "idle"; mode = "disk-only"
        }) -CellDirectory $tmp -SelectedRelease ([pscustomobject]@{ version = "manufactured-v1" }) `
        -PairContext ([pscustomobject]@{ pair_id = "1024-idle" }) -Containment ([ordered]@{ call = "manufactured" })
    if ($timeoutExecution.result.reason -cne "watchdog_timeout_red" -or
        $timeoutExecution.result.terminal_state -cne "unverified_unknown") {
        throw "watchdog timeout must remain unverified unknown without lifecycle recovery"
    }
    Write-Output "PASS watchdog_timeout_is_red_and_stops_promotion"
    Write-Output "PASS watchdog_timeout_is_red_and_unverified_unknown"
    Write-Output "PASS watchdog_timeout_never_uses_vm_lifecycle"

    $entrypoint = $source.Substring($source.LastIndexOf("Assert-InputContract"))
    if ($entrypoint -notmatch 'if \(\$PlanOnly\)[\s\S]*Assert-LiveConfiguration[\s\S]*Resolve-SelectedRelease[\s\S]*Invoke-CampaignProductOffPreflight[\s\S]*New-Plan') {
        throw "live campaign must prove PRODUCT_OFF before its first live headroom plan"
    }
    if ($entrypoint -notmatch '\$resultRecords\s*=\s*@\(\$results\s*\|\s*ForEach-Object\s*\{\s*\$_.result\s*\}\)' -or
        $entrypoint -notmatch 'Write-MatrixArtifactInventory') {
        throw "matrix must summarize execution records and write its hashed inventory"
    }
    if ($source -notmatch 'Apply-BaselineVerdictToPair[\s\S]*baseline_regression_red' -or
        $source -notmatch 'Apply-BaselineVerdictToPair[\s\S]*baseline_regression_yellow') {
        throw "baseline verdicts must block pair promotion"
    }
    $baselineDecisionSource = [regex]::Match(
        $source,
        '(?ms)^function Apply-BaselineVerdictToPair \{.*?(?=^function Get-PairDecisionVerdict \{)'
    ).Value
    if ([string]::IsNullOrWhiteSpace($baselineDecisionSource) -or
        $baselineDecisionSource -match '\.status\s*=' -or
        $baselineDecisionSource -notmatch 'pair_decision' -or
        $baselineDecisionSource -notmatch 'promotion_stopped') {
        throw "baseline classification must separate pair decision from immutable cell summary"
    }
    $campaignPreflightSource = [regex]::Match(
        $source,
        '(?ms)^function Invoke-CampaignProductOffPreflight \{.*?(?=^function New-CellResult \{)'
    ).Value
    if ([string]::IsNullOrWhiteSpace($campaignPreflightSource) -or
        $campaignPreflightSource -match 'cascade-down\.sh|/opt/ramshared/current' -or
        $campaignPreflightSource -notmatch '\$before\s*=\s*Invoke-PinnedCampaignProductPreflight[\s\S]*?-Phase\s+"before"[\s\S]*?New-PinnedNbdDeactivationArguments[\s\S]*?\$after\s*=\s*Invoke-PinnedCampaignProductPreflight[\s\S]*?-Phase\s+"after"[\s\S]*?product_state\s*-ne\s*"PRODUCT_OFF"') {
        throw "campaign preflight must be pinned before action, use the pinned CLI, and require pinned PRODUCT_OFF after action"
    }
    $pinnedPreflightSource = [regex]::Match(
        $source,
        '(?ms)^function New-PinnedNbdProductPreflightArguments \{.*?(?=^function New-PinnedNbdDeactivationArguments \{)'
    ).Value
    $pinnedDeactivationSource = [regex]::Match(
        $source,
        '(?ms)^function New-PinnedNbdDeactivationArguments \{.*?(?=^function ConvertFrom-KeyValueOutput \{)'
    ).Value
    if ([string]::IsNullOrWhiteSpace($pinnedPreflightSource) -or [string]::IsNullOrWhiteSpace($pinnedDeactivationSource) -or
        $pinnedPreflightSource -match 'current|cascade-down\.sh' -or $pinnedDeactivationSource -match 'current|cascade-down\.sh' -or
        $pinnedPreflightSource -notmatch 'nbd-product-preflight\.sh' -or
        $pinnedDeactivationSource -notmatch 'RAMSHARED_NBD_LIFECYCLE_APPROVAL=deactivate:' -or
        $pinnedDeactivationSource -notmatch '/bin/ramshared' -or $pinnedDeactivationSource -notmatch '"down"') {
        throw "pinned release helpers must not consult the selector or cascade-down wrapper"
    }
    $pairFunctionSource = [regex]::Match(
        $source,
        '(?ms)^function Invoke-CellPair \{.*?(?=^function Write-MatrixArtifactInventory \{)'
    ).Value
    if ([string]::IsNullOrWhiteSpace($pairFunctionSource) -or
        $pairFunctionSource -notmatch 'raw_measurement_status[\s\S]*Apply-BaselineVerdictToPair' -or
        $pairFunctionSource -notmatch 'finally\s*\{[\s\S]*Complete-CudaWorkload[\s\S]*Write-PublicPairEvidence' -or
        $pairFunctionSource -notmatch '\$pairCudaHoldSec\s*=\s*\[int\]\$pairContext\.timeout_budget\.cuda_hold_min_sec' -or
        $pairFunctionSource -notmatch 'Start-CudaWorkload\s+-PairDir\s+\$pairDir\s+-CudaHoldSec\s+\$pairCudaHoldSec' -or
        $pairFunctionSource -notmatch 'cuda_hold_sec\s*=\s+\$pairCudaHoldSec') {
        throw "public pair evidence must bind raw PASS cells and wait for CUDA cleanup"
    }
    $publicPairWriterSource = [regex]::Match(
        $source,
        '(?ms)^function Write-PublicPairEvidence \{.*?(?=^function Get-ContextArgumentValue \{)'
    ).Value
    if ([string]::IsNullOrWhiteSpace($publicPairWriterSource) -or
        $publicPairWriterSource -notmatch '\$validatedPair\s*=\s*Assert-PublicPairEvidenceEligibility' -or
        $publicPairWriterSource -notmatch 'Write-JsonNoBom\s+-Value\s+\$publicComparison\s+-Path\s+\$comparisonPath[\s\S]*?\$comparisonSha256\s*=\s*Get-Sha256File\s+-Path\s+\$comparisonPath' -or
        $publicPairWriterSource -notmatch 'comparison_sha256\s*=\s+\$comparisonSha256' -or
        $publicPairWriterSource -notmatch 'cuda_hold_sec\s*=\s+\[int\]\$PairContext\.cuda_hold_sec' -or
        $publicPairWriterSource -notmatch 'pair_comparison_sha256\s*=\s+\$comparisonSha256' -or
        $publicPairWriterSource -notmatch 'New-PublicMetric\s+-Summary\s+\$diskSummary' -or
        $publicPairWriterSource -notmatch 'New-PublicMetric\s+-Summary\s+\$nbdSummary' -or
        $publicPairWriterSource -notmatch 'Assert-PublicPairArtifactBinding[\s\S]*?Write-JsonNoBom\s+-Value\s+\$record\s+-Path\s+\$publicEnvelopePath[\s\S]*?Assert-PublicPairArtifactBinding') {
        throw "public pair writer must bind fresh summaries and exact comparison artifact bytes"
    }
    Write-Output "PASS public_pair_writer_binds_fresh_summaries_and_exact_artifacts"
    Write-Output "PASS live_preflight_precedes_headroom_and_baseline_blocks_promotion"
    Write-Output "PASS reviewed_release_preflight_and_deactivation_remain_pinned_after_selector_flip"
    Write-Output "PASS plan_only_terminal_state_is_unobserved"
    Write-Output "PASS wsl_release_discovery_and_cells_are_bounded"

    $selfTestCases = @(
        @{ Name = "timeout"; Expected = "terminal_state=unverified_unknown" },
        @{ Name = "timeout"; Expected = "timeout_vm_lifecycle=NOT_INVOKED" },
        @{ Name = "watchdog-cuda-composition"; Expected = "watchdog_cuda_composition=PASS" },
        @{ Name = "watchdog-cuda-serialization"; Expected = "watchdog_cuda_serialization_sanitized=PASS" },
        @{ Name = "promotion"; Expected = "next_pair_started=False" },
        @{ Name = "selector-flip"; Expected = "selector_flip_deactivation=PINNED" },
        @{ Name = "nbd-identity"; Expected = "nbd_identity_contract=PASS" },
        @{ Name = "nbd-identity"; Expected = "nbd_identity_invalid_fields=REFUSED" },
        @{ Name = "nbd-identity"; Expected = "nbd_identity_lower_and_sink_aliases=REFUSED" },
        @{ Name = "matrix-inventory"; Expected = "matrix_inventory=PASS" },
        @{ Name = "windows-command-line"; Expected = "windows_command_line=PASS" },
        @{ Name = "selected-release-direct-argv"; Expected = "selected_release_direct_argv=PASS" },
        @{ Name = "cuda-cleanup"; Expected = "cuda_process_terminated=True" },
        @{ Name = "cuda-post-start-cleanup"; Expected = "cuda_post_start_cleanup=PASS" },
        @{ Name = "cuda-native-cleanup"; Expected = "cuda_native_cleanup_failure=PASS" },
        @{ Name = "timeout-budget"; Expected = "timeout_budget=PASS" },
        @{ Name = "timeout-budget"; Expected = "timeout_budget_refusal=REFUSED" },
        @{ Name = "timeout-budget-property-order"; Expected = "cell_timeout_budget_property_order_is_semantic=PASS" },
        @{ Name = "timeout-budget-property-order"; Expected = "cell_timeout_budget_property_order_mismatch=REFUSED" },
        @{ Name = "timeout-budget-property-order"; Expected = "cell_timeout_budget_property_order_noncanonical=REFUSED" },
        @{ Name = "failure-receipt"; Expected = "cell_failure_receipt_product_off=PASS" },
        @{ Name = "failure-receipt"; Expected = "cell_failure_receipt_failed_start=REFUSED" },
        @{ Name = "failure-receipt"; Expected = "cell_failure_receipt_non_boolean_timeout=REFUSED" },
        @{ Name = "failure-receipt"; Expected = "cell_failure_receipt_invalid=REFUSED" },
        @{ Name = "partial-timeout-sample"; Expected = "partial_timeout_integrity_not_promoted=REFUSED" }
    )
    foreach ($case in $selfTestCases) {
        $output = @(& $script -ManufacturedSelfTestCase $case.Name -ArtifactRoot $tmp 2>&1)
        if (($output -join "`n") -notmatch [regex]::Escape($case.Expected)) {
            throw "manufactured $($case.Name) behavior failed: $($output -join '; ')"
        }
        Write-Output ("PASS manufactured_" + $case.Name + "_behavior")
    }
    Write-Output "PASS manufactured_nbd_identity_behavior"
    Write-Output "PASS matrix_inventory_is_ps51_safe_and_repository_relative"
    Write-Output "PASS windows_command_line_preserves_exact_wsl_shell_argument"
    Write-Output "PASS selected_release_discovery_uses_direct_pinned_argv"

    $contractSelfTests = @(
        @{ Name = "source-identity"; Expected = "source_identity=REFUSED" },
        @{ Name = "cuda-deadline"; Expected = "cuda_deadline=REFUSED" },
        @{ Name = "comparison"; Expected = "baseline_verdict=YELLOW" },
        @{ Name = "comparison"; Expected = "comparison_thresholds=PASS" },
        @{ Name = "comparison"; Expected = "baseline_pair_actions=PASS" },
        @{ Name = "comparison"; Expected = "comparison_identity_contract=PASS" },
        @{ Name = "comparison"; Expected = "comparison_zram_usable_size_bounds=PASS" },
        @{ Name = "comparison"; Expected = "comparison_zram_raw_numeric_types=REFUSED" },
        @{ Name = "comparison"; Expected = "comparison_zram_pair_equality=REFUSED" },
        @{ Name = "comparison"; Expected = "comparison_lower_mode_binding=REFUSED" },
        @{ Name = "comparison"; Expected = "comparison_lower_wrong_type=REFUSED" },
        @{ Name = "comparison"; Expected = "comparison_lower_type_missing=REFUSED" },
        @{ Name = "comparison"; Expected = "comparison_lower_identity_missing=REFUSED" },
        @{ Name = "comparison"; Expected = "comparison_lower_sink_identity=REFUSED" },
        @{ Name = "comparison"; Expected = "comparison_distinct_second_tier_identity=PASS" },
        @{ Name = "comparison"; Expected = "comparison_nbd_identity_contract=PASS" },
        @{ Name = "comparison"; Expected = "comparison_nbd_identity_invalid=REFUSED" },
        @{ Name = "comparison"; Expected = "comparison_second_tier_identity_not_distinct=REFUSED" },
        @{ Name = "evidence-chain"; Expected = "evidence_chain=PASS" },
        @{ Name = "evidence-chain"; Expected = "evidence_chain_mismatch=REFUSED" },
        @{ Name = "evidence-chain"; Expected = "evidence_chain_all_inventory_entries=PASS" },
        @{ Name = "evidence-chain"; Expected = "evidence_chain_unsafe_path=REFUSED" },
        @{ Name = "evidence-chain"; Expected = "evidence_chain_unlisted_file=REFUSED" },
        @{ Name = "evidence-chain"; Expected = "evidence_chain_envelope=PASS" },
        @{ Name = "evidence-chain"; Expected = "evidence_chain_private_envelope=REFUSED" },
        @{ Name = "evidence-chain"; Expected = "evidence_chain_empty_receipt=REFUSED" },
        @{ Name = "evidence-chain"; Expected = "evidence_chain_timeout_budget=PASS" },
        @{ Name = "evidence-chain"; Expected = "evidence_chain_timeout_budget_tamper=REFUSED" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence=PASS" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_mapping=PASS" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_yellow_red_publish_without_mutating_cell_summary=PASS" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_nbd_binary_match=REFUSED" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_raw_measurement=REFUSED" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_lower_mode_binding=REFUSED" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_lower_wrong_type=REFUSED" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_lower_type_missing=REFUSED" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_lower_identity_missing=REFUSED" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_lower_sink_identity=REFUSED" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_distinct_second_tier_identity=PASS" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_nbd_identity_contract=PASS" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_nbd_identity_invalid=REFUSED" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_second_tier_identity_not_distinct=REFUSED" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_requires_nbd_binary_match_and_comparison=PASS" },
        @{ Name = "public-pair-evidence"; Expected = "public_pair_evidence_maps_baseline_candidate_incomparable_and_pass_exactly=PASS" },
        @{ Name = "campaign-preflight"; Expected = "campaign_preflight_pinned=PASS" },
        @{ Name = "campaign-preflight"; Expected = "campaign_ready_binary_match=REFUSED" },
        @{ Name = "campaign-preflight"; Expected = "campaign_product_off_preflight=PASS" }
    )
    foreach ($case in $contractSelfTests) {
        $output = @(& $script -ManufacturedSelfTestCase $case.Name -ArtifactRoot $tmp 2>&1)
        if (($output -join "`n") -notmatch [regex]::Escape($case.Expected)) {
            throw "manufactured $($case.Name) contract failed: $($output -join '; ')"
        }
        Write-Output ("PASS manufactured_" + $case.Name + "_contract")
    }

    $liveSeamRefused = $false
    try {
        & $script -ManufacturedSelfTestCase "timeout" -ApproveSharedDailyHost -ArtifactRoot $tmp | Out-Null
    } catch {
        $liveSeamRefused = $_.Exception.Message -eq "manufactured_self_test_live_approval_conflict"
    }
    if (-not $liveSeamRefused) { throw "approved live mode must reject manufactured self-test seam" }
    Write-Output "PASS manufactured_self_test_is_unavailable_in_live_mode"

    if ($source -notmatch 'Start-CudaWorkload[\s\S]*foreach \(\$cell in \$PairCells\)[\s\S]*Complete-CudaWorkload') {
        throw "one CUDA context must enclose disk then NBD pair"
    }
    Write-Output "PASS one_cuda_context_covers_one_disk_nbd_pair"

    $q4HoldReady = Join-Path $tmp "cuda-q4-hold-ready.txt"
    $q4HoldAccepted = $false
    try {
        & $cudaScript -HandshakeSelfTest -HoldSec 7920 -ReadyFile $q4HoldReady | Out-Null
        $q4HoldAccepted = $true
    } catch {
        $q4HoldAccepted = $false
    }
    if (-not $q4HoldAccepted) {
        throw "cuda_workload_hold_cap_rejected_q4_timeout_budget"
    }
    $cudaOverCapRefused = $false
    try {
        & $cudaScript -HandshakeSelfTest -HoldSec 7921 -ReadyFile (Join-Path $tmp "cuda-over-cap-ready.txt") | Out-Null
    } catch {
        $cudaOverCapRefused = $true
    }
    if (-not $cudaOverCapRefused) {
        throw "cuda_workload_hold_cap_accepted_above_q4_timeout_budget"
    }
    Write-Output "PASS cuda_workload_hold_cap_matches_q4_timeout_budget"

    $handshake = Join-Path $tmp "cuda-handshake.txt"
    & $cudaScript -HandshakeSelfTest -ReadyFile $handshake | Out-Null
    if ((Get-Content -LiteralPath $handshake -Raw) -notmatch '^cuda_allocation_ready\r?\n$') {
        throw "cuda create-once handshake did not publish the exact ready receipt"
    }
    $duplicateRefused = $false
    try { & $cudaScript -HandshakeSelfTest -ReadyFile $handshake | Out-Null } catch {
        $duplicateRefused = $true
    }
    if (-not $duplicateRefused) { throw "cuda ready handshake overwrite was accepted" }
    $liveHandshakeRefused = $false
    try { & $cudaScript -HandshakeSelfTest -LiveCampaign -ReadyFile (Join-Path $tmp "live-handshake.txt") | Out-Null } catch {
        $liveHandshakeRefused = $_.Exception.Message -eq "handshake_self_test_live_campaign_conflict"
    }
    if (-not $liveHandshakeRefused) { throw "cuda live campaign accepted handshake self-test seam" }
    Write-Output "PASS cuda_create_once_handshake_and_live_seam_refusal"

    $cudaCleanupExit = 0
    try { & $cudaScript -CleanupSelfTest } catch { $cudaCleanupExit = 1 }
    if ($cudaCleanupExit -ne 0) {
        throw "cuda cleanup self-test did not preserve native cleanup failure"
    }
    $liveCleanupRefused = $false
    try { & $cudaScript -CleanupSelfTest -LiveCampaign | Out-Null } catch {
        $liveCleanupRefused = $_.Exception.Message -eq "cleanup_self_test_live_campaign_conflict"
    }
    if (-not $liveCleanupRefused) { throw "cuda live campaign accepted cleanup self-test seam" }
    Write-Output "PASS cuda_workload_uses_fresh_handshakes_and_finally_releases_context"

    # These receipt names are deliberately exact SSDV3 TestName values, not
    # prose aliases: CI and audit scripts confront them mechanically.
    Write-Output "PASS pair_ratios_and_compatible_baseline_thresholds_are_exact"
} finally {
    try {
        Remove-TestOwnedTemporaryRoot -Path $cleanupTmp -MaxAttempts 20 -RetryDelayMs 100
    } finally {
        Remove-TestOwnedTemporaryRoot -Path $tmp -MaxAttempts 20 -RetryDelayMs 100
    }
}

Write-Output "PASS Test-NbdBenchmarkMatrixStatic"
