#Requires -Version 5.1
<#
.SYNOPSIS
  Run the ordered 1/2/4 GiB disk-only versus NBD benchmark on shared WSL2.

.DESCRIPTION
  Windows owns bounded process execution, the outer watchdog, and the optional
  CUDA condition. A bounded CUDA context covers exactly one disk/NBD pair. A
  watchdog timeout is RED and leaves the terminal product state unverified; it
  stops only the bounded launched host child and is never evidence that the
  product is off.
#>
[CmdletBinding()]
param(
    [switch]$ApproveSharedDailyHost,
    [switch]$PlanOnly,
    [string]$Distro = "Ubuntu-24.04",
    [Parameter(Mandatory = $true)][string]$ArtifactRoot,
    [string]$PlanFileName = "nbd-benchmark-plan.json",
    [string]$NvidiaSmiPath = "nvidia-smi.exe",
    [ValidateRange(360, 7920)][int]$CudaMaxHoldSec = 7920,
    [string]$ExpectedSourceCommit = "",
    [string]$BaselineFile = "",
    [string]$ManufacturedSelfTestCase = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$tiers = @(1024, 2048, 4096)
$conditions = @("idle", "bounded") # idle,bounded
$modes = @("disk-only", "nbd")
$external_workload_mib = 512
$reserve_mib = 512
$sample_timeout_max_sec = 600
$cell_setup_cleanup_timeout_sec = 300
$cell_outer_timeout_min_sec = 900
$cell_outer_timeout_max_sec = 3900

function Write-JsonNoBom {
    param([Parameter(Mandatory = $true)]$Value, [Parameter(Mandatory = $true)][string]$Path)
    $json = $Value | ConvertTo-Json -Depth 20
    [IO.File]::WriteAllText($Path, $json + "`n", [Text.UTF8Encoding]::new($false))
}

function ConvertFrom-JsonPreservingDateStrings {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string]$Json)
    $parameters = @{ InputObject = $Json; ErrorAction = "Stop" }
    $convertCommand = Get-Command ConvertFrom-Json -ErrorAction Stop
    if ($convertCommand.Parameters.ContainsKey("DateKind")) {
        $parameters["DateKind"] = "String"
    }
    ConvertFrom-Json @parameters
}

function Convert-ToWslPath {
    param([Parameter(Mandatory = $true)][string]$Path)
    if ($Path -match '^([A-Za-z]):\\(.*)$') {
        return "/mnt/" + $Matches[1].ToLowerInvariant() + "/" + ($Matches[2] -replace '\\', '/')
    }
    throw "artifact_root_must_be_windows_drive_path"
}

function ConvertTo-WindowsCommandLineArgument {
    param([Parameter(Mandatory = $true)][string]$Value)
    if ($Value.Length -eq 0) { return '""' }
    if ($Value -notmatch '[\s"]') { return $Value }
    '"' + ($Value -replace '(\\*)"', '$1$1\"' -replace '(\\+)$', '$1$1') + '"'
}

function ConvertTo-WindowsCommandLine {
    param([Parameter(Mandatory = $true)][string[]]$ArgumentValues)
    (($ArgumentValues | ForEach-Object {
        ConvertTo-WindowsCommandLineArgument -Value ([string]$_)
    }) -join " ")
}

function Get-CurrentPowerShellExecutable {
    $path = (Get-Process -Id $PID -ErrorAction Stop).Path
    if ([string]::IsNullOrWhiteSpace($path) -or -not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "current_powershell_executable_unavailable"
    }
    $path
}

function Get-WslExecutable {
    $candidate = Join-Path $env:SystemRoot "System32\wsl.exe"
    if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
        throw "wsl_executable_unavailable"
    }
    $candidate
}

function Limit-ChildDiagnostics {
    param([AllowNull()][string]$Value)
    if ($null -eq $Value) { return "" }
    $limit = 1048576
    if ($Value.Length -le $limit) { return $Value }
    $Value.Substring(0, $limit) + "`n[diagnostic truncated]"
}

function New-RedirectedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$ArgumentValues
    )
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $FilePath
    $info.Arguments = ConvertTo-WindowsCommandLine -ArgumentValues $ArgumentValues
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) { throw "bounded_process_start_failed" }
        return [pscustomobject]@{
            process = $process
            stdout_task = $process.StandardOutput.ReadToEndAsync()
            stderr_task = $process.StandardError.ReadToEndAsync()
        }
    } catch {
        $process.Dispose()
        throw
    }
}

function Receive-RedirectedProcessOutput {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [ValidateRange(1, 30000)][int]$DrainTimeoutMs = 5000
    )
    $drained = $false
    try {
        $drained = [Threading.Tasks.Task]::WaitAll(
            [Threading.Tasks.Task[]]@($Handle.stdout_task, $Handle.stderr_task), $DrainTimeoutMs)
    } catch {
        $drained = $false
    }
    try { $stdout = if ($Handle.stdout_task.IsCompleted) { [string]$Handle.stdout_task.Result } else { "" } } catch { $stdout = "" }
    try { $stderr = if ($Handle.stderr_task.IsCompleted) { [string]$Handle.stderr_task.Result } else { "" } } catch { $stderr = "" }
    [pscustomobject]@{
        stdout = Limit-ChildDiagnostics -Value $stdout
        stderr = Limit-ChildDiagnostics -Value $stderr
        stream_drain_complete = $drained
    }
}

function Stop-RedirectedProcess {
    param([Parameter(Mandatory = $true)]$Handle)
    $forced = $false
    try {
        if (-not $Handle.process.HasExited) {
            $Handle.process.Kill()
            $forced = $true
            $null = $Handle.process.WaitForExit(5000)
        }
    } catch {}
    $forced
}

function Invoke-BoundedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$ArgumentValues,
        [ValidateRange(1, 3900)][int]$TimeoutSec
    )
    $handle = $null
    try {
        $handle = New-RedirectedProcess -FilePath $FilePath -ArgumentValues $ArgumentValues
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSec)
        while (-not $handle.process.HasExited -and [DateTime]::UtcNow -lt $deadline) {
            $null = $handle.process.WaitForExit(100)
        }
        $timedOut = -not $handle.process.HasExited
        $forced = $false
        if ($timedOut) { $forced = Stop-RedirectedProcess -Handle $handle }
        $output = Receive-RedirectedProcessOutput -Handle $handle
        $exitCode = if ($handle.process.HasExited) { [int]$handle.process.ExitCode } else { $null }
        return [pscustomobject]@{
            started = $true
            completed = -not $timedOut
            timed_out = $timedOut
            forced_termination = $forced
            exit_code = $exitCode
            stdout = $output.stdout
            stderr = $output.stderr
            stream_drain_complete = $output.stream_drain_complete
            error = ""
        }
    } catch {
        return [pscustomobject]@{
            started = $false
            completed = $false
            timed_out = $false
            forced_termination = $false
            exit_code = $null
            stdout = ""
            stderr = ""
            stream_drain_complete = $false
            error = $_.Exception.Message
        }
    } finally {
        if ($null -ne $handle) { $handle.process.Dispose() }
    }
}

function Write-ProcessLogs {
    param(
        [Parameter(Mandatory = $true)]$Run,
        [Parameter(Mandatory = $true)][string]$StdoutPath,
        [Parameter(Mandatory = $true)][string]$StderrPath
    )
    [IO.File]::WriteAllText($StdoutPath, [string]$Run.stdout, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText($StderrPath, [string]$Run.stderr, [Text.UTF8Encoding]::new($false))
}

function New-ExclusiveSignalFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Content
    )
    $stream = New-Object IO.FileStream($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes($Content)
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush()
    } finally {
        $stream.Dispose()
    }
}

function Complete-CudaWorkload {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)][string]$ReleaseFile,
        [Parameter(Mandatory = $true)][string]$StdoutPath,
        [Parameter(Mandatory = $true)][string]$StderrPath,
        [ValidateRange(1, 30000)][int]$WaitTimeoutMs = 30000
    )
    $releaseSignalCreated = $false
    $forced = $false
    $completionError = ""
    try {
        New-ExclusiveSignalFile -Path $ReleaseFile -Content "cuda_release_requested`n"
        $releaseSignalCreated = $true
        if (-not $Handle.process.WaitForExit($WaitTimeoutMs)) {
            $forced = Stop-RedirectedProcess -Handle $Handle
        }
    } catch {
        $completionError = $_.Exception.Message
    } finally {
        if (-not $Handle.process.HasExited) {
            $forced = (Stop-RedirectedProcess -Handle $Handle) -or $forced
        }
        $output = Receive-RedirectedProcessOutput -Handle $Handle
        [IO.File]::WriteAllText($StdoutPath, [string]$output.stdout, [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText($StderrPath, [string]$output.stderr, [Text.UTF8Encoding]::new($false))
        $exitCode = if ($Handle.process.HasExited) { [int]$Handle.process.ExitCode } else { $null }
        $Handle.process.Dispose()
    }
    [pscustomobject]@{
        released = $releaseSignalCreated -and -not $forced -and $exitCode -eq 0 -and
            ((Get-Content -LiteralPath $StdoutPath -Raw -ErrorAction SilentlyContinue) -match '\[cuda-vram-workload\] released')
        release_signal_created = $releaseSignalCreated
        forced_termination = $forced
        exit_code = $exitCode
        stream_drain_complete = $output.stream_drain_complete
        error = $completionError
    }
}

function Get-GpuMemory {
    if ([IO.Path]::GetExtension($NvidiaSmiPath).ToLowerInvariant() -eq ".ps1") {
        $run = Invoke-BoundedProcess -FilePath (Get-CurrentPowerShellExecutable) -ArgumentValues @(
            "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", $NvidiaSmiPath) -TimeoutSec 15
        if (-not $run.completed -or $run.exit_code -ne 0 -or
            -not [string]::IsNullOrWhiteSpace($run.stderr)) {
            throw "gpu_headroom_unknown"
        }
        $raw = @($run.stdout.Trim())
    } else {
        $run = Invoke-BoundedProcess -FilePath $NvidiaSmiPath -ArgumentValues @(
            "--query-gpu=memory.total,memory.free,utilization.gpu,temperature.gpu", "--format=csv,noheader,nounits") -TimeoutSec 15
        if (-not $run.completed -or $run.exit_code -ne 0 -or
            -not [string]::IsNullOrWhiteSpace($run.stderr)) {
            throw "gpu_headroom_unknown"
        }
        $raw = @($run.stdout.Trim())
    }
    if (@($raw).Count -ne 1 -or [string]$raw[0] -notmatch '^\s*([0-9]+)\s*,\s*([0-9]+)\s*,\s*([0-9]+)\s*,\s*([0-9]+)\s*$') {
        throw "gpu_headroom_unknown"
    }
    [pscustomobject]@{
        total_mib = [int]$Matches[1]
        free_vram_mib = [int]$Matches[2]
        utilization_percent = [int]$Matches[3]
        temperature_celsius = [int]$Matches[4]
    }
}

function Get-GpuIdentity {
    if ([IO.Path]::GetExtension($NvidiaSmiPath).ToLowerInvariant() -eq ".ps1") {
        throw "gpu_identity_fixture_forbidden"
    }
    $run = Invoke-BoundedProcess -FilePath $NvidiaSmiPath -ArgumentValues @(
        "--query-gpu=name,driver_version", "--format=csv,noheader") -TimeoutSec 15
    if (-not $run.completed -or $run.exit_code -ne 0 -or -not [string]::IsNullOrWhiteSpace($run.stderr)) {
        throw "gpu_identity_unknown"
    }
    if ($run.stdout.Trim() -notmatch '^([^\r\n,]+),\s*([A-Za-z0-9._-]+)$') {
        throw "gpu_identity_unknown"
    }
    [pscustomobject]@{
        gpu_model = $Matches[1].Trim()
        gpu_driver = $Matches[2]
    }
}

function Assert-InputContract {
    if ($Distro -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$') {
        throw "distro_name_invalid"
    }
    if ($PlanFileName -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}\.json$') {
        throw "plan_file_name_invalid"
    }
}

function Assert-LiveConfiguration {
    if ($ExpectedSourceCommit -notmatch '^[0-9a-f]{40}$') {
        throw "expected_source_commit_invalid"
    }
    if ([IO.Path]::GetExtension($NvidiaSmiPath).ToLowerInvariant() -eq ".ps1") {
        throw "live_gpu_probe_script_forbidden"
    }
    $maximumPairOuterTimeoutSec = Get-PairTimeoutBudget -TierMiB 4096
    $minimumCudaHoldSec = $maximumPairOuterTimeoutSec.cuda_hold_min_sec
    if ($CudaMaxHoldSec -lt $minimumCudaHoldSec) {
        throw "cuda_pair_hold_too_short required_sec=$minimumCudaHoldSec configured_sec=$CudaMaxHoldSec"
    }
    if (-not [string]::IsNullOrWhiteSpace($BaselineFile) -and
        -not (Test-Path -LiteralPath $BaselineFile -PathType Leaf)) {
        throw "baseline_file_missing"
    }
}

function Get-CellTimeoutBudget {
    param([Parameter(Mandatory = $true)][int]$TierMiB)
    $sampleTimeoutSec = switch ($TierMiB) {
        1024 { 240; break }
        2048 { 240; break }
        4096 { 600; break }
        default { throw "cell_timeout_tier_invalid" }
    }
    # Finalization begins only after HOLD has been observed and TERM starts.
    # It remains independent from the allocation-to-HOLD containment cap.
    $integrityFinalizationTimeoutSec = switch ($TierMiB) {
        1024 { 120; break }
        2048 { 240; break }
        4096 { 600; break }
        default { throw "cell_timeout_tier_invalid" }
    }
    $setupCleanupTimeoutSec = 300
    $cellOuterTimeoutMinSec = 900
    $cellOuterTimeoutMaxSec = 3900
    $sampleTimeoutMaxSec = 600
    $derivedOuterTimeoutSec = 3 * ($sampleTimeoutSec + $integrityFinalizationTimeoutSec) + $setupCleanupTimeoutSec
    $cellOuterTimeoutSec = $derivedOuterTimeoutSec
    if ($sampleTimeoutSec -lt 1 -or $sampleTimeoutSec -gt $sampleTimeoutMaxSec -or
        $cellOuterTimeoutSec -lt $cellOuterTimeoutMinSec -or $cellOuterTimeoutSec -gt $cellOuterTimeoutMaxSec) {
        throw "cell_timeout_budget_invalid"
    }
    [pscustomobject]@{
        sample_timeout_sec = $sampleTimeoutSec
        integrity_finalization_timeout_sec = $integrityFinalizationTimeoutSec
        samples = 3
        setup_cleanup_timeout_sec = $setupCleanupTimeoutSec
        cell_outer_timeout_sec = $cellOuterTimeoutSec
    }
}

function Get-PairTimeoutBudget {
    param([Parameter(Mandatory = $true)][int]$TierMiB)
    $cell = Get-CellTimeoutBudget -TierMiB $TierMiB
    $cudaHoldMinSec = (2 * [int]$cell.cell_outer_timeout_sec) + 120
    if ($cudaHoldMinSec -lt 1 -or $cudaHoldMinSec -gt 7920) { throw "pair_timeout_budget_invalid" }
    [pscustomobject]@{
        cell = $cell
        cuda_hold_min_sec = $cudaHoldMinSec
    }
}

function Get-StrictCellTimeoutBudget {
    param(
        [Parameter(Mandatory = $true)]$Budget,
        [Parameter(Mandatory = $true)][int]$TierMiB,
        [Parameter(Mandatory = $true)][string]$FailureReason
    )
    $fields = @("sample_timeout_sec", "integrity_finalization_timeout_sec", "samples", "setup_cleanup_timeout_sec", "cell_outer_timeout_sec")
    $propertyNames = if ($Budget -is [Collections.IDictionary]) {
        @($Budget.Keys | ForEach-Object { [string]$_ })
    } else {
        @($Budget.PSObject.Properties | ForEach-Object { $_.Name })
    }
    if ($propertyNames.Count -ne $fields.Count -or @($propertyNames | Where-Object { $_ -notin $fields }).Count -ne 0) {
        throw $FailureReason
    }
    try {
        $expected = Get-CellTimeoutBudget -TierMiB $TierMiB
        $canonical = [ordered]@{}
        foreach ($field in $fields) {
            $actual = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $Budget -Name $field) -Name ("timeout_budget_" + $field)
            if ($actual -ne [int64](Get-RequiredProperty -Object $expected -Name $field)) { throw $FailureReason }
            $canonical[$field] = $actual
        }
        return [pscustomobject]$canonical
    } catch {
        throw $FailureReason
    }
}

function Assert-CellTimeoutBudgetMatch {
    param(
        [Parameter(Mandatory = $true)]$Budget,
        [Parameter(Mandatory = $true)][int]$TierMiB,
        [Parameter(Mandatory = $true)][string]$FailureReason
    )
    try {
        $expected = Get-StrictCellTimeoutBudget -Budget (Get-CellTimeoutBudget -TierMiB $TierMiB) `
            -TierMiB $TierMiB -FailureReason $FailureReason
        $observed = Get-StrictCellTimeoutBudget -Budget $Budget -TierMiB $TierMiB -FailureReason $FailureReason
        if ((ConvertTo-CanonicalJson -Value $observed) -cne (ConvertTo-CanonicalJson -Value $expected)) {
            throw $FailureReason
        }
        return $observed
    } catch {
        throw $FailureReason
    }
}

function New-Plan {
    param(
        [ValidateSet("unobserved_plan_only", "PRODUCT_OFF")]
        [string]$TerminalState = "unobserved_plan_only"
    )
    $gpu = Get-GpuMemory
    $cells = @()
    $blockedTier = $null
    foreach ($tier in $tiers) {
        $boundedPairRequired = $tier + $external_workload_mib + $reserve_mib
        if ($gpu.free_vram_mib -lt $boundedPairRequired) {
            $blockedTier = $tier
            break
        }
        foreach ($condition in $conditions) {
            foreach ($mode in $modes) {
                $timeoutBudget = Get-CellTimeoutBudget -TierMiB $tier
                $cells += [ordered]@{
                    tier_mib = $tier
                    condition = $condition
                    mode = $mode
                    runs = 3
                    pattern = "shake256-v1"
                    measurement = "allocation_to_hold_ms"
                    allocation_chunk_bytes = 67108864
                    worker_threads = [Math]::Min(1, [Environment]::ProcessorCount)
                    workload = "anonymous_memory_sequential_write"
                    allocated_mib = $tier + 2560
                    memory_high_mib = 1200
                    memory_max_mib = $tier + 3072
                    external_workload_mib = if ($condition -eq "bounded") { 512 } else { 0 }
                    reserve_mib = 512
                    cell_required_free_vram_mib = $tier + $reserve_mib
                    bounded_pair_required_free_vram_mib = if ($condition -eq "bounded") { $boundedPairRequired } else { $tier + $reserve_mib }
                    plan_free_vram_mib = $gpu.free_vram_mib
                    sample_timeout_sec = $timeoutBudget.sample_timeout_sec
                    integrity_finalization_timeout_sec = $timeoutBudget.integrity_finalization_timeout_sec
                    setup_cleanup_timeout_sec = $timeoutBudget.setup_cleanup_timeout_sec
                    cell_outer_timeout_sec = $timeoutBudget.cell_outer_timeout_sec
                }
            }
        }
    }
    [ordered]@{
        schema = 1
        status = if ($null -eq $blockedTier) { "PLAN" } else { "REFUSED" }
        reason = if ($null -eq $blockedTier) { "complete_ordered_plan" } else { "gpu_headroom_shortfall" }
        blocked_tier_mib = $blockedTier
        gpu_total_mib = $gpu.total_mib
        free_vram_mib = $gpu.free_vram_mib
        external_workload_mib = 512
        reserve_mib = 512
        terminal_state = $TerminalState
        promotion_policy = "promotion_stopped on any non-PASS pair"
        cells = $cells
    }
}

function New-DirectWslRootArguments {
    param([Parameter(Mandatory = $true)][string[]]$LinuxArguments)
    @(
        "-d", $Distro, "-u", "root", "--", "env", "-i",
        "PATH=/usr/sbin:/usr/bin:/sbin:/bin", "HOME=/root"
    ) + @($LinuxArguments)
}

function Invoke-SelectedReleaseDirectRead {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string[]]$LinuxArguments
    )
    $run = Invoke-BoundedProcess -FilePath (Get-WslExecutable) -ArgumentValues @(
        New-DirectWslRootArguments -LinuxArguments $LinuxArguments
    ) -TimeoutSec 30
    if ($run.timed_out) { throw ("selected_release_timeout:" + $Name) }
    if (-not $run.completed -or $run.exit_code -ne 0 -or -not $run.stream_drain_complete) {
        throw ("selected_release_read_failed:" + $Name)
    }
    $lines = @($run.stdout -split '\r?\n' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($lines.Count -ne 1) { throw ("selected_release_read_invalid:" + $Name) }
    [pscustomobject]@{ name = $Name; line = [string]$lines[0]; run = $run }
}

function Assert-SelectedReleaseRecords {
    param(
        [Parameter(Mandatory = $true)][string]$Selected,
        [Parameter(Mandatory = $true)][string]$SourceCommit,
        [Parameter(Mandatory = $true)][string]$SourceTreeState,
        [Parameter(Mandatory = $true)][string]$InstalledManifestLine,
        [Parameter(Mandatory = $true)][string]$StoredInstalledManifest,
        [Parameter(Mandatory = $true)][string]$InputManifestLine,
        [Parameter(Mandatory = $true)][string]$ProvenanceJson,
        [Parameter(Mandatory = $true)][string]$ExpectedCommit
    )
    $selectorMatch = [regex]::Match($Selected, '^/opt/ramshared/releases/([A-Za-z0-9][A-Za-z0-9._-]{0,127})$')
    if (-not $selectorMatch.Success) { throw "selected_release_invalid" }
    $version = $selectorMatch.Groups[1].Value
    if ($SourceCommit -cnotmatch '^[0-9a-f]{40}$' -or $SourceTreeState -cne "clean") {
        throw "selected_release_source_identity_invalid"
    }
    $installedMatch = [regex]::Match($InstalledManifestLine, '^([0-9a-f]{64})\s+\S+$')
    if (-not $installedMatch.Success) { throw "selected_release_manifest_identity_invalid" }
    $installedManifestSha256 = $installedMatch.Groups[1].Value
    if ($StoredInstalledManifest -cne $installedManifestSha256) {
        throw "selected_release_manifest_identity_invalid"
    }
    $inputMatch = [regex]::Match($InputManifestLine, '^([0-9a-f]{64})\s+\S+$')
    if (-not $inputMatch.Success) { throw "selected_release_input_bundle_manifest_invalid" }
    $inputBundleManifestSha256 = $inputMatch.Groups[1].Value
    try {
        $provenance = ConvertFrom-JsonPreservingDateStrings -Json $ProvenanceJson
    } catch {
        throw "selected_release_provenance_invalid"
    }
    foreach ($propertyName in @("schema_version", "source_commit", "source_tree_state", "input_bundle_manifest_sha256")) {
        if ($provenance.PSObject.Properties.Name -notcontains $propertyName) {
            throw "selected_release_provenance_invalid"
        }
    }
    if ([string]$provenance.schema_version -cne "ramshared-installed-release-provenance/v1" -or
        [string]$provenance.source_commit -cne $SourceCommit -or
        [string]$provenance.source_tree_state -cne $SourceTreeState -or
        [string]$provenance.input_bundle_manifest_sha256 -cne $inputBundleManifestSha256) {
        throw "selected_release_provenance_invalid"
    }
    if ($SourceCommit -cne $ExpectedCommit) { throw "selected_release_source_mismatch" }
    [pscustomobject]@{
        selected = $Selected
        version = $version
        source_commit = $SourceCommit
        source_tree_state = $SourceTreeState
        manifest_sha256 = $installedManifestSha256
        installed_manifest_sha256 = $installedManifestSha256
        input_bundle_manifest_sha256 = $inputBundleManifestSha256
    }
}

function Resolve-SelectedRelease {
    $reads = @()
    $selectedRead = Invoke-SelectedReleaseDirectRead -Name "selector" -LinuxArguments @(
        "readlink", "-f", "/opt/ramshared/current"
    )
    $reads += $selectedRead
    $selected = $selectedRead.line
    if ($selected -cnotmatch '^/opt/ramshared/releases/[A-Za-z0-9][A-Za-z0-9._-]{0,127}$') {
        throw "selected_release_invalid"
    }

    $sourceRead = Invoke-SelectedReleaseDirectRead -Name "source_commit" -LinuxArguments @(
        "cat", "--", ($selected + "/SOURCE_COMMIT")
    )
    $treeRead = Invoke-SelectedReleaseDirectRead -Name "source_tree_state" -LinuxArguments @(
        "cat", "--", ($selected + "/SOURCE_TREE_STATE")
    )
    $installedManifestRead = Invoke-SelectedReleaseDirectRead -Name "installed_manifest" -LinuxArguments @(
        "sha256sum", "--", ($selected + "/SHA256SUMS")
    )
    $storedManifestRead = Invoke-SelectedReleaseDirectRead -Name "stored_installed_manifest" -LinuxArguments @(
        "cat", "--", ($selected + "/INSTALLED_MANIFEST_SHA256")
    )
    $provenanceRead = Invoke-SelectedReleaseDirectRead -Name "install_provenance" -LinuxArguments @(
        "cat", "--", ($selected + "/INSTALL_PROVENANCE.json")
    )
    $inputManifestRead = Invoke-SelectedReleaseDirectRead -Name "input_bundle_manifest" -LinuxArguments @(
        "sha256sum", "--", ($selected + "/INPUT_BUNDLE_SHA256SUMS")
    )
    $reads += @($sourceRead, $treeRead, $installedManifestRead, $storedManifestRead, $provenanceRead, $inputManifestRead)

    $identity = Assert-SelectedReleaseRecords -Selected $selected `
        -SourceCommit $sourceRead.line -SourceTreeState $treeRead.line `
        -InstalledManifestLine $installedManifestRead.line `
        -StoredInstalledManifest $storedManifestRead.line `
        -InputManifestLine $inputManifestRead.line -ProvenanceJson $provenanceRead.line `
        -ExpectedCommit $ExpectedSourceCommit
    $allStreamsDrained = @($reads | Where-Object { -not $_.run.stream_drain_complete }).Count -eq 0
    [pscustomobject]@{
        selected = $identity.selected
        version = $identity.version
        source_commit = $identity.source_commit
        source_tree_state = $identity.source_tree_state
        manifest_sha256 = $identity.manifest_sha256
        installed_manifest_sha256 = $identity.installed_manifest_sha256
        input_bundle_manifest_sha256 = $identity.input_bundle_manifest_sha256
        containment = [ordered]@{
            call = "selected_release_discovery_direct_argv"
            timeout_sec = 30
            call_count = $reads.Count
            completed = $true
            exit_code = 0
            stream_drain_complete = $allStreamsDrained
        }
    }
}

function Assert-PinnedReleaseIdentity {
    param([Parameter(Mandatory = $true)]$SelectedRelease)
    $selected = [string]$SelectedRelease.selected
    $version = [string]$SelectedRelease.version
    $sourceCommit = [string]$SelectedRelease.source_commit
    $manifestSha256 = [string]$SelectedRelease.manifest_sha256
    $inputBundleManifestSha256 = [string]$SelectedRelease.input_bundle_manifest_sha256
    if ($version -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$' -or
        $selected -cne ("/opt/ramshared/releases/" + $version)) {
        throw "pinned_release_identity_invalid"
    }
    if ($sourceCommit -notmatch '^[0-9a-f]{40}$' -or $manifestSha256 -notmatch '^[0-9a-f]{64}$' -or
        $inputBundleManifestSha256 -notmatch '^[0-9a-f]{64}$') {
        throw "pinned_release_identity_invalid"
    }
    if (-not [string]::IsNullOrWhiteSpace($ExpectedSourceCommit) -and
        $sourceCommit -cne $ExpectedSourceCommit) {
        throw "selected_release_source_mismatch"
    }
    [pscustomobject]@{
        selected = $selected
        version = $version
        source_commit = $sourceCommit
        manifest_sha256 = $manifestSha256
        input_bundle_manifest_sha256 = $inputBundleManifestSha256
    }
}

function New-PinnedNbdProductPreflightArguments {
    param([Parameter(Mandatory = $true)]$SelectedRelease)
    $identity = Assert-PinnedReleaseIdentity -SelectedRelease $SelectedRelease
    @(
        "-d", $Distro, "-u", "root", "--", "env", "-i",
        "PATH=/usr/sbin:/usr/bin:/sbin:/bin", "HOME=/root",
        ($identity.selected + "/scripts/safety/nbd-product-preflight.sh"), "--check",
        "--sealed-release-root", $identity.selected,
        "--release-version", $identity.version,
        "--expected-source-commit", $identity.source_commit,
        "--expected-manifest-sha256", $identity.manifest_sha256
    )
}

function New-PinnedNbdDeactivationArguments {
    param([Parameter(Mandatory = $true)]$SelectedRelease)
    $identity = Assert-PinnedReleaseIdentity -SelectedRelease $SelectedRelease
    @(
        "-d", $Distro, "-u", "root", "--", "env", "-i",
        "PATH=/usr/sbin:/usr/bin:/sbin:/bin", "HOME=/root",
        ("RAMSHARED_NBD_LIFECYCLE_APPROVAL=deactivate:" + $identity.version),
        ($identity.selected + "/bin/ramshared"), "down"
    )
}

function ConvertFrom-KeyValueOutput {
    param([Parameter(Mandatory = $true)][string]$Text)
    $fields = @{}
    foreach ($line in ($Text -split '\r?\n')) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        $parts = $line.Split('=', 2)
        if ($parts.Count -ne 2 -or [string]::IsNullOrWhiteSpace($parts[0]) -or
            $fields.ContainsKey($parts[0])) {
            throw "key_value_output_invalid"
        }
        $fields[$parts[0]] = $parts[1]
    }
    $fields
}

