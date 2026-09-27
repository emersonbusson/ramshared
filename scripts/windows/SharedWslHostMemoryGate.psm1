Set-StrictMode -Version Latest

$nativeMemoryType = 'RamShared.Native.PerformanceInfoApi' -as [type]
if ($null -eq $nativeMemoryType) {
    $nativeMemorySource = @'
using System;
using System.Runtime.InteropServices;

namespace RamShared.Native
{
    [StructLayout(LayoutKind.Sequential)]
    public struct PerformanceInformation
    {
        public UInt32 cb;
        public UIntPtr CommitTotal;
        public UIntPtr CommitLimit;
        public UIntPtr CommitPeak;
        public UIntPtr PhysicalTotal;
        public UIntPtr PhysicalAvailable;
        public UIntPtr SystemCache;
        public UIntPtr KernelTotal;
        public UIntPtr KernelPaged;
        public UIntPtr KernelNonpaged;
        public UIntPtr PageSize;
        public UInt32 HandleCount;
        public UInt32 ProcessCount;
        public UInt32 ThreadCount;
    }

    public static class PerformanceInfoApi
    {
        [DllImport("psapi.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool GetPerformanceInfo(
            out PerformanceInformation information,
            UInt32 size);
    }
}
'@
    Add-Type -TypeDefinition $nativeMemorySource -Language CSharp -ErrorAction Stop | Out-Null
}

function Convert-SharedWslPagesToKiB {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][UInt64]$PageCount,
        [Parameter(Mandatory = $true)][UInt64]$PageSizeBytes
    )

    if ($PageSizeBytes -eq 0) { throw "host_memory_page_size_invalid" }
    return [UInt64][Math]::Floor(([double]$PageCount * [double]$PageSizeBytes) / 1024.0)
}

function Convert-SharedWslPagesToMiB {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][UInt64]$PageCount,
        [Parameter(Mandatory = $true)][UInt64]$PageSizeBytes
    )

    if ($PageSizeBytes -eq 0) { throw "host_memory_page_size_invalid" }
    return [int][Math]::Floor(([double]$PageCount * [double]$PageSizeBytes) / 1048576.0)
}

function Get-SharedWslCommitHeadroomMiB {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][UInt64]$CommitTotalPages,
        [Parameter(Mandatory = $true)][UInt64]$CommitLimitPages,
        [Parameter(Mandatory = $true)][UInt64]$PageSizeBytes
    )

    if ($CommitTotalPages -gt $CommitLimitPages) { throw "host_commit_counters_invalid" }
    return Convert-SharedWslPagesToMiB -PageCount ($CommitLimitPages - $CommitTotalPages) -PageSizeBytes $PageSizeBytes
}

function Get-SharedWslHostRequiredHeadroomMiB {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][ValidateRange(0.0, 16.0)][double]$PressureAllocGiB,
        [Parameter(Mandatory = $true)][ValidateRange(4096, 2147483647)][int]$ReserveMiB
    )

    return [int]([Math]::Ceiling($PressureAllocGiB * 1024.0) + $ReserveMiB)
}

function Get-SharedWslHostMemorySample {
    [CmdletBinding()]
    param()

    $timestamp = Get-Date -Format "o"
    try {
        $information = New-Object -TypeName RamShared.Native.PerformanceInformation
        $structureSize = [uint32][Runtime.InteropServices.Marshal]::SizeOf($information)
        $information.cb = $structureSize
        if (-not [RamShared.Native.PerformanceInfoApi]::GetPerformanceInfo([ref]$information, $structureSize)) {
            throw "get_performance_info_failed"
        }

        $pageSizeBytes = [UInt64]$information.PageSize.ToUInt64()
        $commitTotalPages = [UInt64]$information.CommitTotal.ToUInt64()
        $commitLimitPages = [UInt64]$information.CommitLimit.ToUInt64()
        $physicalTotalPages = [UInt64]$information.PhysicalTotal.ToUInt64()
        $physicalAvailablePages = [UInt64]$information.PhysicalAvailable.ToUInt64()
        if ($pageSizeBytes -eq 0 -or $physicalTotalPages -eq 0 -or
            $physicalAvailablePages -gt $physicalTotalPages -or
            $commitLimitPages -eq 0 -or $commitTotalPages -gt $commitLimitPages) {
            throw "invalid_performance_information_counters"
        }

        $totalPhysicalKiB = Convert-SharedWslPagesToKiB -PageCount $physicalTotalPages -PageSizeBytes $pageSizeBytes
        $physicalAvailableKiB = Convert-SharedWslPagesToKiB -PageCount $physicalAvailablePages -PageSizeBytes $pageSizeBytes
        $commitLimitKiB = Convert-SharedWslPagesToKiB -PageCount $commitLimitPages -PageSizeBytes $pageSizeBytes
        $commitUsedKiB = Convert-SharedWslPagesToKiB -PageCount $commitTotalPages -PageSizeBytes $pageSizeBytes
        $commitAvailableKiB = Convert-SharedWslPagesToKiB `
            -PageCount ($commitLimitPages - $commitTotalPages) -PageSizeBytes $pageSizeBytes

        return [pscustomobject][ordered]@{
            ts = $timestamp
            ok = $true
            total_physical_kib = $totalPhysicalKiB
            physical_available_kib = $physicalAvailableKiB
            commit_limit_kib = $commitLimitKiB
            commit_used_kib = $commitUsedKiB
            commit_headroom_kib = $commitAvailableKiB
            total_physical_mib = Convert-SharedWslPagesToMiB -PageCount $physicalTotalPages -PageSizeBytes $pageSizeBytes
            physical_headroom_mib = Convert-SharedWslPagesToMiB -PageCount $physicalAvailablePages -PageSizeBytes $pageSizeBytes
            total_commit_limit_mib = Convert-SharedWslPagesToMiB -PageCount $commitLimitPages -PageSizeBytes $pageSizeBytes
            commit_used_mib = Convert-SharedWslPagesToMiB -PageCount $commitTotalPages -PageSizeBytes $pageSizeBytes
            commit_headroom_mib = Get-SharedWslCommitHeadroomMiB `
                -CommitTotalPages $commitTotalPages -CommitLimitPages $commitLimitPages -PageSizeBytes $pageSizeBytes
        }
    } catch {
        return [pscustomobject][ordered]@{
            ts = $timestamp
            ok = $false
            error = "host_memory_query_failed"
            detail = "host_performance_info_unavailable"
            total_physical_kib = $null
            physical_available_kib = $null
            commit_limit_kib = $null
            commit_used_kib = $null
            commit_headroom_kib = $null
            total_physical_mib = $null
            physical_headroom_mib = $null
            total_commit_limit_mib = $null
            commit_used_mib = $null
            commit_headroom_mib = $null
        }
    }
}

