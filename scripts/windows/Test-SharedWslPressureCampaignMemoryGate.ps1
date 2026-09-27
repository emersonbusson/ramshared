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

$oneMiB = Convert-SharedWslPagesToMiB -PageCount 256 -PageSizeBytes 4096
Assert-Equal -Actual $oneMiB -Expected 1 -Message "performance API page counts convert to MiB"
$commitHeadroom = Get-SharedWslCommitHeadroomMiB `
    -CommitTotalPages 200000 -CommitLimitPages 300000 -PageSizeBytes 4096
Assert-Equal -Actual $commitHeadroom -Expected 390 -Message "commit headroom is limit minus committed pages"
$invalidCommitCountersRefused = $false
try {
    Get-SharedWslCommitHeadroomMiB -CommitTotalPages 2 -CommitLimitPages 1 -PageSizeBytes 4096 | Out-Null
} catch {
    $invalidCommitCountersRefused = $true
}
Assert-Equal -Actual $invalidCommitCountersRefused -Expected $true `
    -Message "commit total above limit is rejected"

$liveMemorySample = Get-SharedWslHostMemorySample
Assert-Equal -Actual $liveMemorySample.ok -Expected $true `
    -Message "native host performance query returns live counters"
if ($liveMemorySample.commit_headroom_mib -le 0 -or $liveMemorySample.total_commit_limit_mib -le 0 -or
    $liveMemorySample.commit_used_mib -le 0 -or $liveMemorySample.physical_headroom_mib -le 0 -or
    $liveMemorySample.total_physical_mib -le 0) {
    throw "native host performance query returned non-positive counters"
}
Assert-Equal -Actual $liveMemorySample.commit_headroom_kib `
    -Expected ($liveMemorySample.commit_limit_kib - $liveMemorySample.commit_used_kib) `
    -Message "live commit headroom equals commit limit minus current commit"
if ($liveMemorySample.physical_available_kib -gt $liveMemorySample.total_physical_kib) {
    throw "native host performance query returned impossible physical availability"
}

$required = Get-SharedWslHostRequiredHeadroomMiB -PressureAllocGiB 2.92 -ReserveMiB 4096
Assert-Equal -Actual $required -Expected 7087 -Message "planned headroom requirement"

$belowPlan = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 7086 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 10000; physical_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 8000; physical_headroom_mib = 8000 }
) -RequiredCommitMiB $required -RequiredPhysicalMiB $required
Assert-Equal -Actual $belowPlan.ok -Expected $false -Message "host_memory_admission_refuses_below_plan_plus_reserve"
Assert-Equal -Actual $belowPlan.reason -Expected "host_physical_headroom_insufficient" -Message "below physical plan refusal reason"
Assert-Equal -Actual $belowPlan.commit_headroom_mib -Expected 8000 -Message "commit admission metric is preserved"
Assert-Equal -Actual $belowPlan.physical_headroom_mib -Expected 7086 -Message "physical admission metric is preserved"

$belowCommit = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 7086; physical_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 10000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 8000; physical_headroom_mib = 8000 }
) -RequiredCommitMiB $required -RequiredPhysicalMiB $required
Assert-Equal -Actual $belowCommit.ok -Expected $false -Message "host_memory_admission_refuses_low_commit_with_physical_headroom"
Assert-Equal -Actual $belowCommit.reason -Expected "host_commit_headroom_insufficient" -Message "below-commit refusal reason"

$atBoundary = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 7087; physical_headroom_mib = 7087 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 8000; physical_headroom_mib = 8000 }
) -RequiredCommitMiB $required -RequiredPhysicalMiB $required
Assert-Equal -Actual $atBoundary.ok -Expected $true -Message "host_memory_admission_passes_at_exact_commit_and_physical_boundaries"
Assert-Equal -Actual $atBoundary.commit_headroom_mib -Expected 7087 -Message "minimum commit headroom is retained"
Assert-Equal -Actual $atBoundary.physical_headroom_mib -Expected 7087 -Message "minimum physical headroom is retained"

$queryFailure = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $false; error = "cim_query_failed" },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 9000 }
) -RequiredCommitMiB $required -RequiredPhysicalMiB $required
Assert-Equal -Actual $queryFailure.ok -Expected $false -Message "host_memory_query_failure_refuses_before_wsl_launch"
Assert-Equal -Actual $queryFailure.reason -Expected "host_memory_query_failed" -Message "query refusal reason"

$malformedFailure = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = "not-a-number"; physical_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 9000 }
) -RequiredCommitMiB $required -RequiredPhysicalMiB $required
Assert-Equal -Actual $malformedFailure.reason -Expected "host_memory_query_failed" -Message "malformed telemetry refuses"

$nullHeadroomFailure = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = $null; physical_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 9000 }
) -RequiredCommitMiB $required -RequiredPhysicalMiB $required
Assert-Equal -Actual $nullHeadroomFailure.reason -Expected "host_memory_query_failed" -Message "null headroom refuses as query failure"

$missingPhysicalFailure = Test-SharedWslHostMemoryAdmission -Samples @(
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = $null },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 9000 },
    [pscustomobject]@{ ok = $true; commit_headroom_mib = 9000; physical_headroom_mib = 9000 }
) -RequiredCommitMiB $required -RequiredPhysicalMiB $required
Assert-Equal -Actual $missingPhysicalFailure.reason -Expected "host_memory_query_failed" -Message "missing physical memory refuses"

$belowPhysicalReserve = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{
    ok = $true
    commit_headroom_mib = 8192
    physical_headroom_mib = 4095
}) -HostCommitReserveMiB 4096 -HostPhysicalReserveMiB 4096 -InvalidSampleCount 0
Assert-Equal -Actual $belowPhysicalReserve.trip -Expected $true -Message "runtime_guard_trips_when_physical_reserve_is_breached"
Assert-Equal -Actual $belowPhysicalReserve.reason -Expected "host_physical_reserve_breached" -Message "physical reserve breach reason"

$belowCommitReserve = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{
    ok = $true
    commit_headroom_mib = 4095
    physical_headroom_mib = 8192
}) -HostCommitReserveMiB 4096 -HostPhysicalReserveMiB 4096 -InvalidSampleCount 0
Assert-Equal -Actual $belowCommitReserve.trip -Expected $true -Message "runtime_guard_trips_when_commit_reserve_is_breached"
Assert-Equal -Actual $belowCommitReserve.reason -Expected "host_commit_reserve_breached" -Message "commit reserve breach reason"

$healthyRuntime = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{
    ok = $true
    commit_headroom_mib = 4096
    physical_headroom_mib = 4096
}) -HostCommitReserveMiB 4096 -HostPhysicalReserveMiB 4096 -InvalidSampleCount 2
Assert-Equal -Actual $healthyRuntime.trip -Expected $false -Message "runtime guard accepts reserve boundary"
Assert-Equal -Actual $healthyRuntime.invalid_sample_count -Expected 0 -Message "valid sample clears telemetry loss state"

$missingHeadroom = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{ ok = $true }) `
    -HostCommitReserveMiB 4096 -HostPhysicalReserveMiB 4096 -InvalidSampleCount 0