function Assert-CampaignProductOffOutput {
    param(
        [Parameter(Mandatory = $true)][string]$Text,
        [Parameter(Mandatory = $true)][string]$ExpectedVersion
    )
    $fields = ConvertFrom-KeyValueOutput -Text $Text
    if ($fields["NBD_PRODUCT_STATE"] -ne "PRODUCT_OFF" -or
        $fields["NBD_RELEASE_VERSION"] -ne $ExpectedVersion -or
        $fields["NBD_RELEASE_GATE"] -ne "PASS") {
        throw "campaign_product_off_not_proven"
    }
    $fields
}

function Assert-PinnedCampaignPreflightOutput {
    param(
        [Parameter(Mandatory = $true)][string]$Text,
        [Parameter(Mandatory = $true)]$SelectedRelease
    )
    $identity = Assert-PinnedReleaseIdentity -SelectedRelease $SelectedRelease
    $fields = ConvertFrom-KeyValueOutput -Text $Text
    foreach ($name in @(
        "NBD_RELEASE_VERSION", "NBD_RELEASE_MANIFEST_SHA256", "NBD_INPUT_BUNDLE_MANIFEST_SHA256",
        "NBD_INSTALL_PROVENANCE", "NBD_RELEASE_GATE", "NBD_SELECTOR", "NBD_LOWER_TIER_BINDING",
        "NBD_LOWER_TIER_CAPACITY", "NBD_RELAY_GATE", "NBD_BINARY_MATCH", "NBD_TRANSPORT",
        "NBD_PRODUCT_STATE", "NBD_READINESS_REASON"
    )) {
        if (-not $fields.ContainsKey($name)) { throw ("campaign_preflight_field_missing:" + $name) }
    }
    if ($fields["NBD_RELEASE_VERSION"] -cne $identity.version -or
        $fields["NBD_RELEASE_MANIFEST_SHA256"] -cne $identity.manifest_sha256 -or
        $fields["NBD_INPUT_BUNDLE_MANIFEST_SHA256"] -cne $identity.input_bundle_manifest_sha256 -or
        $fields["NBD_INSTALL_PROVENANCE"] -ne "PASS" -or $fields["NBD_RELEASE_GATE"] -ne "PASS" -or
        $fields["NBD_SELECTOR"] -ne "PASS" -or $fields["NBD_LOWER_TIER_BINDING"] -ne "bound" -or
        $fields["NBD_LOWER_TIER_CAPACITY"] -ne "PASS" -or $fields["NBD_RELAY_GATE"] -ne "PASS") {
        throw "campaign_preflight_release_binding_invalid"
    }
    $productState = [string]$fields["NBD_PRODUCT_STATE"]
    $binaryMatch = [string]$fields["NBD_BINARY_MATCH"]
    if ($productState -eq "READY") {
        if ($binaryMatch -ne "PASS" -or $fields["NBD_TRANSPORT"] -ne "nbd" -or
            $fields["NBD_READINESS_REASON"] -ne "all_gates_pass") {
            throw "campaign_preflight_ready_binary_match_required"
        }
    } elseif ($productState -eq "PRODUCT_OFF") {
        if ($binaryMatch -ne "NOT_APPLICABLE" -or $fields["NBD_TRANSPORT"] -ne "none" -or
            $fields["NBD_READINESS_REASON"] -ne "product_off") {
            throw "campaign_preflight_product_off_invalid"
        }
    } else {
        throw "campaign_preflight_product_state_invalid"
    }
    [pscustomobject]@{
        fields = $fields
        product_state = $productState
        binary_match = $binaryMatch
    }
}

function Get-Sha256File {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "artifact_hash_target_missing" }
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Get-WindowsPairScriptHashes {
    param([Parameter(Mandatory = $true)][string]$Condition)
    $matrixScript = Join-Path $PSScriptRoot "Invoke-NbdBenchmarkMatrix.ps1"
    $cudaScript = Join-Path $PSScriptRoot "..\p0\Start-CudaVramWorkload.ps1"
    [ordered]@{
        "Invoke-NbdBenchmarkMatrix.ps1" = Get-Sha256File -Path $matrixScript
        "Start-CudaVramWorkload.ps1" = if ($Condition -eq "bounded") { Get-Sha256File -Path $cudaScript } else { "N/A" }
    }
}

function Invoke-PinnedCampaignProductPreflight {
    param(
        [Parameter(Mandatory = $true)]$SelectedRelease,
        [Parameter(Mandatory = $true)][string]$CampaignRoot,
        [Parameter(Mandatory = $true)][ValidateSet("before", "after")][string]$Phase
    )
    $out = Join-Path $CampaignRoot ("campaign-preflight-" + $Phase + ".out")
    $err = Join-Path $CampaignRoot ("campaign-preflight-" + $Phase + ".err")
    $arguments = @(New-PinnedNbdProductPreflightArguments -SelectedRelease $SelectedRelease)
    $check = Invoke-BoundedProcess -FilePath (Get-WslExecutable) -ArgumentValues $arguments -TimeoutSec 60
    Write-ProcessLogs -Run $check -StdoutPath $out -StderrPath $err
    if ($check.timed_out -or -not $check.completed -or $check.exit_code -ne 0) {
        throw ("campaign_product_off_preflight_" + $Phase + "_failed")
    }
    try {
        $parsed = Assert-PinnedCampaignPreflightOutput -Text $check.stdout -SelectedRelease $SelectedRelease
    } catch {
        throw ("campaign_product_off_preflight_" + $Phase + "_invalid:" + $_.Exception.Message)
    }
    [pscustomobject]@{
        status = "PASS"
        phase = $Phase
        product_state = $parsed.product_state
        binary_match = $parsed.binary_match
        source_commit = $SelectedRelease.source_commit
        manifest_sha256 = $SelectedRelease.manifest_sha256
        input_bundle_manifest_sha256 = $SelectedRelease.input_bundle_manifest_sha256
        preflight = [ordered]@{
            timeout_sec = 60; completed = $check.completed; exit_code = $check.exit_code
            stream_drain_complete = $check.stream_drain_complete
            stdout_sha256 = Get-Sha256File -Path $out; stderr_sha256 = Get-Sha256File -Path $err
            release_version = $parsed.fields["NBD_RELEASE_VERSION"]
        }
    }
}

function Invoke-CampaignProductOffPreflight {
    param(
        [Parameter(Mandatory = $true)]$SelectedRelease,
        [Parameter(Mandatory = $true)][string]$CampaignRoot
    )
    $before = Invoke-PinnedCampaignProductPreflight -SelectedRelease $SelectedRelease -CampaignRoot $CampaignRoot -Phase "before"
    $deactivation = $null
    if ($before.product_state -eq "READY") {
        $downOut = Join-Path $CampaignRoot "campaign-preflight-down.out"
        $downErr = Join-Path $CampaignRoot "campaign-preflight-down.err"
        $arguments = @(New-PinnedNbdDeactivationArguments -SelectedRelease $SelectedRelease)
        $down = Invoke-BoundedProcess -FilePath (Get-WslExecutable) -ArgumentValues $arguments -TimeoutSec 120
        Write-ProcessLogs -Run $down -StdoutPath $downOut -StderrPath $downErr
        if ($down.timed_out -or -not $down.completed -or $down.exit_code -ne 0) {
            throw "campaign_product_off_deactivation_failed"
        }
        $deactivation = [ordered]@{
            action = "pinned_ramshared_down"
            binary = ($SelectedRelease.selected + "/bin/ramshared")
            lifecycle_approval = ("deactivate:" + $SelectedRelease.version)
            timeout_sec = 120; completed = $down.completed; exit_code = $down.exit_code
            stream_drain_complete = $down.stream_drain_complete
            stdout_sha256 = Get-Sha256File -Path $downOut; stderr_sha256 = Get-Sha256File -Path $downErr
        }
    } else {
        $deactivation = [ordered]@{
            action = "not_required_product_off"
            binary = ($SelectedRelease.selected + "/bin/ramshared")
            lifecycle_approval = ("deactivate:" + $SelectedRelease.version)
        }
    }
    $after = Invoke-PinnedCampaignProductPreflight -SelectedRelease $SelectedRelease -CampaignRoot $CampaignRoot -Phase "after"
    if ($after.product_state -ne "PRODUCT_OFF") {
        throw "campaign_product_off_not_proven"
    }
    [pscustomobject]@{
        status = "PASS"
        product_state = "PRODUCT_OFF"
        source_commit = $SelectedRelease.source_commit
        manifest_sha256 = $SelectedRelease.manifest_sha256
        input_bundle_manifest_sha256 = $SelectedRelease.input_bundle_manifest_sha256
        before = $before
        deactivation = $deactivation
        after = $after
    }
}

function New-CellResult {
    param(
        [Parameter(Mandatory = $true)]$Cell,
        [Parameter(Mandatory = $true)][string]$Status,
        [Parameter(Mandatory = $true)][string]$Reason,
        [Parameter(Mandatory = $true)][string]$TerminalState,
        [hashtable]$Extra = @{}
    )
    $record = [ordered]@{
        status = $Status
        reason = $Reason
        tier_mib = $Cell.tier_mib
        condition = $Cell.condition
        mode = $Cell.mode
        terminal_state = $TerminalState
        promotion = if ($Status -eq "PASS") { "eligible" } else { "promotion_stopped" }
    }
    foreach ($key in $Extra.Keys) { $record[$key] = $Extra[$key] }
    [pscustomobject]$record
}

function New-CellExecution {
    param(
        [Parameter(Mandatory = $true)]$Result,
        $Context = $null,
        $Evidence = $null
    )
    [pscustomobject]@{ result = $Result; context = $Context; evidence = $Evidence }
}

function New-SanitizedCudaCompletion {
    param([Parameter(Mandatory = $true)]$Completion)
    $exitCode = $null
    try {
        if ($null -ne $Completion.exit_code) {
            $candidate = [int64]$Completion.exit_code
            $exitCode = $candidate
        }
    } catch {
        $exitCode = $null
    }
    [pscustomobject][ordered]@{
        released = [bool]$Completion.released
        forced_termination = [bool]$Completion.forced_termination
        exit_code = $exitCode
        stream_drain_complete = [bool]$Completion.stream_drain_complete
        error_present = -not [string]::IsNullOrWhiteSpace([string]$Completion.error)
    }
}

function Apply-CudaCompletionToPairResult {
    param(
        [Parameter(Mandatory = $true)][object[]]$PairResults,
        [Parameter(Mandatory = $true)]$Completion
    )
    if (@($PairResults).Count -eq 0) { return }
    $result = $PairResults[@($PairResults).Count - 1].result
    $secondary = New-SanitizedCudaCompletion -Completion $Completion
    if ([string]$result.reason -eq "watchdog_timeout_red") {
        $result | Add-Member -NotePropertyName "cuda_cleanup_secondary" `
            -NotePropertyValue ([pscustomobject]$secondary) -Force
        return
    }
    $result.status = "RED"
    $result.reason = "bounded_cuda_workload_failed"
    $result.terminal_state = "unverified_unknown"
    $result.promotion = "promotion_stopped"
    $result | Add-Member -NotePropertyName "cuda_completion" -NotePropertyValue $secondary -Force
}

function Assert-CellFailureReceipt {
    param(
        [Parameter(Mandatory = $true)][string]$ReceiptPath,
        [Parameter(Mandatory = $true)]$Cell,
        [Parameter(Mandatory = $true)]$SelectedRelease,
        [Parameter(Mandatory = $true)]$PairContext
    )
    if (-not (Test-Path -LiteralPath $ReceiptPath -PathType Leaf)) {
        throw "cell_failure_receipt_invalid"
    }
    $item = Get-Item -LiteralPath $ReceiptPath -Force
    if ($item.PSIsContainer -or (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) -or
        $item.Length -lt 1 -or $item.Length -gt 16384) {
        throw "cell_failure_receipt_invalid"
    }
    try { $receipt = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $ReceiptPath -Raw) } catch {
        throw "cell_failure_receipt_invalid"
    }
    $allowed = @(
        "schema_version", "status", "reason", "terminal_state", "release_version",
        "pair_id", "mode", "condition", "tier_mib"
    )
    $properties = @($receipt.PSObject.Properties | ForEach-Object { $_.Name })
    if ($properties.Count -ne $allowed.Count -or
        @($properties | Where-Object { $_ -notin $allowed }).Count -ne 0) {
        throw "cell_failure_receipt_invalid"
    }
    foreach ($name in $allowed) {
        if ($null -eq $receipt.PSObject.Properties[$name]) { throw "cell_failure_receipt_invalid" }
    }
    if ([string]$receipt.schema_version -cne "ramshared-nbd-cell-failure/v1" -or
        [string]$receipt.status -cne "RED" -or
        [string]$receipt.terminal_state -cne "PRODUCT_OFF") {
        throw "cell_failure_receipt_product_off"
    }
    if ([string]$receipt.reason -notmatch '^[A-Z0-9_]{1,96}$' -or
        [string]$receipt.reason -eq "WATCHDOG_TIMEOUT_RED") {
        throw "cell_failure_receipt_invalid"
    }
    if ([string]$receipt.release_version -cne [string]$SelectedRelease.version -or
        [string]$receipt.pair_id -cne [string]$PairContext.pair_id -or
        [string]$receipt.mode -cne [string]$Cell.mode -or
        [string]$receipt.condition -cne [string]$Cell.condition) {
        throw "cell_failure_receipt_invalid"
    }
    try {
        $tier = ConvertTo-StrictInt64 -Value $receipt.tier_mib -Name "failure_receipt_tier_mib"
    } catch {
        throw "cell_failure_receipt_invalid"
    }
    if ($tier -ne [int64]$Cell.tier_mib) { throw "cell_failure_receipt_invalid" }
    [pscustomobject]@{
        schema_version = [string]$receipt.schema_version
        status = [string]$receipt.status
        reason = [string]$receipt.reason
        terminal_state = [string]$receipt.terminal_state
        release_version = [string]$receipt.release_version
        pair_id = [string]$receipt.pair_id
        mode = [string]$receipt.mode
        condition = [string]$receipt.condition
        tier_mib = $tier
        sha256 = Get-Sha256File -Path $ReceiptPath
    }
}

function New-CellControllerFailureExecution {
    param(
        [Parameter(Mandatory = $true)]$Run,
        [Parameter(Mandatory = $true)]$Cell,
        [Parameter(Mandatory = $true)][string]$CellDirectory,
        [Parameter(Mandatory = $true)]$SelectedRelease,
        [Parameter(Mandatory = $true)]$PairContext,
        [Parameter(Mandatory = $true)]$Containment
    )
    if ($Run.timed_out -isnot [bool]) {
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "RED" `
            -Reason "wsl_controller_failed" -TerminalState "unverified_unknown" -Extra @{
                containment = $Containment; pair_context = $PairContext
            })
    }
    if ($Run.timed_out -eq $true) {
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "RED" `
            -Reason "watchdog_timeout_red" -TerminalState "unverified_unknown" -Extra @{
                containment = $Containment; pair_context = $PairContext
            })
    }
    $runIsTerminalFailure = $false
    if ($Run.completed -is [bool] -and $Run.completed -eq $true -and
        $Run.timed_out -is [bool] -and $Run.timed_out -eq $false) {
        try {
            $failureExitCode = ConvertTo-StrictInt64 -Value $Run.exit_code -Name "cell_failure_exit_code"
            $runIsTerminalFailure = $failureExitCode -ne 0
        } catch {
            $runIsTerminalFailure = $false
        }
    }
    if (-not $runIsTerminalFailure) {
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "RED" `
            -Reason "wsl_controller_failed" -TerminalState "unverified_unknown" -Extra @{
                containment = $Containment; pair_context = $PairContext
            })
    }
    $failureReceiptPath = Join-Path $CellDirectory "result\failure-receipt.json"
    try {
        $failureReceipt = Assert-CellFailureReceipt -ReceiptPath $failureReceiptPath -Cell $Cell `
            -SelectedRelease $SelectedRelease -PairContext $PairContext
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "RED" `
            -Reason $failureReceipt.reason -TerminalState "PRODUCT_OFF" -Extra @{
                containment = $Containment; pair_context = $PairContext
                failure_receipt = $failureReceipt
            })
    } catch {
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "RED" `
            -Reason "wsl_controller_failed" -TerminalState "unverified_unknown" -Extra @{
                containment = $Containment; pair_context = $PairContext
            })
    }
}

function Get-RequiredProperty {
    param(
        [Parameter(Mandatory = $true)]$Object,
        [Parameter(Mandatory = $true)][string]$Name
    )
    if ($Object -is [Collections.IDictionary]) {
        if (-not $Object.Contains($Name)) { throw "required_property_missing:$Name" }
        return $Object[$Name]
    }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) { throw "required_property_missing:$Name" }
    $property.Value
}

function ConvertTo-FiniteNumber {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Name,
        [switch]$AllowZero
    )
    try { $number = [double]$Value } catch { throw "metric_not_numeric:$Name" }
    $invalidSign = if ($AllowZero) { $number -lt 0 } else { $number -le 0 }
    if ([double]::IsNaN($number) -or [double]::IsInfinity($number) -or $invalidSign) {
        throw "metric_not_finite_positive:$Name"
    }
    $number
}

function Assert-Sha256Value {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Name
    )
    $text = [string]$Value
    if ($text -notmatch '^[0-9a-f]{64}$') { throw "sha256_invalid:$Name" }
    $text
}

function Resolve-CellArtifactPath {
    param(
        [Parameter(Mandatory = $true)][string]$CellResultDirectory,
        [Parameter(Mandatory = $true)]$Name
    )
    if ($Name -isnot [string]) { throw "cell_evidence_inventory_path_invalid" }
    $relative = [string]$Name
    if ([string]::IsNullOrWhiteSpace($relative) -or $relative -match '[\\:\r\n]' -or
        $relative -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*(?:/[A-Za-z0-9][A-Za-z0-9._-]*)*$' -or
        $relative -match '(^|/)\.\.?(/|$)') {
        throw "cell_evidence_inventory_path_invalid"
    }
    $root = [IO.Path]::GetFullPath($CellResultDirectory)
    $separator = [IO.Path]::DirectorySeparatorChar.ToString()
    $prefix = if ($root.EndsWith($separator)) { $root } else { $root + $separator }
    $candidate = [IO.Path]::GetFullPath((Join-Path $root ($relative -replace '/', '\\')))
    if (-not $candidate.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "cell_evidence_inventory_path_invalid"
    }
    [pscustomobject]@{ name = ($relative -replace '\\', '/'); path = $candidate }
}

function Get-ArtifactInventoryEntry {
    param(
        [Parameter(Mandatory = $true)]$Inventory,
        [Parameter(Mandatory = $true)][string]$Name
    )
    $matches = @($Inventory.files | Where-Object { $_.name -ceq $Name })
    if ($matches.Count -ne 1) { throw "artifact_inventory_entry_invalid:$Name" }
    $matches[0]
}

function Assert-CellEvidence {
    param(
        [Parameter(Mandatory = $true)]$Summary,
        [Parameter(Mandatory = $true)][string]$CellResultDirectory
    )
    $root = [IO.Path]::GetFullPath($CellResultDirectory)
    if (-not (Test-Path -LiteralPath $root -PathType Container)) { throw "cell_evidence_artifact_missing" }
    $contextPath = Join-Path $root "context.json"
    $inventoryPath = Join-Path $root "artifact-inventory.json"
    $summaryPath = Join-Path $root "summary.json"
    $envelopePath = Join-Path $root "evidence-envelope.json"
    foreach ($path in @($contextPath, $inventoryPath, $summaryPath, $envelopePath)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "cell_evidence_artifact_missing" }
    }
    $contextHash = Get-Sha256File -Path $contextPath
    $summaryHash = Get-Sha256File -Path $summaryPath
    $inventoryHash = Get-Sha256File -Path $inventoryPath
    $summaryContextHash = Assert-Sha256Value -Value (Get-RequiredProperty -Object $Summary -Name "context_sha256") -Name "summary_context"
    if ($summaryContextHash -cne $contextHash) { throw "cell_evidence_context_sha256_mismatch" }
    try {
        $context = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $contextPath -Raw)
        $summaryArtifact = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $summaryPath -Raw)
        $inventory = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $inventoryPath -Raw)
        $envelope = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $envelopePath -Raw)
    } catch {
        throw "cell_evidence_json_invalid"
    }
    if ($context.schema -ne 2 -or $inventory.schema -ne 2 -or $null -eq $inventory.files) {
        throw "cell_evidence_schema_invalid"
    }
    $contextMode = [string](Get-RequiredProperty -Object $context -Name "mode")
    $contextBinaryMatch = [string](Get-RequiredProperty -Object $context -Name "binary_match")
    $expectedBinaryMatch = if ($contextMode -eq "nbd") { "PASS" } elseif ($contextMode -eq "disk-only") { "N/A" } else { "" }
    if ($expectedBinaryMatch -eq "" -or $contextBinaryMatch -ne $expectedBinaryMatch -or
        [string](Get-RequiredProperty -Object $Summary -Name "mode") -ne $contextMode -or
        [string](Get-RequiredProperty -Object $Summary -Name "binary_match") -ne $contextBinaryMatch -or
        [string](Get-RequiredProperty -Object $Summary -Name "status") -ne "PASS" -or
        [string](Get-RequiredProperty -Object $Summary -Name "terminal_state") -ne "PRODUCT_OFF") {
        throw "cell_evidence_summary_contract_invalid"
    }
    try {
        $contextTierMiB = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $context -Name "tier_mib") -Name "context_tier_mib"
        $null = Get-StrictCellTimeoutBudget -Budget (Get-RequiredProperty -Object $context -Name "timeout_budget") -TierMiB ([int]$contextTierMiB) -FailureReason "cell_evidence_timeout_budget_mismatch"
        $null = Get-StrictCellTimeoutBudget -Budget (Get-RequiredProperty -Object $summaryArtifact -Name "timeout_budget") -TierMiB ([int]$contextTierMiB) -FailureReason "cell_evidence_timeout_budget_mismatch"
        $null = Get-StrictCellTimeoutBudget -Budget (Get-RequiredProperty -Object $Summary -Name "timeout_budget") -TierMiB ([int]$contextTierMiB) -FailureReason "cell_evidence_timeout_budget_mismatch"
        $null = Get-StrictCellTimeoutBudget -Budget (Get-RequiredProperty -Object $envelope -Name "timeout_budget") -TierMiB ([int]$contextTierMiB) -FailureReason "cell_evidence_timeout_budget_mismatch"
    } catch {
        throw "cell_evidence_timeout_budget_mismatch"
    }
    $cellLowerTopology = Get-ModeBoundLowerTopology -Context $context -ExpectedMode $contextMode -FailurePrefix "cell_evidence"
    if ($contextMode -eq "nbd") {
        try {
            $contextTierMiB = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $context -Name "tier_mib") -Name "context_tier_mib"
        } catch {
            throw "cell_evidence_nbd_identity_invalid"
        }
        $null = Get-NbdIdentity -Context $context -ExpectedTierMiB $contextTierMiB -LowerTopology $cellLowerTopology -FailurePrefix "cell_evidence"
    } elseif ($null -ne $context.PSObject.Properties["nbd"]) {
        throw "cell_evidence_nbd_identity_unexpected"
    }
    $declared = @{}
    $entries = @($inventory.files)
    if ($entries.Count -eq 0) { throw "cell_evidence_inventory_empty" }
    foreach ($entry in $entries) {
        $resolved = Resolve-CellArtifactPath -CellResultDirectory $root -Name (Get-RequiredProperty -Object $entry -Name "name")
        $key = $resolved.name.ToLowerInvariant()
        if ($declared.ContainsKey($key) -or $resolved.name -in @("artifact-inventory.json", "evidence-envelope.json")) {
            throw "cell_evidence_inventory_path_invalid"
        }
        if (-not (Test-Path -LiteralPath $resolved.path -PathType Leaf)) { throw "cell_evidence_inventory_file_missing:$($resolved.name)" }
        $item = Get-Item -LiteralPath $resolved.path -Force
        if ($item.PSIsContainer -or (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
            throw "cell_evidence_inventory_file_invalid:$($resolved.name)"
        }
        try { $declaredBytes = [int64](Get-RequiredProperty -Object $entry -Name "bytes") } catch {
            throw "cell_evidence_inventory_bytes_invalid:$($resolved.name)"
        }
        if ($declaredBytes -lt 0 -or $declaredBytes -ne [int64]$item.Length) {
            throw "cell_evidence_inventory_bytes_invalid:$($resolved.name)"
        }
        $declaredHash = Assert-Sha256Value -Value (Get-RequiredProperty -Object $entry -Name "sha256") -Name ("inventory_" + $resolved.name)
        if ($declaredHash -cne (Get-Sha256File -Path $resolved.path)) {
            throw "cell_evidence_inventory_hash_mismatch:$($resolved.name)"
        }
        $declared[$key] = $true
    }
    foreach ($name in @("before.txt", "action.txt", "after.txt", "context.json", "summary.json", "samples.jsonl")) {
        if (-not $declared.ContainsKey($name.ToLowerInvariant())) { throw "cell_evidence_required_artifact_missing:$name" }
        $requiredEntry = Get-ArtifactInventoryEntry -Inventory $inventory -Name $name
        if ([int64]$requiredEntry.bytes -lt 1) { throw "cell_evidence_required_artifact_empty:$name" }
    }
    $rootPrefix = if ($root.EndsWith([IO.Path]::DirectorySeparatorChar.ToString())) { $root } else { $root + [IO.Path]::DirectorySeparatorChar }
    Get-ChildItem -LiteralPath $root -File -Recurse -Force | ForEach-Object {
        if (($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "cell_evidence_inventory_file_invalid" }
        $relative = $_.FullName.Substring($rootPrefix.Length) -replace '\\', '/'
        if ($relative -in @("artifact-inventory.json", "evidence-envelope.json")) { return }
        if (-not $declared.ContainsKey($relative.ToLowerInvariant())) { throw "cell_evidence_inventory_unlisted_file:$relative" }
    }
    $allowedEnvelopeProperties = @(
        "schema_version", "pair_id", "mode", "release", "context_sha256", "summary_sha256",
        "artifact_inventory_sha256", "artifacts", "binary_match", "watchdog", "timeout_budget", "classification"
    )
    foreach ($property in $envelope.PSObject.Properties) {
        if ($property.Name -notin $allowedEnvelopeProperties) { throw "cell_evidence_envelope_internal_invalid" }
    }
    foreach ($name in $allowedEnvelopeProperties) {
        $null = Get-RequiredProperty -Object $envelope -Name $name
    }
    if ([string](Get-RequiredProperty -Object $envelope -Name "schema_version") -ne "ramshared-nbd-cell-evidence/v1" -or
        [string](Get-RequiredProperty -Object $envelope -Name "pair_id") -cne [string]$context.pair_id -or
        [string](Get-RequiredProperty -Object $envelope -Name "mode") -cne $contextMode -or
        (Assert-Sha256Value -Value (Get-RequiredProperty -Object $envelope -Name "context_sha256") -Name "envelope_context") -cne $contextHash -or
        (Assert-Sha256Value -Value (Get-RequiredProperty -Object $envelope -Name "summary_sha256") -Name "envelope_summary") -cne $summaryHash -or
        (Assert-Sha256Value -Value (Get-RequiredProperty -Object $envelope -Name "artifact_inventory_sha256") -Name "envelope_inventory") -cne $inventoryHash -or
        [string](Get-RequiredProperty -Object $envelope -Name "binary_match") -cne $contextBinaryMatch -or
        [string](Get-RequiredProperty -Object $envelope -Name "classification") -ne "INCOMPARABLE") {
        throw "cell_evidence_envelope_internal_invalid"
    }
    $contextRelease = Get-RequiredProperty -Object $context -Name "release"
    $envelopeRelease = Get-RequiredProperty -Object $envelope -Name "release"
    $allowedReleaseProperties = @("version", "source_commit", "manifest_sha256", "input_bundle_manifest_sha256")
    foreach ($property in $envelopeRelease.PSObject.Properties) {
        if ($property.Name -notin $allowedReleaseProperties) { throw "cell_evidence_envelope_release_mismatch" }
    }
    foreach ($name in @("version", "source_commit", "manifest_sha256")) {
        if ([string](Get-RequiredProperty -Object $envelopeRelease -Name $name) -cne [string](Get-RequiredProperty -Object $contextRelease -Name $name)) {
            throw "cell_evidence_envelope_release_mismatch"
        }
    }
    $contextInputBundleProperty = $contextRelease.PSObject.Properties["input_bundle_manifest_sha256"]
    $envelopeInputBundleProperty = $envelopeRelease.PSObject.Properties["input_bundle_manifest_sha256"]
    $contextInputBundle = if ($null -eq $contextInputBundleProperty) { "not_exposed" } else { [string]$contextInputBundleProperty.Value }
    $envelopeInputBundle = if ($null -eq $envelopeInputBundleProperty) { "not_exposed" } else { [string]$envelopeInputBundleProperty.Value }
    if ($contextInputBundle -ne $envelopeInputBundle -or
        ($contextInputBundle -ne "not_exposed" -and $contextInputBundle -notmatch '^[0-9a-f]{64}$')) {
        throw "cell_evidence_envelope_release_mismatch"
    }
    $contextWatchdog = Get-RequiredProperty -Object $context -Name "watchdog"
    $envelopeWatchdog = Get-RequiredProperty -Object $envelope -Name "watchdog"
    if ([bool](Get-RequiredProperty -Object $contextWatchdog -Name "armed") -ne $true -or
        [bool](Get-RequiredProperty -Object $envelopeWatchdog -Name "armed") -ne $true -or
        [string](Get-RequiredProperty -Object $contextWatchdog -Name "outcome") -ne "not_fired" -or
        [string](Get-RequiredProperty -Object $envelopeWatchdog -Name "outcome") -cne [string](Get-RequiredProperty -Object $contextWatchdog -Name "outcome")) {
        throw "cell_evidence_envelope_watchdog_mismatch"
    }
    $envelopeArtifacts = @((Get-RequiredProperty -Object $envelope -Name "artifacts"))
    $envelopeDeclared = @{}
    foreach ($artifact in $envelopeArtifacts) {
        $resolved = Resolve-CellArtifactPath -CellResultDirectory $root -Name (Get-RequiredProperty -Object $artifact -Name "path")
        $key = $resolved.name.ToLowerInvariant()
        if ($envelopeDeclared.ContainsKey($key) -or -not $declared.ContainsKey($key) -or
            [int64](Get-RequiredProperty -Object $artifact -Name "bytes") -ne [int64](Get-Item -LiteralPath $resolved.path).Length -or
            (Assert-Sha256Value -Value (Get-RequiredProperty -Object $artifact -Name "sha256") -Name ("envelope_" + $resolved.name)) -cne (Get-Sha256File -Path $resolved.path)) {
            throw "cell_evidence_envelope_artifact_mismatch"
        }
        $envelopeDeclared[$key] = $true
    }
    if ($envelopeDeclared.Count -ne $declared.Count) { throw "cell_evidence_envelope_artifact_mismatch" }
    $evidence = [pscustomobject]@{
        cell_result_directory = $root
        context = $context
        context_sha256 = $contextHash
        summary_sha256 = $summaryHash
        artifact_inventory_sha256 = $inventoryHash
        internal_envelope_sha256 = Get-Sha256File -Path $envelopePath
    }
    $evidence | Add-Member -NotePropertyName "custody_fingerprint" `
        -NotePropertyValue (Get-CellEvidenceCustodyFingerprint -Evidence $evidence) -Force
    $evidence
}