function Test-SharedWslGuardianFresh {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$Guardian,
        [ValidateRange(1, 300)][int]$MaximumAgeSeconds = 15,
        [DateTimeOffset]$NowUtc = [DateTimeOffset]::UtcNow
    )

    if ($null -eq $Guardian -or [string]$Guardian.state -cne "HEALTHY") { return $false }
    $property = $Guardian.PSObject.Properties["timestamp_utc"]
    if ($null -eq $property -or $null -eq $property.Value) { return $false }
    $raw = $property.Value
    try {
        if ($raw -is [DateTimeOffset]) {
            $timestamp = $raw.ToUniversalTime()
        } elseif ($raw -is [DateTime]) {
            $date = [DateTime]$raw
            if ($date.Kind -eq [DateTimeKind]::Unspecified) {
                $date = [DateTime]::SpecifyKind($date, [DateTimeKind]::Utc)
            }
            $timestamp = [DateTimeOffset]($date.ToUniversalTime())
        } else {
            $parsed = [DateTimeOffset]::MinValue
            $styles = [Globalization.DateTimeStyles]::AssumeUniversal -bor [Globalization.DateTimeStyles]::AdjustToUniversal
            if (-not [DateTimeOffset]::TryParse([string]$raw, [Globalization.CultureInfo]::InvariantCulture, $styles, [ref]$parsed)) {
                return $false
            }
            $timestamp = $parsed.ToUniversalTime()
        }
    } catch {
        return $false
    }

    $ageSeconds = ($NowUtc.ToUniversalTime() - $timestamp).TotalSeconds
    return ($ageSeconds -ge 0 -and $ageSeconds -le $MaximumAgeSeconds)
}

