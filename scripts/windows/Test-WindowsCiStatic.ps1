#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$RepoRoot = "",
    [switch]$Install,
    [switch]$Service,
    [switch]$Vm,
    [switch]$Hardware,
    [switch]$Gpu,
    [switch]$Pressure,
    [switch]$Shutdown,
    [switch]$Reboot,
    [switch]$PhysicalHost
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Assert-StaticOnlyInvocation {
    [CmdletBinding()]
    param(
        [hashtable]$BoundParameters
    )

    if ($BoundParameters.ContainsKey("PhysicalHost")) {
        throw "windows_static_suite_rejects_physical_host_flag: physical-host execution is forbidden"
    }

    $forbidden = @(
        "Install", "Service", "Vm", "Hardware", "Gpu", "Pressure", "Shutdown", "Reboot"
    )
    $attempted = @($forbidden | Where-Object { $BoundParameters.ContainsKey($_) })
    if ($attempted.Count -gt 0) {
        throw ("windows_static_suite_refuses_mutating_switches: forbidden=" + ($attempted -join ","))
    }
}

function Quote-WindowsCiChildArgument {
    param([Parameter(Mandatory = $true)][string]$Value)
    if ($Value -notmatch '[\s"]') { return $Value }
    return '"' + ($Value -replace '(\\*)"', '$1$1\\"' -replace '(\\+)$', '$1$1') + '"'
}

function Invoke-WindowsCiStaticHarness {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][hashtable]$Arguments,
        # The normal static-child cap remains two minutes.  A named fixture may
        # declare at most three minutes when its own bounded self-tests prove
        # that it needs the extra finite cleanup allowance.
        [ValidateRange(120, 180)][int]$TimeoutSec = 120
    )
    # A static fixture intentionally exercises refusal branches and may write
    # expected errors to stderr.  Run it in an isolated child so those bytes
    # and its exit code become test data; a real nonzero still fails this
    # aggregate immediately after its captured diagnostics are reported.
    $powerShellPath = (Get-Process -Id $PID -ErrorAction Stop).Path
    if (-not (Test-Path -LiteralPath $powerShellPath -PathType Leaf)) { throw "windows_ci_static: PowerShell host unavailable" }
    $parts = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', (Quote-WindowsCiChildArgument -Value $Path))
    foreach ($key in @($Arguments.Keys | Sort-Object)) {
        $parts += ('-' + [string]$key)
        $parts += (Quote-WindowsCiChildArgument -Value ([string]$Arguments[$key]))
    }
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $powerShellPath
    $info.Arguments = $parts -join ' '
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) { throw "windows_ci_static: cannot start static harness $(Split-Path -Leaf $Path)" }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutSec * 1000)) {
            try { $process.Kill() } catch {}
            if (-not $process.WaitForExit(5000)) { throw "windows_ci_static: static harness deadline unreaped $(Split-Path -Leaf $Path)" }
            throw "windows_ci_static: static harness deadline exceeded $(Split-Path -Leaf $Path)"
        }
        if (-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdout, $stderr), 5000)) {
            throw "windows_ci_static: static harness stream drain failed $(Split-Path -Leaf $Path)"
        }
        $captured = [string]$stdout.Result + [string]$stderr.Result
        if (-not [string]::IsNullOrWhiteSpace($captured)) { Write-Output $captured.TrimEnd() }
        if ($process.ExitCode -ne 0) {
            throw "windows_ci_static: static harness failed $(Split-Path -Leaf $Path) exit=$($process.ExitCode)"
        }
    } finally {
        $process.Dispose()
    }
}

function Invoke-WindowsCiStaticSuite {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root
    )

    $resolvedRoot = (Resolve-Path -LiteralPath $Root -ErrorAction Stop).Path
    if (-not (Test-Path -LiteralPath (Join-Path $resolvedRoot "Cargo.toml") -PathType Leaf)) {
        throw "windows_ci_static: repository root is missing Cargo.toml"
    }

    $harnesses = @(
        @{ Name = "Test-AutonomousBrokerStatic.ps1"; Arguments = @{ RepoRoot = $resolvedRoot } },
        @{ Name = "Test-ProductOnlineStatic.ps1"; Arguments = @{ RepoRoot = $resolvedRoot } },
        @{ Name = "Test-RamSharedInfIsolationStatic.ps1"; Arguments = @{ RepoRoot = $resolvedRoot } },
        @{ Name = "Test-WindowsDiskCounterAuditStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-WindowsStorageMatrixStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-Win11LabMediaContractStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-Win11LabNetworkTransitionStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-Win11LabReadyStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-Win11LabReadyBaseStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-Win11LabReadyCloneStatic.ps1"; Arguments = @{} },
        # Direct isolated evidence is 2:04.52 on this runner; retain a
        # matrix-only 180 s cap rather than weakening the 120 s default.
        @{ Name = "Test-NbdBenchmarkMatrixStatic.ps1"; Arguments = @{}; TimeoutSec = 180 },
        @{ Name = "Test-Win11LabDriverTestFirmwareStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-WinDriveIoctlValidationStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-HostAutonomousLifecycleStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-GuestExhaustiveStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-RecoverGuestVerifierExactRunStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-RamSharedWslWatchdogStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-RamSharedOriginStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-RamSharedLaunchersStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-GuestPsDirectDeadlineStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-SharedWslPressureCampaignMemoryGate.ps1"; Arguments = @{} },
        @{ Name = "Test-SharedWslPressureCampaignStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-Win11WslRuntimeProbeStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-Win11Wsl2LabOfflineAccessStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-RamSharedWslStatusStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-WslIncidentSnapshotStatic.ps1"; Arguments = @{} }
    )

    foreach ($harness in $harnesses) {
        $path = Join-Path $PSScriptRoot $harness.Name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw ("windows_ci_static: missing static harness " + $harness.Name)
        }
        $timeoutSec = if ($harness.ContainsKey("TimeoutSec")) { [int]$harness.TimeoutSec } else { 120 }
        Invoke-WindowsCiStaticHarness -Path $path -Arguments $harness.Arguments -TimeoutSec $timeoutSec
    }

    Write-Output "PASS windows_static_nbd_matrix_deadline_is_specific_and_bounded"
    Write-Output "PASS windows_static_wrapper_includes_nbd_benchmark_harness"
    Write-Output "PASS windows_static_suite_runs_named_static_harnesses"
}

Assert-StaticOnlyInvocation -BoundParameters $PSBoundParameters
if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
}
Invoke-WindowsCiStaticSuite -Root $RepoRoot