function Get-Sha256Text {
    param([Parameter(Mandatory = $true)][string]$Text)
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes($Text)
        ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace("-", "").ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}

function Get-CellEvidenceCustodyFingerprint {
    param([Parameter(Mandatory = $true)]$Evidence)
    try {
        $declaredRoot = Get-RequiredProperty -Object $Evidence -Name "cell_result_directory"
        if ($declaredRoot -isnot [string] -or [string]::IsNullOrWhiteSpace($declaredRoot) -or
            -not [IO.Path]::IsPathRooted([string]$declaredRoot)) {
            throw "invalid_root"
        }
        $root = [IO.Path]::GetFullPath([string]$declaredRoot)
        if ($root -cne [string]$declaredRoot) { throw "noncanonical_root" }
        $material = [ordered]@{
            schema_version = "ramshared-nbd-cell-custody-fingerprint/v1"
            cell_result_directory = $root
            context_sha256 = Assert-Sha256Value -Value (Get-RequiredProperty -Object $Evidence -Name "context_sha256") -Name "cell_context"
            summary_sha256 = Assert-Sha256Value -Value (Get-RequiredProperty -Object $Evidence -Name "summary_sha256") -Name "cell_summary"
            artifact_inventory_sha256 = Assert-Sha256Value -Value (Get-RequiredProperty -Object $Evidence -Name "artifact_inventory_sha256") -Name "cell_inventory"
            internal_envelope_sha256 = Assert-Sha256Value -Value (Get-RequiredProperty -Object $Evidence -Name "internal_envelope_sha256") -Name "cell_envelope"
        }
        Get-Sha256Text -Text (ConvertTo-CanonicalJson -Value $material)
    } catch {
        throw "public_pair_evidence_custody_required"
    }
}

function ConvertTo-CanonicalJson {
    param([AllowNull()]$Value)
    if ($null -eq $Value) { return "null" }
    if ($Value -is [datetime] -or $Value -is [datetimeoffset]) {
        $utc = if ($Value -is [datetimeoffset]) {
            ([datetimeoffset]$Value).UtcDateTime
        } else {
            ([datetime]$Value).ToUniversalTime()
        }
        $iso8601 = $utc.ToString(
            "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'",
            [Globalization.CultureInfo]::InvariantCulture
        )
        $iso8601 = $iso8601 -replace '(\.\d*?[1-9])0+Z$', '$1Z'
        $iso8601 = $iso8601 -replace '\.0+Z$', 'Z'
        return ($iso8601 | ConvertTo-Json -Compress)
    }
    if ($Value -is [string] -or $Value -is [char]) {
        return ($Value | ConvertTo-Json -Compress)
    }
    if ($Value -is [bool]) {
        if ($Value) { return "true" }
        return "false"
    }
    if ($Value -is [byte] -or $Value -is [sbyte] -or $Value -is [int16] -or $Value -is [uint16] -or
        $Value -is [int32] -or $Value -is [uint32] -or $Value -is [int64] -or $Value -is [uint64] -or
        $Value -is [decimal]) {
        return ([System.Convert]::ToString($Value, [Globalization.CultureInfo]::InvariantCulture))
    }
    if ($Value -is [single] -or $Value -is [double]) {
        if ([double]::IsNaN([double]$Value) -or [double]::IsInfinity([double]$Value)) {
            throw "canonical_json_nonfinite_number"
        }
        return ([double]$Value).ToString("R", [Globalization.CultureInfo]::InvariantCulture)
    }
    if ($Value -is [Collections.IDictionary]) {
        $parts = @()
        foreach ($key in @($Value.Keys | ForEach-Object { [string]$_ } | Sort-Object)) {
            $parts += (($key | ConvertTo-Json -Compress) + ":" + (ConvertTo-CanonicalJson -Value $Value[$key]))
        }
        return "{" + ($parts -join ",") + "}"
    }
    if ($Value -is [Collections.IEnumerable]) {
        $parts = @()
        foreach ($item in @($Value)) { $parts += (ConvertTo-CanonicalJson -Value $item) }
        return "[" + ($parts -join ",") + "]"
    }
    $parts = @()
    foreach ($property in @($Value.PSObject.Properties | Sort-Object Name)) {
        $parts += (($property.Name | ConvertTo-Json -Compress) + ":" + (ConvertTo-CanonicalJson -Value $property.Value))
    }
    "{" + ($parts -join ",") + "}"
}

function Get-PublicEvidenceFingerprint {
    param(
        [Parameter(Mandatory = $true)][string]$HarnessRevision,
        [Parameter(Mandatory = $true)]$Platform,
        [Parameter(Mandatory = $true)]$Workload
    )
    $material = [ordered]@{
        schema_version = "ramshared-evidence/v1"
        harness_revision = $HarnessRevision
        platform = $Platform
        workload = $Workload
    }
    Get-Sha256Text -Text (ConvertTo-CanonicalJson -Value $material)
}

function New-PublicMetric {
    param(
        [Parameter(Mandatory = $true)]$Summary,
        [Parameter(Mandatory = $true)][string]$Name
    )
    $samples = @($Summary.samples | ForEach-Object {
        ConvertTo-FiniteNumber -Value (Get-RequiredProperty -Object $_ -Name "allocation_to_hold_ms") -Name $Name
    })
    if ($samples.Count -ne 3) { throw "public_pair_evidence_metric_sample_count_invalid" }
    $ordered = @($samples | Sort-Object)
    $mean = ($samples | Measure-Object -Average).Average
    $variance = 0.0
    foreach ($sample in $samples) { $variance += [math]::Pow(([double]$sample - [double]$mean), 2) }
    [ordered]@{
        unit = "ms"
        samples = $samples
        n = $samples.Count
        aggregation = "median-nearest-rank-p99-population-stddev"
        median = [double]$ordered[1]
        p99_nearest_rank = [double]$ordered[2]
        stddev = [math]::Sqrt($variance / $samples.Count)
        min = [double]$ordered[0]
        max = [double]$ordered[2]
    }
}

function Get-PublicPairDecision {
    param([Parameter(Mandatory = $true)]$Comparison)
    switch ([string](Get-RequiredProperty -Object $Comparison -Name "baseline_verdict")) {
        "BASELINE_CANDIDATE" {
            return [pscustomobject]@{
                verdict = "BASELINE"; promotable = $false; qualified = $false
                baseline_run_id = "candidate-self"; baseline_fingerprint_mode = "self"
                gaps = @("baseline_absent", "candidate/noncanonical")
            }
        }
        "NOT_COMPARABLE" {
            return [pscustomobject]@{
                verdict = "INCOMPARABLE"; promotable = $false; qualified = $false
                baseline_run_id = "unavailable"; baseline_fingerprint_mode = "unavailable"
                gaps = @("baseline_fingerprint_mismatch", "candidate/noncanonical")
            }
        }
        "GREEN" {
            return [pscustomobject]@{
                verdict = "PASS"; promotable = $true; qualified = $true
                baseline_run_id = "compatible-baseline"; baseline_fingerprint_mode = "compatible"
                gaps = @("candidate/noncanonical_pending_repository_copy")
            }
        }
        "YELLOW" {
            return [pscustomobject]@{
                verdict = "YELLOW"; promotable = $false; qualified = $true
                baseline_run_id = "compatible-baseline"; baseline_fingerprint_mode = "compatible"
                gaps = @("baseline_regression_yellow", "candidate/noncanonical")
            }
        }
        "RED" {
            return [pscustomobject]@{
                verdict = "RED"; promotable = $false; qualified = $true
                baseline_run_id = "compatible-baseline"; baseline_fingerprint_mode = "compatible"
                gaps = @("baseline_regression_red", "candidate/noncanonical")
            }
        }
        default { throw "public_pair_evidence_baseline_verdict_invalid" }
    }
}

function Assert-PublicPairEvidenceCustodyCurrent {
    param([Parameter(Mandatory = $true)][object[]]$PairResults)
    if ($PairResults.Count -ne 2) { throw "public_pair_evidence_custody_required" }
    $freshCells = @()
    foreach ($pairResult in $PairResults) {
        if ($null -eq $pairResult.Evidence -or $null -eq $pairResult.Context -or $null -eq $pairResult.result) {
            throw "public_pair_evidence_custody_required"
        }
        $cachedFingerprint = Get-CellEvidenceCustodyFingerprint -Evidence $pairResult.Evidence
        try {
            $declaredFingerprint = Assert-Sha256Value -Value (Get-RequiredProperty -Object $pairResult.Evidence -Name "custody_fingerprint") `
                -Name "cached_cell_custody"
            if ($declaredFingerprint -cne $cachedFingerprint) { throw "cached_fingerprint_mismatch" }
            $summaryPath = Join-Path ([string](Get-RequiredProperty -Object $pairResult.Evidence -Name "cell_result_directory")) "summary.json"
            $freshSummary = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $summaryPath -Raw)
            $freshEvidence = Assert-CellEvidence -Summary $freshSummary `
                -CellResultDirectory ([string]$pairResult.Evidence.cell_result_directory)
            $freshFingerprint = Get-CellEvidenceCustodyFingerprint -Evidence $freshEvidence
            if ($freshFingerprint -cne $cachedFingerprint -or
                (ConvertTo-CanonicalJson -Value $pairResult.Context) -cne (ConvertTo-CanonicalJson -Value $freshEvidence.context) -or
                [string](Get-RequiredProperty -Object $pairResult.result -Name "context_sha256") -cne $freshEvidence.context_sha256 -or
                [string](Get-RequiredProperty -Object $pairResult.result -Name "raw_measurement_status") -ne "PASS") {
                throw "fresh_custody_mismatch"
            }
            foreach ($property in @($freshSummary.PSObject.Properties)) {
                $cachedValue = Get-RequiredProperty -Object $pairResult.result -Name $property.Name
                if ((ConvertTo-CanonicalJson -Value $cachedValue) -cne (ConvertTo-CanonicalJson -Value $property.Value)) {
                    throw "fresh_summary_mismatch"
                }
            }
        } catch {
            throw "public_pair_evidence_custody_stale"
        }
        $pairResult.Context = $freshEvidence.context
        $pairResult.Evidence = $freshEvidence
        $freshCells += [pscustomobject]@{
            summary = $freshSummary
            context = $freshEvidence.context
            evidence = $freshEvidence
        }
    }
    [pscustomobject]@{ cells = @($freshCells) }
}

function Assert-PublicPairEvidenceEligibility {
    param(
        [Parameter(Mandatory = $true)][object[]]$PairResults,
        [Parameter(Mandatory = $true)]$Comparison
    )
    if ($PairResults.Count -ne 2 -or $null -eq $PairResults[0].Evidence -or $null -eq $PairResults[1].Evidence -or
        $null -eq $PairResults[0].Context -or $null -eq $PairResults[1].Context) {
        throw "public_pair_evidence_custody_required"
    }
    $freshPair = Assert-PublicPairEvidenceCustodyCurrent -PairResults $PairResults
    $freshCells = @($freshPair.cells)
    if ($freshCells.Count -ne 2) { throw "public_pair_evidence_custody_required" }
    $diskCell = $freshCells[0]
    $nbdCell = $freshCells[1]
    if ([string]$diskCell.summary.mode -ne "disk-only" -or [string]$nbdCell.summary.mode -ne "nbd" -or
        [string]$diskCell.context.binary_match -ne "N/A" -or [string]$nbdCell.context.binary_match -ne "PASS" -or
        [string]$nbdCell.summary.binary_match -ne "PASS") {
        throw "public_pair_evidence_nbd_binary_match_required"
    }
    $diskLowerTopology = Get-ModeBoundLowerTopology -Context $diskCell.context -ExpectedMode "disk-only" -FailurePrefix "public_pair_evidence"
    $nbdLowerTopology = Get-ModeBoundLowerTopology -Context $nbdCell.context -ExpectedMode "nbd" -FailurePrefix "public_pair_evidence"
    try {
        $nbdTierMiB = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $nbdCell.context -Name "tier_mib") -Name "public_nbd_tier_mib"
    } catch {
        throw "public_pair_evidence_nbd_identity_invalid"
    }
    $null = Get-NbdIdentity -Context $nbdCell.context -ExpectedTierMiB $nbdTierMiB -LowerTopology $nbdLowerTopology -FailurePrefix "public_pair_evidence"
    if ($diskLowerTopology.sink_type -cne $nbdLowerTopology.sink_type -or
        $diskLowerTopology.sink_identity_sha256 -cne $nbdLowerTopology.sink_identity_sha256) {
        throw "public_pair_evidence_lower_sink_binding_mismatch"
    }
    if ($diskLowerTopology.identity_sha256 -ceq $nbdLowerTopology.identity_sha256) {
        throw "public_pair_evidence_second_tier_identity_not_distinct"
    }
    if ([string]$diskCell.summary.terminal_state -ne "PRODUCT_OFF" -or
        [string]$nbdCell.summary.terminal_state -ne "PRODUCT_OFF" -or
        [string]$diskCell.summary.status -ne "PASS" -or
        [string]$nbdCell.summary.status -ne "PASS" -or
        [string]::IsNullOrWhiteSpace([string]$Comparison.environment_fingerprint)) {
        throw "public_pair_evidence_comparison_required"
    }
    $freshPair
}

function New-PublicPairArtifactReference {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryArtifactRoot,
        [Parameter(Mandatory = $true)][string]$LocalPath
    )
    [ordered]@{
        path = ($RepositoryArtifactRoot.TrimEnd('/') + "/" + [IO.Path]::GetFileName($LocalPath))
        bytes = [int64](Get-Item -LiteralPath $LocalPath -Force).Length
        sha256 = Get-Sha256File -Path $LocalPath
    }
}

function Assert-PublicPairArtifactBinding {
    param(
        [Parameter(Mandatory = $true)]$Record,
        [Parameter(Mandatory = $true)][string]$PairCustodyPath,
        [Parameter(Mandatory = $true)][string]$ComparisonPath
    )
    try {
        $candidate = Get-RequiredProperty -Object $Record -Name "candidate"
        $repositoryArtifactRoot = [string](Get-RequiredProperty -Object $candidate -Name "repository_artifact_root")
        $custodyHash = Get-Sha256File -Path $PairCustodyPath
        $comparisonHash = Get-Sha256File -Path $ComparisonPath
        if ((Assert-Sha256Value -Value (Get-RequiredProperty -Object $candidate -Name "pair_custody_sha256") -Name "candidate_pair_custody") -cne $custodyHash -or
            (Assert-Sha256Value -Value (Get-RequiredProperty -Object $candidate -Name "pair_comparison_sha256") -Name "candidate_pair_comparison") -cne $comparisonHash) {
            throw "candidate_hash_mismatch"
        }
        $custody = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $PairCustodyPath -Raw)
        if ((Assert-Sha256Value -Value (Get-RequiredProperty -Object $custody -Name "comparison_sha256") -Name "custody_comparison") -cne $comparisonHash) {
            throw "custody_comparison_hash_mismatch"
        }
        $artifacts = @((Get-RequiredProperty -Object $Record -Name "artifacts"))
        if ($artifacts.Count -ne 2) { throw "artifact_count" }
        $expected = @(
            [ordered]@{ path = $repositoryArtifactRoot.TrimEnd('/') + "/pair-custody.json"; bytes = [int64](Get-Item -LiteralPath $PairCustodyPath -Force).Length; sha256 = $custodyHash },
            [ordered]@{ path = $repositoryArtifactRoot.TrimEnd('/') + "/pair-comparison.json"; bytes = [int64](Get-Item -LiteralPath $ComparisonPath -Force).Length; sha256 = $comparisonHash }
        )
        for ($index = 0; $index -lt $expected.Count; $index++) {
            $artifact = $artifacts[$index]
            if ([string](Get-RequiredProperty -Object $artifact -Name "path") -cne [string]$expected[$index].path -or
                [int64](Get-RequiredProperty -Object $artifact -Name "bytes") -ne [int64]$expected[$index].bytes -or
                (Assert-Sha256Value -Value (Get-RequiredProperty -Object $artifact -Name "sha256") -Name ("artifact_" + $index)) -cne [string]$expected[$index].sha256) {
                throw "artifact_reference_mismatch"
            }
        }
    } catch {
        throw "public_pair_evidence_artifact_binding_invalid"
    }
}

function Write-PublicPairEvidence {
    param(
        [Parameter(Mandatory = $true)][object[]]$PairResults,
        [Parameter(Mandatory = $true)]$Comparison,
        [Parameter(Mandatory = $true)]$PairContext,
        [Parameter(Mandatory = $true)]$SelectedRelease,
        [Parameter(Mandatory = $true)][string]$PairDir
    )
    $validatedPair = Assert-PublicPairEvidenceEligibility -PairResults $PairResults -Comparison $Comparison
    $validatedCells = @($validatedPair.cells)
    if ($validatedCells.Count -ne 2) { throw "public_pair_evidence_custody_required" }
    $diskCell = $validatedCells[0]
    $nbdCell = $validatedCells[1]
    $diskSummary = $diskCell.summary
    $nbdSummary = $nbdCell.summary
    $diskContext = $diskCell.context
    $nbdContext = $nbdCell.context
    $runId = "wsl2-nbd-" + [string]$PairContext.pair_id + "-" + [guid]::NewGuid().ToString("N")
    $publicDirectory = Join-Path $PairDir "public-evidence"
    if (Test-Path -LiteralPath $publicDirectory -PathType Any) { throw "public_pair_evidence_directory_already_exists" }
    New-Item -ItemType Directory -Path $publicDirectory | Out-Null
    $repositoryArtifactRoot = "docs/specs/no-milestone/wsl2-nbd-product-readiness/evidence/" + $runId
    $diskEvidence = $diskCell.evidence
    $nbdEvidence = $nbdCell.evidence
    $installedManifest = Assert-Sha256Value -Value (Get-RequiredProperty -Object $SelectedRelease -Name "installed_manifest_sha256") -Name "installed_manifest"
    $inputBundleManifest = [string](Get-RequiredProperty -Object $SelectedRelease -Name "input_bundle_manifest_sha256")
    if ($inputBundleManifest -ne "not_exposed") {
        $inputBundleManifest = Assert-Sha256Value -Value $inputBundleManifest -Name "input_bundle_manifest"
    }
    $pairCustodyPath = Join-Path $publicDirectory "pair-custody.json"
    $comparisonPath = Join-Path $publicDirectory "pair-comparison.json"
    $publicComparison = [ordered]@{
        schema_version = "ramshared-nbd-public-pair-comparison/v1"
        pair_id = [string]$PairContext.pair_id
        environment_fingerprint = [string]$Comparison.environment_fingerprint
        baseline_verdict = [string]$Comparison.baseline_verdict
        baseline_reason = [string]$Comparison.baseline_reason
        nbd_vs_disk_median_ratio = [double]$Comparison.nbd_vs_disk_median_ratio
        nbd_vs_disk_p99_ratio = [double]$Comparison.nbd_vs_disk_p99_ratio
        nbd_vs_disk_population_stddev_ratio = $Comparison.nbd_vs_disk_population_stddev_ratio
        timeout_budget = $PairContext.timeout_budget
    }
    Write-JsonNoBom -Value $publicComparison -Path $comparisonPath
    $comparisonSha256 = Get-Sha256File -Path $comparisonPath
    $pairCustody = [ordered]@{
        schema_version = "ramshared-nbd-public-pair-custody/v1"
        pair_id = [string]$PairContext.pair_id
        release = [ordered]@{
            version = [string]$SelectedRelease.version
            source_commit = [string]$SelectedRelease.source_commit
            installed_manifest_sha256 = $installedManifest
            input_bundle_manifest_sha256 = $inputBundleManifest
        }
        cells = @(
            [ordered]@{
                mode = "disk-only"; binary_match = "N/A"; context_sha256 = $diskEvidence.context_sha256
                summary_sha256 = $diskEvidence.summary_sha256; artifact_inventory_sha256 = $diskEvidence.artifact_inventory_sha256
                internal_envelope_sha256 = $diskEvidence.internal_envelope_sha256
                timeout_budget = $diskContext.timeout_budget
            },
            [ordered]@{
                mode = "nbd"; binary_match = "PASS"; context_sha256 = $nbdEvidence.context_sha256
                summary_sha256 = $nbdEvidence.summary_sha256; artifact_inventory_sha256 = $nbdEvidence.artifact_inventory_sha256
                internal_envelope_sha256 = $nbdEvidence.internal_envelope_sha256
                timeout_budget = $nbdContext.timeout_budget
            }
        )
        timeout_budget = $PairContext.timeout_budget
        cuda_hold_sec = [int]$PairContext.cuda_hold_sec
        comparison_sha256 = $comparisonSha256
        cleanup = [ordered]@{ complete = $true; terminal_state = "PRODUCT_OFF" }
    }
    Write-JsonNoBom -Value $pairCustody -Path $pairCustodyPath
    $platform = [ordered]@{
        kernel_release = [string]$nbdContext.kernel_release
        gpu_model = [string]$PairContext.gpu_identity.gpu_model
        gpu_driver = [string]$PairContext.gpu_identity.gpu_driver
        zram = [ordered]@{
            device = [string]$nbdContext.zram.device
            algorithm = [string]$nbdContext.zram.algorithm
            size_kib = [int]$nbdContext.zram.size_kib
            priority = [int]$nbdContext.zram.priority
            identity_sha256 = [string]$nbdContext.zram.identity_sha256
        }
        lower = [ordered]@{
            type = [string]$nbdContext.lower.type
            sink_type = [string]$nbdContext.lower.sink_type
            sink_identity_sha256 = [string]$nbdContext.lower.sink_identity_sha256
        }
    }
    $workload = [ordered]@{
        profile = "anonymous_memory_sequential_write"
        parameters = [ordered]@{
            tier_mib = [int]$nbdContext.tier_mib
            condition = [string]$nbdContext.condition
            pattern = [string]$nbdContext.workload.pattern
            allocation_chunk_bytes = [int]$nbdContext.workload.allocation_chunk_bytes
            worker_threads = [Math]::Min([int]$nbdContext.workload.worker_threads, [Environment]::ProcessorCount)
            allocated_mib = [int]$nbdContext.workload.allocated_mib
            timeout_budget = $PairContext.timeout_budget
        }
        warmup_seconds = 0
        runs = 3
    }
    $harnessRevision = Assert-Sha256Value -Value (Get-RequiredProperty -Object $PairContext.windows_script_sha256 -Name "Invoke-NbdBenchmarkMatrix.ps1") -Name "public_harness_revision"
    $fingerprint = Get-PublicEvidenceFingerprint -HarnessRevision $harnessRevision -Platform $platform -Workload $workload
    $decision = Get-PublicPairDecision -Comparison $Comparison
    $baselineFingerprint = if ($decision.baseline_fingerprint_mode -eq "unavailable") { "unavailable" } else { $fingerprint }
    $artifacts = @(
        (New-PublicPairArtifactReference -RepositoryArtifactRoot $repositoryArtifactRoot -LocalPath $pairCustodyPath),
        (New-PublicPairArtifactReference -RepositoryArtifactRoot $repositoryArtifactRoot -LocalPath $comparisonPath)
    )
    $record = [ordered]@{
        schema_version = "ramshared-evidence/v1"
        run_id = $runId
        surface = "wsl2-nbd"
        slug = "wsl2-nbd-product-readiness"
        utc = [ordered]@{ started = [string]$diskContext.utc.started; ended = [DateTime]::UtcNow.ToString("o") }
        source = [ordered]@{
            commit = [string]$SelectedRelease.source_commit
            dirty = $false
            dirty_entry_count = 0
            invocation = "Invoke-NbdBenchmarkMatrix.ps1 approved pair " + [string]$PairContext.pair_id
            harness_revision = $harnessRevision
        }
        platform = $platform
        candidate = [ordered]@{
            classification = "candidate/noncanonical"
            canonical = $false
            publication_state = "campaign-root-pending-repository-copy"
            repository_artifact_root = $repositoryArtifactRoot
            installed_manifest_sha256 = $installedManifest
            input_bundle_manifest_sha256 = $inputBundleManifest
            pair_custody_sha256 = Get-Sha256File -Path $pairCustodyPath
            pair_comparison_sha256 = $comparisonSha256
        }
        workload = $workload
        comparison = [ordered]@{
            platform_fingerprint = $fingerprint
            baseline_run_id = $decision.baseline_run_id
            baseline_fingerprint = $baselineFingerprint
            qualified = [bool]$decision.qualified
            pair_environment_fingerprint = [string]$Comparison.environment_fingerprint
            baseline_verdict = [string]$Comparison.baseline_verdict
        }
        metrics = [ordered]@{
            disk_allocation_to_hold_ms = New-PublicMetric -Summary $diskSummary -Name "disk_public_metric"
            nbd_allocation_to_hold_ms = New-PublicMetric -Summary $nbdSummary -Name "nbd_public_metric"
        }
        lifecycle = [ordered]@{
            before = [ordered]@{ custody_sha256 = $diskEvidence.context_sha256 }
            action = [ordered]@{ pair_id = [string]$PairContext.pair_id; mode_order = @("disk-only", "nbd") }
            after = [ordered]@{ terminal_state = "PRODUCT_OFF"; custody_sha256 = $nbdEvidence.context_sha256 }
            binary_match = $true
            legitimate = [ordered]@{ verdict = "PASS" }
            refusals = @([ordered]@{ name = "approved_live_fixture_seams_forbidden"; verdict = "PASS" })
            cleanup = [ordered]@{ complete = $true }
            residue = 0
        }
        artifacts = $artifacts
        decision = [ordered]@{
            verdict = [string]$decision.verdict
            promotable = [bool]$decision.promotable
            gaps = @($decision.gaps)
            rollback_trigger = "Any NBD identity, comparison, custody, cleanup, or repository-copy validation mismatch."
        }
    }
    $publicEnvelopePath = Join-Path $publicDirectory "public-pair-evidence.json"
    Assert-PublicPairArtifactBinding -Record $record -PairCustodyPath $pairCustodyPath -ComparisonPath $comparisonPath
    Write-JsonNoBom -Value $record -Path $publicEnvelopePath
    Assert-PublicPairArtifactBinding -Record $record -PairCustodyPath $pairCustodyPath -ComparisonPath $comparisonPath
    [pscustomobject]@{
        # `public_pair_evidence_noncanonical` is a deliberate custody marker:
        # this host-local record is never a repository publication by itself.
        public_pair_evidence_noncanonical = $true
        public_envelope_path = $publicEnvelopePath
        public_artifact_directory = $publicDirectory
        repository_artifact_root = $repositoryArtifactRoot
        record = [pscustomobject]$record
    }
}

