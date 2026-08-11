#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ScriptPath
)

$ErrorActionPreference = "Stop"

function Get-CurrentPowerShellExecutable {
    $path = (Get-Process -Id $PID -ErrorAction Stop).Path
    if ([string]::IsNullOrWhiteSpace($path) -or
        -not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "static_child_uses_current_host_executable failed: current PowerShell path is unavailable"
    }
    $path
}
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Measure-RamSharedDiskIo.ps1"
}
$text = Get-Content -LiteralPath $ScriptPath -Raw

function Import-ProductionFunction([string]$Name) {
    $tokens = $null
    $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput(
        $text, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "ramshared_disk_io_static: production script does not parse"
    }
    $functionAst = $ast.Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $Name
        }, $true)
    if ($null -eq $functionAst) {
        throw "ramshared_disk_io_static: missing production function $Name"
    }
    $body = $functionAst.Body.Extent.Text
    $body = $body.Substring(1, $body.Length - 2)
    Set-Item -Path ("Function:\script:{0}" -f $Name) `
        -Value ([scriptblock]::Create($body))
}

foreach ($needle in @(
    "Win32_PerfFormattedData_PerfDisk_PhysicalDisk",
    "DiskReadBytesPersec",
    "DiskWriteBytesPersec",
    "PercentDiskTime",
    "Get-Sha256Hex",
    "Start-Job",
    "probe_during_sampling",
    "Direct load during sampling",
    "three_rounds_emit_p50_p95_p99",
    "matching_checksum_exits_0",
    "checksum_mismatch_exits_6",
    "FILE_FLAG_NO_BUFFERING",
    "FILE_FLAG_WRITE_THROUGH",
    "UNCACHED_READ_BYTES",
    "UNCACHED_WRITE_BYTES"
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("ramshared_disk_io_static: missing " + $needle)
    }
}

$checksum = $text.IndexOf('if ($ChecksumRounds -gt 0)')
$defaultExit = $text.IndexOf('# Exit 0 if we have disk')
if ($checksum -lt 0 -or $defaultExit -lt 0 -or $defaultExit -lt $checksum) {
    throw "ramshared_disk_io_static: default exit must remain after checksum mode"
}

if ($text -match '\$match\s*=\s*\(\$got\.Length\s*-eq\s*\$bytes\.Length\)') {
    throw "ramshared_disk_io_static: length-only direct match is forbidden"
}

foreach ($name in @(
    "Get-Sha256Hex",
    "Resolve-RamSharedDiskBinding",
    "Test-RamSharedPayloadMatch",
    "Assert-RamSharedUncachedProbe",
    "Assert-RamSharedCounterActivity",
    "New-RamSharedCounterEvidence",
    "Write-Utf8JsonLine"
)) {
    Import-ProductionFunction $name
}

foreach ($productionCall in @(
    "Resolve-RamSharedDiskBinding",
    "Test-RamSharedPayloadMatch",
    "Assert-RamSharedUncachedProbe",
    "Assert-RamSharedCounterActivity",
    "New-RamSharedCounterEvidence",
    "Write-Utf8JsonLine"
)) {
    if ([regex]::Matches($text, [regex]::Escape($productionCall)).Count -lt 2) {
        throw "ramshared_disk_io_static: production helper is not integrated: $productionCall"
    }
}
if ($text -match 'Add-Content\s+-LiteralPath\s+\$JsonlOut') {
    throw "jsonl_is_utf8_without_bom failed: Add-Content UTF-8 is forbidden on PowerShell 5.1"
}
if ($text -match 'Stop-Job\s+\$sampleLoadJob\s+-Force' -or
    $text -notmatch 'uncached_probe_uses_powershell51_job_cleanup') {
    throw "uncached_probe_uses_powershell51_job_cleanup failed"
}
Write-Output "PASS uncached_probe_uses_powershell51_job_cleanup"

$disk = [pscustomobject]@{
    Number = 5
    FriendlyName = "RAMSHARE VRAMDISK"
    SerialNumber = "ABCDEF0123456789"
    Size = [uint64](1GB)
    BusType = "Virtual"
}
$partition = [pscustomobject]@{ DiskNumber = 5; DriveLetter = "R"; AccessPaths = @("R:\\") }

function Assert-IdentityProcessRefuses {
    param(
        [string]$TestName,
        [object[]]$Disks,
        [object[]]$Partitions,
        [string]$Serial
    )
    $wrapperPath = Join-Path $env:TEMP ("ramshared-identity-{0}.ps1" -f [guid]::NewGuid())
    try {
        $diskJson = ($Disks | ConvertTo-Json -Compress).Replace("'", "''")
        $partitionJson = ($Partitions | ConvertTo-Json -Compress).Replace("'", "''")
        $scriptLiteral = $ScriptPath.Replace("'", "''")
        $serialLiteral = $Serial.Replace("'", "''")
        $wrapper = @"
`$script:mockDisks = @('$diskJson' | ConvertFrom-Json)
`$script:mockPartitions = @('$partitionJson' | ConvertFrom-Json)
function Get-Disk { [CmdletBinding()] param(); return `$script:mockDisks }
function Get-Partition {
    [CmdletBinding()]
    param([string]`$DriveLetter, [string]`$AccessPath)
    return `$script:mockPartitions
}
& '$scriptLiteral' -Seconds 1 -DriveLetter R -ExpectedSerial '$serialLiteral' -ExpectedSizeBytes 1073741824
exit `$LASTEXITCODE
"@
        [IO.File]::WriteAllText(
            $wrapperPath, $wrapper, (New-Object System.Text.UTF8Encoding($false)))
        $priorErrorAction = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            $output = & (Get-CurrentPowerShellExecutable) -NoProfile -NonInteractive `
                -ExecutionPolicy Bypass -File $wrapperPath 2>&1
            $observedExit = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $priorErrorAction
        }
        if ($observedExit -ne 7) {
            throw "$TestName failed: expected process exit 7, observed=$observedExit output=$output"
        }
    } finally {
        Remove-Item -LiteralPath $wrapperPath -Force -ErrorAction SilentlyContinue
    }
}

Write-Output "PASS static_child_uses_current_host_executable"

try {
    Resolve-RamSharedDiskBinding @($disk) @($partition) `
        "0000000000000000" ([uint64](1GB)) "R" "" | Out-Null
    throw "expected_serial_mismatch_exits_nonzero failed: mismatch accepted"
} catch {
    if ($_.Exception.Message -like "expected_serial_mismatch_exits_nonzero failed:*") { throw }
}
Assert-IdentityProcessRefuses "expected_serial_mismatch_exits_nonzero" `
    @($disk) @($partition) "0000000000000000"
Write-Output "PASS expected_serial_mismatch_exits_nonzero"

try {
    Resolve-RamSharedDiskBinding @($disk, $disk) @($partition) `
        "ABCDEF0123456789" ([uint64](1GB)) "R" "" | Out-Null
    throw "ambiguous_product_disk_exits_nonzero failed: ambiguity accepted"
} catch {
    if ($_.Exception.Message -like "ambiguous_product_disk_exits_nonzero failed:*") { throw }
}
Assert-IdentityProcessRefuses "ambiguous_product_disk_exits_nonzero" `
    @($disk, $disk) @($partition) "ABCDEF0123456789"
Write-Output "PASS ambiguous_product_disk_exits_nonzero"

$foreignPartition = [pscustomobject]@{ DiskNumber = 9; DriveLetter = "R"; AccessPaths = @("R:\\") }
try {
    Resolve-RamSharedDiskBinding @($disk) @($foreignPartition) `
        "ABCDEF0123456789" ([uint64](1GB)) "R" "" | Out-Null
    throw "foreign_letter_remap_exits_nonzero failed: remap accepted"
} catch {
    if ($_.Exception.Message -like "foreign_letter_remap_exits_nonzero failed:*") { throw }
}
Assert-IdentityProcessRefuses "foreign_letter_remap_exits_nonzero" `
    @($disk) @($foreignPartition) "ABCDEF0123456789"
Write-Output "PASS foreign_letter_remap_exits_nonzero"

$intended = [byte[]](1, 2, 3, 4)
$consistentlyCorrupted = [byte[]](9, 2, 3, 4)
if (Test-RamSharedPayloadMatch $intended $consistentlyCorrupted) {
    throw "consistent_corruption_cannot_pass_checksum failed"
}
Write-Output "PASS consistent_corruption_cannot_pass_checksum"

try {
    Assert-RamSharedUncachedProbe ([pscustomobject]@{
            probe_during_sampling = $true
            UNCACHED_WRITE_BYTES = 0
            UNCACHED_READ_BYTES = 0
            match = $false
        }) | Out-Null
    throw "uncached_probe_failure_exits_nonzero failed: failed probe accepted"
} catch {
    if ($_.Exception.Message -like "uncached_probe_failure_exits_nonzero failed:*") { throw }
}
Write-Output "PASS uncached_probe_failure_exits_nonzero"

try {
    Assert-RamSharedCounterActivity 0 0 0 0 | Out-Null
    throw "zero_counter_activity_exits_nonzero failed: zero counters accepted"
} catch {
    if ($_.Exception.Message -like "zero_counter_activity_exits_nonzero failed:*") { throw }
}
Write-Output "PASS zero_counter_activity_exits_nonzero"

$counterEvidence = New-RamSharedCounterEvidence `
    @{ avg = 1024; max = 2048 } @{ avg = 4096; max = 8192 } `
    @{ avg = 10; max = 20 } @{ avg = 1; max = 2 } `
    @{ avg = 3; max = 4 } @{ avg = 5; max = 6 } `
    7 16384 32768 "ABCDEF0123456789" ([uint64](1GB))
foreach ($field in @(
    "perf_row_samples", "disk_read_bytes_per_sec_avg", "disk_read_bytes_per_sec_max",
    "disk_write_bytes_per_sec_avg", "disk_write_bytes_per_sec_max",
    "percent_disk_time_avg", "percent_disk_time_max", "avg_disk_sec_per_read_ms_avg",
    "avg_disk_sec_per_write_ms_avg", "current_disk_queue_length_avg",
    "uncached_write_bytes", "uncached_read_bytes"
)) {
    $property = $counterEvidence.PSObject.Properties[$field]
    if ($null -eq $property -or $null -eq $property.Value) {
        throw "counter_jsonl_contains_raw_activity failed: missing $field"
    }
}
Write-Output "PASS counter_jsonl_contains_raw_activity"

if ([string]$counterEvidence.serial -ne "ABCDEF0123456789" -or
    [uint64]$counterEvidence.expected_size_bytes -ne [uint64](1GB)) {
    throw "counter_jsonl_persists_expected_size failed"
}
Write-Output "PASS counter_jsonl_persists_expected_size"

$jsonlPath = Join-Path $env:TEMP ("ramshared-jsonl-{0}.jsonl" -f [guid]::NewGuid())
try {
    Write-Utf8JsonLine $jsonlPath ([ordered]@{ schema = 1; value = "ok" })
    $raw = [IO.File]::ReadAllBytes($jsonlPath)
    if ($raw.Length -ge 3 -and $raw[0] -eq 0xEF -and $raw[1] -eq 0xBB -and $raw[2] -eq 0xBF) {
        throw "jsonl_is_utf8_without_bom failed: BOM present"
    }
    $lines = [IO.File]::ReadAllLines($jsonlPath)
    if ($lines.Count -ne 1 -or ($lines[0] | ConvertFrom-Json).value -ne "ok") {
        throw "jsonl_is_utf8_without_bom failed: invalid JSONL"
    }
} finally {
    Remove-Item -LiteralPath $jsonlPath -Force -ErrorAction SilentlyContinue
}
Write-Output "PASS jsonl_is_utf8_without_bom"

$uncached = [regex]::Match($text, "(?s)`\$uncachedSource = @'\r?\n(.*?)\r?\n'@")
if (-not $uncached.Success) {
    throw "ramshared_disk_io_static: uncached C# source block missing"
}
Add-Type -TypeDefinition $uncached.Groups[1].Value -ErrorAction Stop

Write-Output "PASS Test-RamSharedDiskIoStatic"