Assert-Equal -Actual $missingHeadroom.trip -Expected $false -Message "missing headroom is telemetry loss, not reserve breach"
Assert-Equal -Actual $missingHeadroom.invalid_sample_count -Expected 1 -Message "missing headroom increments telemetry loss"

$firstLoss = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{ ok = $false }) `
    -HostCommitReserveMiB 4096 -HostPhysicalReserveMiB 4096 -InvalidSampleCount 0
$secondLoss = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{ ok = $false }) `
    -HostCommitReserveMiB 4096 -HostPhysicalReserveMiB 4096 -InvalidSampleCount $firstLoss.invalid_sample_count
$thirdLoss = Test-SharedWslHostMemoryGuardian -Sample ([pscustomobject]@{ ok = $false }) `
    -HostCommitReserveMiB 4096 -HostPhysicalReserveMiB 4096 -InvalidSampleCount $secondLoss.invalid_sample_count
Assert-Equal -Actual $firstLoss.trip -Expected $false -Message "first telemetry loss must not trip"
Assert-Equal -Actual $secondLoss.trip -Expected $false -Message "second telemetry loss must not trip"
Assert-Equal -Actual $thirdLoss.trip -Expected $true -Message "telemetry_loss_trips_after_three_samples"
Assert-Equal -Actual $thirdLoss.reason -Expected "host_memory_telemetry_stale" -Message "telemetry loss reason"

$nowUtc = [DateTimeOffset]::UtcNow
$serializedGuardian = [ordered]@{
    state = "HEALTHY"
    timestamp_utc = $nowUtc.ToString("o")
} | ConvertTo-Json -Compress
$deserializedGuardian = $serializedGuardian | ConvertFrom-Json
Assert-Equal -Actual (Test-SharedWslGuardianFresh -Guardian $deserializedGuardian -NowUtc $nowUtc) `
    -Expected $true -Message "fresh ISO guardian time survives PowerShell JSON conversion"
Assert-Equal -Actual (Test-SharedWslGuardianFresh -Guardian ([pscustomobject]@{
        state = "HEALTHY"
        timestamp_utc = [DateTime]::SpecifyKind($nowUtc.UtcDateTime, [DateTimeKind]::Utc)
    }) -NowUtc $nowUtc) -Expected $true -Message "fresh deserialized UTC DateTime is accepted"
Assert-Equal -Actual (Test-SharedWslGuardianFresh -Guardian ([pscustomobject]@{
        state = "HEALTHY"
        timestamp_utc = $nowUtc.UtcDateTime.ToString("MM/dd/yyyy HH:mm:ss", [Globalization.CultureInfo]::GetCultureInfo("en-US"))
    }) -NowUtc $nowUtc) -Expected $true -Message "localized legacy DateTime text is parsed invariantly"
Assert-Equal -Actual (Test-SharedWslGuardianFresh -Guardian ([pscustomobject]@{
        state = "HEALTHY"
        timestamp_utc = $nowUtc.AddSeconds(-16)
    }) -NowUtc $nowUtc) -Expected $false -Message "stale guardian is rejected"
Assert-Equal -Actual (Test-SharedWslGuardianFresh -Guardian ([pscustomobject]@{
        state = "HEALTHY"
        timestamp_utc = $nowUtc.AddSeconds(1)
    }) -NowUtc $nowUtc) -Expected $false -Message "future guardian time is rejected"
Assert-Equal -Actual (Test-SharedWslGuardianFresh -Guardian ([pscustomobject]@{
        state = "HEALTHY"
        timestamp_utc = "not-a-date"
    }) -NowUtc $nowUtc) -Expected $false -Message "malformed guardian time is rejected"

Write-Host "SHARED_WSL_PRESSURE_MEMORY_GATE=PASS"