function Get-ContextArgumentValue {
    param(
        [Parameter(Mandatory = $true)]$Arguments,
        [Parameter(Mandatory = $true)][string]$Name
    )
    $argumentArray = @($Arguments)
    $positions = @()
    for ($index = 0; $index -lt $argumentArray.Count; $index++) {
        if ([string]$argumentArray[$index] -ceq $Name) { $positions += $index }
    }
    if ($positions.Count -ne 1 -or $positions[0] -ge ($argumentArray.Count - 1)) {
        throw "comparison_context_contract_mismatch"
    }
    [string]$argumentArray[$positions[0] + 1]
}

function Get-ModeBoundLowerTopology {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$ExpectedMode,
        [Parameter(Mandatory = $true)][string]$FailurePrefix
    )
    $expectedType = switch ($ExpectedMode) {
        "disk-only" { "scratch" }
        "nbd" { "nbd" }
        default { throw ($FailurePrefix + "_lower_topology_mismatch") }
    }
    try {
        $lower = Get-RequiredProperty -Object $Context -Name "lower"
        $actualType = [string](Get-RequiredProperty -Object $lower -Name "type")
        $identitySha256 = Assert-Sha256Value -Value (Get-RequiredProperty -Object $lower -Name "identity_sha256") -Name "lower_identity"
    } catch {
        throw ($FailurePrefix + "_lower_topology_mismatch")
    }
    if ($actualType -cne $expectedType) {
        throw ($FailurePrefix + "_lower_topology_mismatch")
    }
    try {
        $sinkType = [string](Get-RequiredProperty -Object $lower -Name "sink_type")
        $sinkIdentitySha256 = Assert-Sha256Value -Value (Get-RequiredProperty -Object $lower -Name "sink_identity_sha256") -Name "lower_sink_identity"
    } catch {
        throw ($FailurePrefix + "_lower_sink_binding_mismatch")
    }
    if ($sinkType -ne "directory") {
        throw ($FailurePrefix + "_lower_sink_binding_mismatch")
    }
    [pscustomobject]@{
        mode = $ExpectedMode
        type = $actualType
        identity_sha256 = $identitySha256
        sink_type = $sinkType
        sink_identity_sha256 = $sinkIdentitySha256
    }
}

function ConvertTo-StrictInt64 {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Name
    )
    $canonicalIntegral = $Value -is [sbyte] -or $Value -is [byte] -or
        $Value -is [int16] -or $Value -is [int32] -or $Value -is [int64]
    if (-not $canonicalIntegral) {
        throw ("integer_invalid:" + $Name)
    }
    $number = [int64]$Value
    if ($number -lt 0) {
        throw ("integer_invalid:" + $Name)
    }
    $number
}

function Get-NbdIdentity {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][int64]$ExpectedTierMiB,
        [Parameter(Mandatory = $true)]$LowerTopology,
        [Parameter(Mandatory = $true)][string]$FailurePrefix
    )
    try {
        $nbd = Get-RequiredProperty -Object $Context -Name "nbd"
        $device = [string](Get-RequiredProperty -Object $nbd -Name "device")
        $blockMajorMinor = [string](Get-RequiredProperty -Object $nbd -Name "block_major_minor")
        $sizeKib = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $nbd -Name "size_kib") -Name "nbd_size_kib"
        $capacitySectors = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $nbd -Name "capacity_sectors") -Name "nbd_capacity_sectors"
        $usableSizeKib = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $nbd -Name "usable_size_kib") -Name "nbd_usable_size_kib"
        $priority = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $nbd -Name "priority") -Name "nbd_priority"
        $serverPid = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $nbd -Name "server_pid") -Name "nbd_server_pid"
        $daemonRelativePath = [string](Get-RequiredProperty -Object $nbd -Name "daemon_executable_relative_path")
        $daemonManifestSha256 = Assert-Sha256Value -Value (Get-RequiredProperty -Object $nbd -Name "daemon_manifest_sha256") -Name "nbd_daemon_manifest"
        $identitySha256 = Assert-Sha256Value -Value (Get-RequiredProperty -Object $nbd -Name "identity_sha256") -Name "nbd_identity"
    } catch {
        throw ($FailurePrefix + "_nbd_identity_invalid")
    }
    if ($ExpectedTierMiB -lt 1 -or $ExpectedTierMiB -gt ([int64]::MaxValue / 2048) -or
        $device -notmatch '^/dev/nbd[0-9]+$' -or
        $blockMajorMinor -notmatch '^[1-9][0-9]*:[0-9]+$' -or
        $sizeKib -ne ($ExpectedTierMiB * 1024) -or
        $capacitySectors -ne ($ExpectedTierMiB * 2048) -or
        $usableSizeKib -lt ($sizeKib - 8) -or $usableSizeKib -gt $sizeKib -or
        $priority -ne 100 -or $serverPid -lt 1 -or
        $daemonRelativePath -cne "bin/ramsharedd") {
        throw ($FailurePrefix + "_nbd_identity_invalid")
    }
    if ($identitySha256 -cne [string]$LowerTopology.identity_sha256) {
        throw ($FailurePrefix + "_nbd_identity_lower_mismatch")
    }
    if ([string]$LowerTopology.identity_sha256 -ceq [string]$LowerTopology.sink_identity_sha256) {
        throw ($FailurePrefix + "_nbd_identity_sink_alias")
    }
    [pscustomobject]@{
        device = $device
        block_major_minor = $blockMajorMinor
        size_kib = $sizeKib
        capacity_sectors = $capacitySectors
        usable_size_kib = $usableSizeKib
        priority = $priority
        server_pid = $serverPid
        daemon_executable_relative_path = $daemonRelativePath
        daemon_manifest_sha256 = $daemonManifestSha256
        identity_sha256 = $identitySha256
    }
}

function Get-ComparisonContract {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)]$Cell,
        [Parameter(Mandatory = $true)][string]$ExpectedMode,
        [Parameter(Mandatory = $true)]$ExpectedRelease,
        [Parameter(Mandatory = $true)][string]$ExpectedPairId
    )
    $workload = Get-RequiredProperty -Object $Context -Name "workload"
    $release = Get-RequiredProperty -Object $Context -Name "release"
    $zram = Get-RequiredProperty -Object $Context -Name "zram"
    $watchdog = Get-RequiredProperty -Object $Context -Name "watchdog"
    $timeoutBudget = Get-RequiredProperty -Object $Context -Name "timeout_budget"
    $argv = Get-RequiredProperty -Object $Context -Name "argv"
    $utc = Get-RequiredProperty -Object $Context -Name "utc"
    $scriptHashes = Get-RequiredProperty -Object $Context -Name "script_sha256"
    if ($Context.schema -ne 2 -or $Context.mode -ne $ExpectedMode -or $Context.pair_id -ne $ExpectedPairId -or
        [int]$Context.tier_mib -ne [int]$Cell.tier_mib -or $Context.condition -ne $Cell.condition -or
        $workload.name -ne "anonymous_memory_sequential_write" -or $workload.pattern -ne "shake256-v1" -or
        [int]$workload.allocated_mib -ne [int]$Cell.allocated_mib -or
        [int]$workload.memory_high_mib -ne [int]$Cell.memory_high_mib -or
        [int]$workload.memory_max_mib -ne [int]$Cell.memory_max_mib -or
        [int]$workload.allocation_chunk_bytes -ne [int]$Cell.allocation_chunk_bytes -or
        [int]$workload.worker_threads -ne [int]$Cell.worker_threads -or
        @($argv).Count -lt 8 -or [string]$argv[0] -ne "nbd-benchmark-cell.sh" -or
        [string](Get-RequiredProperty -Object $utc -Name "started") -notmatch '^\d{4}-\d{2}-\d{2}T' -or
        [bool](Get-RequiredProperty -Object $watchdog -Name "armed") -ne $true -or
        [string](Get-RequiredProperty -Object $watchdog -Name "outcome") -ne "not_fired") {
        throw "comparison_context_contract_mismatch"
    }
    try {
        $expectedTimeoutBudget = Get-CellTimeoutBudget -TierMiB ([int]$Cell.tier_mib)
        $observedSampleTimeout = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $timeoutBudget -Name "sample_timeout_sec") -Name "sample_timeout_sec"
        $observedFinalizationTimeout = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $timeoutBudget -Name "integrity_finalization_timeout_sec") -Name "integrity_finalization_timeout_sec"
        $observedSamples = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $timeoutBudget -Name "samples") -Name "timeout_samples"
        $observedSetupCleanup = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $timeoutBudget -Name "setup_cleanup_timeout_sec") -Name "setup_cleanup_timeout_sec"
        $observedCellOuter = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $timeoutBudget -Name "cell_outer_timeout_sec") -Name "cell_outer_timeout_sec"
    } catch {
        throw "comparison_timeout_budget_mismatch"
    }
    if ($observedSampleTimeout -ne [int64]$expectedTimeoutBudget.sample_timeout_sec -or
        $observedFinalizationTimeout -ne [int64]$expectedTimeoutBudget.integrity_finalization_timeout_sec -or
        $observedSamples -ne [int64]$expectedTimeoutBudget.samples -or
        $observedSetupCleanup -ne [int64]$expectedTimeoutBudget.setup_cleanup_timeout_sec -or
        $observedCellOuter -ne [int64]$expectedTimeoutBudget.cell_outer_timeout_sec -or
        (Get-ContextArgumentValue -Arguments $argv -Name "--sample-timeout-sec") -cne [string]$expectedTimeoutBudget.sample_timeout_sec) {
        throw "comparison_timeout_budget_mismatch"
    }
    if ([string](Get-RequiredProperty -Object $release -Name "root") -cne [string]$ExpectedRelease.selected -or
        [string](Get-RequiredProperty -Object $release -Name "version") -cne [string]$ExpectedRelease.version -or
        [string](Get-RequiredProperty -Object $release -Name "source_commit") -cne [string]$ExpectedRelease.source_commit -or
        [string](Get-RequiredProperty -Object $release -Name "source_tree_state") -cne "clean" -or
        [string](Get-RequiredProperty -Object $release -Name "manifest_sha256") -cne [string]$ExpectedRelease.manifest_sha256) {
        throw "comparison_release_identity_mismatch"
    }
    $expectedContextArguments = [ordered]@{
        "--sealed-release-root" = [string]$ExpectedRelease.selected
        "--release-version" = [string]$ExpectedRelease.version
        "--expected-source-commit" = [string]$ExpectedRelease.source_commit
        "--expected-manifest-sha256" = [string]$ExpectedRelease.manifest_sha256
        "--pair-id" = $ExpectedPairId
        "--mode" = $ExpectedMode
        "--condition" = [string]$Cell.condition
        "--tier-mib" = [string]$Cell.tier_mib
    }
    foreach ($argumentName in $expectedContextArguments.Keys) {
        if ((Get-ContextArgumentValue -Arguments $argv -Name $argumentName) -cne $expectedContextArguments[$argumentName]) {
            throw "comparison_release_identity_mismatch"
        }
    }
    $expectedBinaryMatch = if ($ExpectedMode -eq "nbd") { "PASS" } else { "N/A" }
    if ([string](Get-RequiredProperty -Object $Context -Name "binary_match") -ne $expectedBinaryMatch) {
        throw "comparison_context_contract_mismatch"
    }
    try {
        $zramTopology = [ordered]@{
            device = [string](Get-RequiredProperty -Object $zram -Name "device")
            size_kib = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $zram -Name "size_kib") -Name "zram_size_kib"
            priority = ConvertTo-StrictInt64 -Value (Get-RequiredProperty -Object $zram -Name "priority") -Name "zram_priority"
            algorithm = [string](Get-RequiredProperty -Object $zram -Name "algorithm")
            identity_sha256 = Assert-Sha256Value -Value (Get-RequiredProperty -Object $zram -Name "identity_sha256") -Name "zram_identity"
        }
    } catch {
        throw "comparison_zram_topology_mismatch"
    }
    if ($zramTopology.size_kib -lt 1048568 -or $zramTopology.size_kib -gt 1048576 -or
        $zramTopology.priority -ne 200 -or
        $zramTopology.algorithm -notmatch '^[A-Za-z0-9_-]+$') {
        throw "comparison_zram_topology_mismatch"
    }
    if ($zramTopology.device -notmatch '^zram[0-9]+$') {
        throw "comparison_context_contract_mismatch"
    }
    $lowerTopology = Get-ModeBoundLowerTopology -Context $Context -ExpectedMode $ExpectedMode -FailurePrefix "comparison"
    if ($ExpectedMode -eq "nbd") {
        $null = Get-NbdIdentity -Context $Context -ExpectedTierMiB ([int64]$Cell.tier_mib) -LowerTopology $lowerTopology -FailurePrefix "comparison"
    } elseif ($null -ne $Context.PSObject.Properties["nbd"]) {
        throw "comparison_nbd_identity_unexpected"
    }
    $lowerSinkBinding = [ordered]@{
        type = $lowerTopology.sink_type
        identity_sha256 = $lowerTopology.sink_identity_sha256
    }
    $requiredScriptHashNames = @(
        "nbd-benchmark-cell.sh",
        "nbd-benchmark-cgroup-launch.sh",
        "nbd-benchmark-lib.sh",
        "cascade_pressure_integrity_worker.py",
        "nbd-product-preflight.sh",
        "cascade-up.sh",
        "cascade-down.sh"
    )
    $normalizedScriptHashes = [ordered]@{}
    foreach ($name in $requiredScriptHashNames) {
        $normalizedScriptHashes[$name] = Assert-Sha256Value -Value (Get-RequiredProperty -Object $scriptHashes -Name $name) -Name ("script_" + $name)
    }
    [ordered]@{
        kernel_release = [string](Get-RequiredProperty -Object $Context -Name "kernel_release")
        pair_id = $ExpectedPairId
        source_commit = [string]$ExpectedRelease.source_commit
        release_manifest_sha256 = [string]$ExpectedRelease.manifest_sha256
        zram_topology = $zramTopology
        lower_sink_binding = $lowerSinkBinding
        script_sha256 = $normalizedScriptHashes
        memory_max_mib = [int]$workload.memory_max_mib
        memory_high_mib = [int]$workload.memory_high_mib
        allocation_chunk_bytes = [int]$workload.allocation_chunk_bytes
        worker_threads = [Math]::Min([int]$workload.worker_threads, [Environment]::ProcessorCount)
        allocated_mib = [int]$workload.allocated_mib
        pattern = [string]$workload.pattern
        workload = [string]$workload.name
        tier_mib = [int]$Cell.tier_mib
        condition = [string]$Cell.condition
        timeout_budget = $expectedTimeoutBudget
        command_contract = "nbd-benchmark-cell.sh:run:v1"
    }
}

function Get-BaselineVerdict {
    param(
        [Parameter(Mandatory = $true)]$Comparison,
        [Parameter(Mandatory = $true)][double]$CandidateNbdStddev
    )
    if ([string]::IsNullOrWhiteSpace($BaselineFile)) {
        return [pscustomobject]@{ verdict = "BASELINE_CANDIDATE"; reason = "baseline_absent" }
    }
    try {
        $baseline = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $BaselineFile -Raw)
        if ($baseline.schema -ne 1 -or $baseline.workload_schema -ne "ramshared-nbd-pair/v1" -or
            $baseline.environment_fingerprint -cne $Comparison.environment_fingerprint) {
            return [pscustomobject]@{ verdict = "NOT_COMPARABLE"; reason = "baseline_fingerprint_mismatch" }
        }
        $median = ConvertTo-FiniteNumber -Value (Get-RequiredProperty $baseline "nbd_vs_disk_median_ratio") -Name "baseline_median_ratio"
        $p99 = ConvertTo-FiniteNumber -Value (Get-RequiredProperty $baseline "nbd_vs_disk_p99_ratio") -Name "baseline_p99_ratio"
        $stddev = ConvertTo-FiniteNumber -Value (Get-RequiredProperty $baseline "population_stddev_allocation_to_hold_ms") -Name "baseline_population_stddev"
    } catch {
        return [pscustomobject]@{ verdict = "NOT_COMPARABLE"; reason = "baseline_schema_invalid" }
    }
    $medianRegression = [math]::Round((($Comparison.nbd_vs_disk_median_ratio / $median) - 1.0) * 100.0, 6)
    $p99Regression = [math]::Round((($Comparison.nbd_vs_disk_p99_ratio / $p99) - 1.0) * 100.0, 6)
    $stddevMultiple = [math]::Round($CandidateNbdStddev / $stddev, 6)
    $verdict = if ($medianRegression -gt 15.0 -or $p99Regression -gt 25.0) {
        "RED"
    } elseif ($medianRegression -gt 10.0 -or $p99Regression -gt 15.0 -or $stddevMultiple -gt 2.0) {
        "YELLOW"
    } else {
        "GREEN"
    }
    [pscustomobject]@{
        verdict = $verdict
        reason = "baseline_compatible"
        baseline_sha256 = Get-Sha256File -Path $BaselineFile
        median_regression_percent = $medianRegression
        p99_regression_percent = $p99Regression
        population_stddev_multiple = $stddevMultiple
    }
}

function New-PairComparison {
    param(
        [Parameter(Mandatory = $true)]$DiskSummary,
        [Parameter(Mandatory = $true)]$NbdSummary,
        [Parameter(Mandatory = $true)]$DiskContext,
        [Parameter(Mandatory = $true)]$NbdContext,
        [Parameter(Mandatory = $true)]$DiskCell,
        [Parameter(Mandatory = $true)]$NbdCell,
        [Parameter(Mandatory = $true)]$PairContext,
        [Parameter(Mandatory = $true)]$SelectedRelease
    )
    $diskContract = Get-ComparisonContract -Context $DiskContext -Cell $DiskCell -ExpectedMode "disk-only" -ExpectedRelease $SelectedRelease -ExpectedPairId $PairContext.pair_id
    $nbdContract = Get-ComparisonContract -Context $NbdContext -Cell $NbdCell -ExpectedMode "nbd" -ExpectedRelease $SelectedRelease -ExpectedPairId $PairContext.pair_id
    if ($diskContract.source_commit -cne $nbdContract.source_commit -or
        $diskContract.release_manifest_sha256 -cne $nbdContract.release_manifest_sha256) {
        throw "comparison_release_identity_mismatch"
    }
    if (($diskContract.script_sha256 | ConvertTo-Json -Compress -Depth 8) -cne ($nbdContract.script_sha256 | ConvertTo-Json -Compress -Depth 8)) {
        throw "comparison_script_hash_mismatch"
    }
    if (($diskContract.zram_topology | ConvertTo-Json -Compress -Depth 8) -cne ($nbdContract.zram_topology | ConvertTo-Json -Compress -Depth 8)) {
        throw "comparison_zram_topology_mismatch"
    }
    if (($diskContract.lower_sink_binding | ConvertTo-Json -Compress -Depth 8) -cne ($nbdContract.lower_sink_binding | ConvertTo-Json -Compress -Depth 8)) {
        throw "comparison_lower_sink_binding_mismatch"
    }
    $diskLowerTopology = Get-ModeBoundLowerTopology -Context $DiskContext -ExpectedMode "disk-only" -FailurePrefix "comparison"
    $nbdLowerTopology = Get-ModeBoundLowerTopology -Context $NbdContext -ExpectedMode "nbd" -FailurePrefix "comparison"
    if ($diskLowerTopology.identity_sha256 -ceq $nbdLowerTopology.identity_sha256) {
        throw "comparison_second_tier_identity_not_distinct"
    }
    $windowsScriptHashes = Get-RequiredProperty -Object $PairContext -Name "windows_script_sha256"
    $matrixScriptHash = Assert-Sha256Value -Value (Get-RequiredProperty -Object $windowsScriptHashes -Name "Invoke-NbdBenchmarkMatrix.ps1") -Name "windows_matrix_script"
    $cudaScriptHash = [string](Get-RequiredProperty -Object $windowsScriptHashes -Name "Start-CudaVramWorkload.ps1")
    if (($diskContract.condition -eq "bounded" -and $cudaScriptHash -notmatch '^[0-9a-f]{64}$') -or
        ($diskContract.condition -eq "idle" -and $cudaScriptHash -ne "N/A")) {
        throw "comparison_windows_script_hash_invalid"
    }
    $sharedContract = [ordered]@{
        kernel_release = $diskContract.kernel_release
        pair_id = $diskContract.pair_id
        source_commit = $diskContract.source_commit
        release_manifest_sha256 = $diskContract.release_manifest_sha256
        zram_topology = $diskContract.zram_topology
        lower_sink_binding = $diskContract.lower_sink_binding
        script_sha256 = $diskContract.script_sha256
        memory_max_mib = $diskContract.memory_max_mib
        memory_high_mib = $diskContract.memory_high_mib
        allocation_chunk_bytes = $diskContract.allocation_chunk_bytes
        worker_threads = [Math]::Min([int]$diskContract.worker_threads, [Environment]::ProcessorCount)
        allocated_mib = $diskContract.allocated_mib
        pattern = $diskContract.pattern
        workload = $diskContract.workload
        tier_mib = $diskContract.tier_mib
        condition = $diskContract.condition
        timeout_budget = $diskContract.timeout_budget
        command_contract = $diskContract.command_contract
    }
    if (($sharedContract | ConvertTo-Json -Compress -Depth 12) -cne ($nbdContract | ConvertTo-Json -Compress -Depth 12) -or
        $null -eq $PairContext.gpu_identity) {
        throw "comparison_pair_contract_mismatch"
    }
    if ((ConvertTo-CanonicalJson -Value $diskContract.timeout_budget) -cne
        (ConvertTo-CanonicalJson -Value $nbdContract.timeout_budget)) {
        throw "comparison_timeout_budget_mismatch"
    }
    $fingerprintMaterial = [ordered]@{
        workload_schema = "ramshared-nbd-pair/v1"
        environment = [ordered]@{
            kernel_release = $diskContract.kernel_release
            gpu_model = [string]$PairContext.gpu_identity.gpu_model
            gpu_driver = [string]$PairContext.gpu_identity.gpu_driver
            source_commit = $diskContract.source_commit
            release_manifest_sha256 = $diskContract.release_manifest_sha256
            zram_topology = $diskContract.zram_topology
            lower_sink_binding = $diskContract.lower_sink_binding
            script_sha256 = $diskContract.script_sha256
            windows_script_sha256 = [ordered]@{
                "Invoke-NbdBenchmarkMatrix.ps1" = $matrixScriptHash
                "Start-CudaVramWorkload.ps1" = $cudaScriptHash
            }
            memory_max_mib = $diskContract.memory_max_mib
            memory_high_mib = $diskContract.memory_high_mib
            tier_mib = $diskContract.tier_mib
            condition = $diskContract.condition
            timeout_budget = $diskContract.timeout_budget
            command_contract = $diskContract.command_contract
        }
        workload = [ordered]@{
            pattern = $diskContract.pattern
            allocation_chunk_bytes = $diskContract.allocation_chunk_bytes
            worker_threads = [Math]::Min([int]$diskContract.worker_threads, [Environment]::ProcessorCount)
            allocated_mib = $diskContract.allocated_mib
            name = $diskContract.workload
        }
    }
    $fingerprint = Get-Sha256Text -Text ($fingerprintMaterial | ConvertTo-Json -Depth 12 -Compress)
    $diskMedian = ConvertTo-FiniteNumber -Value (Get-RequiredProperty $DiskSummary "median_allocation_to_hold_ms") -Name "disk_median"
    $diskP99 = ConvertTo-FiniteNumber -Value (Get-RequiredProperty $DiskSummary "p99_allocation_to_hold_ms") -Name "disk_p99"
    $diskStddev = ConvertTo-FiniteNumber -Value (Get-RequiredProperty $DiskSummary "population_stddev_allocation_to_hold_ms") -Name "disk_population_stddev" -AllowZero
    $nbdMedian = ConvertTo-FiniteNumber -Value (Get-RequiredProperty $NbdSummary "median_allocation_to_hold_ms") -Name "nbd_median"
    $nbdP99 = ConvertTo-FiniteNumber -Value (Get-RequiredProperty $NbdSummary "p99_allocation_to_hold_ms") -Name "nbd_p99"
    $nbdStddev = ConvertTo-FiniteNumber -Value (Get-RequiredProperty $NbdSummary "population_stddev_allocation_to_hold_ms") -Name "nbd_population_stddev" -AllowZero
    $comparison = [ordered]@{
        schema = 1
        workload_schema = "ramshared-nbd-pair/v1"
        environment_fingerprint = $fingerprint
        environment_fingerprint_material = $fingerprintMaterial
        nbd_vs_disk_median_ratio = [math]::Round($nbdMedian / $diskMedian, 6)
        nbd_vs_disk_p99_ratio = [math]::Round($nbdP99 / $diskP99, 6)
        nbd_vs_disk_population_stddev_ratio = if ($diskStddev -gt 0) { [math]::Round($nbdStddev / $diskStddev, 6) } else { $null }
    }
    $baseline = Get-BaselineVerdict -Comparison ([pscustomobject]$comparison) -CandidateNbdStddev $nbdStddev
    $comparison.baseline_verdict = $baseline.verdict
    $comparison.baseline_reason = $baseline.reason
    foreach ($property in @("baseline_sha256", "median_regression_percent", "p99_regression_percent", "population_stddev_multiple")) {
        if ($null -ne $baseline.PSObject.Properties[$property]) { $comparison[$property] = $baseline.$property }
    }
    [pscustomobject]$comparison
}

