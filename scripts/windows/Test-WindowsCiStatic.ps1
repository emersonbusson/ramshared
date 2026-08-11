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
        @{ Name = "Test-Win11LabDriverTestFirmwareStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-WinDriveIoctlValidationStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-HostAutonomousLifecycleStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-GuestExhaustiveStatic.ps1"; Arguments = @{} },
        @{ Name = "Test-RecoverGuestVerifierExactRunStatic.ps1"; Arguments = @{} }
    )

    foreach ($harness in $harnesses) {
        $path = Join-Path $PSScriptRoot $harness.Name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw ("windows_ci_static: missing static harness " + $harness.Name)
        }
        $harnessArguments = $harness.Arguments
        & $path @harnessArguments
    }

    Write-Output "PASS windows_static_suite_runs_named_static_harnesses"
}

Assert-StaticOnlyInvocation -BoundParameters $PSBoundParameters
if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
}
Invoke-WindowsCiStaticSuite -Root $RepoRoot
