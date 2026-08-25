Set-StrictMode -Version Latest

function Invoke-SharedWslBoundedPowerShellQuery {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Query,
        [ValidateRange(1, 30)][int]$TimeoutSeconds = 5
    )

    $powerShellPath = Join-Path $PSHOME "powershell.exe"
    if (-not (Test-Path -LiteralPath $powerShellPath -PathType Leaf)) {
        return [pscustomobject]@{ completed = $false; data = $null; reason = "host_memory_query_host_unavailable" }
    }
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes(
            '$ErrorActionPreference = "Stop"; ' + $Query))
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $powerShellPath
    $info.Arguments = "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded"
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) { return [pscustomobject]@{ completed = $false; data = $null; reason = "host_memory_query_start_failed" } }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            try { $process.Kill() } catch {}
            $process.WaitForExit(5000) | Out-Null
            return [pscustomobject]@{ completed = $false; data = $null; reason = "host_memory_query_deadline_exceeded" }
        }
        if (-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdout, $stderr), 5000)) {
            return [pscustomobject]@{ completed = $false; data = $null; reason = "host_memory_query_stream_drain_failed" }
        }
        if ($process.ExitCode -ne 0) {
            return [pscustomobject]@{ completed = $false; data = $null; reason = "host_memory_query_failed" }
        }
        try {
            return [pscustomobject]@{ completed = $true; data = ($stdout.Result | ConvertFrom-Json -ErrorAction Stop); reason = "complete" }
        } catch {
            return [pscustomobject]@{ completed = $false; data = $null; reason = "host_memory_query_output_invalid" }
        }
    } finally {
        $process.Dispose()
    }
}

function Get-SharedWslHostCommitRequiredMiB {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][ValidateRange(0.0, 16.0)][double]$PressureAllocGiB,
        [Parameter(Mandatory = $true)][ValidateRange(4096, 2147483647)][int]$HostCommitReserveMiB
    )

    return [int]([Math]::Ceiling($PressureAllocGiB * 1024.0) + $HostCommitReserveMiB)
}

function Get-SharedWslHostMemorySample {
    [CmdletBinding()]
    param()

    $timestamp = Get-Date -Format "o"
    try {
        $query = Invoke-SharedWslBoundedPowerShellQuery `
            -Query 'Get-CimInstance -ClassName Win32_OperatingSystem | Select-Object TotalVirtualMemorySize, FreeVirtualMemory | ConvertTo-Json -Compress'
        if (-not $query.completed) { throw $query.reason }
        $operatingSystem = $query.data
        $totalVirtualKiB = [double]$operatingSystem.TotalVirtualMemorySize
        $freeVirtualKiB = [double]$operatingSystem.FreeVirtualMemory
        if ($totalVirtualKiB -le 0 -or $freeVirtualKiB -lt 0 -or $freeVirtualKiB -gt $totalVirtualKiB) {
            throw "invalid_virtual_memory_counters"
        }

        return [pscustomobject][ordered]@{
            ts = $timestamp
            ok = $true
            total_commit_limit_mib = [int][Math]::Floor($totalVirtualKiB / 1024.0)
            commit_headroom_mib = [int][Math]::Floor($freeVirtualKiB / 1024.0)
        }
    } catch {
        return [pscustomobject][ordered]@{
            ts = $timestamp
            ok = $false
            error = "host_memory_query_failed"
        }
    }
}

function Test-SharedWslHostMemoryAdmission {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object[]]$Samples,
        [Parameter(Mandatory = $true)][ValidateRange(1, 2147483647)][int]$RequiredMiB
    )

    if ($Samples.Count -ne 3) {
        return [pscustomobject][ordered]@{
            ok = $false
            reason = "host_memory_query_failed"
            commit_headroom_mib = $null
        }
    }

    $headrooms = @()
    foreach ($sample in $Samples) {
        try {
            if ($null -eq $sample) { throw "sample_missing" }
            $sampleOk = [bool]$sample.ok
            $headroomProperty = $sample.PSObject.Properties["commit_headroom_mib"]
            if ($null -eq $headroomProperty -or $null -eq $headroomProperty.Value) {
                throw "commit_headroom_missing"
            }
            $sampleHeadroomMiB = [int]$headroomProperty.Value
        } catch {
            $sampleOk = $false
            $sampleHeadroomMiB = $null
        }
        if ($null -eq $sample -or -not $sampleOk -or $null -eq $sampleHeadroomMiB -or
            $sampleHeadroomMiB -lt 0) {
            return [pscustomobject][ordered]@{
                ok = $false
                reason = "host_memory_query_failed"
                commit_headroom_mib = $null
            }
        }
        $headrooms += $sampleHeadroomMiB
    }

    $minimumHeadroomMiB = [int]($headrooms | Measure-Object -Minimum | Select-Object -ExpandProperty Minimum)
    if ($minimumHeadroomMiB -lt $RequiredMiB) {
        return [pscustomobject][ordered]@{
            ok = $false
            reason = "host_commit_headroom_insufficient"
            commit_headroom_mib = $minimumHeadroomMiB
        }
    }

    return [pscustomobject][ordered]@{
        ok = $true
        reason = "complete"
        commit_headroom_mib = $minimumHeadroomMiB
    }
}

function Test-SharedWslHostMemoryGuardian {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Sample,
        [Parameter(Mandatory = $true)][ValidateRange(4096, 2147483647)][int]$HostCommitReserveMiB,
        [Parameter(Mandatory = $true)][ValidateRange(0, 2)][int]$InvalidSampleCount
    )

    try {
        if ($null -eq $Sample) { throw "sample_missing" }
        $sampleOk = [bool]$Sample.ok
        $headroomProperty = $Sample.PSObject.Properties["commit_headroom_mib"]
        if ($null -eq $headroomProperty -or $null -eq $headroomProperty.Value) {
            throw "commit_headroom_missing"
        }
        $sampleHeadroomMiB = [int]$headroomProperty.Value
    } catch {
        $sampleOk = $false
        $sampleHeadroomMiB = $null
    }
    if ($null -eq $Sample -or -not $sampleOk -or $null -eq $sampleHeadroomMiB -or
        $sampleHeadroomMiB -lt 0) {
        $nextInvalidSampleCount = $InvalidSampleCount + 1
        return [pscustomobject][ordered]@{
            trip = ($nextInvalidSampleCount -ge 3)
            reason = if ($nextInvalidSampleCount -ge 3) { "host_memory_telemetry_stale" } else { $null }
            invalid_sample_count = $nextInvalidSampleCount
            commit_headroom_mib = $null
        }
    }

    $headroomMiB = $sampleHeadroomMiB
    if ($headroomMiB -lt $HostCommitReserveMiB) {
        return [pscustomobject][ordered]@{
            trip = $true
            reason = "host_commit_reserve_breached"
            invalid_sample_count = 0
            commit_headroom_mib = $headroomMiB
        }
    }

    return [pscustomobject][ordered]@{
        trip = $false
        reason = $null
        invalid_sample_count = 0
        commit_headroom_mib = $headroomMiB
    }
}

Export-ModuleMember -Function @(
    "Invoke-SharedWslBoundedPowerShellQuery",
    "Get-SharedWslHostCommitRequiredMiB",
    "Get-SharedWslHostMemorySample",
    "Test-SharedWslHostMemoryAdmission",
    "Test-SharedWslHostMemoryGuardian"
)