function Test-CellGpuHeadroom {
    param(
        [Parameter(Mandatory = $true)]$Cell,
        [Parameter(Mandatory = $true)][string]$Phase
    )
    $gpu = Get-GpuMemory
    $required = [int]$Cell.cell_required_free_vram_mib
    [pscustomobject]@{
        phase = $Phase
        total_mib = $gpu.total_mib
        free_vram_mib = $gpu.free_vram_mib
        utilization_percent = $gpu.utilization_percent
        temperature_celsius = $gpu.temperature_celsius
        required_free_vram_mib = $required
        accepted = ($gpu.free_vram_mib -ge $required)
    }
}

function Start-CudaWorkload {
    param(
        [Parameter(Mandatory = $true)][string]$PairDir,
        [Parameter(Mandatory = $true)][ValidateRange(1, 7920)][int]$CudaHoldSec,
        [string]$TestCudaSource = "",
        [switch]$InjectFailureAfterChildStart
    )
    if ((-not [string]::IsNullOrWhiteSpace($TestCudaSource) -or $InjectFailureAfterChildStart) -and
        [string]::IsNullOrWhiteSpace($ManufacturedSelfTestCase)) {
        throw "cuda_start_test_seam_forbidden"
    }
    $cudaSource = if ([string]::IsNullOrWhiteSpace($TestCudaSource)) {
        Join-Path $PSScriptRoot "..\p0\Start-CudaVramWorkload.ps1"
    } else {
        $TestCudaSource
    }
    if (-not (Test-Path -LiteralPath $cudaSource -PathType Leaf)) { throw "cuda_workload_missing" }
    $ready = Join-Path $PairDir "cuda-ready.txt"
    $release = Join-Path $PairDir "external-release.txt"
    $stdout = Join-Path $PairDir "cuda.out"
    $stderr = Join-Path $PairDir "cuda.err"
    if ((Test-Path -LiteralPath $ready -PathType Any) -or (Test-Path -LiteralPath $release -PathType Any)) {
        throw "cuda_handshake_path_already_exists"
    }
    $handle = $null
    $transferredToCaller = $false
    try {
        $handle = New-RedirectedProcess -FilePath (Get-CurrentPowerShellExecutable) -ArgumentValues @(
            "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", $cudaSource,
            "-MiB", [string]$external_workload_mib, "-HoldSec", [string]$CudaHoldSec,
            "-ReadyFile", $ready, "-ReleaseFile", $release, "-LiveCampaign"
        )
        $deadline = [DateTime]::UtcNow.AddSeconds(30)
        $readyContents = ""
        while (-not $handle.process.HasExited -and [DateTime]::UtcNow -lt $deadline) {
            if (Test-Path -LiteralPath $ready -PathType Leaf) {
                try { $readyContents = Get-Content -LiteralPath $ready -Raw -ErrorAction Stop } catch { $readyContents = "" }
                if ($readyContents -match '^cuda_allocation_ready\r?\n$') { break }
            }
            Start-Sleep -Milliseconds 200
            $handle.process.Refresh()
        }
        if ($handle.process.HasExited -or $readyContents -notmatch '^cuda_allocation_ready\r?\n$') {
            throw "bounded_cuda_workload_failed_to_start"
        }
        if ($InjectFailureAfterChildStart) { throw "injected_cuda_post_start_failure" }
        $transferredToCaller = $true
        return [pscustomobject]@{
            handle = $handle
            ready_file = $ready
            release_file = $release
            stdout_path = $stdout
            stderr_path = $stderr
            startup_containment = [ordered]@{
                call = "cuda_vram_workload"
                readiness_timeout_sec = 30
                ready_observed = $true
                process_exited_before_ready = $false
            }
        }
    } finally {
        if (-not $transferredToCaller -and $null -ne $handle) {
            # The caller cannot own this handle until a full startup receipt is returned.
            # Cleanup here prevents an exceptional post-start path from leaking CUDA pressure.
            $null = Complete-CudaWorkload -Handle $handle -ReleaseFile $release -StdoutPath $stdout -StderrPath $stderr
        }
    }
}