function Test-SharedWslHostMemoryAdmission {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object[]]$Samples,
        [Parameter(Mandatory = $true)][ValidateRange(1, 2147483647)][int]$RequiredCommitMiB,
        [Parameter(Mandatory = $true)][ValidateRange(1, 2147483647)][int]$RequiredPhysicalMiB
    )

    if ($Samples.Count -ne 3) {
        return [pscustomobject][ordered]@{
            ok = $false
            reason = "host_memory_query_failed"
            commit_headroom_mib = $null
            physical_headroom_mib = $null
        }
    }

    $commitHeadrooms = @()
    $physicalHeadrooms = @()
    foreach ($sample in $Samples) {
        try {
            if ($null -eq $sample) { throw "sample_missing" }
            $sampleOk = [bool]$sample.ok
            $commitProperty = $sample.PSObject.Properties["commit_headroom_mib"]
            $physicalProperty = $sample.PSObject.Properties["physical_headroom_mib"]
            if ($null -eq $commitProperty -or $null -eq $commitProperty.Value -or
                $null -eq $physicalProperty -or $null -eq $physicalProperty.Value) {
                throw "host_memory_headroom_missing"
            }
            $sampleCommitMiB = [int]$commitProperty.Value
            $samplePhysicalMiB = [int]$physicalProperty.Value
        } catch {
            $sampleOk = $false
            $sampleCommitMiB = $null
            $samplePhysicalMiB = $null
        }
        if ($null -eq $sample -or -not $sampleOk -or $null -eq $sampleCommitMiB -or
            $null -eq $samplePhysicalMiB -or $sampleCommitMiB -lt 0 -or $samplePhysicalMiB -lt 0) {
            return [pscustomobject][ordered]@{
                ok = $false
                reason = "host_memory_query_failed"
                commit_headroom_mib = $null
                physical_headroom_mib = $null
            }
        }
        $commitHeadrooms += $sampleCommitMiB
        $physicalHeadrooms += $samplePhysicalMiB
    }

    $minimumCommitHeadroomMiB = [int]($commitHeadrooms | Measure-Object -Minimum | Select-Object -ExpandProperty Minimum)
    $minimumPhysicalHeadroomMiB = [int]($physicalHeadrooms | Measure-Object -Minimum | Select-Object -ExpandProperty Minimum)
    if ($minimumPhysicalHeadroomMiB -lt $RequiredPhysicalMiB) {
        return [pscustomobject][ordered]@{
            ok = $false
            reason = "host_physical_headroom_insufficient"
            commit_headroom_mib = $minimumCommitHeadroomMiB
            physical_headroom_mib = $minimumPhysicalHeadroomMiB
        }
    }
    if ($minimumCommitHeadroomMiB -lt $RequiredCommitMiB) {
        return [pscustomobject][ordered]@{
            ok = $false
            reason = "host_commit_headroom_insufficient"
            commit_headroom_mib = $minimumCommitHeadroomMiB
            physical_headroom_mib = $minimumPhysicalHeadroomMiB
        }
    }

    return [pscustomobject][ordered]@{
        ok = $true
        reason = "complete"
        commit_headroom_mib = $minimumCommitHeadroomMiB
        physical_headroom_mib = $minimumPhysicalHeadroomMiB
    }
}

function Test-SharedWslHostMemoryGuardian {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Sample,
        [Parameter(Mandatory = $true)][ValidateRange(4096, 2147483647)][int]$HostCommitReserveMiB,
        [Parameter(Mandatory = $true)][ValidateRange(4096, 2147483647)][int]$HostPhysicalReserveMiB,
        [Parameter(Mandatory = $true)][ValidateRange(0, 2)][int]$InvalidSampleCount
    )

    try {
        if ($null -eq $Sample) { throw "sample_missing" }
        $sampleOk = [bool]$Sample.ok
        $commitProperty = $Sample.PSObject.Properties["commit_headroom_mib"]
        $physicalProperty = $Sample.PSObject.Properties["physical_headroom_mib"]
        if ($null -eq $commitProperty -or $null -eq $commitProperty.Value -or
            $null -eq $physicalProperty -or $null -eq $physicalProperty.Value) {
            throw "host_memory_headroom_missing"
        }
        $sampleCommitMiB = [int]$commitProperty.Value
        $samplePhysicalMiB = [int]$physicalProperty.Value
    } catch {
        $sampleOk = $false
        $sampleCommitMiB = $null
        $samplePhysicalMiB = $null
    }
    if ($null -eq $Sample -or -not $sampleOk -or $null -eq $sampleCommitMiB -or
        $null -eq $samplePhysicalMiB -or $sampleCommitMiB -lt 0 -or $samplePhysicalMiB -lt 0) {
        $nextInvalidSampleCount = $InvalidSampleCount + 1
        return [pscustomobject][ordered]@{
            trip = ($nextInvalidSampleCount -ge 3)
            reason = if ($nextInvalidSampleCount -ge 3) { "host_memory_telemetry_stale" } else { $null }
            invalid_sample_count = $nextInvalidSampleCount
            commit_headroom_mib = $null
            physical_headroom_mib = $null
        }
    }

    if ($samplePhysicalMiB -lt $HostPhysicalReserveMiB) {
        return [pscustomobject][ordered]@{
            trip = $true
            reason = "host_physical_reserve_breached"
            invalid_sample_count = 0
            commit_headroom_mib = $sampleCommitMiB
            physical_headroom_mib = $samplePhysicalMiB
        }
    }
    if ($sampleCommitMiB -lt $HostCommitReserveMiB) {
        return [pscustomobject][ordered]@{
            trip = $true
            reason = "host_commit_reserve_breached"
            invalid_sample_count = 0
            commit_headroom_mib = $sampleCommitMiB
            physical_headroom_mib = $samplePhysicalMiB
        }
    }

    return [pscustomobject][ordered]@{
        trip = $false
        reason = $null
        invalid_sample_count = 0
        commit_headroom_mib = $sampleCommitMiB
        physical_headroom_mib = $samplePhysicalMiB
    }
}

Export-ModuleMember -Function @(
    "Convert-SharedWslPagesToKiB",
    "Convert-SharedWslPagesToMiB",
    "Get-SharedWslCommitHeadroomMiB",
    "Get-SharedWslHostRequiredHeadroomMiB",
    "Get-SharedWslHostMemorySample",
    "Test-SharedWslGuardianFresh",
    "Test-SharedWslHostMemoryAdmission",
    "Test-SharedWslHostMemoryGuardian"
)
