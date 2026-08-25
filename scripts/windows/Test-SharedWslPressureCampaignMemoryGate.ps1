#Requires -Version 5.1
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$module = Join-Path $root "scripts\windows\SharedWslHostMemoryGate.psm1"

Import-Module $module -Force

function Assert-Equal {
    param(
        [Parameter(Mandatory = $true)]$Actual,
        [Parameter(Mandatory = $true)]$Expected,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if ($Actual -ne $Expected) {
        throw "${Message}: expected '$Expected', got '$Actual'"
    }
}

$required = Get-SharedWslHostCommitRequiredMiB -PressureAllocGiB 2.92 -HostCommitReserveMiB 4096
Assert-Equal -Actual $required -Expected 7087 -Message "planned commit requirement"

$belowPlan = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 7086 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 8000 }
) -RequiredMiB $required
Assert-Equal -Actual $belowPlan.ok -Expected $false -Message "host_memory_admission_refuses_below_plan_plus_reserve"
Assert-Equal -Actual $belowPlan.reason -Expected "host_commit_headroom_insufficient" -Message "below-plan refusal reason"

$atBoundary = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 7087 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 8000 }
) -RequiredMiB $required
Assert-Equal -Actual $atBoundary.ok -Expected $true -Message "host_memory_admission_passes_at_exact_boundary"
Assert-Equal -Actual $atBoundary.commit_headroom_mib -Expected 7087 -Message "minimum headroom is retained"

$queryFailure = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $false; error = "cim_query_failed" },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000 }
) -RequiredMiB $required
Assert-Equal -Actual $queryFailure.ok -Expected $false -Message "host_memory_query_failure_refuses_before_wsl_launch"
Assert-Equal -Actual $queryFailure.reason -Expected "host_memory_query_failed" -Message "query refusal reason"

$malformedFailure = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = "not-a-number" },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000 }
) -RequiredMiB $required
Assert-Equal -Actual $malformedFailure.reason -Expected "host_memory_query_failed" -Message "malformed telemetry refuses"

$nullHeadroomFailure = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = $null },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000 }
) -RequiredMiB $required
Assert-Equal -Actual $nullHeadroomFailure.reason -Expected "host_memory_query_failed" -Message "null headroom refuses as query failure"

$belowReserve = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{
    ok = $true
    commit_headroom_mib = 4095
}) -HostCommitReserveMiB 4096 -InvalidSampleCount 0
Assert-Equal -Actual $belowReserve.trip -Expected $true -Message "runtime_guard_trips_once_below_reserve"
Assert-Equal -Actual $belowReserve.reason -Expected "host_commit_reserve_breached" -Message "reserve breach reason"

$healthyRuntime = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{
    ok = $true
    commit_headroom_mib = 4096
}) -HostCommitReserveMiB 4096 -InvalidSampleCount 2
Assert-Equal -Actual $healthyRuntime.trip -Expected $false -Message "runtime guard accepts reserve boundary"
Assert-Equal -Actual $healthyRuntime.invalid_sample_count -Expected 0 -Message "valid sample clears telemetry loss state"

$missingHeadroom = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{ ok = $true }) `
    -HostCommitReserveMiB 4096 -InvalidSampleCount 0
Assert-Equal -Actual $missingHeadroom.trip -Expected $false -Message "missing headroom is telemetry loss, not reserve breach"
Assert-Equal -Actual $missingHeadroom.invalid_sample_count -Expected 1 -Message "missing headroom increments telemetry loss"

$firstLoss = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{ ok = $false }) `
    -HostCommitReserveMiB 4096 -InvalidSampleCount 0
$secondLoss = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{ ok = $false }) `
    -HostCommitReserveMiB 4096 -InvalidSampleCount $firstLoss.invalid_sample_count
$thirdLoss = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{ ok = $false }) `
    -HostCommitReserveMiB 4096 -InvalidSampleCount $secondLoss.invalid_sample_count
Assert-Equal -Actual $firstLoss.trip -Expected $false -Message "first telemetry loss must not trip"
Assert-Equal -Actual $secondLoss.trip -Expected $false -Message "second telemetry loss must not trip"
Assert-Equal -Actual $thirdLoss.trip -Expected $true -Message "telemetry_loss_trips_after_three_samples"
Assert-Equal -Actual $thirdLoss.reason -Expected "host_memory_telemetry_stale" -Message "telemetry loss reason"

Write-Host "SHARED_WSL_PRESSURE_MEMORY_GATE=PASS"