function Invoke-NbdBenchmarkCell {
    param(
        [Parameter(Mandatory = $true)]$Cell,
        [Parameter(Mandatory = $true)][string]$PairDir,
        [Parameter(Mandatory = $true)]$SelectedRelease,
        [Parameter(Mandatory = $true)]$PairContext
    )
    $timeoutBudget = Get-CellTimeoutBudget -TierMiB ([int]$Cell.tier_mib)
    if ([int]$Cell.sample_timeout_sec -ne [int]$timeoutBudget.sample_timeout_sec -or
        [int]$Cell.integrity_finalization_timeout_sec -ne [int]$timeoutBudget.integrity_finalization_timeout_sec -or
        [int]$Cell.setup_cleanup_timeout_sec -ne [int]$timeoutBudget.setup_cleanup_timeout_sec -or
        [int]$Cell.cell_outer_timeout_sec -ne [int]$timeoutBudget.cell_outer_timeout_sec) {
        throw "cell_timeout_budget_mismatch"
    }
    $headroom = Test-CellGpuHeadroom -Cell $Cell -Phase "immediately_before_cell"
    if (-not $headroom.accepted) {
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "REFUSED" -Reason "gpu_headroom_shortfall" -TerminalState "PRODUCT_OFF" -Extra @{
            gpu_headroom = $headroom; pair_context = $PairContext
        })
    }
    $cellDir = Join-Path $PairDir ([string]$Cell.mode)
    New-Item -ItemType Directory -Path $cellDir | Out-Null
    $cellWsl = Convert-ToWslPath -Path $cellDir
    $stdout = Join-Path $cellDir "wsl.out"
    $stderr = Join-Path $cellDir "wsl.err"
    $approval = "benchmark:{0}:{1}:{2}:{3}" -f $SelectedRelease.version, $Cell.tier_mib, $Cell.condition, $Cell.mode
    $arguments = @(
        "-d", $Distro, "-u", "root", "--", "env", "-i",
        "PATH=/usr/sbin:/usr/bin:/sbin:/bin", "HOME=/root",
        "RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_BOUNDED_SHARED_HOST_PRESSURE",
        "RAMSHARED_WINDOWS_WATCHDOG_ARMED=1",
        "RAMSHARED_NBD_BENCHMARK_APPROVAL=$approval",
        ($SelectedRelease.selected + "/scripts/safety/nbd-benchmark-cell.sh"), "--run",
        "--sealed-release-root", [string]$SelectedRelease.selected,
        "--release-version", [string]$SelectedRelease.version,
        "--expected-source-commit", [string]$SelectedRelease.source_commit,
        "--expected-manifest-sha256", [string]$SelectedRelease.manifest_sha256,
        "--pair-id", [string]$PairContext.pair_id,
        "--mode", [string]$Cell.mode, "--condition", [string]$Cell.condition,
        "--tier-mib", [string]$Cell.tier_mib, "--artifact-dir", "$cellWsl/result",
        "--sample-timeout-sec", [string]$timeoutBudget.sample_timeout_sec
    )
    $run = Invoke-BoundedProcess -FilePath (Get-WslExecutable) -ArgumentValues $arguments -TimeoutSec $timeoutBudget.cell_outer_timeout_sec
    Write-ProcessLogs -Run $run -StdoutPath $stdout -StderrPath $stderr
    $containment = [ordered]@{
        call = "benchmark_cell"
        timeout_sec = $timeoutBudget.cell_outer_timeout_sec
        timeout_budget = $timeoutBudget
        completed = $run.completed
        timed_out = $run.timed_out
        exit_code = $run.exit_code
        stream_drain_complete = $run.stream_drain_complete
        forced_termination = $run.forced_termination
    }
    if ($run.timed_out) {
        $containment.watchdog = [ordered]@{
            outcome = "watchdog_timeout_red"
            bounded_child_stop_attempted = $run.forced_termination
            vm_lifecycle_invoked = $false
        }
        return New-CellControllerFailureExecution -Run $run -Cell $Cell -CellDirectory $cellDir `
            -SelectedRelease $SelectedRelease -PairContext $PairContext -Containment $containment
    }
    if (-not $run.completed -or $run.exit_code -ne 0) {
        return New-CellControllerFailureExecution -Run $run -Cell $Cell -CellDirectory $cellDir `
            -SelectedRelease $SelectedRelease -PairContext $PairContext -Containment $containment
    }
    $summaryPath = Join-Path $cellDir "result\summary.json"
    if (-not (Test-Path -LiteralPath $summaryPath -PathType Leaf)) {
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "RED" -Reason "cell_summary_missing" -TerminalState "unverified_unknown" -Extra @{
            containment = $containment; pair_context = $PairContext
        })
    }
    try { $summary = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $summaryPath -Raw) } catch {
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "RED" -Reason "cell_summary_invalid" -TerminalState "unverified_unknown" -Extra @{
            containment = $containment; pair_context = $PairContext
        })
    }
    if ($summary.status -ne "PASS" -or $summary.terminal_state -ne "PRODUCT_OFF") {
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "RED" -Reason "cell_summary_invalid" -TerminalState "unverified_unknown" -Extra @{
            containment = $containment; pair_context = $PairContext
        })
    }
    $cellResultDirectory = Join-Path $cellDir "result"
    try { $evidence = Assert-CellEvidence -Summary $summary -CellResultDirectory $cellResultDirectory } catch {
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "RED" -Reason "cell_evidence_invalid" -TerminalState "unverified_unknown" -Extra @{
            containment = $containment; pair_context = $PairContext; error = $_.Exception.Message
        })
    }
    $summary | Add-Member -NotePropertyName "windows_gpu_headroom" -NotePropertyValue $headroom -Force
    try {
        $null = Assert-CellTimeoutBudgetMatch -Budget (Get-RequiredProperty -Object $summary -Name "timeout_budget") `
            -TierMiB ([int]$Cell.tier_mib) -FailureReason "cell_timeout_budget_mismatch"
    } catch {
        return New-CellExecution -Result (New-CellResult -Cell $Cell -Status "RED" -Reason "cell_timeout_budget_mismatch" -TerminalState "unverified_unknown" -Extra @{
            containment = $containment; pair_context = $PairContext
        })
    }
    $summary | Add-Member -NotePropertyName "pair_context" -NotePropertyValue $PairContext -Force
    $summary | Add-Member -NotePropertyName "containment" -NotePropertyValue $containment -Force
    New-CellExecution -Result $summary -Context $evidence.context -Evidence $evidence
}

function Test-PromotionMayAdvance {
    param([Parameter(Mandatory = $true)][object[]]$PairResults)
    if (@($PairResults).Count -ne 2) { return $false }
    foreach ($pairResult in @($PairResults)) {
        if ($null -eq $pairResult -or $null -eq $pairResult.result -or [string]$pairResult.result.status -ne "PASS") {
            return $false
        }
        # Cell summaries are immutable custody inputs. A separate pair decision
        # may stop promotion without rewriting an otherwise valid summary.
        if ($null -ne $pairResult.result.PSObject.Properties["pair_decision"]) { return $false }
    }
    $true
}

function Apply-BaselineVerdictToPair {
    param(
        [Parameter(Mandatory = $true)][object[]]$PairResults,
        [Parameter(Mandatory = $true)]$Comparison
    )
    if (@($PairResults).Count -ne 2) { throw "baseline_pair_result_count_invalid" }
    $pairDecision = switch ([string]$Comparison.baseline_verdict) {
        "RED" {
            [ordered]@{
                baseline_verdict = "RED"; verdict = "RED"; reason = "baseline_regression_red"; promotable = $false
            }
            break
        }
        "YELLOW" {
            [ordered]@{
                baseline_verdict = "YELLOW"; verdict = "YELLOW"; reason = "baseline_regression_yellow"; promotable = $false
            }
            break
        }
        default { $null }
    }
    if ($null -eq $pairDecision) { return }
    $PairResults[1].result | Add-Member -NotePropertyName "pair_decision" -NotePropertyValue ([pscustomobject]$pairDecision) -Force
    $PairResults[1].result | Add-Member -NotePropertyName "promotion" -NotePropertyValue "promotion_stopped" -Force
}

function Get-PairDecisionVerdict {
    param([Parameter(Mandatory = $true)]$PairResult)
    if ($null -eq $PairResult -or $null -eq $PairResult.result) { return $null }
    $property = $PairResult.result.PSObject.Properties["pair_decision"]
    if ($null -eq $property -or $null -eq $property.Value) { return $null }
    [string]$property.Value.verdict
}

function Test-PairDecisionIsRed {
    param([Parameter(Mandatory = $true)][object[]]$PairResults)
    @($PairResults | Where-Object { (Get-PairDecisionVerdict -PairResult $_) -eq "RED" }).Count -gt 0
}

function Test-PairHasBaselineDecision {
    param([Parameter(Mandatory = $true)][object[]]$PairResults)
    @($PairResults | Where-Object { $null -ne $_.result.PSObject.Properties["pair_decision"] }).Count -gt 0
}

function Invoke-CellPair {
    param(
        [Parameter(Mandatory = $true)][object[]]$PairCells,
        [Parameter(Mandatory = $true)][string]$CampaignRoot,
        [Parameter(Mandatory = $true)]$SelectedRelease
    )
    if ($PairCells.Count -ne 2 -or $PairCells[0].mode -ne "disk-only" -or $PairCells[1].mode -ne "nbd" -or
        $PairCells[0].tier_mib -ne $PairCells[1].tier_mib -or $PairCells[0].condition -ne $PairCells[1].condition) {
        throw "pair_contract_invalid"
    }
    $first = $PairCells[0]
    $pairId = "{0}-{1}" -f $first.tier_mib, $first.condition
    $pairDir = Join-Path $CampaignRoot $pairId
    New-Item -ItemType Directory -Path $pairDir | Out-Null
    $pairResults = @()
    $cuda = $null
    $comparison = $null
    $publicEvidence = $null
    $pairContext = [ordered]@{
        pair_id = $pairId
        timeout_budget = Get-PairTimeoutBudget -TierMiB ([int]$first.tier_mib)
        cuda_context = "none"
        cuda_hold_sec = 0
        windows_script_sha256 = Get-WindowsPairScriptHashes -Condition ([string]$first.condition)
        gpu_identity = $null
        gpu_before_pair = $null
        gpu_after_cuda_ready = $null
        cuda_containment = [ordered]@{
            startup = $null
            cuda_completion = $null
        }
    }
    try {
        $pairContext.gpu_identity = Get-GpuIdentity
        $beforePair = Get-GpuMemory
        $pairContext.gpu_before_pair = $beforePair
        $pairRequired = [int]$first.bounded_pair_required_free_vram_mib
        if ($beforePair.free_vram_mib -lt $pairRequired) {
            $pairResults += New-CellExecution -Result (New-CellResult -Cell $first -Status "REFUSED" -Reason "gpu_headroom_shortfall" -TerminalState "PRODUCT_OFF" -Extra @{
                pair_context = $pairContext; required_free_vram_mib = $pairRequired
            })
            return $pairResults
        }
        if ($first.condition -eq "bounded") {
            if ($CudaMaxHoldSec -lt [int]$pairContext.timeout_budget.cuda_hold_min_sec) {
                throw "cuda_pair_hold_too_short required_sec=$($pairContext.timeout_budget.cuda_hold_min_sec) configured_sec=$CudaMaxHoldSec"
            }
            $pairCudaHoldSec = [int]$pairContext.timeout_budget.cuda_hold_min_sec
            $cuda = Start-CudaWorkload -PairDir $pairDir -CudaHoldSec $pairCudaHoldSec
            $pairContext.cuda_context = "one_context_for_disk_then_nbd"
            $pairContext.cuda_hold_sec = $pairCudaHoldSec
            $pairContext.cuda_containment.startup = $cuda.startup_containment
            $afterReady = Get-GpuMemory
            $pairContext.gpu_after_cuda_ready = $afterReady
            if ($afterReady.free_vram_mib -lt [int]$first.cell_required_free_vram_mib) {
                $pairResults += New-CellExecution -Result (New-CellResult -Cell $first -Status "REFUSED" -Reason "gpu_headroom_shortfall_after_cuda_ready" -TerminalState "PRODUCT_OFF" -Extra @{
                    pair_context = $pairContext; gpu_after_cuda_ready = $afterReady
                })
                return $pairResults
            }
        }
        foreach ($cell in $PairCells) {
            $execution = Invoke-NbdBenchmarkCell -Cell $cell -PairDir $pairDir -SelectedRelease $SelectedRelease -PairContext $pairContext
            $pairResults += $execution
            if ($execution.result.status -ne "PASS") { return $pairResults }
            $execution.result | Add-Member -NotePropertyName "raw_measurement_status" -NotePropertyValue "PASS" -Force
        }
        $comparison = New-PairComparison -DiskSummary $pairResults[0].result -NbdSummary $pairResults[1].result `
            -DiskContext $pairResults[0].context -NbdContext $pairResults[1].context -DiskCell $PairCells[0] `
            -NbdCell $PairCells[1] -PairContext ([pscustomobject]$pairContext) -SelectedRelease $SelectedRelease
        $comparisonPath = Join-Path $pairDir "comparison.json"
        Write-JsonNoBom -Value $comparison -Path $comparisonPath
        foreach ($execution in $pairResults) {
            $execution.result | Add-Member -NotePropertyName "comparison" -NotePropertyValue $comparison -Force
            $execution.result | Add-Member -NotePropertyName "comparison_sha256" -NotePropertyValue (Get-Sha256File -Path $comparisonPath) -Force
        }
        Apply-BaselineVerdictToPair -PairResults $pairResults -Comparison $comparison
        $pairResults
    } catch {
        $pairResults += New-CellExecution -Result (New-CellResult -Cell $first -Status "RED" -Reason "pair_controller_failed" -TerminalState "unverified_unknown" -Extra @{
            pair_context = $pairContext; error = $_.Exception.Message
        })
        $pairResults
    } finally {
        if ($null -ne $cuda) {
            $completion = Complete-CudaWorkload -Handle $cuda.handle -ReleaseFile $cuda.release_file `
                -StdoutPath $cuda.stdout_path -StderrPath $cuda.stderr_path
            $pairContext.cuda_containment.cuda_completion = New-SanitizedCudaCompletion -Completion $completion
            if (-not $completion.released -and $pairResults.Count -gt 0) {
                Apply-CudaCompletionToPairResult -PairResults $pairResults -Completion $completion
            }
        }
        if ($null -ne $comparison -and $pairResults.Count -eq 2 -and
            $null -ne $pairResults[0].Evidence -and $null -ne $pairResults[1].Evidence -and
            [string]$pairResults[0].result.terminal_state -eq "PRODUCT_OFF" -and
            [string]$pairResults[1].result.terminal_state -eq "PRODUCT_OFF" -and
            ($null -eq $cuda -or ($null -ne $pairContext.cuda_containment.cuda_completion -and
                $pairContext.cuda_containment.cuda_completion.released))) {
            try {
                $publicEvidence = Write-PublicPairEvidence -PairResults $pairResults -Comparison $comparison `
                    -PairContext ([pscustomobject]$pairContext) -SelectedRelease $SelectedRelease -PairDir $pairDir
                foreach ($execution in $pairResults) {
                    $execution.result | Add-Member -NotePropertyName "public_pair_evidence" -NotePropertyValue ([ordered]@{
                        state = "candidate/noncanonical"
                        local_path = $publicEvidence.public_envelope_path
                        repository_artifact_root = $publicEvidence.repository_artifact_root
                        sha256 = Get-Sha256File -Path $publicEvidence.public_envelope_path
                    }) -Force
                }
            } catch {
                if ($pairResults.Count -gt 0) {
                    $pairResults[$pairResults.Count - 1].result.status = "RED"
                    $pairResults[$pairResults.Count - 1].result.reason = "public_pair_evidence_invalid"
                    $pairResults[$pairResults.Count - 1].result.promotion = "promotion_stopped"
                    $pairResults[$pairResults.Count - 1].result | Add-Member -NotePropertyName "public_pair_evidence_error" -NotePropertyValue $_.Exception.Message -Force
                }
            }
        }
    }
}

function Write-MatrixArtifactInventory {
    param([Parameter(Mandatory = $true)][string]$CampaignRoot)
    $rows = @()
    Get-ChildItem -LiteralPath $CampaignRoot -File -Recurse | Sort-Object FullName | ForEach-Object {
        if ($_.Name -eq "matrix-artifact-inventory.json") { return }
        $relative = $_.FullName.Substring($CampaignRoot.Length).TrimStart([char]'\', [char]'/') -replace '\\', '/'
        $rows += [ordered]@{
            path = $relative
            bytes = $_.Length
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
        }
    }
    Write-JsonNoBom -Value ([ordered]@{ schema = 1; files = $rows }) -Path (Join-Path $CampaignRoot "matrix-artifact-inventory.json")
}

function Invoke-ManufacturedSelfTest {
    param([Parameter(Mandatory = $true)][string]$Case)
    $cell = [pscustomobject]@{ tier_mib = 1024; condition = "bounded"; mode = "disk-only" }
    switch ($Case) {
        "timeout" {
            $run = Invoke-BoundedProcess -FilePath (Get-CurrentPowerShellExecutable) -ArgumentValues @(
                "-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 15") -TimeoutSec 1
            if (-not $run.timed_out -or -not $run.forced_termination) { throw "manufactured_timeout_not_contained" }
            $execution = New-CellControllerFailureExecution -Run $run -Cell $cell -CellDirectory $ArtifactRoot `
                -SelectedRelease ([pscustomobject]@{ version = "manufactured-v1" }) `
                -PairContext ([pscustomobject]@{ pair_id = "1024-bounded" }) `
                -Containment ([ordered]@{ call = "manufactured_timeout"; bounded_child_stop_attempted = $run.forced_termination; vm_lifecycle_invoked = $false })
            if ($execution.result.reason -ne "watchdog_timeout_red" -or
                $execution.result.terminal_state -ne "unverified_unknown") {
                throw "manufactured_timeout_classification_invalid"
            }
            Write-Output "terminal_state=$($execution.result.terminal_state)"
            Write-Output "timeout_vm_lifecycle=NOT_INVOKED"
            return
        }
        "watchdog-cuda-composition" {
            $cell = [pscustomobject]@{ tier_mib = 1024; condition = "bounded"; mode = "nbd" }
            $pairResults = @(
                [pscustomobject]@{
                    result = (New-CellResult -Cell $cell -Status "RED" -Reason "watchdog_timeout_red" `
                        -TerminalState "unverified_unknown")
                }
            )
            $completion = [pscustomobject]@{
                released = $false
                forced_termination = $true
                exit_code = 137
                stream_drain_complete = $false
                error = "private path C:\\campaign\\cuda.err"
            }
            Apply-CudaCompletionToPairResult -PairResults $pairResults -Completion $completion
            $result = $pairResults[0].result
            if ($result.reason -cne "watchdog_timeout_red" -or
                $result.terminal_state -cne "unverified_unknown" -or
                $result.promotion -ne "promotion_stopped") {
                throw "manufactured_watchdog_cuda_primary_reason_overwritten"
            }
            if ($null -eq $result.PSObject.Properties["cuda_cleanup_secondary"] -or
                $result.cuda_cleanup_secondary.released -ne $false -or
                $result.cuda_cleanup_secondary.error_present -ne $true -or
                $result.cuda_cleanup_secondary.PSObject.Properties["error"] -ne $null) {
                throw "manufactured_watchdog_cuda_secondary_receipt_invalid"
            }
            Write-Output "watchdog_cuda_composition=PASS"
            return
        }
        "watchdog-cuda-serialization" {
            $cell = [pscustomobject]@{ tier_mib = 1024; condition = "bounded"; mode = "nbd" }
            $completion = [pscustomobject]@{
                released = $false
                forced_termination = $true
                exit_code = 137
                stream_drain_complete = $false
                error = 'C:\secret\cuda.err'
            }
            $pairContext = [pscustomobject]@{
                pair_id = "1024-bounded"
                cuda_containment = [pscustomobject]@{ cuda_completion = $completion }
            }
            $pairResults = @(
                [pscustomobject]@{
                    result = (New-CellResult -Cell $cell -Status "RED" -Reason "watchdog_timeout_red" `
                        -TerminalState "unverified_unknown" -Extra @{ pair_context = $pairContext })
                }
            )
            $pairContext.cuda_containment.cuda_completion = New-SanitizedCudaCompletion -Completion $completion
            Apply-CudaCompletionToPairResult -PairResults $pairResults -Completion $completion
            $serialized = $pairResults[0].result | ConvertTo-Json -Depth 20
            if ($serialized.Contains('C:\secret\cuda.err')) {
                throw "manufactured_watchdog_cuda_private_error_leaked"
            }
            $decoded = ConvertFrom-JsonPreservingDateStrings -Json $serialized
            foreach ($occurrence in @(
                $decoded.cuda_cleanup_secondary,
                $decoded.pair_context.cuda_containment.cuda_completion
            )) {
                if ($null -eq $occurrence -or $occurrence.PSObject.Properties["error"] -ne $null -or
                    $occurrence.PSObject.Properties["stderr"] -ne $null -or
                    $occurrence.PSObject.Properties["stdout"] -ne $null) {
                    throw "manufactured_watchdog_cuda_serialization_unsanitized"
                }
            }
            Write-Output "watchdog_cuda_serialization_sanitized=PASS"
            return
        }
        "promotion" {
            $failed = @(
                (New-CellExecution -Result ([pscustomobject]@{ status = "RED"; mode = "disk-only" })),
                (New-CellExecution -Result ([pscustomobject]@{ status = "PASS"; mode = "nbd" }))
            )
            $canAdvance = Test-PromotionMayAdvance -PairResults $failed
            Write-Output "next_pair_started=$canAdvance"
            if ($canAdvance) { throw "manufactured_promotion_advanced_after_failure" }
            return
        }
        "selector-flip" {
            $reviewed = [pscustomobject]@{
                selected = "/opt/ramshared/releases/reviewed-v1"
                version = "reviewed-v1"
                source_commit = ("a" * 40) -join ""
                manifest_sha256 = ("b" * 64) -join ""
                input_bundle_manifest_sha256 = ("c" * 64) -join ""
            }
            # Model the selector changing after the reviewed identity is captured.
            $replacementAfterReview = "/opt/ramshared/releases/replacement-v2"
            $preflightArguments = @(New-PinnedNbdProductPreflightArguments -SelectedRelease $reviewed)
            $deactivationArguments = @(New-PinnedNbdDeactivationArguments -SelectedRelease $reviewed)
            $allArguments = @($preflightArguments + $deactivationArguments)
            if ($allArguments -contains $replacementAfterReview -or
                ($allArguments -join "`n") -match '/opt/ramshared/current' -or
                ($allArguments -join "`n") -match 'cascade-down\.sh') {
                throw "manufactured_selector_flip_redirected_action"
            }
            if ($deactivationArguments -notcontains "/opt/ramshared/releases/reviewed-v1/bin/ramshared" -or
                $deactivationArguments -notcontains "RAMSHARED_NBD_LIFECYCLE_APPROVAL=deactivate:reviewed-v1" -or
                $deactivationArguments[-1] -ne "down" -or
                $preflightArguments -notcontains "/opt/ramshared/releases/reviewed-v1/scripts/safety/nbd-product-preflight.sh" -or
                $preflightArguments -notcontains "--sealed-release-root" -or
                $preflightArguments -notcontains "/opt/ramshared/releases/reviewed-v1") {
                throw "manufactured_selector_flip_pinned_contract_invalid"
            }
            Write-Output "selector_flip_deactivation=PINNED"
            Write-Output "selector_flip_preflight=PINNED"
            return
        }
        "matrix-inventory" {
            $root = Join-Path $ArtifactRoot "matrix-inventory-manufactured"
            $nested = Join-Path $root "nested"
            New-Item -ItemType Directory -Force -Path $nested | Out-Null
            [IO.File]::WriteAllText((Join-Path $nested "file.txt"), "fixture`n", [Text.UTF8Encoding]::new($false))
            Write-MatrixArtifactInventory -CampaignRoot $root
            $record = ConvertFrom-JsonPreservingDateStrings -Json `
                (Get-Content -LiteralPath (Join-Path $root "matrix-artifact-inventory.json") -Raw)
            if (@($record.files).Count -ne 1 -or [string]$record.files[0].path -cne "nested/file.txt") {
                throw "manufactured_matrix_inventory_invalid"
            }
            Write-Output "matrix_inventory=PASS"
            return
        }
        "windows-command-line" {
            $root = Join-Path $ArtifactRoot "windows command line manufactured"
            New-Item -ItemType Directory -Force -Path $root | Out-Null
            $childPath = Join-Path $root "argument child.ps1"
            $outputPath = Join-Path $root "argument output.txt"
            $childSource = @'
param(
    [Parameter(Mandatory = $true)][string]$Value,
    [Parameter(Mandatory = $true)][string]$OutputPath
)
[IO.File]::WriteAllText($OutputPath, $Value, [Text.UTF8Encoding]::new($false))
'@
            [IO.File]::WriteAllText($childPath, $childSource, [Text.UTF8Encoding]::new($false))
            $expected = 'selected="$(readlink -f /opt/ramshared/current)" && printf "%s\n" "$selected"'
            $run = Invoke-BoundedProcess -FilePath (Get-CurrentPowerShellExecutable) -ArgumentValues @(
                "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", $childPath,
                "-Value", $expected, "-OutputPath", $outputPath
            ) -TimeoutSec 15
            if (-not $run.completed -or $run.exit_code -ne 0 -or -not (Test-Path -LiteralPath $outputPath -PathType Leaf)) {
                throw "manufactured_windows_command_line_child_failed"
            }
            $actual = [IO.File]::ReadAllText($outputPath, [Text.Encoding]::UTF8)
            if ($actual -cne $expected) { throw "manufactured_windows_command_line_corrupted" }
            Write-Output "windows_command_line=PASS"
            return
        }
        "selected-release-direct-argv" {
            $arguments = @(New-DirectWslRootArguments -LinuxArguments @(
                "readlink", "-f", "/opt/ramshared/current"
            ))
            $expected = @(
                "-d", $Distro, "-u", "root", "--", "env", "-i",
                "PATH=/usr/sbin:/usr/bin:/sbin:/bin", "HOME=/root",
                "readlink", "-f", "/opt/ramshared/current"
            )
            if ($arguments.Count -ne $expected.Count) { throw "manufactured_selected_release_argv_count_invalid" }
            for ($index = 0; $index -lt $expected.Count; $index++) {
                if ([string]$arguments[$index] -cne [string]$expected[$index]) {
                    throw "manufactured_selected_release_argv_invalid"
                }
            }
            if ($arguments -contains "bash" -or $arguments -contains "-c" -or
                (@($arguments | Where-Object { $_ -match '["$]' }).Count -ne 0)) {
                throw "manufactured_selected_release_shell_program_present"
            }
            $commit = ("a" * 40) -join ""
            $installed = ("b" * 64) -join ""
            $inputBundle = ("c" * 64) -join ""
            $provenance = [ordered]@{
                schema_version = "ramshared-installed-release-provenance/v1"
                source_commit = $commit
                source_tree_state = "clean"
                input_bundle_manifest_sha256 = $inputBundle
            } | ConvertTo-Json -Compress
            $valid = @{
                Selected = "/opt/ramshared/releases/reviewed-v1"
                SourceCommit = $commit
                SourceTreeState = "clean"
                InstalledManifestLine = ($installed + "  /opt/ramshared/releases/reviewed-v1/SHA256SUMS")
                StoredInstalledManifest = $installed
                InputManifestLine = ($inputBundle + "  /opt/ramshared/releases/reviewed-v1/INPUT_BUNDLE_SHA256SUMS")
                ProvenanceJson = $provenance
                ExpectedCommit = $commit
            }
            $identity = Assert-SelectedReleaseRecords @valid
            if ($identity.version -cne "reviewed-v1" -or $identity.manifest_sha256 -cne $installed -or
                $identity.input_bundle_manifest_sha256 -cne $inputBundle) {
                throw "manufactured_selected_release_valid_records_failed"
            }
            foreach ($mutation in @(
                @{ Name = "selector_case"; Key = "Selected"; Value = "/OPT/RAMSHARED/RELEASES/reviewed-v1" },
                @{ Name = "source"; Key = "SourceCommit"; Value = (("d" * 40) -join "") },
                @{ Name = "tree_case"; Key = "SourceTreeState"; Value = "Clean" },
                @{ Name = "installed"; Key = "StoredInstalledManifest"; Value = (("d" * 64) -join "") },
                @{ Name = "input"; Key = "InputManifestLine"; Value = (("d" * 64) -join "") + "  input" },
                @{ Name = "provenance"; Key = "ProvenanceJson"; Value = '{"schema_version":"ramshared-installed-release-provenance/v1","source_commit":"0000000000000000000000000000000000000000","source_tree_state":"clean","input_bundle_manifest_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}' }
            )) {
                $mutationArguments = @{} + $valid
                $mutationArguments[$mutation.Key] = $mutation.Value
                $refused = $false
                try { $null = Assert-SelectedReleaseRecords @mutationArguments } catch { $refused = $true }
                if (-not $refused) { throw ("manufactured_selected_release_mutation_accepted:" + $mutation.Name) }
            }
            Write-Output "selected_release_direct_argv=PASS"
            return
        }
        "nbd-identity" {
            $sourceCommit = ("a" * 40) -join ""
            $manifestSha256 = ("b" * 64) -join ""
            $lowerIdentity = ("c" * 64) -join ""
            $sinkIdentity = ("d" * 64) -join ""
            $newContext = {
                param([hashtable]$Overrides)
                $nbd = [ordered]@{
                    device = "/dev/nbd0"; block_major_minor = "43:0"; size_kib = 1048576
                    capacity_sectors = 2097152; usable_size_kib = 1048572; priority = 100
                    server_pid = 4242; daemon_executable_relative_path = "bin/ramsharedd"
                    daemon_manifest_sha256 = ("e" * 64) -join ""; identity_sha256 = $lowerIdentity
                }
                foreach ($key in $Overrides.Keys) {
                    if ($null -eq $Overrides[$key]) { $null = $nbd.Remove($key) } else { $nbd[$key] = $Overrides[$key] }
                }
                [pscustomobject]@{
                    schema = 2; mode = "nbd"; tier_mib = 1024
                    lower = [pscustomobject]@{
                        type = "nbd"; identity_sha256 = $lowerIdentity
                        sink_type = "directory"; sink_identity_sha256 = $sinkIdentity
                    }
                    nbd = [pscustomobject]$nbd
                }
            }
            $assertIdentity = {
                param([hashtable]$Overrides, [string]$ExpectedReason)
                $context = & $newContext $Overrides
                $lower = Get-ModeBoundLowerTopology -Context $context -ExpectedMode "nbd" -FailurePrefix "manufactured"
                $refused = $false
                try { Get-NbdIdentity -Context $context -ExpectedTierMiB 1024 -LowerTopology $lower -FailurePrefix "manufactured" | Out-Null } catch {
                    $refused = $_.Exception.Message -eq $ExpectedReason
                }
                if (-not $refused) { throw ("manufactured_nbd_identity_was_accepted:" + $ExpectedReason) }
            }
            $validContext = & $newContext @{}
            $validLower = Get-ModeBoundLowerTopology -Context $validContext -ExpectedMode "nbd" -FailurePrefix "manufactured"
            $valid = Get-NbdIdentity -Context $validContext -ExpectedTierMiB 1024 -LowerTopology $validLower -FailurePrefix "manufactured"
            if ($valid.device -ne "/dev/nbd0" -or $valid.size_kib -ne 1048576 -or $valid.identity_sha256 -ne $lowerIdentity) {
                throw "manufactured_nbd_identity_positive_invalid"
            }
            foreach ($field in @(
                "device", "block_major_minor", "size_kib", "capacity_sectors", "usable_size_kib", "priority", "server_pid",
                "daemon_executable_relative_path", "daemon_manifest_sha256", "identity_sha256"
            )) {
                & $assertIdentity @{ $field = $null } "manufactured_nbd_identity_invalid"
            }
            & $assertIdentity @{ device = "/dev/nvme0n1" } "manufactured_nbd_identity_invalid"
            & $assertIdentity @{ block_major_minor = "43-0" } "manufactured_nbd_identity_invalid"
            & $assertIdentity @{ size_kib = 1048575 } "manufactured_nbd_identity_invalid"
            & $assertIdentity @{ capacity_sectors = 2097151 } "manufactured_nbd_identity_invalid"
            & $assertIdentity @{ usable_size_kib = 1048567 } "manufactured_nbd_identity_invalid"
            & $assertIdentity @{ priority = 99 } "manufactured_nbd_identity_invalid"
            & $assertIdentity @{ server_pid = 0 } "manufactured_nbd_identity_invalid"
            & $assertIdentity @{ daemon_executable_relative_path = "bin/foreign" } "manufactured_nbd_identity_invalid"
            & $assertIdentity @{ daemon_manifest_sha256 = "not-a-sha" } "manufactured_nbd_identity_invalid"
            & $assertIdentity @{ identity_sha256 = $sinkIdentity } "manufactured_nbd_identity_lower_mismatch"
            $aliasContext = & $newContext @{}
            $aliasContext.lower.sink_identity_sha256 = $lowerIdentity
            $aliasLower = Get-ModeBoundLowerTopology -Context $aliasContext -ExpectedMode "nbd" -FailurePrefix "manufactured"
            $aliasRefused = $false
            try { Get-NbdIdentity -Context $aliasContext -ExpectedTierMiB 1024 -LowerTopology $aliasLower -FailurePrefix "manufactured" | Out-Null } catch {
                $aliasRefused = $_.Exception.Message -eq "manufactured_nbd_identity_sink_alias"
            }
            if (-not $aliasRefused) { throw "manufactured_nbd_identity_sink_alias_was_accepted" }
            Write-Output "nbd_identity_contract=PASS"
            Write-Output "nbd_identity_invalid_fields=REFUSED"
            Write-Output "nbd_identity_lower_and_sink_aliases=REFUSED"
            return
        }
        "cuda-cleanup" {
            $dir = Join-Path ([IO.Path]::GetTempPath()) ("ramshared-nbd-cuda-cleanup-" + [guid]::NewGuid().ToString("N"))
            New-Item -ItemType Directory -Path $dir | Out-Null
            try {
                $handle = New-RedirectedProcess -FilePath (Get-CurrentPowerShellExecutable) -ArgumentValues @(
                    "-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 15")
                $completion = Complete-CudaWorkload -Handle $handle -ReleaseFile (Join-Path $dir "release") `
                    -StdoutPath (Join-Path $dir "stdout") -StderrPath (Join-Path $dir "stderr") -WaitTimeoutMs 100
                if (-not $completion.forced_termination) { throw "manufactured_cuda_process_survived" }
                Write-Output "cuda_process_terminated=True"
            } finally {
                Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
            }
            return
        }
        "cuda-post-start-cleanup" {
            $dir = Join-Path ([IO.Path]::GetTempPath()) ("ramshared-nbd-cuda-post-start-" + [guid]::NewGuid().ToString("N"))
            New-Item -ItemType Directory -Path $dir | Out-Null
            try {
                $fakeCuda = Join-Path $dir "manufactured-cuda-child.ps1"
                @'
#Requires -Version 5.1
[CmdletBinding()]
param(
    [int]$MiB,
    [int]$HoldSec,
    [string]$ReadyFile,
    [string]$ReleaseFile,
    [switch]$LiveCampaign
)

$pairDir = Split-Path -Parent $ReadyFile
[IO.File]::WriteAllText((Join-Path $pairDir "manufactured-cuda-child.pid"), [string]$PID, [Text.UTF8Encoding]::new($false))
$readyBytes = [Text.Encoding]::UTF8.GetBytes("cuda_allocation_ready`n")
$readyStream = New-Object IO.FileStream($ReadyFile, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
try {
    $readyStream.Write($readyBytes, 0, $readyBytes.Length)
    $readyStream.Flush()
} finally {
    $readyStream.Dispose()
}
$deadline = [DateTime]::UtcNow.AddSeconds($HoldSec)
while (-not (Test-Path -LiteralPath $ReleaseFile -PathType Leaf)) {
    if ([DateTime]::UtcNow -ge $deadline) { throw "manufactured_cuda_release_timeout" }
    Start-Sleep -Milliseconds 50
}
Write-Output "[cuda-vram-workload] released"
'@ | Set-Content -LiteralPath $fakeCuda -Encoding Ascii
                $injectedFailureObserved = $false
                try {
                    Start-CudaWorkload -PairDir $dir -CudaHoldSec 120 -TestCudaSource $fakeCuda -InjectFailureAfterChildStart | Out-Null
                } catch {
                    $injectedFailureObserved = $_.Exception.Message -eq "injected_cuda_post_start_failure"
                }
                if (-not $injectedFailureObserved) { throw "manufactured_cuda_post_start_failure_not_observed" }
                $pidPath = Join-Path $dir "manufactured-cuda-child.pid"
                if (-not (Test-Path -LiteralPath $pidPath -PathType Leaf)) { throw "manufactured_cuda_child_pid_missing" }
                try { $childPid = [int](Get-Content -LiteralPath $pidPath -Raw -ErrorAction Stop) } catch {
                    throw "manufactured_cuda_child_pid_invalid"
                }
                $childExited = $false
                try { Get-Process -Id $childPid -ErrorAction Stop | Out-Null } catch { $childExited = $true }
                if (-not $childExited) { throw "manufactured_cuda_child_leaked_after_post_start_failure" }
                if (-not (Test-Path -LiteralPath (Join-Path $dir "external-release.txt") -PathType Leaf) -or
                    (Get-Content -LiteralPath (Join-Path $dir "cuda.out") -Raw -ErrorAction Stop) -notmatch '\[cuda-vram-workload\] released') {
                    throw "manufactured_cuda_post_start_cleanup_receipt_missing"
                }
                Write-Output "cuda_post_start_cleanup=PASS"
            } finally {
                Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
            }
            return
        }
        "cuda-native-cleanup" {
            $cudaSource = Join-Path $PSScriptRoot "..\p0\Start-CudaVramWorkload.ps1"
            $output = @(& (Get-CurrentPowerShellExecutable) -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $cudaSource -CleanupSelfTest 2>&1)
            if (($output -join "`n") -notmatch "cuda_cleanup_self_test_pass") {
                throw "manufactured_cuda_native_cleanup_failure_not_observed"
            }
            Write-Output "cuda_native_cleanup_failure=PASS"
            return
        }
        "source-identity" {
            $savedExpected = $script:ExpectedSourceCommit
            try {
                $script:ExpectedSourceCommit = "not-a-commit"
                $refused = $false
                try { Assert-LiveConfiguration } catch { $refused = $_.Exception.Message -eq "expected_source_commit_invalid" }
                if (-not $refused) { throw "manufactured_source_identity_was_accepted" }
                Write-Output "source_identity=REFUSED"
            } finally {
                $script:ExpectedSourceCommit = $savedExpected
            }
            return
        }
        "cuda-deadline" {
            $savedHold = $script:CudaMaxHoldSec
            $savedExpected = $script:ExpectedSourceCommit
            $savedProbe = $script:NvidiaSmiPath
            try {
                $script:CudaMaxHoldSec = 360
                $script:ExpectedSourceCommit = ("a" * 40)
                $script:NvidiaSmiPath = "nvidia-smi.exe"
                $refused = $false
                try { Assert-LiveConfiguration } catch { $refused = $_.Exception.Message -like "cuda_pair_hold_too_short*" }
                if (-not $refused) { throw "manufactured_cuda_deadline_was_accepted" }
                Write-Output "cuda_deadline=REFUSED"
            } finally {
                $script:CudaMaxHoldSec = $savedHold
                $script:ExpectedSourceCommit = $savedExpected
                $script:NvidiaSmiPath = $savedProbe
            }
            return
        }
        "timeout-budget" {
            $observed = @()
            foreach ($tier in @(1024, 2048, 4096)) {
                $cellBudget = Get-CellTimeoutBudget -TierMiB $tier
                $expectedSample = switch ($tier) { 1024 { 240 }; 2048 { 240 }; 4096 { 600 } }
                $expectedFinalization = switch ($tier) { 1024 { 120 }; 2048 { 240 }; 4096 { 600 } }
                $expectedOuter = switch ($tier) { 1024 { 1380 }; 2048 { 1740 }; 4096 { 3900 } }
                if ($cellBudget.sample_timeout_sec -ne $expectedSample -or $cellBudget.samples -ne 3 -or
                    $cellBudget.integrity_finalization_timeout_sec -ne $expectedFinalization -or
                    $cellBudget.setup_cleanup_timeout_sec -ne 300 -or $cellBudget.cell_outer_timeout_sec -ne $expectedOuter) {
                    throw "manufactured_timeout_budget_invalid"
                }
                $observed += $cellBudget
            }
            $q4PairBudget = Get-PairTimeoutBudget -TierMiB 4096
            if ($q4PairBudget.cuda_hold_min_sec -ne 7920) { throw "manufactured_q4_cuda_budget_invalid" }
            $refused = $false
            try { Get-CellTimeoutBudget -TierMiB 8192 | Out-Null } catch { $refused = $_.Exception.Message -eq "cell_timeout_tier_invalid" }
            if (-not $refused) { throw "manufactured_timeout_budget_unsupported_tier_accepted" }
            Write-Output "timeout_budget=PASS"
            Write-Output "timeout_budget_refusal=REFUSED"
            return
        }
        "timeout-budget-property-order" {
            $canonicalOrder = [ordered]@{
                sample_timeout_sec = 240
                integrity_finalization_timeout_sec = 120
                samples = 3
                setup_cleanup_timeout_sec = 300
                cell_outer_timeout_sec = 1380
            }
            $permutedOrder = [ordered]@{
                cell_outer_timeout_sec = 1380
                setup_cleanup_timeout_sec = 300
                samples = 3
                integrity_finalization_timeout_sec = 120
                sample_timeout_sec = 240
            }
            $rawJsonDiffers = (($canonicalOrder | ConvertTo-Json -Compress -Depth 8) -cne
                ($permutedOrder | ConvertTo-Json -Compress -Depth 8))
            if (-not $rawJsonDiffers) {
                throw "manufactured_timeout_budget_property_order_fixture_not_distinct"
            }
            $semantic = Assert-CellTimeoutBudgetMatch -Budget ([pscustomobject]$permutedOrder) -TierMiB 1024 `
                -FailureReason "manufactured_timeout_budget_property_order_mismatch"
            if ($semantic.sample_timeout_sec -ne 240 -or $semantic.samples -ne 3 -or
                $semantic.integrity_finalization_timeout_sec -ne 120 -or
                $semantic.setup_cleanup_timeout_sec -ne 300 -or $semantic.cell_outer_timeout_sec -ne 1380) {
                throw "manufactured_timeout_budget_property_order_semantic_values_invalid"
            }
            $mismatchRefused = $false
            $mismatch = [ordered]@{
                sample_timeout_sec = 241
                integrity_finalization_timeout_sec = 120
                samples = 3
                setup_cleanup_timeout_sec = 300
                cell_outer_timeout_sec = 1380
            }
            try {
                Assert-CellTimeoutBudgetMatch -Budget ([pscustomobject]$mismatch) -TierMiB 1024 `
                    -FailureReason "manufactured_timeout_budget_mismatch" | Out-Null
            } catch {
                $mismatchRefused = $_.Exception.Message -eq "manufactured_timeout_budget_mismatch"
            }
            if (-not $mismatchRefused) { throw "manufactured_timeout_budget_mismatch_was_accepted" }
            $noncanonicalRefused = $false
            $noncanonical = [ordered]@{
                sample_timeout_sec = 240.0
                integrity_finalization_timeout_sec = 120
                samples = 3
                setup_cleanup_timeout_sec = 300
                cell_outer_timeout_sec = 1380
            }
            try {
                Assert-CellTimeoutBudgetMatch -Budget ([pscustomobject]$noncanonical) -TierMiB 1024 `
                    -FailureReason "manufactured_timeout_budget_noncanonical" | Out-Null
            } catch {
                $noncanonicalRefused = $_.Exception.Message -eq "manufactured_timeout_budget_noncanonical"
            }
            if (-not $noncanonicalRefused) { throw "manufactured_timeout_budget_noncanonical_was_accepted" }
            Write-Output "cell_timeout_budget_property_order_is_semantic=PASS"
            Write-Output "cell_timeout_budget_property_order_mismatch=REFUSED"
            Write-Output "cell_timeout_budget_property_order_noncanonical=REFUSED"
            return
        }
        "failure-receipt" {
            $dir = Join-Path $ArtifactRoot "failure-receipt-manufactured"
            $resultDir = Join-Path $dir "result"
            New-Item -ItemType Directory -Force -Path $resultDir | Out-Null
            $receiptPath = Join-Path $resultDir "failure-receipt.json"
            $cell = [pscustomobject]@{ tier_mib = 1024; condition = "bounded"; mode = "disk-only" }
            $pairContext = [pscustomobject]@{ pair_id = "1024-bounded" }
            $selectedRelease = [pscustomobject]@{ version = "manufactured-v1" }
            $valid = [ordered]@{
                schema_version = "ramshared-nbd-cell-failure/v1"
                status = "RED"; reason = "SAMPLE_TIMEOUT"; terminal_state = "PRODUCT_OFF"
                release_version = "manufactured-v1"; pair_id = "1024-bounded"
                mode = "disk-only"; condition = "bounded"; tier_mib = 1024
            }
            Write-JsonNoBom -Value $valid -Path $receiptPath
            $run = [pscustomobject]@{ completed = $true; timed_out = $false; exit_code = 2 }
            $containment = [ordered]@{ call = "manufactured" }
            $execution = New-CellControllerFailureExecution -Run $run -Cell $cell -CellDirectory $dir `
                -SelectedRelease $selectedRelease -PairContext $pairContext -Containment $containment
            if ($execution.result.reason -cne "SAMPLE_TIMEOUT" -or
                $execution.result.terminal_state -cne "PRODUCT_OFF" -or
                $execution.result.failure_receipt.tier_mib -ne 1024) {
                throw "manufactured_failure_receipt_positive_invalid"
            }
            Write-Output "cell_failure_receipt_product_off=PASS"
            $failedStart = New-CellControllerFailureExecution -Run ([pscustomobject]@{
                    completed = $false; timed_out = $false; exit_code = $null
                }) -Cell $cell -CellDirectory $dir -SelectedRelease $selectedRelease `
                -PairContext $pairContext -Containment $containment
            if ($failedStart.result.reason -ne "wsl_controller_failed" -or
                $failedStart.result.terminal_state -ne "unverified_unknown") {
                throw "manufactured_failure_receipt_failed_start_accepted"
            }
            Write-Output "cell_failure_receipt_failed_start=REFUSED"
            foreach ($timedOut in @("false", "true", 0, 1, $null)) {
                Write-JsonNoBom -Value $valid -Path $receiptPath
                $rawRun = [pscustomobject]@{
                    completed = $true; timed_out = $timedOut; exit_code = 2
                }
                $execution = New-CellControllerFailureExecution -Run $rawRun -Cell $cell -CellDirectory $dir `
                    -SelectedRelease $selectedRelease -PairContext $pairContext -Containment $containment
                if ($execution.result.reason -ne "wsl_controller_failed" -or
                    $execution.result.terminal_state -ne "unverified_unknown") {
                    $kind = if ($null -eq $timedOut) { "null" } else { $timedOut.GetType().FullName }
                    throw ("manufactured_failure_receipt_non_boolean_timeout_accepted:" + $kind)
                }
            }
            Write-Output "cell_failure_receipt_non_boolean_timeout=REFUSED"
            foreach ($mutation in @(
                @{ Name = "extra"; Value = @{ path = "C:\\private" } },
                @{ Name = "watchdog"; Value = @{ reason = "WATCHDOG_TIMEOUT_RED" } },
                @{ Name = "wrong-pair"; Value = @{ pair_id = "2048-bounded" } },
                @{ Name = "wrong-tier-type"; Value = @{ tier_mib = "1024" } }
            )) {
                $mutated = [ordered]@{} + $valid
                foreach ($key in $mutation.Value.Keys) { $mutated[$key] = $mutation.Value[$key] }
                if ($mutation.Name -eq "extra") { $mutated["path"] = "C:\\private" }
                Write-JsonNoBom -Value $mutated -Path $receiptPath
                $execution = New-CellControllerFailureExecution -Run $run -Cell $cell -CellDirectory $dir `
                    -SelectedRelease $selectedRelease -PairContext $pairContext -Containment $containment
                if ($execution.result.reason -ne "wsl_controller_failed" -or
                    $execution.result.terminal_state -ne "unverified_unknown") {
                    throw ("manufactured_failure_receipt_mutation_accepted:" + $mutation.Name)
                }
            }
            Remove-Item -LiteralPath $receiptPath -Force
            $missing = New-CellControllerFailureExecution -Run $run -Cell $cell -CellDirectory $dir `
                -SelectedRelease $selectedRelease -PairContext $pairContext -Containment $containment
            if ($missing.result.reason -ne "wsl_controller_failed" -or
                $missing.result.terminal_state -ne "unverified_unknown") {
                throw "manufactured_failure_receipt_missing_accepted"
            }
            $timeout = New-CellControllerFailureExecution -Run ([pscustomobject]@{
                    completed = $false; timed_out = $true; exit_code = $null
                }) -Cell $cell -CellDirectory $dir -SelectedRelease $selectedRelease `
                -PairContext $pairContext -Containment $containment
            if ($timeout.result.reason -ne "watchdog_timeout_red" -or
                $timeout.result.terminal_state -ne "unverified_unknown") {
                throw "manufactured_failure_receipt_timeout_promoted"
            }
            Write-Output "cell_failure_receipt_invalid=REFUSED"
            return
        }
        "partial-timeout-sample" {
            $dir = Join-Path $ArtifactRoot "partial-timeout-sample"
            $resultDir = Join-Path $dir "result"
            New-Item -ItemType Directory -Force -Path $resultDir | Out-Null
            $partialIntegrity = Join-Path $resultDir "run-3-integrity.json"
            Write-JsonNoBom -Value ([ordered]@{
                    status = "PASS"; hold = $true; checksum_match = $true
                    allocated_mib = 6016; required_allocated_mib = 6656
                    cleanup = "timeout_containment"
                }) -Path $partialIntegrity
            Write-JsonNoBom -Value ([ordered]@{
                    status = "PASS"; terminal_state = "PRODUCT_OFF"
                    binary_match = "PASS"; public_schema = "ramshared-evidence/v1"
                }) -Path (Join-Path $resultDir "summary.json")
            Write-JsonNoBom -Value ([ordered]@{
                    schema_version = "ramshared-evidence/v1"; lifecycle = [ordered]@{ binary_match = $true }
                }) -Path (Join-Path $resultDir "public-evidence.json")
            $receiptPath = Join-Path $resultDir "failure-receipt.json"
            Write-JsonNoBom -Value ([ordered]@{
                    schema_version = "ramshared-nbd-cell-failure/v1"; status = "RED"
                    reason = "SAMPLE_TIMEOUT"; terminal_state = "PRODUCT_OFF"
                    release_version = "manufactured-v1"; pair_id = "1024-bounded"
                    mode = "disk-only"; condition = "bounded"; tier_mib = 1024
                }) -Path $receiptPath
            $cell = [pscustomobject]@{ tier_mib = 1024; condition = "bounded"; mode = "disk-only" }
            $pairContext = [pscustomobject]@{ pair_id = "1024-bounded" }
            $selectedRelease = [pscustomobject]@{ version = "manufactured-v1" }
            $execution = New-CellControllerFailureExecution -Run ([pscustomobject]@{
                    completed = $false; timed_out = $true; exit_code = $null
                }) -Cell $cell -CellDirectory $dir -SelectedRelease $selectedRelease `
                -PairContext $pairContext -Containment ([ordered]@{ call = "manufactured" })
            if ($execution.result.status -ne "RED" -or
                $execution.result.reason -ne "watchdog_timeout_red" -or
                $execution.result.terminal_state -ne "unverified_unknown" -or
                $execution.result.promotion -ne "promotion_stopped" -or
                $execution.result.PSObject.Properties["summary"] -ne $null) {
                throw "partial_timeout_integrity_was_promoted"
            }
            if (Test-Path -LiteralPath (Join-Path $resultDir "public-evidence.json") -PathType Leaf) {
                Write-Output "partial_timeout_public_evidence_ignored=REFUSED"
            }
            Write-Output "partial_timeout_integrity_not_promoted=REFUSED"
            return
        }
        "comparison" {
            $temp = Join-Path ([IO.Path]::GetTempPath()) ("ramshared-nbd-comparison-" + [guid]::NewGuid().ToString("N"))
            New-Item -ItemType Directory -Path $temp | Out-Null
            $savedBaseline = $script:BaselineFile
            try {
                $cellDisk = [pscustomobject]@{ tier_mib = 1024; condition = "idle"; allocated_mib = 3584; memory_high_mib = 1200; memory_max_mib = 4096; allocation_chunk_bytes = 67108864; worker_threads = [Math]::Min(1, [Environment]::ProcessorCount) }
                $cellNbd = [pscustomobject]@{ tier_mib = 1024; condition = "idle"; allocated_mib = 3584; memory_high_mib = 1200; memory_max_mib = 4096; allocation_chunk_bytes = 67108864; worker_threads = [Math]::Min(1, [Environment]::ProcessorCount) }
                $sourceCommit = ("a" * 40) -join ""
                $manifestSha256 = ("b" * 64) -join ""
                $scriptHash = ("c" * 64) -join ""
                $selectedRelease = [pscustomobject]@{
                    selected = "/opt/ramshared/releases/manufactured-v1"
                    version = "manufactured-v1"; source_commit = $sourceCommit; manifest_sha256 = $manifestSha256
                }
                $scriptHashes = [ordered]@{
                    "nbd-benchmark-cell.sh" = $scriptHash
                    "nbd-benchmark-cgroup-launch.sh" = $scriptHash
                    "nbd-benchmark-lib.sh" = $scriptHash
                    "cascade_pressure_integrity_worker.py" = $scriptHash
                    "nbd-product-preflight.sh" = $scriptHash
                    "cascade-up.sh" = $scriptHash
                    "cascade-down.sh" = $scriptHash
                }
                $newContext = {
                    param([string]$Mode, [string]$BinaryMatch)
                    $lowerIdentity = if ($Mode -eq "nbd") { ("e" * 64) -join "" } else { ("f" * 64) -join "" }
                    $record = [ordered]@{
                        schema = 2; pair_id = "1024-idle"; condition = "idle"; tier_mib = 1024; mode = $Mode
                        kernel_release = "manufactured-kernel"; binary_match = $BinaryMatch
                        release = [pscustomobject]@{ root = $selectedRelease.selected; version = "manufactured-v1"; source_commit = $sourceCommit; source_tree_state = "clean"; manifest_sha256 = $manifestSha256 }
                        zram = [pscustomobject]@{ device = "zram0"; size_kib = 1048572; priority = 200; algorithm = "lzo-rle"; identity_sha256 = $scriptHash }
                        lower = [pscustomobject]@{
                            type = if ($Mode -eq "nbd") { "nbd" } else { "scratch" }
                            identity_sha256 = $lowerIdentity
                            sink_type = "directory"
                            sink_identity_sha256 = $manifestSha256
                        }
                        utc = [pscustomobject]@{ started = "2026-08-12T00:00:00Z" }
                        watchdog = [pscustomobject]@{ armed = $true; outcome = "not_fired" }
                        timeout_budget = [pscustomobject]@{ sample_timeout_sec = 240; integrity_finalization_timeout_sec = 120; samples = 3; setup_cleanup_timeout_sec = 300; cell_outer_timeout_sec = 1380 }
                        argv = @(
                            "nbd-benchmark-cell.sh", "--run", "--mode", $Mode, "--condition", "idle", "--tier-mib", "1024",
                            "--artifact-dir", "<campaign-artifact-dir>", "--sealed-release-root", $selectedRelease.selected,
                            "--release-version", "manufactured-v1", "--expected-source-commit", $sourceCommit,
                            "--expected-manifest-sha256", $manifestSha256, "--pair-id", "1024-idle", "--runs", "3", "--sample-timeout-sec", "240"
                        )
                        script_sha256 = [pscustomobject]$scriptHashes
                        workload = [pscustomobject]@{ name = "anonymous_memory_sequential_write"; pattern = "shake256-v1"; allocated_mib = 3584; memory_high_mib = 1200; memory_max_mib = 4096; allocation_chunk_bytes = 67108864; worker_threads = [Math]::Min(1, [Environment]::ProcessorCount) }
                    }
                    if ($Mode -eq "nbd") {
                        $record["nbd"] = [pscustomobject]@{
                            device = "/dev/nbd0"; block_major_minor = "43:0"; size_kib = 1048576
                            capacity_sectors = 2097152; usable_size_kib = 1048572; priority = 100
                            server_pid = 4242; daemon_executable_relative_path = "bin/ramsharedd"
                            daemon_manifest_sha256 = $scriptHash; identity_sha256 = $lowerIdentity
                        }
                    }
                    [pscustomobject]$record
                }
                $diskContext = & $newContext "disk-only" "N/A"
                $nbdContext = & $newContext "nbd" "PASS"
                $diskSummary = [pscustomobject]@{ median_allocation_to_hold_ms = 100; p99_allocation_to_hold_ms = 100; population_stddev_allocation_to_hold_ms = 10 }
                $newNbdSummary = {
                    param([double]$Median, [double]$P99, [double]$Stddev)
                    [pscustomobject]@{
                        median_allocation_to_hold_ms = $Median
                        p99_allocation_to_hold_ms = $P99
                        population_stddev_allocation_to_hold_ms = $Stddev
                    }
                }
                $nbdSummary = & $newNbdSummary 112 120 10
                $pairContext = [pscustomobject]@{
                    pair_id = "1024-idle"
                    timeout_budget = Get-PairTimeoutBudget -TierMiB 1024
                    cuda_hold_sec = 0
                    gpu_identity = [pscustomobject]@{ gpu_model = "Manufactured GPU"; gpu_driver = "1.2.3" }
                    windows_script_sha256 = [pscustomobject]@{
                        "Invoke-NbdBenchmarkMatrix.ps1" = $scriptHash
                        "Start-CudaVramWorkload.ps1" = "N/A"
                    }
                }
                $script:BaselineFile = ""
                $candidate = New-PairComparison -DiskSummary $diskSummary -NbdSummary $nbdSummary -DiskContext $diskContext -NbdContext $nbdContext -DiskCell $cellDisk -NbdCell $cellNbd -PairContext $pairContext -SelectedRelease $selectedRelease
                if ($candidate.baseline_verdict -ne "BASELINE_CANDIDATE" -or $candidate.nbd_vs_disk_median_ratio -ne 1.12 -or $candidate.nbd_vs_disk_p99_ratio -ne 1.2) {
                    throw "manufactured_ratio_or_candidate_invalid"
                }
                if ([int64]$candidate.environment_fingerprint_material.environment.zram_topology.size_kib -ne 1048572) {
                    throw "manufactured_zram_observed_usable_size_not_retained"
                }
                $baselinePath = Join-Path $temp "baseline.json"
                [ordered]@{
                    schema = 1; workload_schema = "ramshared-nbd-pair/v1"; environment_fingerprint = $candidate.environment_fingerprint;
                    nbd_vs_disk_median_ratio = 1.0; nbd_vs_disk_p99_ratio = 1.0; population_stddev_allocation_to_hold_ms = 10
                } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $baselinePath -Encoding UTF8
                $script:BaselineFile = $baselinePath
                $comparisonArguments = @{
                    DiskSummary = $diskSummary
                    NbdSummary = $null
                    DiskContext = $diskContext
                    NbdContext = $nbdContext
                    DiskCell = $cellDisk
                    NbdCell = $cellNbd
                    PairContext = $pairContext
                    SelectedRelease = $selectedRelease
                }
                $assertVerdict = {
                    param([string]$Name, [double]$Median, [double]$P99, [double]$Stddev, [string]$Expected)
                    $comparisonArguments["NbdSummary"] = & $newNbdSummary $Median $P99 $Stddev
                    $comparison = New-PairComparison @comparisonArguments
                    if ($comparison.baseline_verdict -ne $Expected) {
                        throw ("manufactured_" + $Name + "_expected_" + $Expected + "_actual_" + $comparison.baseline_verdict)
                    }
                    $comparison
                }
                $null = & $assertVerdict "green_at_exact_boundaries" 110 115 20 "GREEN"
                $yellowMedian = & $assertVerdict "yellow_median" 110.001 115 20 "YELLOW"
                $null = & $assertVerdict "yellow_p99" 110 115.001 20 "YELLOW"
                $null = & $assertVerdict "yellow_stddev" 110 115 20.001 "YELLOW"
                $null = & $assertVerdict "red_median" 115.001 115 20 "RED"
                $null = & $assertVerdict "red_p99" 110 125.001 20 "RED"
                $mismatch = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $baselinePath -Raw)
                $mismatch.environment_fingerprint = "mismatch"
                Write-JsonNoBom -Value $mismatch -Path $baselinePath
                $comparisonArguments["NbdSummary"] = & $newNbdSummary 112 120 10
                $notComparable = New-PairComparison @comparisonArguments
                if ($notComparable.baseline_verdict -ne "NOT_COMPARABLE") { throw "manufactured_baseline_mismatch_invalid" }
                $yellowPair = @(
                    (New-CellExecution -Result ([pscustomobject]@{ status = "PASS"; mode = "disk-only" })),
                    (New-CellExecution -Result ([pscustomobject]@{ status = "PASS"; mode = "nbd" }))
                )
                $redPair = @(
                    (New-CellExecution -Result ([pscustomobject]@{ status = "PASS"; mode = "disk-only" })),
                    (New-CellExecution -Result ([pscustomobject]@{ status = "PASS"; mode = "nbd" }))
                )
                Apply-BaselineVerdictToPair -PairResults $yellowPair -Comparison ([pscustomobject]@{ baseline_verdict = "YELLOW" })
                Apply-BaselineVerdictToPair -PairResults $redPair -Comparison ([pscustomobject]@{ baseline_verdict = "RED" })
                if ($yellowPair[1].result.status -ne "PASS" -or $yellowPair[1].result.promotion -ne "promotion_stopped" -or
                    $yellowPair[1].result.pair_decision.verdict -ne "YELLOW" -or $yellowPair[1].result.pair_decision.reason -ne "baseline_regression_yellow" -or
                    $redPair[1].result.status -ne "PASS" -or $redPair[1].result.promotion -ne "promotion_stopped" -or
                    $redPair[1].result.pair_decision.verdict -ne "RED" -or $redPair[1].result.pair_decision.reason -ne "baseline_regression_red") {
                    throw "manufactured_baseline_pair_status_invalid"
                }
                if ((Test-PromotionMayAdvance -PairResults $yellowPair) -or (Test-PromotionMayAdvance -PairResults $redPair)) {
                    throw "manufactured_baseline_promotion_was_not_stopped"
                }
                $assertComparisonRefusal = {
                    param([string]$Name, $MutatedDiskContext, $MutatedNbdContext, [string]$ExpectedReason)
                    $refused = $false
                    try {
                        New-PairComparison -DiskSummary $diskSummary -NbdSummary $nbdSummary -DiskContext $MutatedDiskContext -NbdContext $MutatedNbdContext -DiskCell $cellDisk -NbdCell $cellNbd -PairContext $pairContext -SelectedRelease $selectedRelease | Out-Null
                    } catch {
                        $refused = $_.Exception.Message -eq $ExpectedReason
                    }
                    if (-not $refused) { throw ("manufactured_" + $Name + "_identity_was_accepted") }
                }
                $assertZramComparisonAccepted = {
                    param([string]$Name, [int64]$SizeKib)
                    $mutatedDisk = ConvertFrom-JsonPreservingDateStrings -Json ($diskContext | ConvertTo-Json -Depth 16)
                    $mutatedNbd = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                    $mutatedDisk.zram.size_kib = $SizeKib
                    $mutatedNbd.zram.size_kib = $SizeKib
                    $comparison = New-PairComparison -DiskSummary $diskSummary -NbdSummary $nbdSummary -DiskContext $mutatedDisk -NbdContext $mutatedNbd -DiskCell $cellDisk -NbdCell $cellNbd -PairContext $pairContext -SelectedRelease $selectedRelease
                    if ([int64]$comparison.environment_fingerprint_material.environment.zram_topology.size_kib -ne $SizeKib) {
                        throw ("manufactured_" + $Name + "_zram_size_not_retained")
                    }
                }
                $assertZramComparisonRefusal = {
                    param([string]$Name, $DiskSizeKib, $NbdSizeKib, [string]$ExpectedReason)
                    $mutatedDisk = ConvertFrom-JsonPreservingDateStrings -Json ($diskContext | ConvertTo-Json -Depth 16)
                    $mutatedNbd = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                    $mutatedDisk.zram.size_kib = $DiskSizeKib
                    $mutatedNbd.zram.size_kib = $NbdSizeKib
                    & $assertComparisonRefusal $Name $mutatedDisk $mutatedNbd $ExpectedReason
                }
                & $assertZramComparisonAccepted "zram_usable_lower_bound" 1048568
                & $assertZramComparisonAccepted "zram_usable_observed" 1048572
                & $assertZramComparisonAccepted "zram_usable_upper_bound" 1048576
                & $assertZramComparisonRefusal "zram_usable_below_lower_bound" 1048567 1048567 "comparison_zram_topology_mismatch"
                & $assertZramComparisonRefusal "zram_usable_above_upper_bound" 1048577 1048577 "comparison_zram_topology_mismatch"
                & $assertZramComparisonRefusal "zram_usable_noncanonical" "1048572.0" "1048572.0" "comparison_zram_topology_mismatch"
                $rawDecimalZram = ConvertFrom-Json -InputObject '{"size_kib":1048572.0}'
                $rawExponentZram = ConvertFrom-Json -InputObject '{"size_kib":1.048572e6}'
                if (($rawDecimalZram.size_kib -isnot [decimal] -and $rawDecimalZram.size_kib -isnot [double]) -or
                    $rawExponentZram.size_kib -isnot [double]) {
                    throw "manufactured_zram_raw_numeric_fixture_types_invalid"
                }
                & $assertZramComparisonRefusal "zram_usable_raw_decimal" $rawDecimalZram.size_kib $rawDecimalZram.size_kib "comparison_zram_topology_mismatch"
                & $assertZramComparisonRefusal "zram_usable_raw_exponent" $rawExponentZram.size_kib $rawExponentZram.size_kib "comparison_zram_topology_mismatch"
                & $assertZramComparisonRefusal "zram_usable_overflow" "18446744073710600192" "18446744073710600192" "comparison_zram_topology_mismatch"
                & $assertZramComparisonRefusal "zram_pair_observed_drift" 1048572 1048576 "comparison_zram_topology_mismatch"
                $releaseMismatch = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $releaseMismatch.release.source_commit = ("d" * 40) -join ""
                & $assertComparisonRefusal "release" $diskContext $releaseMismatch "comparison_release_identity_mismatch"
                $scriptMismatch = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $scriptMismatch.script_sha256."cascade-down.sh" = ("d" * 64) -join ""
                & $assertComparisonRefusal "script" $diskContext $scriptMismatch "comparison_script_hash_mismatch"
                $zramMismatch = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $zramMismatch.zram.algorithm = "lz4"
                & $assertComparisonRefusal "zram" $diskContext $zramMismatch "comparison_zram_topology_mismatch"
                $sinkMismatch = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $sinkMismatch.lower.sink_identity_sha256 = ("d" * 64) -join ""
                & $assertComparisonRefusal "sink" $diskContext $sinkMismatch "comparison_lower_sink_binding_mismatch"
                $argvMismatch = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $argvMismatch.argv[15] = ("d" * 40) -join ""
                & $assertComparisonRefusal "argv" $diskContext $argvMismatch "comparison_release_identity_mismatch"
                $swappedDiskLower = ConvertFrom-JsonPreservingDateStrings -Json ($diskContext | ConvertTo-Json -Depth 16)
                $swappedNbdLower = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $swappedDiskLower.lower.type = "nbd"
                $swappedNbdLower.lower.type = "scratch"
                & $assertComparisonRefusal "lower_mode_binding" $swappedDiskLower $swappedNbdLower "comparison_lower_topology_mismatch"
                $wrongDiskLower = ConvertFrom-JsonPreservingDateStrings -Json ($diskContext | ConvertTo-Json -Depth 16)
                $wrongDiskLower.lower.type = "lower_disk"
                & $assertComparisonRefusal "lower_wrong_type" $wrongDiskLower $nbdContext "comparison_lower_topology_mismatch"
                $missingNbdLowerType = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $null = $missingNbdLowerType.lower.PSObject.Properties.Remove("type")
                & $assertComparisonRefusal "lower_type_missing" $diskContext $missingNbdLowerType "comparison_lower_topology_mismatch"
                $missingNbdLowerIdentity = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $null = $missingNbdLowerIdentity.lower.PSObject.Properties.Remove("identity_sha256")
                & $assertComparisonRefusal "lower_identity_missing" $diskContext $missingNbdLowerIdentity "comparison_lower_topology_mismatch"
                $nbdDeviceMismatch = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $nbdDeviceMismatch.nbd.device = "/dev/nvme0n1"
                & $assertComparisonRefusal "nbd_device" $diskContext $nbdDeviceMismatch "comparison_nbd_identity_invalid"
                $nbdIdentityMismatch = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $nbdIdentityMismatch.nbd.identity_sha256 = ("d" * 64) -join ""
                & $assertComparisonRefusal "nbd_identity" $diskContext $nbdIdentityMismatch "comparison_nbd_identity_lower_mismatch"
                $nbdSinkAlias = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $nbdSinkAlias.lower.sink_identity_sha256 = $nbdSinkAlias.lower.identity_sha256
                & $assertComparisonRefusal "nbd_sink_alias" $diskContext $nbdSinkAlias "comparison_nbd_identity_sink_alias"
                $sameSecondTier = ConvertFrom-JsonPreservingDateStrings -Json ($nbdContext | ConvertTo-Json -Depth 16)
                $sameSecondTier.lower.identity_sha256 = $diskContext.lower.identity_sha256
                $sameSecondTier.nbd.identity_sha256 = $sameSecondTier.lower.identity_sha256
                & $assertComparisonRefusal "second_tier_identity" $diskContext $sameSecondTier "comparison_second_tier_identity_not_distinct"
                Write-Output "baseline_verdict=$($yellowMedian.baseline_verdict)"
                Write-Output "comparison_thresholds=PASS"
                Write-Output "baseline_pair_actions=PASS"
                Write-Output "comparison_identity_contract=PASS"
                Write-Output "comparison_zram_usable_size_bounds=PASS"
                Write-Output "comparison_zram_raw_numeric_types=REFUSED"
                Write-Output "comparison_zram_pair_equality=REFUSED"
                Write-Output "comparison_lower_mode_binding=REFUSED"
                Write-Output "comparison_lower_wrong_type=REFUSED"
                Write-Output "comparison_lower_type_missing=REFUSED"
                Write-Output "comparison_lower_identity_missing=REFUSED"
                Write-Output "comparison_lower_sink_identity=REFUSED"
                Write-Output "comparison_distinct_second_tier_identity=PASS"
                Write-Output "comparison_nbd_identity_contract=PASS"
                Write-Output "comparison_nbd_identity_invalid=REFUSED"
                Write-Output "comparison_second_tier_identity_not_distinct=REFUSED"
            } finally {
                $script:BaselineFile = $savedBaseline
                Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
            }
            return
        }
        "evidence-chain" {
            $dir = Join-Path ([IO.Path]::GetTempPath()) ("ramshared-nbd-evidence-" + [guid]::NewGuid().ToString("N"))
            New-Item -ItemType Directory -Path $dir | Out-Null
            try {
                $contextPath = Join-Path $dir "context.json"
                $samplesPath = Join-Path $dir "samples.jsonl"
                $summaryPath = Join-Path $dir "summary.json"
                $inventoryPath = Join-Path $dir "artifact-inventory.json"
                $envelopePath = Join-Path $dir "evidence-envelope.json"
                $beforePath = Join-Path $dir "before.txt"
                $actionPath = Join-Path $dir "action.txt"
                $afterPath = Join-Path $dir "after.txt"
                $sourceCommit = ("a" * 40) -join ""
                $manifestSha256 = ("b" * 64) -join ""
                $context = [ordered]@{
                    schema = 2; pair_id = "1024-idle"; mode = "nbd"; condition = "idle"; tier_mib = 1024
                    release = [ordered]@{ version = "manufactured-v1"; source_commit = $sourceCommit; source_tree_state = "clean"; manifest_sha256 = $manifestSha256 }
                    binary_match = "PASS"; watchdog = [ordered]@{ armed = $true; outcome = "not_fired" }
                    timeout_budget = [ordered]@{ sample_timeout_sec = 240; integrity_finalization_timeout_sec = 120; samples = 3; setup_cleanup_timeout_sec = 300; cell_outer_timeout_sec = 1380 }
                    lower = [ordered]@{
                        type = "nbd"; identity_sha256 = ("c" * 64) -join ""
                        sink_type = "directory"; sink_identity_sha256 = ("d" * 64) -join ""
                    }
                    nbd = [ordered]@{
                        device = "/dev/nbd0"; block_major_minor = "43:0"; size_kib = 1048576
                        capacity_sectors = 2097152; usable_size_kib = 1048572; priority = 100
                        server_pid = 4242; daemon_executable_relative_path = "bin/ramsharedd"
                        daemon_manifest_sha256 = ("e" * 64) -join ""; identity_sha256 = ("c" * 64) -join ""
                    }
                }
                Write-JsonNoBom -Value $context -Path $contextPath
                [IO.File]::WriteAllText($samplesPath, '{"sample":"manufactured"}' + "`n", [Text.UTF8Encoding]::new($false))
                [IO.File]::WriteAllText($beforePath, "before=manufactured`n", [Text.UTF8Encoding]::new($false))
                [IO.File]::WriteAllText($actionPath, "action=manufactured`n", [Text.UTF8Encoding]::new($false))
                [IO.File]::WriteAllText($afterPath, "after=manufactured`n", [Text.UTF8Encoding]::new($false))
                $writeEvidence = {
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
                    $envelopeArtifacts = @($inventory.files | ForEach-Object { [ordered]@{ path = $_.name; bytes = $_.bytes; sha256 = $_.sha256 } })
                    $envelope = [ordered]@{
                        schema_version = "ramshared-nbd-cell-evidence/v1"; pair_id = "1024-idle"; mode = "nbd"
                        release = [ordered]@{ version = "manufactured-v1"; source_commit = $sourceCommit; manifest_sha256 = $manifestSha256 }
                        context_sha256 = Get-Sha256File -Path $contextPath
                        summary_sha256 = Get-Sha256File -Path $summaryPath
                        artifact_inventory_sha256 = Get-Sha256File -Path $inventoryPath
                        binary_match = "PASS"
                        watchdog = [ordered]@{ armed = $true; outcome = "not_fired" }
                        timeout_budget = $context.timeout_budget
                        classification = "INCOMPARABLE"
                        artifacts = $envelopeArtifacts
                    }
                    Write-JsonNoBom -Value $envelope -Path $envelopePath
                }
                . $writeEvidence
                $evidence = Assert-CellEvidence -Summary ([pscustomobject]$summary) -CellResultDirectory $dir
                if ($evidence.context_sha256 -ne $summary.context_sha256) { throw "manufactured_evidence_chain_invalid" }
                $timeoutBudgetEnvelope = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $envelopePath -Raw)
                $timeoutBudgetEnvelope.timeout_budget.sample_timeout_sec = 600
                Write-JsonNoBom -Value $timeoutBudgetEnvelope -Path $envelopePath
                $timeoutBudgetTamperRefused = $false
                try { Assert-CellEvidence -Summary ([pscustomobject]$summary) -CellResultDirectory $dir | Out-Null } catch {
                    $timeoutBudgetTamperRefused = $_.Exception.Message -eq "cell_evidence_timeout_budget_mismatch"
                }
                if (-not $timeoutBudgetTamperRefused) { throw "manufactured_evidence_timeout_budget_tamper_was_accepted" }
                . $writeEvidence
                [IO.File]::WriteAllText($actionPath, "", [Text.UTF8Encoding]::new($false))
                . $writeEvidence
                $emptyReceiptRefused = $false
                try { Assert-CellEvidence -Summary ([pscustomobject]$summary) -CellResultDirectory $dir | Out-Null } catch {
                    $emptyReceiptRefused = $_.Exception.Message -eq "cell_evidence_required_artifact_empty:action.txt"
                }
                if (-not $emptyReceiptRefused) { throw "manufactured_evidence_empty_receipt_was_accepted" }
                [IO.File]::WriteAllText($actionPath, "action=manufactured`n", [Text.UTF8Encoding]::new($false))
                . $writeEvidence
                $invalidSummary = [pscustomobject]@{ context_sha256 = ("0" * 64) }
                $hashMismatchRefused = $false
                try { Assert-CellEvidence -Summary $invalidSummary -CellResultDirectory $dir | Out-Null } catch {
                    $hashMismatchRefused = $_.Exception.Message -eq "cell_evidence_context_sha256_mismatch"
                }
                if (-not $hashMismatchRefused) { throw "manufactured_evidence_hash_mismatch_was_accepted" }
                [IO.File]::WriteAllText($samplesPath, '{"sample":"tampered"}' + "`n", [Text.UTF8Encoding]::new($false))
                $tamperedInventoryRefused = $false
                try { Assert-CellEvidence -Summary ([pscustomobject]$summary) -CellResultDirectory $dir | Out-Null } catch {
                    $tamperedInventoryRefused = $_.Exception.Message -like "cell_evidence_inventory_*:samples.jsonl"
                }
                if (-not $tamperedInventoryRefused) { throw "manufactured_evidence_inventory_tamper_was_accepted" }
                [IO.File]::WriteAllText($samplesPath, '{"sample":"manufactured"}' + "`n", [Text.UTF8Encoding]::new($false))
                . $writeEvidence
                $unsafeInventory = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $inventoryPath -Raw)
                $unsafeInventory.files[0].name = "../unsafe.txt"
                Write-JsonNoBom -Value $unsafeInventory -Path $inventoryPath
                $unsafePathRefused = $false
                try { Assert-CellEvidence -Summary ([pscustomobject]$summary) -CellResultDirectory $dir | Out-Null } catch {
                    $unsafePathRefused = $_.Exception.Message -eq "cell_evidence_inventory_path_invalid"
                }
                if (-not $unsafePathRefused) { throw "manufactured_evidence_unsafe_path_was_accepted" }
                . $writeEvidence
                [IO.File]::WriteAllText((Join-Path $dir "unlisted.txt"), "unlisted`n", [Text.UTF8Encoding]::new($false))
                $unlistedRefused = $false
                try { Assert-CellEvidence -Summary ([pscustomobject]$summary) -CellResultDirectory $dir | Out-Null } catch {
                    $unlistedRefused = $_.Exception.Message -eq "cell_evidence_inventory_unlisted_file:unlisted.txt"
                }
                if (-not $unlistedRefused) { throw "manufactured_evidence_unlisted_file_was_accepted" }
                Remove-Item -LiteralPath (Join-Path $dir "unlisted.txt") -Force
                . $writeEvidence
                $leakyEnvelope = ConvertFrom-JsonPreservingDateStrings -Json (Get-Content -LiteralPath $envelopePath -Raw)
                $leakyEnvelope | Add-Member -NotePropertyName "hostname" -NotePropertyValue "private-host" -Force
                Write-JsonNoBom -Value $leakyEnvelope -Path $envelopePath
                $privateEnvelopeRefused = $false
                try { Assert-CellEvidence -Summary ([pscustomobject]$summary) -CellResultDirectory $dir | Out-Null } catch {
                    $privateEnvelopeRefused = $_.Exception.Message -eq "cell_evidence_envelope_internal_invalid"
                }
                if (-not $privateEnvelopeRefused) { throw "manufactured_evidence_private_envelope_was_accepted" }
                Remove-Item -LiteralPath $inventoryPath -Force
                $missingInventoryRefused = $false
                try { Assert-CellEvidence -Summary ([pscustomobject]$summary) -CellResultDirectory $dir | Out-Null } catch {
                    $missingInventoryRefused = $_.Exception.Message -eq "cell_evidence_artifact_missing"
                }
                if (-not $missingInventoryRefused) { throw "manufactured_evidence_missing_inventory_was_accepted" }
                Write-Output "evidence_chain=PASS"
                Write-Output "evidence_chain_mismatch=REFUSED"
                Write-Output "evidence_chain_all_inventory_entries=PASS"
                Write-Output "evidence_chain_unsafe_path=REFUSED"
                Write-Output "evidence_chain_unlisted_file=REFUSED"
                Write-Output "evidence_chain_envelope=PASS"
                Write-Output "evidence_chain_private_envelope=REFUSED"
                Write-Output "evidence_chain_empty_receipt=REFUSED"
                Write-Output "evidence_chain_timeout_budget=PASS"
                Write-Output "evidence_chain_timeout_budget_tamper=REFUSED"
            } finally {
                Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
            }
            return
        }
        "public-pair-evidence" {
            $dir = Join-Path ([IO.Path]::GetTempPath()) ("ramshared-nbd-public-pair-" + [guid]::NewGuid().ToString("N"))
            New-Item -ItemType Directory -Path $dir | Out-Null
            try {
                $sourceCommit = ("a" * 40) -join ""
                $manifestSha256 = ("b" * 64) -join ""
                $newExecution = {
                    param([string]$Mode, [string]$BinaryMatch, [int[]]$Samples)
                    $lowerType = if ($Mode -eq "nbd") { "nbd" } else { "scratch" }
                    $lowerIdentityHashCharacter = if ($Mode -eq "nbd") { "a" } else { "b" }
                    $cellDirectory = Join-Path $dir ("cell-" + [guid]::NewGuid().ToString("N"))
                    New-Item -ItemType Directory -Path $cellDirectory | Out-Null
                    $contextPath = Join-Path $cellDirectory "context.json"
                    $samplesPath = Join-Path $cellDirectory "samples.jsonl"
                    $summaryPath = Join-Path $cellDirectory "summary.json"
                    $inventoryPath = Join-Path $cellDirectory "artifact-inventory.json"
                    $envelopePath = Join-Path $cellDirectory "evidence-envelope.json"
                    $beforePath = Join-Path $cellDirectory "before.txt"
                    $actionPath = Join-Path $cellDirectory "action.txt"
                    $afterPath = Join-Path $cellDirectory "after.txt"
                    $context = [ordered]@{
                        schema = 2; pair_id = "1024-idle"; mode = $Mode; condition = "idle"; tier_mib = 1024
                        utc = [pscustomobject]@{ started = "2026-08-12T12:00:00Z" }
                        kernel_release = "6.6.0-manufactured"
                        release = [ordered]@{ version = "manufactured-v1"; source_commit = $sourceCommit; source_tree_state = "clean"; manifest_sha256 = $manifestSha256 }
                        binary_match = $BinaryMatch
                        watchdog = [ordered]@{ armed = $true; outcome = "not_fired" }
                        timeout_budget = [pscustomobject]@{ sample_timeout_sec = 240; integrity_finalization_timeout_sec = 120; samples = 3; setup_cleanup_timeout_sec = 300; cell_outer_timeout_sec = 1380 }
                        zram = [pscustomobject]@{
                            device = "zram0"; algorithm = "zstd"; size_kib = 1048576; priority = 200
                            identity_sha256 = ("c" * 64) -join ""
                        }
                        lower = [pscustomobject]@{
                            type = $lowerType
                            identity_sha256 = ($lowerIdentityHashCharacter * 64) -join ""
                            sink_type = "directory"; sink_identity_sha256 = ("d" * 64) -join ""
                        }
                        workload = [pscustomobject]@{
                            pattern = "shake256-v1"; allocation_chunk_bytes = 67108864; worker_threads = [Math]::Min(1, [Environment]::ProcessorCount); allocated_mib = 3584
                        }
                    }
                    if ($Mode -eq "nbd") {
                        $context | Add-Member -NotePropertyName "nbd" -NotePropertyValue ([pscustomobject]@{
                            device = "/dev/nbd0"; block_major_minor = "43:0"; size_kib = 1048576
                            capacity_sectors = 2097152; usable_size_kib = 1048572; priority = 100
                            server_pid = 4242; daemon_executable_relative_path = "bin/ramsharedd"
                            daemon_manifest_sha256 = ("8" * 64) -join ""; identity_sha256 = ($lowerIdentityHashCharacter * 64) -join ""
                        })
                    }
                    Write-JsonNoBom -Value $context -Path $contextPath
                    [IO.File]::WriteAllText($samplesPath, '{"sample":"manufactured"}' + "`n", [Text.UTF8Encoding]::new($false))
                    [IO.File]::WriteAllText($beforePath, "before=manufactured`n", [Text.UTF8Encoding]::new($false))
                    [IO.File]::WriteAllText($actionPath, "action=manufactured`n", [Text.UTF8Encoding]::new($false))
                    [IO.File]::WriteAllText($afterPath, "after=manufactured`n", [Text.UTF8Encoding]::new($false))
                    $summary = [ordered]@{
                        mode = $Mode; binary_match = $BinaryMatch; status = "PASS"; terminal_state = "PRODUCT_OFF"
                        raw_measurement_status = "PASS"
                        context_sha256 = Get-Sha256File -Path $contextPath
                        timeout_budget = $context.timeout_budget
                        samples = @($Samples | ForEach-Object { [pscustomobject]@{ allocation_to_hold_ms = [double]$_ } })
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
                        release = [ordered]@{ version = "manufactured-v1"; source_commit = $sourceCommit; manifest_sha256 = $manifestSha256 }
                        context_sha256 = Get-Sha256File -Path $contextPath; summary_sha256 = Get-Sha256File -Path $summaryPath
                        artifact_inventory_sha256 = Get-Sha256File -Path $inventoryPath; binary_match = $BinaryMatch
                        watchdog = [ordered]@{ armed = $true; outcome = "not_fired" }
                        timeout_budget = $context.timeout_budget; classification = "INCOMPARABLE"
                        artifacts = @($inventory.files | ForEach-Object { [ordered]@{ path = $_.name; bytes = $_.bytes; sha256 = $_.sha256 } })
                    }
                    Write-JsonNoBom -Value $envelope -Path $envelopePath
                    $evidence = Assert-CellEvidence -Summary ([pscustomobject]$summary) -CellResultDirectory $cellDirectory
                    New-CellExecution -Result ([pscustomobject]$summary) -Context $evidence.context -Evidence $evidence
                }
                $pairResults = @(
                    (& $newExecution "disk-only" "N/A" @(100, 110, 120)),
                    (& $newExecution "nbd" "PASS" @(105, 115, 125))
                )
                $pairContext = [pscustomobject]@{
                    pair_id = "1024-idle"
                    timeout_budget = Get-PairTimeoutBudget -TierMiB 1024
                    cuda_hold_sec = 0
                    gpu_identity = [pscustomobject]@{ gpu_model = "Manufactured GPU"; gpu_driver = "1.2.3" }
                    windows_script_sha256 = Get-WindowsPairScriptHashes -Condition "idle"
                }
                $selectedRelease = [pscustomobject]@{
                    version = "manufactured-v1"; source_commit = $sourceCommit
                    installed_manifest_sha256 = $manifestSha256; input_bundle_manifest_sha256 = "not_exposed"
                }
                $newComparison = {
                    param([string]$BaselineVerdict)
                    [pscustomobject]@{
                        environment_fingerprint = (("7") * 64) -join ""
                        baseline_verdict = $BaselineVerdict; baseline_reason = "manufactured"
                        nbd_vs_disk_median_ratio = 1.05; nbd_vs_disk_p99_ratio = 1.04
                        nbd_vs_disk_population_stddev_ratio = 1.0
                    }
                }
                $expectedDecisions = @{
                    BASELINE_CANDIDATE = @{ verdict = "BASELINE"; promotable = $false; qualified = $false }
                    NOT_COMPARABLE = @{ verdict = "INCOMPARABLE"; promotable = $false; qualified = $false }
                    GREEN = @{ verdict = "PASS"; promotable = $true; qualified = $true }
                    YELLOW = @{ verdict = "YELLOW"; promotable = $false; qualified = $true }
                    RED = @{ verdict = "RED"; promotable = $false; qualified = $true }
                }
                foreach ($baselineVerdict in $expectedDecisions.Keys) {
                    $decision = Get-PublicPairDecision -Comparison (& $newComparison $baselineVerdict)
                    $expected = $expectedDecisions[$baselineVerdict]
                    if ($decision.verdict -ne $expected.verdict -or [bool]$decision.promotable -ne [bool]$expected.promotable -or
                        [bool]$decision.qualified -ne [bool]$expected.qualified) {
                        throw "manufactured_public_pair_decision_mapping_invalid:$baselineVerdict"
                    }
                }
                $candidate = Write-PublicPairEvidence -PairResults $pairResults -Comparison (& $newComparison "BASELINE_CANDIDATE") `
                    -PairContext $pairContext -SelectedRelease $selectedRelease -PairDir $dir
                if ($candidate.record.schema_version -ne "ramshared-evidence/v1" -or
                    $candidate.record.decision.verdict -ne "BASELINE" -or $candidate.record.decision.promotable -or
                    $candidate.record.candidate.classification -ne "candidate/noncanonical" -or
                    -not $candidate.public_pair_evidence_noncanonical -or
                    -not (Test-Path -LiteralPath $candidate.public_envelope_path -PathType Leaf) -or
                    (Test-Path -LiteralPath (Join-Path $dir "results.jsonl") -PathType Leaf)) {
                    throw "manufactured_public_pair_candidate_invalid"
                }
                foreach ($baselineVerdict in @("YELLOW", "RED")) {
                    $classifiedPair = @(
                        (& $newExecution "disk-only" "N/A" @(100, 110, 120)),
                        (& $newExecution "nbd" "PASS" @(105, 115, 125))
                    )
                    $classifiedComparison = & $newComparison $baselineVerdict
                    Apply-BaselineVerdictToPair -PairResults $classifiedPair -Comparison $classifiedComparison
                    $classifiedPairDirectory = Join-Path $dir ("classified-" + $baselineVerdict.ToLowerInvariant())
                    New-Item -ItemType Directory -Path $classifiedPairDirectory | Out-Null
                    $classifiedCandidate = Write-PublicPairEvidence -PairResults $classifiedPair -Comparison $classifiedComparison `
                        -PairContext $pairContext -SelectedRelease $selectedRelease -PairDir $classifiedPairDirectory
                    $classifiedCell = $classifiedPair[1].result
                    if ($classifiedCandidate.record.decision.verdict -ne $baselineVerdict -or
                        [bool]$classifiedCandidate.record.decision.promotable -or
                        -not [bool]$classifiedCandidate.record.comparison.qualified -or
                        $classifiedCell.status -ne "PASS" -or $classifiedCell.promotion -ne "promotion_stopped" -or
                        $classifiedCell.pair_decision.verdict -ne $baselineVerdict -or
                        [bool]$classifiedCell.pair_decision.promotable -or
                        (Test-PromotionMayAdvance -PairResults $classifiedPair)) {
                        throw ("manufactured_public_pair_baseline_custody_invalid:" + $baselineVerdict)
                    }
                }
                $binaryRefused = $false
                $badBinaryResults = @(
                    (& $newExecution "disk-only" "N/A" @(100, 110, 120)),
                    (& $newExecution "nbd" "PASS" @(105, 115, 125))
                )
                $badBinaryResults[1].result.binary_match = "N/A"
                try {
                    Assert-PublicPairEvidenceEligibility -PairResults $badBinaryResults -Comparison (& $newComparison "GREEN")
                } catch { $binaryRefused = $_.Exception.Message -eq "public_pair_evidence_custody_stale" }
                if (-not $binaryRefused) { throw "manufactured_public_pair_binary_match_was_accepted" }
                $rawRefused = $false
                $badRawResults = @(
                    (& $newExecution "disk-only" "N/A" @(100, 110, 120)),
                    (& $newExecution "nbd" "PASS" @(105, 115, 125))
                )
                $badRawResults[1].result.raw_measurement_status = "RED"
                try {
                    Assert-PublicPairEvidenceEligibility -PairResults $badRawResults -Comparison (& $newComparison "GREEN")
                } catch { $rawRefused = $_.Exception.Message -eq "public_pair_evidence_custody_stale" }
                if (-not $rawRefused) { throw "manufactured_public_pair_raw_measurement_was_accepted" }
                $newPairResults = {
                    @(
                        (& $newExecution "disk-only" "N/A" @(100, 110, 120)),
                        (& $newExecution "nbd" "PASS" @(105, 115, 125))
                    )
                }
                $assertPublicPairRefusal = {
                    param([string]$Name, [object[]]$MutatedPairResults, [string]$ExpectedReason)
                    $refused = $false
                    try {
                        Assert-PublicPairEvidenceEligibility -PairResults $MutatedPairResults -Comparison (& $newComparison "GREEN")
                    } catch {
                        $refused = $_.Exception.Message -eq $ExpectedReason
                    }
                    if (-not $refused) { throw ("manufactured_public_pair_" + $Name + "_was_accepted") }
                }
                $swappedLowerPairResults = & $newPairResults
                $swappedLowerPairResults[0].Context.lower.type = "nbd"
                $swappedLowerPairResults[1].Context.lower.type = "scratch"
                & $assertPublicPairRefusal "lower_mode_binding" $swappedLowerPairResults "public_pair_evidence_custody_stale"
                $wrongDiskLowerPairResults = & $newPairResults
                $wrongDiskLowerPairResults[0].Context.lower.type = "lower_disk"
                & $assertPublicPairRefusal "lower_wrong_type" $wrongDiskLowerPairResults "public_pair_evidence_custody_stale"
                $missingNbdLowerTypePairResults = & $newPairResults
                $null = $missingNbdLowerTypePairResults[1].Context.lower.PSObject.Properties.Remove("type")
                & $assertPublicPairRefusal "lower_type_missing" $missingNbdLowerTypePairResults "public_pair_evidence_custody_stale"
                $missingNbdLowerIdentityPairResults = & $newPairResults
                $null = $missingNbdLowerIdentityPairResults[1].Context.lower.PSObject.Properties.Remove("identity_sha256")
                & $assertPublicPairRefusal "lower_identity_missing" $missingNbdLowerIdentityPairResults "public_pair_evidence_custody_stale"
                $sinkIdentityMismatchPairResults = & $newPairResults
                $sinkIdentityMismatchPairResults[1].Context.lower.sink_identity_sha256 = ("9" * 64) -join ""
                & $assertPublicPairRefusal "lower_sink_identity" $sinkIdentityMismatchPairResults "public_pair_evidence_custody_stale"
                $nbdDevicePairResults = & $newPairResults
                $nbdDevicePairResults[1].Context.nbd.device = "/dev/nvme0n1"
                & $assertPublicPairRefusal "nbd_device" $nbdDevicePairResults "public_pair_evidence_custody_stale"
                $sameSecondTierPairResults = & $newPairResults
                $sameSecondTierPairResults[1].Context.lower.identity_sha256 = $sameSecondTierPairResults[0].Context.lower.identity_sha256
                $sameSecondTierPairResults[1].Context.nbd.identity_sha256 = $sameSecondTierPairResults[1].Context.lower.identity_sha256
                & $assertPublicPairRefusal "second_tier_identity" $sameSecondTierPairResults "public_pair_evidence_custody_stale"
                Write-Output "public_pair_evidence=PASS"
                Write-Output "public_pair_evidence_mapping=PASS"
                Write-Output "public_pair_evidence_yellow_red_publish_without_mutating_cell_summary=PASS"
                Write-Output "public_pair_evidence_nbd_binary_match=REFUSED"
                Write-Output "public_pair_evidence_raw_measurement=REFUSED"
                Write-Output "public_pair_evidence_lower_mode_binding=REFUSED"
                Write-Output "public_pair_evidence_lower_wrong_type=REFUSED"
                Write-Output "public_pair_evidence_lower_type_missing=REFUSED"
                Write-Output "public_pair_evidence_lower_identity_missing=REFUSED"
                Write-Output "public_pair_evidence_lower_sink_identity=REFUSED"
                Write-Output "public_pair_evidence_distinct_second_tier_identity=PASS"
                Write-Output "public_pair_evidence_nbd_identity_contract=PASS"
                Write-Output "public_pair_evidence_nbd_identity_invalid=REFUSED"
                Write-Output "public_pair_evidence_second_tier_identity_not_distinct=REFUSED"
                Write-Output "public_pair_evidence_requires_nbd_binary_match_and_comparison=PASS"
                Write-Output "public_pair_evidence_maps_baseline_candidate_incomparable_and_pass_exactly=PASS"
            } finally {
                Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
            }
            return
        }
        "campaign-preflight" {
            $selectedRelease = [pscustomobject]@{
                selected = "/opt/ramshared/releases/manufactured-v1"
                version = "manufactured-v1"
                source_commit = ("a" * 40) -join ""
                manifest_sha256 = ("b" * 64) -join ""
                input_bundle_manifest_sha256 = ("c" * 64) -join ""
            }
            $common = @(
                "NBD_RELEASE_VERSION=manufactured-v1",
                ("NBD_RELEASE_MANIFEST_SHA256=" + $selectedRelease.manifest_sha256),
                ("NBD_INPUT_BUNDLE_MANIFEST_SHA256=" + $selectedRelease.input_bundle_manifest_sha256),
                "NBD_INSTALL_PROVENANCE=PASS",
                "NBD_RELEASE_GATE=PASS",
                "NBD_SELECTOR=PASS",
                "NBD_LOWER_TIER_BINDING=bound",
                "NBD_LOWER_TIER_CAPACITY=PASS",
                "NBD_RELAY_GATE=PASS"
            )
            $ready = (($common + @(
                "NBD_BINARY_MATCH=PASS", "NBD_TRANSPORT=nbd", "NBD_PRODUCT_STATE=READY",
                "NBD_READINESS_REASON=all_gates_pass"
            )) -join "`n") + "`n"
            $readyParsed = Assert-PinnedCampaignPreflightOutput -Text $ready -SelectedRelease $selectedRelease
            if ($readyParsed.product_state -ne "READY" -or $readyParsed.binary_match -ne "PASS") {
                throw "manufactured_campaign_ready_invalid"
            }
            $off = (($common + @(
                "NBD_BINARY_MATCH=NOT_APPLICABLE", "NBD_TRANSPORT=none", "NBD_PRODUCT_STATE=PRODUCT_OFF",
                "NBD_READINESS_REASON=product_off"
            )) -join "`n") + "`n"
            $offParsed = Assert-PinnedCampaignPreflightOutput -Text $off -SelectedRelease $selectedRelease
            if ($offParsed.product_state -ne "PRODUCT_OFF" -or $offParsed.binary_match -ne "NOT_APPLICABLE") {
                throw "manufactured_campaign_product_off_invalid"
            }
            $invalidReady = $ready -replace "NBD_BINARY_MATCH=PASS", "NBD_BINARY_MATCH=NOT_APPLICABLE"
            $invalidReadyRefused = $false
            try { Assert-PinnedCampaignPreflightOutput -Text $invalidReady -SelectedRelease $selectedRelease | Out-Null } catch {
                $invalidReadyRefused = $_.Exception.Message -eq "campaign_preflight_ready_binary_match_required"
            }
            if (-not $invalidReadyRefused) { throw "manufactured_campaign_ready_without_binary_match_was_accepted" }
            Write-Output "campaign_preflight_pinned=PASS"
            Write-Output "campaign_ready_binary_match=REFUSED"
            Write-Output "campaign_product_off_preflight=PASS"
            return
        }
        default { throw "manufactured_self_test_case_invalid" }
    }
}

if (-not [string]::IsNullOrWhiteSpace($ManufacturedSelfTestCase)) {
    if ($ApproveSharedDailyHost) { throw "manufactured_self_test_live_approval_conflict" }
    Invoke-ManufacturedSelfTest -Case $ManufacturedSelfTestCase
    return
}

Assert-InputContract
New-Item -ItemType Directory -Force -Path $ArtifactRoot | Out-Null
$planPath = Join-Path $ArtifactRoot $PlanFileName
if ($PlanOnly) {
    $plan = New-Plan
    Write-JsonNoBom -Value $plan -Path $planPath
    if ($plan.status -ne "PLAN") {
        throw ("gpu_headroom_shortfall blocked_tier_mib=" + $plan.blocked_tier_mib)
    }
    Write-Output "NBD_BENCHMARK_MATRIX=PLAN"
    Write-Output ("PLAN=" + $planPath)
    return
}
if (-not $ApproveSharedDailyHost) { throw "missing_ApproveSharedDailyHost" }

Assert-LiveConfiguration
$campaignRoot = Join-Path $ArtifactRoot ("nbd-benchmark-" + (Get-Date -Format "yyyyMMdd-HHmmss"))
New-Item -ItemType Directory -Path $campaignRoot | Out-Null
$selectedRelease = $null
$campaignPreflight = $null
try {
    $selectedRelease = Resolve-SelectedRelease
    $campaignPreflight = Invoke-CampaignProductOffPreflight -SelectedRelease $selectedRelease -CampaignRoot $campaignRoot
} catch {
    $failureReason = $_.Exception.Message
    $failureStatus = if ($failureReason -like "campaign_product_off_*") { "RED" } else { "REFUSED" }
    # Neither discovery nor a failed teardown/check proves the pilot is off.
    # Preserve uncertainty rather than fabricating a PRODUCT_OFF terminal state.
    $failureTerminalState = "unverified_unknown"
    $failureMatrix = [ordered]@{
        schema = 1
        status = $failureStatus
        reason = $failureReason
        NBD_BENCHMARK_MATRIX = $failureStatus
        selected_release = if ($null -eq $selectedRelease) { $null } else { $selectedRelease.selected }
        selected_release_source_commit = if ($null -eq $selectedRelease) { $null } else { $selectedRelease.source_commit }
        selected_release_source_tree_state = if ($null -eq $selectedRelease) { $null } else { $selectedRelease.source_tree_state }
        selected_release_manifest_sha256 = if ($null -eq $selectedRelease) { $null } else { $selectedRelease.manifest_sha256 }
        selected_release_containment = if ($null -eq $selectedRelease) { $null } else { $selectedRelease.containment }
        campaign_preflight = $campaignPreflight
        terminal_state = $failureTerminalState
        results = @()
        pair_contexts = @()
    }
    Write-JsonNoBom -Value $failureMatrix -Path (Join-Path $campaignRoot "matrix-summary.json")
    Write-MatrixArtifactInventory -CampaignRoot $campaignRoot
    throw
}

$plan = New-Plan -TerminalState "PRODUCT_OFF"
Write-JsonNoBom -Value $plan -Path $planPath
if ($plan.status -ne "PLAN") {
    $refusalMatrix = [ordered]@{
        schema = 1
        status = "REFUSED"
        reason = "gpu_headroom_shortfall"
        NBD_BENCHMARK_MATRIX = "REFUSED"
        selected_release = $selectedRelease.selected
        selected_release_source_commit = $selectedRelease.source_commit
        selected_release_source_tree_state = $selectedRelease.source_tree_state
        selected_release_manifest_sha256 = $selectedRelease.manifest_sha256
        selected_release_containment = $selectedRelease.containment
        campaign_preflight = $campaignPreflight
        terminal_state = "PRODUCT_OFF"
        results = @()
        pair_contexts = @()
    }
    Write-JsonNoBom -Value $refusalMatrix -Path (Join-Path $campaignRoot "matrix-summary.json")
    Write-MatrixArtifactInventory -CampaignRoot $campaignRoot
    throw ("gpu_headroom_shortfall blocked_tier_mib=" + $plan.blocked_tier_mib)
}

$results = @()
for ($offset = 0; $offset -lt @($plan.cells).Count; $offset += 2) {
    $pairCells = @($plan.cells[$offset], $plan.cells[$offset + 1])
    $pairResults = @(Invoke-CellPair -PairCells $pairCells -CampaignRoot $campaignRoot -SelectedRelease $selectedRelease)
    $results += $pairResults
    if (-not (Test-PromotionMayAdvance -PairResults $pairResults)) { break }
}

$resultRecords = @($results | ForEach-Object { $_.result })
$pairContextsById = @{}
foreach ($record in $resultRecords) {
    $pairContextProperty = $record.PSObject.Properties["pair_context"]
    if ($null -ne $pairContextProperty -and $null -ne $pairContextProperty.Value) {
        $pairContext = $pairContextProperty.Value
        if ($null -ne $pairContext.pair_id) {
            $pairContextsById[[string]$pairContext.pair_id] = $pairContext
        }
    }
}
$pairContexts = @($pairContextsById.GetEnumerator() | Sort-Object Name | ForEach-Object { $_.Value })
$hasBaselineDecision = Test-PairHasBaselineDecision -PairResults $results
$complete = @($resultRecords).Count -eq 12 -and @($resultRecords | Where-Object { $_.status -eq "PASS" }).Count -eq 12 -and -not $hasBaselineDecision
$hasRed = @($resultRecords | Where-Object { $_.status -eq "RED" }).Count -gt 0 -or (Test-PairDecisionIsRed -PairResults $results)
$terminalState = if (@($resultRecords | Where-Object { $_.terminal_state -ne "PRODUCT_OFF" }).Count -gt 0) {
    "unverified_unknown"
} else {
    "PRODUCT_OFF"
}
$matrix = [ordered]@{
    schema = 1
    status = if ($complete) { "PASS" } elseif ($hasRed) { "RED" } else { "PARTIAL" }
    reason = if ($complete) { "complete_matrix" } elseif ($hasRed) { "failed_pair" } else { "promotion_stopped" }
    NBD_BENCHMARK_MATRIX = if ($complete) { "PASS" } elseif ($hasRed) { "RED" } else { "PARTIAL" }
    selected_release = $selectedRelease.selected
    selected_release_source_commit = $selectedRelease.source_commit
    selected_release_source_tree_state = $selectedRelease.source_tree_state
    selected_release_manifest_sha256 = $selectedRelease.manifest_sha256
    selected_release_containment = $selectedRelease.containment
    campaign_preflight = $campaignPreflight
    terminal_state = $terminalState
    results = $resultRecords
    pair_contexts = $pairContexts
}
Write-JsonNoBom -Value $matrix -Path (Join-Path $campaignRoot "matrix-summary.json")
Write-MatrixArtifactInventory -CampaignRoot $campaignRoot
if (-not $complete) {
    if ($hasRed) { throw "matrix_red" }
    throw "promotion_stopped"
}
Write-Output "NBD_BENCHMARK_MATRIX=PASS"
Write-Output ("ARTIFACT_DIR=" + $campaignRoot)
