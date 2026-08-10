#Requires -Version 5.1
<#
.SYNOPSIS
  Build or run the bounded physical Windows storage matrix from immutable packages.

.DESCRIPTION
  Plan-only by default. Live mode requires explicit physical-host approval,
  elevation, a watchdog, exact product identity, GPU reserve, BINARY_MATCH,
  consumer-first teardown, three repetitions and rollback to the supplied
  last-known-good manifest.
#>
[CmdletBinding()]
param(
    [switch]$Run,
    [switch]$PreparePackages,
    [switch]$SelfTestWorkload,
    [ValidateSet("", "pass", "baseline", "unqualified", "incomparable", "yellow", "red",
        "missing_rows", "timeout", "event_153", "missing_artifact",
        "rollback_driver_mismatch", "counter_timeout", "recovery_phase",
        "recovery_volume", "baseline_invalid", "baseline_key_domain", "counter_semantics",
        "counter_metric_semantics",
        "fresh_outdir", "toml_duplicate", "toml_duplicate_table",
        "online_identity", "pagefile_configured", "pipe_flood")]
    [string]$EvidenceSelfTestCase = "",
    [switch]$StorageProviderWorker,
    [switch]$StorageProviderRecoveryWorker,
    [switch]$StorageProviderObservationWorker,
    [switch]$WorkloadWorker,
    [switch]$EvidenceDelayWorker,
    [switch]$EvidencePipeFloodWorker,
    [switch]$ApprovePhysicalHost,
    [switch]$AllowWatchdogShutdown,
    [string]$PackageRoot = "C:\ramshared\artifacts\windows-storage-matrix-packages",
    [string]$PackageRevision = "",
    [string]$BasePackage = "C:\ramshared\artifacts\active-host-20260725-155910\package",
    [string]$DriverPackage = "C:\ramshared\package",
    [string]$WinsvcBinary = "C:\ramshared\bin\ramshared-winsvc.exe",
    [string]$BrokerBinary = "C:\ramshared\bin\ramshared-winbroker.exe",
    [string]$CounterProbeScript = "",
    [string]$RollbackManifest = "",
    [string]$Controller = "C:\ramshared\bin\ramshared-winsvc.exe",
    [string]$Letter = "S",
    [ValidateRange(3, 3)][int]$Runs = 3,
    [UInt64]$GpuReserveBytes = 536870912,
    [string]$BaselineSummary = "",
    [string]$SourceCommit = "",
    [ValidateSet("", "clean", "dirty")]
    [string]$SourceTreeState = "",
    [int]$SourceDirtyEntryCount = -1,
    [string]$OutDir = "C:\ramshared\artifacts\windows-storage-matrix-$(Get-Date -Format yyyyMMdd-HHmmss)",
    [UInt64]$WorkerSize = 0,
    [UInt32]$WorkerSector = 0,
    [int]$WorkerDiskNumber = -1,
    [string]$WorkerSerial = "",
    [string]$WorkerExpectedSerial = "",
    [string]$WorkerExpectedRunId = "",
    [string]$WorkerJournalPath = "",
    [ValidateSet("", "product_residue", "target_letter", "final_active")]
    [string]$WorkerObservation = "",
    [string]$WorkerPath = "",
    [ValidateSet("", "seq1m", "rand4k", "mixed70r30w", "flush", "integrity")]
    [string]$WorkerWorkload = "",
    [int]$WorkerRun = 0,
    [int]$WorkerQueueDepth = 0,
    [UInt64]$WorkerAvailableBytes = 0,
    [string]$WorkerCell = "",
    [string]$WorkerResult = ""
)

$ErrorActionPreference = "Stop"
$script:HarnessPath = $MyInvocation.MyCommand.Path
$script:InvocationParameters = [ordered]@{}
foreach ($entry in $PSBoundParameters.GetEnumerator() | Sort-Object Key) {
    $script:InvocationParameters[$entry.Key] = $entry.Value
}
if ([string]::IsNullOrWhiteSpace($CounterProbeScript)) {
    $CounterProbeScript = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "Measure-RamSharedDiskIo.ps1"
}
$cells = @(
    @("minimum", 67108864, 4096, 1, 1048576),
    @("small", 268435456, 4096, 4, 1048576),
    @("operator", 1073741824, 4096, 4, 1048576),
    @("compat", 1073741824, 512, 4, 1048576),
    @("concurrency", 2147483648, 4096, 16, 262144)
)
$workloads = @("seq1m", "rand4k", "mixed70r30w", "flush", "integrity")
$throughput_regression_red_pct = 20
$throughput_regression_yellow_pct = 10
$watchdogMarker = Join-Path $OutDir "watchdog.armed"
$watchdogTimeout = Join-Path $OutDir "watchdog.timeout"
$watchdogStaleSeconds = 600
$storageProviderTimeoutSeconds = 60
$storageObservationTimeoutSeconds = 15
$counterProbeTimeoutSeconds = 45
$controllerTimeoutSeconds = 90
$workloadTimeoutSeconds = 150
$expectedRows = 75
$expectedSummaries = 15
$evidenceSchemaVersion = 1
$harnessBehaviorRevision = "windows-storage-matrix-v3"
$requiredCellArtifacts = @(
    "properties.json",
    "storage-provider.json",
    "storage-provider.json.journal.json",
    "counter-direct.jsonl",
    "counter-direct.log",
    "samples.jsonl",
    "event-153.json"
)

function Write-Json($Value, [string]$Path, [int]$Depth = 8) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth $Depth),
        [Text.UTF8Encoding]::new($false))
}
function Get-Sha256([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToUpperInvariant()
}
function Get-SampleStandardDeviation([double[]]$Values) {
    if ($Values.Count -lt 2) { return 0.0 }
    $mean = ($Values | Measure-Object -Average).Average
    $sum = 0.0
    foreach ($value in $Values) {
        $sum += [math]::Pow(([double]$value - [double]$mean), 2)
    }
    [math]::Sqrt($sum / ($Values.Count - 1))
}
function Get-OverallVerdict([object[]]$Rows) {
    foreach ($candidate in @("RED", "YELLOW", "INCOMPARABLE", "BASELINE")) {
        if (@($Rows | Where-Object verdict -eq $candidate).Count -gt 0) {
            return $candidate
        }
    }
    "PASS"
}
function Get-VerdictExitCode([string]$Verdict) {
    switch ($Verdict) {
        "PASS" { return 0 }
        "RED" { return 1 }
        "YELLOW" { return 2 }
        "BASELINE" { return 3 }
        "INCOMPARABLE" { return 4 }
        default { return 1 }
    }
}
function Get-PlatformFingerprint($Fields) {
    $json = $Fields | ConvertTo-Json -Compress -Depth 8
    $bytes = [Text.Encoding]::UTF8.GetBytes($json)
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace("-", "")
    } finally {
        $sha.Dispose()
    }
}
function Quote-ProcessArgument([string]$Value) {
    '"' + $Value.Replace('"', '\"') + '"'
}
function Normalize-RamSharedText([string]$Value) {
    (($Value -replace '\s+', ' ').Trim()).ToUpperInvariant()
}
function Assert-FreshOutDir([string]$Path) {
    if (Test-Path -LiteralPath $Path) {
        throw "output directory already exists: $Path"
    }
}
function ConvertFrom-RamSharedToml([string]$Text, [string]$Name) {
    $values = @{}
    $tables = @{}
    $section = ""
    $lineNumber = 0
    foreach ($rawLine in ($Text -split "`r?`n")) {
        $lineNumber++
        $line = $rawLine.Trim()
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if ($line -match '^\[([A-Za-z0-9_-]+)\]$') {
            $nextSection = $matches[1]
            if ($tables.ContainsKey($nextSection)) {
                throw "duplicate TOML table name=$Name table=$nextSection"
            }
            $tables[$nextSection] = $true
            $section = $nextSection
            continue
        }
        if ($line -notmatch '^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.+?)\s*$') {
            throw "invalid TOML scalar name=$Name line=$lineNumber"
        }
        if ([string]::IsNullOrWhiteSpace($section)) {
            throw "unscoped TOML scalar name=$Name line=$lineNumber"
        }
        $key = "$section.$($matches[1])"
        if ($values.ContainsKey($key)) {
            throw "duplicate TOML scalar name=$Name key=$key"
        }
        $literal = $matches[2] -replace '\s+#.*$', ''
        $literal = $literal.Trim()
        if ($literal -match '^\d+$') {
            $values[$key] = [UInt64]$literal
        } elseif ($literal -match '^"(.*)"$') {
            $values[$key] = $matches[1]
        } elseif ($literal -in @("true", "false")) {
            $values[$key] = [bool]::Parse($literal)
        } else {
            throw "unsupported TOML scalar name=$Name key=$key"
        }
    }
    $values
}
function Set-RamSharedTomlInteger(
    [string]$Text,
    [string]$Name,
    [string]$Section,
    [string]$Key,
    [UInt64]$Value
) {
    $parsed = ConvertFrom-RamSharedToml $Text $Name
    $fullKey = "$Section.$Key"
    if (-not $parsed.ContainsKey($fullKey)) {
        throw "missing TOML scalar name=$Name key=$fullKey"
    }
    $pattern = "(?m)^(\\s*$([regex]::Escape($Key))\\s*=\\s*)\\d+(\\s*(?:#.*)?\\r?)$"
    $matches = [regex]::Matches($Text, $pattern)
    if ($matches.Count -ne 1) {
        throw "ambiguous TOML scalar replacement name=$Name key=$fullKey count=$($matches.Count)"
    }
    [regex]::Replace($Text, $pattern, ('${1}' + $Value + '${2}'), 1)
}
function Assert-EffectiveCellConfig(
    [string]$WinsvcText,
    [string]$BrokerText,
    [UInt64]$Size,
    [UInt32]$Sector,
    [int]$QueueDepth,
    [UInt64]$MaxIo,
    [string]$Cell
) {
    $winsvc = ConvertFrom-RamSharedToml $WinsvcText "winsvc-$Cell"
    $broker = ConvertFrom-RamSharedToml $BrokerText "broker-$Cell"
    $expectedWinsvc = [ordered]@{
        "win_drive.size_bytes" = [UInt64]$Size
        "win_drive.block_size" = [UInt64]$Sector
        "win_drive.queue_depth" = [UInt64]$QueueDepth
        "win_drive.max_io_bytes" = [UInt64]$MaxIo
    }
    foreach ($entry in $expectedWinsvc.GetEnumerator()) {
        if (-not $winsvc.ContainsKey($entry.Key) -or
            [UInt64]$winsvc[$entry.Key] -ne [UInt64]$entry.Value) {
            throw "prepared winsvc config mismatch cell=$Cell field=$($entry.Key)"
        }
    }
    if (-not $broker.ContainsKey("local_broker.capacity_bytes") -or
        [UInt64]$broker["local_broker.capacity_bytes"] -ne $Size) {
        throw "prepared broker capacity mismatch cell=$Cell"
    }
}
function Test-PositiveFinite([double]$Value) {
    -not [double]::IsNaN($Value) -and -not [double]::IsInfinity($Value) -and
    $Value -gt 0
}
function Test-NonNegativeFinite([double]$Value) {
    -not [double]::IsNaN($Value) -and -not [double]::IsInfinity($Value) -and
    $Value -ge 0
}
function Assert-BaselineDocument($Document) {
    if ($null -eq $Document -or
        [int]$Document.schema_version -ne $evidenceSchemaVersion -or
        [string]$Document.harness_behavior_revision -ne $harnessBehaviorRevision -or
        -not [bool]$Document.qualified -or
        [int]$Document.expected_rows -ne $expectedRows -or
        [int]$Document.observed_rows -ne $expectedRows -or
        [int]$Document.expected_summaries -ne $expectedSummaries -or
        [int]$Document.observed_summaries -ne $expectedSummaries) {
        throw "baseline schema/cardinality is invalid"
    }
    $entries = @($Document.entries)
    if ($entries.Count -ne $expectedSummaries) {
        throw "baseline entry cardinality is invalid"
    }
    $expectedKeyIndex = @{}
    foreach ($cell in $cells) {
        foreach ($workload in $workloads) {
            $expectedKeyIndex["$($cell[0])|$workload"] = $true
        }
    }
    if ($expectedKeyIndex.Count -ne $expectedSummaries) {
        throw "expected baseline key domain is internally inconsistent"
    }
    $index = @{}
    foreach ($entry in $entries) {
        $key = [string]$entry.key
        if ([string]::IsNullOrWhiteSpace($key) -or $index.ContainsKey($key) -or
            -not $expectedKeyIndex.ContainsKey($key) -or
            [string]$entry.platform_fingerprint -notmatch '^[0-9A-Fa-f]{64}$' -or
            [int]$entry.runs -ne $Runs -or
            -not (Test-PositiveFinite ([double]$entry.median_mib_per_sec)) -or
            -not (Test-PositiveFinite ([double]$entry.p99_latency_ms)) -or
            -not (Test-NonNegativeFinite ([double]$entry.stddev_mib_per_sec)) -or
            -not (Test-PositiveFinite ([double]$entry.min_mib_per_sec)) -or
            -not (Test-PositiveFinite ([double]$entry.max_mib_per_sec)) -or
            [double]$entry.min_mib_per_sec -gt [double]$entry.median_mib_per_sec -or
            [double]$entry.max_mib_per_sec -lt [double]$entry.median_mib_per_sec) {
            throw "baseline positive finite metric or key is invalid"
        }
        $index[$key] = $entry
    }
    $index
}
function Get-CounterNumber($Row, [string]$Name) {
    $property = $Row.PSObject.Properties[$Name]
    if ($null -eq $property -or -not (Test-NonNegativeFinite ([double]$property.Value))) {
        throw "counter numeric field is invalid: $Name"
    }
    [double]$property.Value
}
function Assert-CounterJsonlSemantics(
    [string]$Path,
    [string]$ExpectedSerial,
    [UInt64]$ExpectedSize
) {
    $rows = @(Get-Content -LiteralPath $Path -ErrorAction Stop | Where-Object {
            -not [string]::IsNullOrWhiteSpace($_)
        })
    if ($rows.Count -eq 0) {
        throw "counter JSONL is empty"
    }
    $normalizedSerial = Normalize-RamSharedText $ExpectedSerial
    foreach ($line in $rows) {
        $row = $line | ConvertFrom-Json -ErrorAction Stop
        if ((Normalize-RamSharedText ([string]$row.serial)) -ne $normalizedSerial) {
            throw "counter expected serial mismatch"
        }
        if ([UInt64]$row.expected_size_bytes -ne $ExpectedSize) {
            throw "counter expected size mismatch"
        }
        if ([int]$row.rounds -lt 3 -or
            [string]$row.last_sha256 -notmatch '^[0-9A-Fa-f]{64}$') {
            throw "counter direct checksum contract failed"
        }
        if ([Int64]$row.uncached_write_bytes -le 0 -or
            [Int64]$row.uncached_read_bytes -le 0 -or
            [Int64]$row.perf_row_samples -le 0) {
            throw "counter positive activity contract failed"
        }
        $numbers = @{}
        foreach ($name in @(
                "disk_read_bytes_per_sec_avg", "disk_read_bytes_per_sec_max",
                "disk_write_bytes_per_sec_avg", "disk_write_bytes_per_sec_max",
                "percent_disk_time_avg", "percent_disk_time_max",
                "avg_disk_sec_per_read_ms_avg", "avg_disk_sec_per_write_ms_avg",
                "current_disk_queue_length_avg", "p50_ms", "p95_ms", "p99_ms")) {
            $numbers[$name] = Get-CounterNumber $row $name
        }
        if ($numbers["disk_read_bytes_per_sec_max"] -le 0 -and
            $numbers["disk_write_bytes_per_sec_max"] -le 0) {
            throw "counter positive activity contract failed"
        }
        $p50 = $numbers["p50_ms"]
        $p95 = $numbers["p95_ms"]
        $p99 = $numbers["p99_ms"]
        if ($p95 -lt $p50 -or $p99 -lt $p95) {
            throw "counter latency percentile ordering failed"
        }
    }
    $rows
}
function Assert-RecoveryJournal($Journal) {
    if ($null -eq $Journal -or [string]$Journal.phase -ne "partition_created") {
        throw "recovery journal phase must be partition_created"
    }
    $events = @($Journal.events)
    $phases = @($events | ForEach-Object { [string]$_.phase })
    if (($phases -join ",") -ne "raw_validated,partition_created" -or
        $phases -contains "volume_published") {
        throw "recovery journal is not monotonic exact scratch evidence"
    }
}
function Assert-RecoveryVolumeCardinality([int]$Count) {
    if ($Count -ne 0) {
        throw "storage_provider_recovery volume refusal count=$Count"
    }
}
function Assert-OnlineStorageBinding($OnlineEvidence, [string]$ExpectedSerial,
    [UInt64]$ExpectedSize) {
    if ($null -eq $OnlineEvidence -or
        [string]$OnlineEvidence.run_id -notmatch '^run-\d+-\d+-\d+$' -or
        (Normalize-RamSharedText ([string]$OnlineEvidence.serial)) -ne
            (Normalize-RamSharedText $ExpectedSerial) -or
        [UInt64]$OnlineEvidence.size -ne $ExpectedSize) {
        throw "current Online evidence does not bind storage identity"
    }
}
function New-MatrixPackages {
    if ($PackageRevision -notmatch '^[0-9A-Za-z][0-9A-Za-z-]*$') {
        throw "PackageRevision is required and must be alphanumeric with optional hyphens"
    }
    if (-not (Test-Path (Join-Path $BasePackage "product-manifest.json"))) {
        throw "base product package missing"
    }
    foreach ($driverFile in @("ramshared.sys", "ramshared.inf", "ramshared.cat")) {
        if (-not (Test-Path (Join-Path $DriverPackage $driverFile))) {
            throw "driver package missing $driverFile"
        }
    }
    if (-not (Test-Path $WinsvcBinary -PathType Leaf)) {
        throw "winsvc candidate missing"
    }
    if (-not (Test-Path $BrokerBinary -PathType Leaf)) {
        throw "broker candidate missing"
    }
    New-Item -Force -ItemType Directory $PackageRoot | Out-Null
    foreach ($cell in $cells) {
        $name, $size, $sector, $qd, $maxIo = $cell
        if ([UInt64]$qd * [UInt64]$maxIo -gt 4MB) {
            throw "matrix in-flight limit exceeded cell=$name"
        }
        $root = Join-Path $PackageRoot $name
        if (Test-Path $root) { throw "immutable matrix package already exists: $root" }
        New-Item -Force -ItemType Directory $root | Out-Null
        Copy-Item (Join-Path $BasePackage "*") $root -Force
        Copy-Item $WinsvcBinary (Join-Path $root "ramshared-winsvc.exe") -Force
        Copy-Item $BrokerBinary (Join-Path $root "ramshared-winbroker.exe") -Force
        foreach ($driverFile in @("ramshared.sys", "ramshared.inf", "ramshared.cat")) {
            Copy-Item (Join-Path $DriverPackage $driverFile) `
                (Join-Path $root $driverFile) -Force
        }
        $winsvcPath = Join-Path $root "winsvc.toml"
        $winsvc = Get-Content $winsvcPath -Raw
        $winsvc = Set-RamSharedTomlInteger $winsvc "winsvc-$name" `
            "win_drive" "size_bytes" $size
        $winsvc = Set-RamSharedTomlInteger $winsvc "winsvc-$name" `
            "win_drive" "block_size" $sector
        $winsvc = Set-RamSharedTomlInteger $winsvc "winsvc-$name" `
            "win_drive" "queue_depth" $qd
        $winsvc = Set-RamSharedTomlInteger $winsvc "winsvc-$name" `
            "win_drive" "max_io_bytes" $maxIo
        [IO.File]::WriteAllText($winsvcPath, $winsvc, [Text.UTF8Encoding]::new($false))
        $brokerPath = Join-Path $root "broker.toml"
        $broker = Get-Content $brokerPath -Raw
        $broker = Set-RamSharedTomlInteger $broker "broker-$name" `
            "local_broker" "capacity_bytes" $size
        [IO.File]::WriteAllText($brokerPath, $broker, [Text.UTF8Encoding]::new($false))
        $effectiveWinsvc = Get-Content -LiteralPath $winsvcPath -Raw
        $effectiveBroker = Get-Content -LiteralPath $brokerPath -Raw
        Assert-EffectiveCellConfig $effectiveWinsvc $effectiveBroker $size $sector `
            $qd $maxIo $name

        $manifestPath = Join-Path $root "product-manifest.json"
        $manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
        $manifest.version = "0.1.2-matrix-$name-$PackageRevision"
        $manifest.start_policy = "demand"
        foreach ($artifact in $manifest.artifacts) {
            $artifact.sha256 = (Get-FileHash (Join-Path $root $artifact.relative_path) `
                    -Algorithm SHA256).Hash
        }
        Write-Json $manifest $manifestPath
        Assert-BinaryMatch $manifest $root
    }
}
function Assert-Admin {
    $p = [Security.Principal.WindowsPrincipal]::new(
        [Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "elevated administrator token required"
    }
}
function Get-ProductDisks {
    $observation = Invoke-BoundedStorageObservation "product_residue" "" 0 0
    @($observation.disks)
}
function Stop-Product {
    $consumer = Get-Service RamSharedWinSvc -ErrorAction SilentlyContinue
    if ($consumer -and $consumer.Status -ne "Stopped") {
        & sc.exe stop RamSharedWinSvc | Out-Null
        $deadline = (Get-Date).AddSeconds(30)
        do {
            Start-Sleep -Milliseconds 250
            $consumer = Get-Service RamSharedWinSvc -ErrorAction Stop
        } while ($consumer.Status -ne "Stopped" -and (Get-Date) -lt $deadline)
        if ($consumer.Status -ne "Stopped") {
            throw "consumer-first stop timeout state=$($consumer.Status)"
        }
    }
    $broker = Get-Service RamSharedBroker -ErrorAction SilentlyContinue
    if ($broker -and $broker.Status -ne "Stopped") {
        & sc.exe stop RamSharedBroker | Out-Null
        $deadline = (Get-Date).AddSeconds(15)
        do {
            Start-Sleep -Milliseconds 250
            $broker = Get-Service RamSharedBroker -ErrorAction Stop
        } while ($broker.Status -ne "Stopped" -and (Get-Date) -lt $deadline)
        if ($broker.Status -ne "Stopped") {
            throw "broker stop timeout state=$($broker.Status)"
        }
    }
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline -and @(Get-ProductDisks).Length -ne 0) {
        Start-Sleep -Milliseconds 250
    }
    if (@(Get-ProductDisks).Length -ne 0) { throw "consumer-first stop left residue" }
}
function Arm-Watchdog {
    [IO.File]::WriteAllText($watchdogMarker, (Get-Date).ToString("o"))
    $marker = $watchdogMarker.Replace("'", "''")
    $timeout = $watchdogTimeout.Replace("'", "''")
    $cleanup = (Join-Path $OutDir "watchdog-cleanup.json").Replace("'", "''")
    $allowShutdown = if ($AllowWatchdogShutdown) { '$true' } else { '$false' }
    Start-Process powershell.exe -WindowStyle Hidden -ArgumentList @(
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command",
        "& { while(Test-Path '$marker') { Start-Sleep 30; " +
        "if(-not (Test-Path '$marker')) { break }; " +
        "`$age=((Get-Date)-(Get-Item '$marker').LastWriteTime).TotalSeconds; " +
        "if(`$age -gt $watchdogStaleSeconds) { " +
        "[IO.File]::WriteAllText('$timeout',(Get-Date).ToString('o')); " +
        "`$null=& sc.exe stop RamSharedWinSvc; " +
        "`$deadline=(Get-Date).AddSeconds(30); " +
        "do { Start-Sleep -Milliseconds 250; `$consumer=Get-Service RamSharedWinSvc -ErrorAction SilentlyContinue } " +
        "while(`$consumer -and `$consumer.Status -ne 'Stopped' -and (Get-Date) -lt `$deadline); " +
        "if(`$consumer -and `$consumer.Status -eq 'Stopped') { `$null=& sc.exe stop RamSharedBroker }; " +
        "`$broker=Get-Service RamSharedBroker -ErrorAction SilentlyContinue; " +
        "`$state=[ordered]@{utc=(Get-Date).ToUniversalTime().ToString('o'); consumer=[string]`$consumer.Status; broker=[string]`$broker.Status; service_force_killed=`$false}; " +
        "[IO.File]::WriteAllText('$cleanup',(`$state|ConvertTo-Json)); " +
        "if($allowShutdown) { shutdown.exe /s /t 0 /f }; break } } }") | Out-Null
}
function Update-WatchdogHeartbeat {
    if (-not (Test-Path $watchdogMarker -PathType Leaf)) {
        throw "watchdog marker missing"
    }
    [IO.File]::SetLastWriteTime($watchdogMarker, (Get-Date))
}
function Assert-GpuReserve([UInt64]$Size) {
    $nvidiaSmi = Join-Path $env:SystemRoot "System32\nvidia-smi.exe"
    if (-not (Test-Path $nvidiaSmi -PathType Leaf)) {
        throw "insufficient GPU reserve: nvidia-smi unavailable"
    }
    $probe = Start-BoundedExternalProcess $nvidiaSmi @(
        "--query-gpu=memory.free", "--format=csv,noheader,nounits") 15
    if (-not $probe.completed) {
        throw "insufficient GPU reserve: nvidia-smi timeout"
    }
    $samples = @($probe.stdout -split "`r?`n" | Where-Object {
            -not [string]::IsNullOrWhiteSpace($_)
        })
    if ($samples.Length -ne 1 -or
        [string]$samples[0] -notmatch '^\s*(\d+)\s*$') {
        throw "insufficient GPU reserve: nvidia-smi unavailable"
    }
    $free = [UInt64]$matches[1] * 1MB
    if ($free -lt ($Size + $GpuReserveBytes)) {
        throw "insufficient GPU reserve: free=$free need=$($Size + $GpuReserveBytes)"
    }
}
function Assert-BinaryMatch($Manifest, [string]$Root) {
    foreach ($role in @("driver_sys", "broker_exe", "winsvc_exe")) {
        $a = $Manifest.artifacts | Where-Object role -eq $role
        if (-not $a) { throw "BINARY_MATCH missing manifest role=$role" }
        $source = Join-Path $Root $a.relative_path
        if ((Get-FileHash $source -Algorithm SHA256).Hash -ne $a.sha256) {
            throw "BINARY_MATCH package hash failed role=$role"
        }
    }
}
function Assert-LiveBinaryMatch($Manifest, [string]$Root) {
    $driver = Get-CimInstance Win32_SystemDriver |
        Where-Object Name -eq "ramshared"
    if (-not $driver -or $driver.State -ne "Running") {
        throw "BINARY_MATCH loaded driver is not Running"
    }
    $driverPath = ([string]$driver.PathName).Trim('"') -replace '^\\\?\?\\', ''
    if ($driverPath -like "\SystemRoot\*") {
        $driverPath = Join-Path $env:SystemRoot $driverPath.Substring(12)
    }
    $driverArtifact = $Manifest.artifacts | Where-Object role -eq "driver_sys"
    $loadedDriverHash = Get-Sha256 $driverPath
    $expectedDriverHash = Get-Sha256 (Join-Path $Root $driverArtifact.relative_path)
    if ($loadedDriverHash -ne $expectedDriverHash) {
        throw "BINARY_MATCH loaded driver hash mismatch"
    }
    $observed = [ordered]@{
        loaded_driver_path = $driverPath
        loaded_driver_sha256 = $loadedDriverHash
        expected_driver_sha256 = $expectedDriverHash
    }
    foreach ($pair in @(
            @("RamSharedBroker", "broker_exe"),
            @("RamSharedWinSvc", "winsvc_exe"))) {
        $service = Get-CimInstance Win32_Service -Filter "Name='$($pair[0])'"
        $process = Get-Process -Id $service.ProcessId
        $artifact = $Manifest.artifacts | Where-Object role -eq $pair[1]
        $loadedHash = Get-Sha256 $process.Path
        $expectedHash = Get-Sha256 (Join-Path $Root $artifact.relative_path)
        if ($loadedHash -ne $expectedHash) {
            throw "BINARY_MATCH live process mismatch service=$($pair[0])"
        }
        if ($pair[0] -eq "RamSharedBroker") {
            $observed.loaded_broker_path = $process.Path
            $observed.loaded_broker_sha256 = $loadedHash
            $observed.expected_broker_sha256 = $expectedHash
        } else {
            $observed.loaded_winsvc_path = $process.Path
            $observed.loaded_winsvc_sha256 = $loadedHash
            $observed.expected_winsvc_sha256 = $expectedHash
        }
    }
    [pscustomobject]$observed
}
function Get-WorkerExactStorageIdentity(
    [string]$ExpectedSerial,
    [UInt64]$ExpectedSize,
    [UInt32]$ExpectedSector,
    [switch]$RequireRaw
) {
    $normalizedSerial = Normalize-RamSharedText $ExpectedSerial
    if ($normalizedSerial -notmatch '^[0-9A-F]{16}$') {
        throw "worker expected serial is invalid"
    }
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $deadline = (Get-Date).AddSeconds(30)
    $physical = @()
    do {
        $physical = @(Get-PhysicalDisk -ErrorAction Stop | Where-Object {
                (Normalize-RamSharedText ([string]$_.FriendlyName)) -eq "RAMSHARE VRAMDISK" -and
                (Normalize-RamSharedText ([string]$_.SerialNumber)) -eq $normalizedSerial -and
                [UInt64]$_.Size -eq $ExpectedSize
            })
        if ($physical.Count -gt 1) {
            throw "exact physical identity count=$($physical.Count)"
        }
        if ($physical.Count -eq 0) { Start-Sleep -Milliseconds 250 }
    } while ($physical.Count -eq 0 -and (Get-Date) -lt $deadline)
    if ($physical.Count -ne 1) {
        throw "exact physical identity count=0 after 30s"
    }
    if ((Normalize-RamSharedText ([string]$physical[0].BusType)) -ne "VIRTUAL" -or
        (Normalize-RamSharedText ([string]$physical[0].MediaType)) -ne "SSD" -or
        [UInt64]$physical[0].SpindleSpeed -ne 0) {
        throw "BusType Virtual MediaType SSD SpindleSpeed 0 identity failed"
    }
    $disk = @(Get-Disk -Number ([int]$physical[0].DeviceId) -ErrorAction Stop)
    if ($disk.Count -ne 1 -or
        (Normalize-RamSharedText ([string]$disk[0].FriendlyName)) -ne "RAMSHARE VRAMDISK" -or
        (Normalize-RamSharedText ([string]$disk[0].SerialNumber)) -ne $normalizedSerial -or
        [UInt64]$disk[0].Size -ne $ExpectedSize -or
        [UInt32]$disk[0].LogicalSectorSize -ne $ExpectedSector -or
        [UInt32]$disk[0].PhysicalSectorSize -ne $ExpectedSector -or
        $disk[0].IsBoot -or $disk[0].IsSystem) {
        throw "exact size/sector/current-run safety identity failed"
    }
    $partitions = @(Get-Partition -DiskNumber ([int]$disk[0].Number) -ErrorAction Stop)
    if ($RequireRaw -and
        ([string]$disk[0].PartitionStyle -ne "RAW" -or $partitions.Count -ne 0)) {
        throw "current-run disk must be RAW with zero partitions before mutation"
    }
    $timer.Stop()
    [pscustomobject]@{
        physical = $physical[0]
        disk = $disk[0]
        partitions = $partitions
        enumeration_ms = [math]::Round($timer.Elapsed.TotalMilliseconds, 3)
    }
}
function Write-StorageJournal($Journal, [string]$Path, [string]$Phase) {
    $valid = @("raw_validated", "partition_created", "volume_published")
    if ($Phase -notin $valid) { throw "invalid storage journal phase=$Phase" }
    $previous = @($Journal.events | ForEach-Object { [string]$_.phase })
    if ($previous.Count -ne ($valid.IndexOf($Phase))) {
        throw "storage journal phase transition is invalid phase=$Phase"
    }
    $Journal.phase = $Phase
    $Journal.events += [ordered]@{
        phase = $Phase
        utc = (Get-Date).ToUniversalTime().ToString("o")
    }
    Write-Json $Journal $Path 10
}
function Invoke-StorageProviderWorkerMode {
    Assert-Admin
    if ($WorkerResult -eq "" -or $WorkerCell -eq "" -or
        $WorkerExpectedRunId -notmatch '^run-\d+-\d+-\d+$' -or
        $WorkerExpectedSerial -notmatch '^[0-9A-Fa-f]{16}$' -or
        $Letter -notmatch '^[D-Z]$' -or $WorkerSize -eq 0 -or
        $WorkerSector -notin @(512, 4096)) {
        throw "invalid storage provider worker arguments"
    }
    New-Item -ItemType Directory -Force (Split-Path $WorkerResult -Parent) | Out-Null
    $journalPath = "$WorkerResult.journal.json"
    try {
        $identity = Get-WorkerExactStorageIdentity $WorkerExpectedSerial `
            $WorkerSize $WorkerSector -RequireRaw
        $physical = $identity.physical
        $disk = $identity.disk
        $journal = [ordered]@{
            schema_version = $evidenceSchemaVersion
            cell = $WorkerCell
            run_id = $WorkerExpectedRunId
            disk_number = [int]$disk.Number
            serial = Normalize-RamSharedText ([string]$disk.SerialNumber)
            size = [UInt64]$disk.Size
            sector = [UInt32]$disk.LogicalSectorSize
            before_partition_style = [string]$disk.PartitionStyle
            phase = ""
            events = @()
            enumeration_ms = [double]$identity.enumeration_ms
        }
        Write-StorageJournal $journal $journalPath "raw_validated"
        Initialize-Disk -Number $disk.Number -PartitionStyle GPT -ErrorAction Stop | Out-Null
        New-Partition -DiskNumber $disk.Number -UseMaximumSize `
            -DriveLetter $Letter -ErrorAction Stop | Out-Null
        Write-StorageJournal $journal $journalPath "partition_created"
        Format-Volume -DriveLetter $Letter -FileSystem NTFS `
            -NewFileSystemLabel RAMSHARE -Confirm:$false -Force `
            -ErrorAction Stop | Out-Null
        $partition = @(Get-Partition -DriveLetter $Letter -ErrorAction Stop)
        if ($partition.Count -ne 1 -or $partition[0].DiskNumber -ne $disk.Number) {
            throw "matrix volume partition identity mismatch"
        }
        $volume = @(Get-Volume -DriveLetter $Letter -ErrorAction Stop)
        if ($volume.Count -ne 1 -or $volume[0].FileSystem -ne "NTFS" -or
            [UInt64]$volume[0].Size -eq 0) {
            throw "matrix volume publication failed"
        }
        Write-StorageJournal $journal $journalPath "volume_published"
        Write-Json ([ordered]@{
                schema_version = $evidenceSchemaVersion
                status = "PASS"
                cell = $WorkerCell
                run_id = $WorkerExpectedRunId
                disk_number = [int]$disk.Number
                serial = [string]$journal.serial
                size = [UInt64]$disk.Size
                logical_sector = [UInt32]$disk.LogicalSectorSize
                physical_sector = [UInt32]$disk.PhysicalSectorSize
                bus = [string]$physical.BusType
                media = [string]$physical.MediaType
                spindle_speed = [UInt64]$physical.SpindleSpeed
                before_partition_style = [string]$journal.before_partition_style
                phase = [string]$journal.phase
                volume_size = [UInt64]$volume[0].Size
                volume_size_remaining = [UInt64]$volume[0].SizeRemaining
                enumeration_ms = [double]$journal.enumeration_ms
            }) $WorkerResult
        exit 0
    } catch {
        Write-Json ([ordered]@{
                schema_version = $evidenceSchemaVersion
                status = "RED"
                failure_phase = "storage_provider_worker"
                cell = $WorkerCell
                error = $_.Exception.Message
            }) $WorkerResult
        exit 1
    }
}
function Invoke-StorageProviderRecoveryWorkerMode {
    Assert-Admin
    if ($WorkerResult -eq "" -or $WorkerCell -eq "" -or
        $WorkerDiskNumber -lt 0 -or $WorkerSerial -notmatch '^[0-9A-Fa-f]{16}$' -or
        $WorkerJournalPath -eq "" -or -not (Test-Path $WorkerJournalPath -PathType Leaf) -or
        $Letter -notmatch '^[D-Z]$' -or $WorkerSize -eq 0 -or
        $WorkerSector -notin @(512, 4096)) {
        throw "invalid storage provider recovery arguments"
    }
    try {
        $journal = Get-Content -LiteralPath $WorkerJournalPath -Raw | ConvertFrom-Json
        Assert-RecoveryJournal $journal
        if ((Normalize-RamSharedText ([string]$journal.serial)) -ne
            (Normalize-RamSharedText $WorkerSerial) -or
            [int]$journal.disk_number -ne $WorkerDiskNumber -or
            [UInt64]$journal.size -ne $WorkerSize -or
            [UInt32]$journal.sector -ne $WorkerSector) {
            throw "storage_provider_recovery journal identity refusal"
        }
        $disk = @(Get-Disk -Number $WorkerDiskNumber -ErrorAction Stop)
        $physical = @(Get-PhysicalDisk -ErrorAction Stop | Where-Object {
                [int]$_.DeviceId -eq $WorkerDiskNumber
            })
        if ($disk.Count -ne 1 -or $physical.Count -ne 1 -or
            (Normalize-RamSharedText ([string]$disk[0].FriendlyName)) -ne "RAMSHARE VRAMDISK" -or
            (Normalize-RamSharedText ([string]$physical[0].FriendlyName)) -ne "RAMSHARE VRAMDISK" -or
            (Normalize-RamSharedText ([string]$disk[0].SerialNumber)) -ne (Normalize-RamSharedText $WorkerSerial) -or
            (Normalize-RamSharedText ([string]$physical[0].SerialNumber)) -ne (Normalize-RamSharedText $WorkerSerial) -or
            [UInt64]$disk[0].Size -ne $WorkerSize -or
            [UInt32]$disk[0].LogicalSectorSize -ne $WorkerSector -or
            [UInt32]$disk[0].PhysicalSectorSize -ne $WorkerSector -or
            (Normalize-RamSharedText ([string]$physical[0].BusType)) -ne "VIRTUAL" -or
            (Normalize-RamSharedText ([string]$physical[0].MediaType)) -ne "SSD" -or
            [UInt64]$physical[0].SpindleSpeed -ne 0 -or
            $disk[0].IsBoot -or $disk[0].IsSystem) {
            throw "storage_provider_recovery exact disk refusal"
        }
        $partitions = @(Get-Partition -DiskNumber $WorkerDiskNumber -ErrorAction Stop)
        if ($partitions.Count -ne 1 -or
            [string]$partitions[0].DriveLetter -ne $Letter) {
            throw "storage_provider_recovery partition refusal"
        }
        $volumes = @($partitions | Get-Volume -ErrorAction Stop)
        Assert-RecoveryVolumeCardinality $volumes.Count
        Clear-Disk -Number $WorkerDiskNumber -RemoveData -RemoveOEM `
            -Confirm:$false -ErrorAction Stop
        $after = Get-Disk -Number $WorkerDiskNumber -ErrorAction Stop
        if ($after.PartitionStyle -ne "RAW" -or
            @(Get-Partition -DiskNumber $WorkerDiskNumber `
                    -ErrorAction SilentlyContinue).Count -ne 0) {
            throw "storage_provider_recovery did not restore RAW"
        }
        Write-Json ([ordered]@{
                schema_version = $evidenceSchemaVersion
                status = "PASS"
                failure_phase = "storage_provider_recovery"
                cell = $WorkerCell
                disk_number = $WorkerDiskNumber
                serial = $WorkerSerial
                size = $WorkerSize
                journal_phase = [string]$journal.phase
                volume_count = $volumes.Count
                before = "partial GPT without volume"
                action = "Clear-Disk exact volatile scratch LUN"
                after = "RAW"
                service_force_killed = $false
            }) $WorkerResult
        exit 0
    } catch {
        Write-Json ([ordered]@{
                schema_version = $evidenceSchemaVersion
                status = "RED"
                failure_phase = "storage_provider_recovery"
                cell = $WorkerCell
                error = $_.Exception.Message
            }) $WorkerResult
        exit 1
    }
}
function ConvertTo-StorageObservationDisk($Physical, $Disk) {
    [ordered]@{
        disk_number = [int]$Disk.Number
        friendly_name = Normalize-RamSharedText ([string]$Disk.FriendlyName)
        serial = Normalize-RamSharedText ([string]$Disk.SerialNumber)
        size = [UInt64]$Disk.Size
        logical_sector = [UInt32]$Disk.LogicalSectorSize
        physical_sector = [UInt32]$Disk.PhysicalSectorSize
        partition_style = [string]$Disk.PartitionStyle
        is_boot = [bool]$Disk.IsBoot
        is_system = [bool]$Disk.IsSystem
        bus = if ($Physical) { Normalize-RamSharedText ([string]$Physical.BusType) } else { "" }
        media = if ($Physical) { Normalize-RamSharedText ([string]$Physical.MediaType) } else { "" }
        spindle_speed = if ($Physical) { [UInt64]$Physical.SpindleSpeed } else { [UInt64]0 }
    }
}
function Invoke-StorageProviderObservationWorkerMode {
    Assert-Admin
    if ($WorkerResult -eq "" -or $WorkerObservation -eq "" -or
        $Letter -notmatch '^[D-Z]$') {
        throw "invalid storage observation worker arguments"
    }
    New-Item -ItemType Directory -Force (Split-Path $WorkerResult -Parent) | Out-Null
    try {
        $result = [ordered]@{
            schema_version = $evidenceSchemaVersion
            status = "PASS"
            operation = $WorkerObservation
            utc = (Get-Date).ToUniversalTime().ToString("o")
        }
        switch ($WorkerObservation) {
            "product_residue" {
                $rows = @()
                foreach ($physical in @(Get-PhysicalDisk -ErrorAction Stop | Where-Object {
                            (Normalize-RamSharedText ([string]$_.FriendlyName)) -like "RAMSHARE*"
                        })) {
                    $disks = @(Get-Disk -Number ([int]$physical.DeviceId) -ErrorAction Stop)
                    if ($disks.Count -ne 1) {
                        throw "product residue disk cardinality is invalid"
                    }
                    $rows += [pscustomobject](ConvertTo-StorageObservationDisk $physical $disks[0])
                }
                $result.disks = $rows
            }
            "target_letter" {
                $partitions = @(Get-Partition -DriveLetter $Letter -ErrorAction SilentlyContinue)
                if ($partitions.Count -gt 1) {
                    throw "target letter partition cardinality is ambiguous"
                }
                $result.occupied = ($partitions.Count -eq 1)
                if ($partitions.Count -eq 1) {
                    $disks = @(Get-Disk -Number ([int]($partitions[0].DiskNumber) ) -ErrorAction Stop)
                    if ($disks.Count -ne 1) {
                        throw "target letter disk cardinality is invalid"
                    }
                    $physical = @(Get-PhysicalDisk -ErrorAction Stop | Where-Object {
                            [int]$_.DeviceId -eq [int]$disks[0].Number
                        })
                    if ($physical.Count -gt 1) {
                        throw "target letter physical identity is ambiguous"
                    }
                    $result.disk = ConvertTo-StorageObservationDisk `
                        $(if ($physical.Count -eq 1) { $physical[0] } else { $null }) $disks[0]
                }
            }
            "final_active" {
                if ($WorkerExpectedSerial -notmatch '^[0-9A-Fa-f]{16}$' -or
                    $WorkerSize -eq 0 -or $WorkerSector -notin @(512, 4096)) {
                    throw "invalid final active identity arguments"
                }
                $identity = Get-WorkerExactStorageIdentity $WorkerExpectedSerial `
                    $WorkerSize $WorkerSector
                $result.disk = ConvertTo-StorageObservationDisk `
                    $identity.physical $identity.disk
                $result.partition_count = @($identity.partitions).Count
            }
            default { throw "unsupported storage observation operation=$WorkerObservation" }
        }
        Write-Json $result $WorkerResult 10
        exit 0
    } catch {
        Write-Json ([ordered]@{
                schema_version = $evidenceSchemaVersion
                status = "RED"
                operation = $WorkerObservation
                failure_phase = "storage_provider_observation"
                error = $_.Exception.Message
            }) $WorkerResult
        exit 1
    }
}
function Invoke-BoundedProcessStart(
    [Diagnostics.ProcessStartInfo]$Start,
    [int]$TimeoutSeconds
) {
    $Start.UseShellExecute = $false
    $Start.RedirectStandardOutput = $true
    $Start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $Start
    $completed = $false
    $processTreeTerminated = $false
    $stdoutTask = $null
    $stderrTask = $null
    try {
        if (-not $process.Start()) { throw "bounded child failed to start" }
        # Begin both reads before waiting. ReadToEndAsync keeps each redirected pipe
        # draining on a CLR task so a verbose child cannot block on a full pipe.
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
        while (-not $process.HasExited -and (Get-Date) -lt $deadline) {
            $null = $process.WaitForExit(100)
        }
        $completed = $process.HasExited
        if (-not $completed) {
            & (Join-Path $env:SystemRoot "System32\taskkill.exe") `
                /PID ([string]$process.Id) /T /F 2>&1 | Out-Null
            $taskkillExit = $LASTEXITCODE
            $processTreeTerminated = $process.WaitForExit(5000)
            if (-not $processTreeTerminated -or
                ($taskkillExit -ne 0 -and -not $process.HasExited)) {
                throw "bounded child process tree termination failed pid=$($process.Id)"
            }
        }
        if ($process.HasExited) { $null = $process.WaitForExit(5000) }
        $drained = [Threading.Tasks.Task]::WaitAll(
            [Threading.Tasks.Task[]]@($stdoutTask, $stderrTask), 5000)
        if (-not $drained) {
            throw "bounded child redirected stream drain timeout pid=$($process.Id)"
        }
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        [pscustomobject]@{
            completed = $completed
            exit_code = if ($completed) { $process.ExitCode } else { $null }
            stdout = $stdout
            stderr = $stderr
            stdout_bytes = [Text.Encoding]::UTF8.GetByteCount($stdout)
            stderr_bytes = [Text.Encoding]::UTF8.GetByteCount($stderr)
            stdout_drained = $drained
            stderr_drained = $drained
            process_tree_terminated = $processTreeTerminated
        }
    } finally {
        if ($process) {
            $process.Dispose()
        }
    }
}
function Start-BoundedExternalProcess(
    [string]$Executable,
    [string[]]$Arguments,
    [int]$TimeoutSeconds
) {
    if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
        throw "bounded executable missing: $Executable"
    }
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable
    $start.Arguments = $Arguments -join " "
    Invoke-BoundedProcessStart $start $TimeoutSeconds
}
function Start-BoundedPowerShellChild(
    [string]$ScriptPath,
    [string[]]$Arguments,
    [int]$TimeoutSeconds
) {
    Start-BoundedExternalProcess (Join-Path $PSHOME "powershell.exe") `
        (@("-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
                (Quote-ProcessArgument $ScriptPath)) + $Arguments) $TimeoutSeconds
}
function Start-BoundedHarnessChild([string[]]$Arguments, [int]$TimeoutSeconds) {
    Start-BoundedPowerShellChild $script:HarnessPath $Arguments $TimeoutSeconds
}
function Invoke-BoundedStorageObservation(
    [string]$Operation,
    [string]$ExpectedSerial,
    [UInt64]$ExpectedSize,
    [UInt32]$ExpectedSector
) {
    $resultPath = Join-Path $OutDir (
        "storage-observation-{0}-{1}.json" -f $Operation,
        [guid]::NewGuid().ToString("N"))
    $arguments = @(
        "-StorageProviderObservationWorker",
        "-WorkerObservation", $Operation,
        "-Letter", $Letter,
        "-WorkerResult", (Quote-ProcessArgument $resultPath),
        "-OutDir", (Quote-ProcessArgument $OutDir)
    )
    if (-not [string]::IsNullOrWhiteSpace($ExpectedSerial)) {
        $arguments += @("-WorkerExpectedSerial", $ExpectedSerial,
            "-WorkerSize", [string]$ExpectedSize,
            "-WorkerSector", [string]$ExpectedSector)
    }
    $run = Start-BoundedHarnessChild $arguments $storageObservationTimeoutSeconds
    if (-not $run.completed) {
        throw "storage observation timeout operation=$Operation timeout=$storageObservationTimeoutSeconds"
    }
    if ($run.exit_code -ne 0 -or -not (Test-Path $resultPath -PathType Leaf)) {
        throw "storage observation failed operation=$Operation exit=$($run.exit_code) stderr=$($run.stderr)"
    }
    $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json -ErrorAction Stop
    if ([string]$result.status -ne "PASS" -or [string]$result.operation -ne $Operation) {
        throw "storage observation result is malformed operation=$Operation"
    }
    $result
}
function Assert-TargetLetterAvailable {
    $observation = Invoke-BoundedStorageObservation "target_letter" "" 0 0
    $script:targetLetterPreflight = $observation
    if ([bool]$observation.occupied) {
        throw "foreign volume occupies target letter"
    }
}
function Invoke-BoundedController([string[]]$Arguments) {
    $run = Start-BoundedExternalProcess $Controller $Arguments $controllerTimeoutSeconds
    if (-not $run.completed) {
        throw "controller timeout seconds=$controllerTimeoutSeconds"
    }
    if ($run.exit_code -ne 0) {
        throw "controller failed exit=$($run.exit_code) stderr=$($run.stderr)"
    }
    $run
}
function Get-ManifestStorageConfig([string]$ManifestPath) {
    if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
        throw "manifest is missing: $ManifestPath"
    }
    $root = Split-Path -Parent $ManifestPath
    $winsvcPath = Join-Path $root "winsvc.toml"
    if (-not (Test-Path -LiteralPath $winsvcPath -PathType Leaf)) {
        throw "manifest winsvc config is missing"
    }
    $winsvc = ConvertFrom-RamSharedToml (Get-Content -LiteralPath $winsvcPath -Raw) `
        "terminal-winsvc"
    foreach ($key in @("win_drive.size_bytes", "win_drive.block_size")) {
        if (-not $winsvc.ContainsKey($key) -or [UInt64]$winsvc[$key] -eq 0) {
            throw "terminal winsvc config is invalid key=$key"
        }
    }
    [pscustomobject]@{
        root = $root
        size = [UInt64]$winsvc["win_drive.size_bytes"]
        sector = [UInt32]$winsvc["win_drive.block_size"]
    }
}
function Start-ManifestProduct([string]$ManifestPath) {
    $config = Get-ManifestStorageConfig $ManifestPath
    $manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json -ErrorAction Stop
    Invoke-BoundedController @("install", "--manifest", (Quote-ProcessArgument $ManifestPath)) | Out-Null
    $serviceStartUtc = (Get-Date).ToUniversalTime()
    Start-Service RamSharedWinSvc -ErrorAction Stop
    (Get-Service RamSharedWinSvc -ErrorAction Stop).WaitForStatus(
        "Running", [timespan]::FromSeconds(30))
    $online = Get-CurrentRunOnlineEvidence $serviceStartUtc $config.size
    $live = Assert-LiveBinaryMatch $manifest $config.root
    $identity = Assert-FinalProductState $online $config.sector
    [pscustomobject]@{
        online = $online
        live_hashes = $live
        identity = $identity
        config = $config
    }
}
function Invoke-BoundedStorageProvider(
    [string]$Cell,
    [UInt64]$Size,
    [UInt32]$Sector,
    $OnlineEvidence
) {
    Assert-OnlineStorageBinding $OnlineEvidence ([string]$OnlineEvidence.serial) $Size
    if ([string]$OnlineEvidence.serial -notmatch '^[0-9A-Fa-f]{16}$') {
        throw "current Online evidence is invalid before storage mutation"
    }
    $resultPath = Join-Path $OutDir "$Cell-storage-provider.json"
    $journalPath = "$resultPath.journal.json"
    $arguments = @(
        "-StorageProviderWorker",
        "-WorkerCell", $Cell,
        "-WorkerSize", [string]$Size,
        "-WorkerSector", [string]$Sector,
        "-WorkerExpectedSerial", ([string]$OnlineEvidence.serial),
        "-WorkerExpectedRunId", ([string]$OnlineEvidence.run_id),
        "-Letter", $Letter,
        "-WorkerResult", (Quote-ProcessArgument $resultPath),
        "-OutDir", (Quote-ProcessArgument $OutDir)
    )
    $run = Start-BoundedHarnessChild $arguments $storageProviderTimeoutSeconds
    if ($run.completed -and $run.exit_code -eq 0 -and
        (Test-Path $resultPath -PathType Leaf)) {
        $result = Get-Content $resultPath -Raw | ConvertFrom-Json -ErrorAction Stop
        if ([string]$result.status -ne "PASS" -or
            [string]$result.run_id -ne [string]$OnlineEvidence.run_id -or
            (Normalize-RamSharedText ([string]$result.serial)) -ne
                (Normalize-RamSharedText ([string]$OnlineEvidence.serial)) -or
            [UInt64]$result.size -ne $Size -or
            [UInt32]$result.logical_sector -ne $Sector -or
            [UInt32]$result.physical_sector -ne $Sector -or
            [string]$result.phase -ne "volume_published") {
            throw "storage provider result did not bind current Online identity"
        }
        return $result
    }
    if (-not $run.completed) {
        $journal = if (Test-Path $journalPath -PathType Leaf) {
            Get-Content $journalPath -Raw | ConvertFrom-Json
        } else { $null }
        Write-Json ([ordered]@{
                schema_version = $evidenceSchemaVersion
                status = "RED"
                failure_phase = "storage_provider_timeout"
                timeout_seconds = $storageProviderTimeoutSeconds
                cell = $Cell
                orchestration_child_terminated = $true
                service_force_killed = $false
                stdout = $run.stdout
                stderr = $run.stderr
            }) $resultPath
        if ($journal) {
            Assert-RecoveryJournal $journal
            $recoveryPath = Join-Path $OutDir `
                "$Cell-storage-provider-recovery.json"
            $recoveryArguments = @(
                "-StorageProviderRecoveryWorker",
                "-WorkerCell", $Cell,
                "-WorkerSize", [string]$journal.size,
                "-WorkerSector", [string]$Sector,
                "-WorkerDiskNumber", [string]$journal.disk_number,
                "-WorkerSerial", $journal.serial,
                "-WorkerJournalPath", (Quote-ProcessArgument $journalPath),
                "-Letter", $Letter,
                "-WorkerResult", (Quote-ProcessArgument $recoveryPath),
                "-OutDir", (Quote-ProcessArgument $OutDir)
            )
            $recovery = Start-BoundedHarnessChild $recoveryArguments `
                $storageProviderTimeoutSeconds
            if (-not $recovery.completed -or $recovery.exit_code -ne 0) {
                throw "storage_provider_timeout and recovery failed cell=$Cell"
            }
        }
        throw "storage_provider_timeout cell=$Cell timeout=$storageProviderTimeoutSeconds"
    }
    throw "storage provider worker failed cell=$Cell exit=$($run.exit_code) stderr=$($run.stderr)"
}
function Get-NearestRank([double[]]$Values, [double]$Percentile) {
    if ($Values.Count -eq 0) { return 0 }
    $sorted = @($Values | Sort-Object)
    $index = [math]::Max(0, [math]::Ceiling($Percentile * $sorted.Count) - 1)
    [double]$sorted[$index]
}
function Get-RegressionVerdict(
    [object]$PriorThroughput,
    [object]$PriorLatency,
    [double]$CurrentThroughput,
    [double]$CurrentLatency
) {
    if ($null -eq $PriorThroughput -or $null -eq $PriorLatency) {
        return "BASELINE"
    }
    $priorThroughputValue = [double]$PriorThroughput
    $priorLatencyValue = [double]$PriorLatency
    $throughputRegression = (($priorThroughputValue - $CurrentThroughput) /
        $priorThroughputValue) * 100
    $latencyRatio = $CurrentLatency / $priorLatencyValue
    if ($throughputRegression -gt $throughput_regression_red_pct -or
        $latencyRatio -gt 2) {
        return "RED"
    }
    if ($throughputRegression -ge $throughput_regression_yellow_pct) {
        return "YELLOW"
    }
    "PASS"
}
function Get-ComparisonVerdict(
    [bool]$BaselineWasSupplied,
    [bool]$BaselineIsQualified,
    $Prior,
    [string]$CurrentFingerprint,
    [double]$CurrentThroughput,
    [double]$CurrentLatency
) {
    if (-not $BaselineWasSupplied) { return "BASELINE" }
    if (-not $BaselineIsQualified -or -not $Prior -or
        [string]$Prior.platform_fingerprint -ne $CurrentFingerprint) {
        return "INCOMPARABLE"
    }
    Get-RegressionVerdict ([double]$Prior.median_mib_per_sec) `
        ([double]$Prior.p99_latency_ms) $CurrentThroughput $CurrentLatency
}
function Get-RepositoryContext {
    $repoRoot = [IO.Path]::GetFullPath((Join-Path (Split-Path $script:HarnessPath -Parent) `
            "..\.."))
    if ($SourceCommit) {
        if ($SourceCommit -notmatch '^[0-9a-fA-F]{40}$' -or
            $SourceTreeState -notin @("clean", "dirty") -or
            $SourceDirtyEntryCount -lt 0 -or
            ($SourceTreeState -eq "clean" -and $SourceDirtyEntryCount -ne 0) -or
            ($SourceTreeState -eq "dirty" -and $SourceDirtyEntryCount -eq 0)) {
            throw "supplied repository context is inconsistent"
        }
        return [ordered]@{
            root = $repoRoot
            commit = $SourceCommit.ToLowerInvariant()
            dirty = ($SourceTreeState -eq "dirty")
            dirty_entry_count = $SourceDirtyEntryCount
            source = "explicit invocation"
        }
    }
    if (-not (Get-Command git.exe -ErrorAction SilentlyContinue)) {
        throw "repository revision unavailable: supply SourceCommit/SourceTreeState"
    }
    $commit = @(& git.exe -C $repoRoot rev-parse HEAD 2>$null)
    if ($LASTEXITCODE -ne 0 -or $commit.Count -ne 1 -or
        $commit[0] -notmatch '^[0-9a-fA-F]{40}$') {
        throw "repository revision unavailable"
    }
    $status = @(& git.exe -C $repoRoot status --porcelain=v1 2>$null)
    if ($LASTEXITCODE -ne 0) { throw "repository dirty state unavailable" }
    [ordered]@{
        root = $repoRoot
        commit = [string]$commit[0]
        dirty = ($status.Count -gt 0)
        dirty_entry_count = $status.Count
        source = "git.exe"
    }
}
function Get-SanitizedInvocation {
    $arguments = [ordered]@{}
    foreach ($entry in $script:InvocationParameters.GetEnumerator() | Sort-Object Key) {
        $arguments[$entry.Key] = if ($entry.Key -match 'secret|token|password|key') {
            "<redacted>"
        } else { [string]$entry.Value }
    }
    [ordered]@{
        executable = (Join-Path $PSHOME "powershell.exe")
        script = $script:HarnessPath
        arguments = $arguments
    }
}
function Get-HostContext {
    $os = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
    $computer = Get-CimInstance Win32_ComputerSystem -ErrorAction Stop
    $processors = @(Get-CimInstance Win32_Processor -ErrorAction Stop |
        Select-Object Name, NumberOfCores, NumberOfLogicalProcessors)
    $video = @(Get-CimInstance Win32_VideoController -ErrorAction Stop |
        Select-Object Name, DriverVersion, AdapterRAM, PNPDeviceID)
    $nvidiaSmi = Join-Path $env:SystemRoot "System32\nvidia-smi.exe"
    $gpuCondition = @()
    if (Test-Path $nvidiaSmi -PathType Leaf) {
        $probe = Start-BoundedExternalProcess $nvidiaSmi @(
            "--query-gpu=name,driver_version,memory.total,memory.used,memory.free",
            "--format=csv,noheader,nounits") 15
        if (-not $probe.completed) { throw "GPU context collection timed out" }
        $gpuCondition = @($probe.stdout -split "`r?`n" | Where-Object {
                -not [string]::IsNullOrWhiteSpace($_)
            })
        if ($gpuCondition.Count -ne 1 -or $gpuCondition[0] -notmatch ',') {
            throw "GPU context collection failed"
        }
    }
    [ordered]@{
        computer_name = $env:COMPUTERNAME
        windows_caption = [string]$os.Caption
        windows_version = [string]$os.Version
        windows_build = [string]$os.BuildNumber
        windows_ubr = [int](Get-ItemProperty `
                'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' `
                -Name UBR -ErrorAction Stop).UBR
        total_physical_memory = [UInt64]$computer.TotalPhysicalMemory
        processors = $processors
        video_controllers = $video
        nvidia_smi = [string[]]$gpuCondition
    }
}
function Get-CellFingerprint($HostContext, [string]$Cell, [UInt64]$Size,
    [UInt32]$Sector, [int]$QueueDepth, [UInt64]$MaxIo) {
    $fields = [ordered]@{
        schema_version = $evidenceSchemaVersion
        harness_behavior_revision = $harnessBehaviorRevision
        windows_version = $HostContext.windows_version
        windows_build = $HostContext.windows_build
        windows_ubr = $HostContext.windows_ubr
        total_physical_memory = $HostContext.total_physical_memory
        video_controllers = $HostContext.video_controllers
        driver_interface = "ramshared-storport-v1"
        cell = $Cell
        size = $Size
        sector = $Sector
        qd = $QueueDepth
        max_io_bytes = $MaxIo
        workloads = $workloads
        workload_revision = $harnessBehaviorRevision
        warmup = "counter-probe-5s"
        runs = $Runs
    }
    [ordered]@{
        sha256 = Get-PlatformFingerprint $fields
        fields = $fields
    }
}
function Get-CurrentRunOnlineEvidence([DateTime]$StartedUtc, [UInt64]$ExpectedSize) {
    $deadline = (Get-Date).AddSeconds(75)
    $evidenceRoot = "C:\ProgramData\RamShared\evidence\winsvc.jsonl"
    do {
        $service = Get-CimInstance Win32_Service -Filter "Name='RamSharedWinSvc'" `
            -ErrorAction Stop
        if ([string]$service.State -ne "Running" -or [uint32]$service.ProcessId -eq 0) {
            throw "current winsvc run is not Running while waiting for Online evidence"
        }
        $pid = [uint32]$service.ProcessId
        $runFiles = @(Get-ChildItem -LiteralPath $evidenceRoot `
                -Filter "run-$pid-*.jsonl" -ErrorAction SilentlyContinue | Where-Object {
                    $_.LastWriteTimeUtc -ge $StartedUtc.AddSeconds(-2)
                })
        if ($runFiles.Count -gt 1) {
            throw "ambiguous current winsvc run evidence"
        }
        if ($runFiles.Count -eq 1) {
            $rows = @(Get-Content -LiteralPath $runFiles[0].FullName -ErrorAction Stop |
                    Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
                    ForEach-Object { $_ | ConvertFrom-Json -ErrorAction Stop })
            if (@($rows | Where-Object {
                    [string]$_.phase -eq "FailedSafe" -and [uint32]$_.pid -eq $pid
                }).Count -ne 0) {
                throw "current winsvc run entered FailedSafe before Online"
            }
            $online = @($rows | Where-Object {
                    [string]$_.phase -eq "Online" -and [uint32]$_.pid -eq $pid
                })
            if ($online.Count -gt 1) {
                throw "current winsvc Online evidence is ambiguous"
            }
            if ($online.Count -eq 1) {
                $row = $online[0]
                if ([string]$row.run_id -notmatch '^run-\d+-\d+-\d+$' -or
                    $runFiles[0].Name -ne "$([string]$row.run_id).jsonl" -or
                    (Normalize-RamSharedText ([string]$row.lun_vendor)) -ne "RAMSHARE" -or
                    (Normalize-RamSharedText ([string]$row.lun_product)) -ne "VRAMDISK" -or
                    (Normalize-RamSharedText ([string]$row.lun_serial)) -notmatch '^[0-9A-F]{16}$' -or
                    [UInt64]$row.lun_size_bytes -ne $ExpectedSize) {
                    throw "current winsvc Online identity is invalid"
                }
                return [pscustomobject]@{
                    run_id = [string]$row.run_id
                    pid = $pid
                    serial = Normalize-RamSharedText ([string]$row.lun_serial)
                    size = [UInt64]$row.lun_size_bytes
                    evidence_path = $runFiles[0].FullName
                    online_utc_ms = [UInt64]$row.ts_utc_ms
                }
            }
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw "current winsvc run did not reach Online within 75 seconds"
}
function Normalize-PagefileEntry([string]$Value) {
    if ([string]::IsNullOrWhiteSpace($Value)) { return $null }
    (($Value -replace '/', '\\') -replace '\s+', ' ').Trim().ToUpperInvariant()
}
function Get-NormalizedPagefilePreflight(
    [string[]]$ActiveEntries,
    [string[]]$ConfiguredEntries,
    [string]$TargetLetter
) {
    $letter = $TargetLetter.Trim().TrimEnd(':').ToUpperInvariant()
    if ($letter -notmatch '^[A-Z]$') {
        throw "invalid product-volume pagefile target letter"
    }
    $active = @($ActiveEntries | ForEach-Object {
            Normalize-PagefileEntry ([string]$_)
        } | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            Sort-Object -Unique)
    $configured = @($ConfiguredEntries | ForEach-Object {
            Normalize-PagefileEntry ([string]$_)
        } | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            Sort-Object -Unique)
    $allEntries = @($active) + @($configured)
    $union = @($allEntries | Sort-Object -Unique)
    $target = '^\s*' + [regex]::Escape($letter) + '\s*:'
    [pscustomobject][ordered]@{
        active = $active
        configured = $configured
        union = $union
        target_letter = $letter
        active_hits = @($active | Where-Object { $_ -match $target })
        configured_hits = @($configured | Where-Object { $_ -match $target })
        union_hits = @($union | Where-Object { $_ -match $target })
    }
}
function Assert-ProductVolumePagefileEntries($Preflight) {
    if ($null -eq $Preflight -or
        [string]$Preflight.target_letter -notmatch '^[A-Z]$') {
        throw "product-volume pagefile preflight is invalid"
    }
    if (@($Preflight.active_hits).Count -ne 0 -or
        @($Preflight.configured_hits).Count -ne 0 -or
        @($Preflight.union_hits).Count -ne 0) {
        throw "product-volume pagefile refusal"
    }
    $Preflight
}
function Get-ProductVolumePagefilePreflight {
    $active = @(Get-CimInstance Win32_PageFileUsage -ErrorAction Stop |
            ForEach-Object { [string]$_.Name })
    $configuredRaw = (Get-ItemProperty `
            "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management" `
            -Name PagingFiles -ErrorAction Stop).PagingFiles
    Get-NormalizedPagefilePreflight $active @($configuredRaw) $Letter
}
function Assert-ProductVolumePagefileFree {
    $script:pagefilePreflight = Get-ProductVolumePagefilePreflight
    Assert-ProductVolumePagefileEntries $script:pagefilePreflight
}
function Get-ProductState {
    [ordered]@{
        utc = (Get-Date).ToUniversalTime().ToString("o")
        winsvc = [string](Get-Service RamSharedWinSvc `
                -ErrorAction SilentlyContinue).Status
        broker = [string](Get-Service RamSharedBroker `
                -ErrorAction SilentlyContinue).Status
        product_disk_count = @(Get-ProductDisks).Count
    }
}
function Assert-FinalProductState($OnlineEvidence, [UInt32]$Sector) {
    if ($null -eq $OnlineEvidence) { throw "final Online evidence is missing" }
    $observation = Invoke-BoundedStorageObservation "final_active" `
        ([string]$OnlineEvidence.serial) ([UInt64]$OnlineEvidence.size) $Sector
    $winsvc = Get-Service RamSharedWinSvc -ErrorAction Stop
    $broker = Get-Service RamSharedBroker -ErrorAction Stop
    if ($winsvc.Status -ne "Running" -or $broker.Status -ne "Running" -or
        $null -eq $observation.disk -or
        (Normalize-RamSharedText ([string]$observation.disk.serial)) -ne
            (Normalize-RamSharedText ([string]$OnlineEvidence.serial))) {
        throw "final active state failed winsvc=$($winsvc.Status) broker=$($broker.Status)"
    }
    $observation
}
function Get-ArtifactInventory([string]$Cell) {
    $paths = @($requiredCellArtifacts | ForEach-Object { "$Cell-$_" })
    @($paths | ForEach-Object {
            $path = Join-Path $OutDir $_
            if (Test-Path $path -PathType Leaf) {
                [ordered]@{
                    path = $_
                    bytes = (Get-Item $path).Length
                    sha256 = Get-Sha256 $path
                }
            }
        })
}
function Assert-RequiredArtifactInventory([string]$Cell) {
    $expected = @($requiredCellArtifacts | ForEach-Object { "$Cell-$_" })
    $inventory = @(Get-ArtifactInventory $Cell)
    foreach ($relativePath in $expected) {
        $entry = @($inventory | Where-Object path -eq $relativePath)
        if ($entry.Count -ne 1) {
            throw "missing required cell artifact cell=$Cell path=$relativePath"
        }
        if ([UInt64]$entry[0].bytes -eq 0) {
            throw "empty required cell artifact cell=$Cell path=$relativePath"
        }
        if ([string]$entry[0].sha256 -notmatch '^[0-9A-F]{64}$') {
            throw "invalid required cell artifact hash cell=$Cell path=$relativePath"
        }
    }
    $counterPath = Join-Path $OutDir "$Cell-counter-direct.jsonl"
    $counterRows = @(Get-Content -LiteralPath $counterPath | Where-Object {
            -not [string]::IsNullOrWhiteSpace($_)
        })
    if ($counterRows.Count -eq 0) {
        throw "empty required cell artifact cell=$Cell path=$Cell-counter-direct.jsonl"
    }
    foreach ($row in $counterRows) {
        $null = $row | ConvertFrom-Json -ErrorAction Stop
    }
    $inventory
}
function Write-CellEvidenceManifest(
    [string]$Cell,
    [string]$ContextPath,
    [object[]]$Inventory
) {
    $relativeContext = Split-Path $ContextPath -Leaf
    $manifestPath = Join-Path $OutDir "$Cell-evidence-manifest.json"
    Write-Json ([ordered]@{
            schema_version = $evidenceSchemaVersion
            cell = $Cell
            context = [ordered]@{
                path = $relativeContext
                bytes = (Get-Item -LiteralPath $ContextPath).Length
                sha256 = Get-Sha256 $ContextPath
            }
            artifacts = @($Inventory)
        }) $manifestPath 10
    [ordered]@{
        path = (Split-Path $manifestPath -Leaf)
        bytes = (Get-Item -LiteralPath $manifestPath).Length
        sha256 = Get-Sha256 $manifestPath
    }
}
function Assert-NoDiskRetryEvents([string]$Cell, [object[]]$Events) {
    $count = @($Events).Count
    if ($count -ne 0) {
        throw "disk retry events detected cell=$Cell EventId=153 count=$count"
    }
}
function Get-CellDiskRetryEvents(
    [string]$Cell,
    [DateTime]$CellStartUtc,
    [DateTime]$CellEndUtc
) {
    if ($CellEndUtc -lt $CellStartUtc) {
        throw "invalid Event 153 cell interval cell=$Cell"
    }
    $events = @()
    try {
        $events = @(Get-WinEvent -FilterHashtable @{
                LogName = "System"
                ProviderName = "disk"
                Id = 153
                StartTime = $CellStartUtc
                EndTime = $CellEndUtc
            } -ErrorAction Stop)
    } catch {
        if ($_.FullyQualifiedErrorId -notmatch '^NoMatchingEventsFound') {
            throw "Disk Event 153 query failed cell=$Cell error=$($_.Exception.Message)"
        }
    }
    $rows = @($events | ForEach-Object {
            [ordered]@{
                time_created_utc = $_.TimeCreated.ToUniversalTime().ToString("o")
                id = [int]$_.Id
                provider = [string]$_.ProviderName
                record_id = [UInt64]$_.RecordId
            }
        })
    $artifact = [ordered]@{
        schema_version = $evidenceSchemaVersion
        cell = $Cell
        start_utc = $CellStartUtc.ToUniversalTime().ToString("o")
        end_utc = $CellEndUtc.ToUniversalTime().ToString("o")
        provider = "disk"
        event_id = 153
        count = $rows.Count
        events = $rows
    }
    Write-Json $artifact (Join-Path $OutDir "$Cell-event-153.json") 8
    Assert-NoDiskRetryEvents $Cell $rows
    $artifact
}
function Invoke-Workload(
    [string]$Path,
    [string]$Kind,
    [int]$Run,
    [int]$QueueDepth,
    [UInt64]$AvailableBytes
) {
    $workerFileBytes = [UInt64]([math]::Floor(
            ([double]$AvailableBytes * 0.25 / $QueueDepth) / 4096) * 4096)
    $workerFileBytes = [UInt64][math]::Max(1MB, [math]::Min(32MB, $workerFileBytes))
    $gateRoot = Join-Path $OutDir ("gate-{0}-{1}-{2}" -f $Kind, $Run, [guid]::NewGuid())
    New-Item -ItemType Directory -Force $gateRoot | Out-Null
    $jobs = @()
    try {
        for ($worker = 0; $worker -lt $QueueDepth; $worker++) {
            $workerPath = "$Path.worker-$worker"
            $jobs += Start-Job -ScriptBlock {
                param(
                    $workerPath, $kind, $run, $worker, $queueDepth, $gateRoot,
                    [UInt64]$workerFileBytes
                )
                $ErrorActionPreference = "Stop"
                $ready = Join-Path $gateRoot "ready-$worker"
                $start = Join-Path $gateRoot "start"
                New-Item -ItemType File -Force $ready | Out-Null
                $deadline = (Get-Date).AddSeconds(120)
                while (-not (Test-Path $start) -and (Get-Date) -lt $deadline) {
                    Start-Sleep -Milliseconds 10
                }
                if (-not (Test-Path $start)) { throw "worker start gate timeout" }
                $latencies = [Collections.Generic.List[double]]::new()
                $hashes = [Collections.Generic.List[string]]::new()
                $bytes = [UInt64]0
                $stream = $null
                try {
                    $stream = [IO.File]::Open(
                        $workerPath, "OpenOrCreate", "ReadWrite", "ReadWrite")
                    $large = New-Object byte[] 1MB
                    [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($large)
                    $small = New-Object byte[] 4096
                    $random = [Random]::new(17000 + ($run * 101) + $worker)
                    switch ($kind) {
                        "seq1m" {
                            $iterations = [math]::Max(
                                1, [math]::Floor($workerFileBytes / 1MB))
                            for ($i = 0; $i -lt $iterations; $i++) {
                                $op = [Diagnostics.Stopwatch]::StartNew()
                                $stream.Write($large, 0, $large.Length)
                                $op.Stop(); $latencies.Add($op.Elapsed.TotalMilliseconds)
                                $bytes += $large.Length
                            }
                            $stream.Flush($true); $stream.Position = 0
                            for ($i = 0; $i -lt $iterations; $i++) {
                                $op = [Diagnostics.Stopwatch]::StartNew()
                                $got = $stream.Read($large, 0, $large.Length)
                                $op.Stop(); $latencies.Add($op.Elapsed.TotalMilliseconds)
                                if ($got -ne $large.Length) { throw "sequential short read" }
                                $bytes += $large.Length
                            }
                        }
                        { $_ -in @("rand4k", "mixed70r30w") } {
                            $stream.SetLength([int64]$workerFileBytes)
                            $operations = [math]::Max(1, [math]::Ceiling(4096 / $queueDepth))
                            $slots = [int]($workerFileBytes / 4096)
                            for ($i = 0; $i -lt $operations; $i++) {
                                $stream.Position = [int64]($random.Next(0, $slots) * 4096)
                                $write = $kind -eq "rand4k" -or $random.Next(0, 10) -ge 7
                                $op = [Diagnostics.Stopwatch]::StartNew()
                                if ($write) { $stream.Write($small, 0, $small.Length) }
                                else { $null = $stream.Read($small, 0, $small.Length) }
                                $op.Stop(); $latencies.Add($op.Elapsed.TotalMilliseconds)
                                $bytes += $small.Length
                            }
                            $stream.Flush($true)
                        }
                        "flush" {
                            $operations = [math]::Max(1, [math]::Ceiling(64 / $queueDepth))
                            for ($i = 0; $i -lt $operations; $i++) {
                                $stream.Write($small, 0, $small.Length)
                                $op = [Diagnostics.Stopwatch]::StartNew()
                                $stream.Flush($true)
                                $op.Stop(); $latencies.Add($op.Elapsed.TotalMilliseconds)
                                $bytes += $small.Length
                            }
                        }
                        "integrity" {
                            for ($round = 0; $round -lt 3; $round++) {
                                $probe = New-Object byte[] ([int][math]::Min(
                                        8MB, $workerFileBytes))
                                [Security.Cryptography.RandomNumberGenerator]::Create().
                                    GetBytes($probe)
                                $stream.SetLength(0)
                                $op = [Diagnostics.Stopwatch]::StartNew()
                                $stream.Write($probe, 0, $probe.Length); $stream.Flush($true)
                                $stream.Position = 0
                                $read = New-Object byte[] $probe.Length
                                $got = $stream.Read($read, 0, $read.Length)
                                $op.Stop(); $latencies.Add($op.Elapsed.TotalMilliseconds)
                                if ($got -ne $read.Length) { throw "integrity short read" }
                                $sha = [Security.Cryptography.SHA256]::Create()
                                try {
                                    $a = [BitConverter]::ToString($sha.ComputeHash($probe))
                                    $b = [BitConverter]::ToString($sha.ComputeHash($read))
                                } finally { $sha.Dispose() }
                                if ($a -ne $b) { throw "integrity mismatch" }
                                $hashes.Add($a); $bytes += [UInt64]($probe.Length * 2)
                            }
                        }
                    }
                } finally {
                    if ($stream) { $stream.Dispose() }
                    Remove-Item $workerPath -Force -ErrorAction SilentlyContinue
                }
                [pscustomobject]@{
                    worker = $worker; bytes = $bytes
                    latencies_ms = [double[]]$latencies.ToArray()
                    sha256 = [string[]]$hashes.ToArray()
                }
            } -ArgumentList $workerPath, $Kind, $Run, $worker, $QueueDepth,
                $gateRoot, $workerFileBytes
        }
        $readyDeadline = (Get-Date).AddSeconds(120)
        while (@(Get-ChildItem $gateRoot -Filter "ready-*").Count -ne $QueueDepth -and
            (Get-Date) -lt $readyDeadline) {
            Start-Sleep -Milliseconds 20
        }
        if (@(Get-ChildItem $gateRoot -Filter "ready-*").Count -ne $QueueDepth) {
            throw "queue-depth workers did not become ready"
        }
        $sw = [Diagnostics.Stopwatch]::StartNew()
        New-Item -ItemType File -Force (Join-Path $gateRoot "start") | Out-Null
        if (-not (Wait-Job $jobs -Timeout 120)) { throw "queued workload timeout" }
        $workerRows = @($jobs | Receive-Job -ErrorAction Stop)
        $sw.Stop()
        if ($workerRows.Count -ne $QueueDepth) {
            throw "effective queue depth mismatch expected=$QueueDepth observed=$($workerRows.Count)"
        }
        $latencies = [double[]]@($workerRows | ForEach-Object { $_.latencies_ms })
        $bytes = [UInt64](($workerRows | Measure-Object bytes -Sum).Sum)
        [ordered]@{
            workload = $Kind; run = $Run; bytes = $bytes
            elapsed_ms = [math]::Round($sw.Elapsed.TotalMilliseconds, 3)
            mib_per_sec = if ($sw.Elapsed.TotalSeconds -gt 0) {
                [math]::Round(($bytes / 1MB) / $sw.Elapsed.TotalSeconds, 3)
            } else { 0 }
            latency_p50_ms = [math]::Round((Get-NearestRank $latencies 0.50), 4)
            latency_p99_ms = [math]::Round((Get-NearestRank $latencies 0.99), 4)
            effective_queue_depth = $workerRows.Count
            worker_file_bytes = $workerFileBytes
            sha256 = [string[]]@($workerRows | ForEach-Object { $_.sha256 } |
                Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
            verdict = "PASS"
        }
    } finally {
        $jobs | Stop-Job -ErrorAction SilentlyContinue
        $jobs | Remove-Job -Force -ErrorAction SilentlyContinue
        Remove-Item $gateRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
function Invoke-WorkloadWorkerMode {
    if ($WorkerResult -eq "" -or $WorkerPath -eq "" -or
        $WorkerWorkload -eq "" -or $WorkerRun -le 0 -or
        $WorkerQueueDepth -le 0 -or $WorkerAvailableBytes -eq 0) {
        throw "invalid workload worker arguments"
    }
    New-Item -ItemType Directory -Force (Split-Path $WorkerResult -Parent) | Out-Null
    try {
        $row = Invoke-Workload $WorkerPath $WorkerWorkload $WorkerRun `
            $WorkerQueueDepth $WorkerAvailableBytes
        Write-Json ([ordered]@{
                schema_version = $evidenceSchemaVersion
                status = "PASS"
                row = $row
            }) $WorkerResult 10
        exit 0
    } catch {
        Write-Json ([ordered]@{
                schema_version = $evidenceSchemaVersion
                status = "RED"
                failure_phase = "workload_worker"
                error = $_.Exception.Message
            }) $WorkerResult
        exit 1
    }
}
function Invoke-BoundedWorkload(
    [string]$Path,
    [string]$Kind,
    [int]$Run,
    [int]$QueueDepth,
    [UInt64]$AvailableBytes
) {
    $resultPath = Join-Path $OutDir (
        "workload-{0}-{1}-{2}.json" -f $Kind, $Run, [guid]::NewGuid().ToString("N"))
    $arguments = @(
        "-WorkloadWorker",
        "-WorkerPath", (Quote-ProcessArgument $Path),
        "-WorkerWorkload", $Kind,
        "-WorkerRun", [string]$Run,
        "-WorkerQueueDepth", [string]$QueueDepth,
        "-WorkerAvailableBytes", [string]$AvailableBytes,
        "-WorkerResult", (Quote-ProcessArgument $resultPath),
        "-OutDir", (Quote-ProcessArgument $OutDir)
    )
    $bounded = Start-BoundedHarnessChild $arguments $workloadTimeoutSeconds
    if (-not $bounded.completed) {
        throw "workload timeout kind=$Kind run=$Run process_tree_terminated=$($bounded.process_tree_terminated)"
    }
    if ($bounded.exit_code -ne 0 -or -not (Test-Path $resultPath -PathType Leaf)) {
        throw "workload child failed kind=$Kind run=$Run exit=$($bounded.exit_code) stderr=$($bounded.stderr)"
    }
    $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json -ErrorAction Stop
    if ([string]$result.status -ne "PASS" -or $null -eq $result.row) {
        throw "workload child result is malformed kind=$Kind run=$Run"
    }
    $result.row
}
function Invoke-ManufacturedGuardCase([string]$Case) {
    try {
        switch ($Case) {
            "recovery_phase" {
                Assert-RecoveryJournal ([pscustomobject]@{
                        phase = "raw_validated"
                        events = @([pscustomobject]@{ phase = "raw_validated" })
                    })
            }
            "recovery_volume" { Assert-RecoveryVolumeCardinality 1 }
            "baseline_invalid" {
                Assert-BaselineDocument ([pscustomobject]@{
                        schema_version = 0; harness_behavior_revision = "wrong"
                        qualified = $true; expected_rows = 0; observed_rows = 0
                        expected_summaries = 0; observed_summaries = 0; entries = @()
                    })
            }
            "baseline_key_domain" {
                $entries = @()
                for ($index = 1; $index -le $expectedSummaries; $index++) {
                    $entries += [pscustomobject]@{
                        key = "unexpected-$index"
                        platform_fingerprint = ("A" * 64)
                        runs = $Runs
                        median_mib_per_sec = 1.0
                        p99_latency_ms = 1.0
                        stddev_mib_per_sec = 0.0
                        min_mib_per_sec = 1.0
                        max_mib_per_sec = 1.0
                    }
                }
                Assert-BaselineDocument ([pscustomobject]@{
                        schema_version = $evidenceSchemaVersion
                        harness_behavior_revision = $harnessBehaviorRevision
                        qualified = $true
                        expected_rows = $expectedRows
                        observed_rows = $expectedRows
                        expected_summaries = $expectedSummaries
                        observed_summaries = $expectedSummaries
                        entries = $entries
                    })
            }
            "counter_semantics" {
                $counterPath = Join-Path $OutDir "manufactured-counter-invalid.jsonl"
                [IO.File]::WriteAllText($counterPath, (([ordered]@{
                            serial = "BAD"; expected_size_bytes = 0; rounds = 0
                            last_sha256 = ""; uncached_write_bytes = 0
                            uncached_read_bytes = 0; perf_row_samples = 0
                            disk_read_bytes_per_sec_max = 0
                            disk_write_bytes_per_sec_max = 0
                            p50_ms = 0; p95_ms = 0; p99_ms = 0
                        } | ConvertTo-Json -Compress) + [Environment]::NewLine),
                    [Text.UTF8Encoding]::new($false))
                Assert-CounterJsonlSemantics $counterPath "0123456789ABCDEF" 64MB
            }
            "counter_metric_semantics" {
                $counterPath = Join-Path $OutDir "manufactured-counter-metric-invalid.jsonl"
                [IO.File]::WriteAllText($counterPath, (([ordered]@{
                            serial = "0123456789ABCDEF"; expected_size_bytes = [UInt64]64MB
                            rounds = 3; last_sha256 = ("A" * 64)
                            uncached_write_bytes = 1; uncached_read_bytes = 1
                            perf_row_samples = 1
                            disk_read_bytes_per_sec_max = 1
                            disk_write_bytes_per_sec_avg = 0
                            disk_write_bytes_per_sec_max = 0
                            percent_disk_time_avg = 0; percent_disk_time_max = 0
                            avg_disk_sec_per_read_ms_avg = 0
                            avg_disk_sec_per_write_ms_avg = 0
                            current_disk_queue_length_avg = 0
                            p50_ms = 0; p95_ms = 0; p99_ms = 0
                        } | ConvertTo-Json -Compress) + [Environment]::NewLine),
                    [Text.UTF8Encoding]::new($false))
                Assert-CounterJsonlSemantics $counterPath "0123456789ABCDEF" 64MB
            }
            "fresh_outdir" {
                $occupied = Join-Path $OutDir "manufactured-existing-output"
                New-Item -ItemType Directory -Force $occupied | Out-Null
                Assert-FreshOutDir $occupied
            }
            "pagefile_configured" {
                $preflight = Get-NormalizedPagefilePreflight @() `
                    @("s:\\pagefile.sys 0 0") "S"
                Assert-ProductVolumePagefileEntries $preflight
            }
            "toml_duplicate" {
                ConvertFrom-RamSharedToml "[win_drive]`nsize_bytes = 1`nsize_bytes = 2" `
                    "manufactured-duplicate"
            }
            "toml_duplicate_table" {
                ConvertFrom-RamSharedToml "[win_drive]`nsize_bytes = 1`n[win_drive]`nblock_size = 4096" `
                    "manufactured-duplicate-table"
            }
            "online_identity" {
                Assert-OnlineStorageBinding ([pscustomobject]@{
                        run_id = "run-1-2-3"; serial = "0123456789ABCDEF"; size = 64MB
                    }) "FEDCBA9876543210" 64MB
            }
            default { throw "unsupported manufactured guard case=$Case" }
        }
        [pscustomobject]@{ executed = $true; accepted = $true; error = $null }
    } catch {
        [pscustomobject]@{
            executed = $true
            accepted = $false
            error = $_.Exception.Message
        }
    }
}

if ($StorageProviderWorker) {
    Invoke-StorageProviderWorkerMode
}
if ($StorageProviderRecoveryWorker) {
    Invoke-StorageProviderRecoveryWorkerMode
}
if ($StorageProviderObservationWorker) {
    Invoke-StorageProviderObservationWorkerMode
}
if ($WorkloadWorker) {
    Invoke-WorkloadWorkerMode
}
if ($EvidenceDelayWorker) {
    Start-Sleep -Seconds 30
    exit 0
}
if ($EvidencePipeFloodWorker) {
    [Console]::Out.WriteLine(("O" * 131072))
    [Console]::Error.WriteLine(("E" * 131072))
    exit 0
}
if ($EvidenceSelfTestCase) {
    New-Item -Force -ItemType Directory $OutDir | Out-Null
    $prior = [pscustomobject]@{
        median_mib_per_sec = 100.0
        p99_latency_ms = 1.0
        platform_fingerprint = "MATCH"
    }
    $comparisonArguments = switch ($EvidenceSelfTestCase) {
        "pass" { @($true, $true, $prior, "MATCH", 95.0, 1.1) }
        "baseline" { @($false, $false, $null, "MATCH", 100.0, 1.0) }
        "unqualified" { @($true, $false, $prior, "MATCH", 100.0, 1.0) }
        "incomparable" { @($true, $true, $prior, "OTHER", 100.0, 1.0) }
        "yellow" { @($true, $true, $prior, "MATCH", 85.0, 1.1) }
        "red" { @($true, $true, $prior, "MATCH", 79.0, 1.1) }
        default { @($true, $true, $prior, "MATCH", 100.0, 1.0) }
    }
    $verdict = Get-ComparisonVerdict @comparisonArguments
    $observedRows = if ($EvidenceSelfTestCase -eq "missing_rows") { 74 } else { 75 }
    if ($EvidenceSelfTestCase -in @("missing_rows", "timeout", "event_153",
            "missing_artifact", "rollback_driver_mismatch", "counter_timeout",
            "recovery_phase", "recovery_volume", "baseline_invalid",
            "baseline_key_domain", "counter_semantics", "counter_metric_semantics",
            "fresh_outdir", "toml_duplicate",
            "toml_duplicate_table", "online_identity", "pagefile_configured")) {
        $verdict = "RED"
    }
    $timeoutProbe = $null
    $counterTimeoutProbe = $null
    $pipeFloodProbe = $null
    $guardResult = $null
    if ($EvidenceSelfTestCase -eq "timeout") {
        $timeoutProbe = Start-BoundedHarnessChild @(
            "-EvidenceDelayWorker",
            "-OutDir", (Quote-ProcessArgument $OutDir)
        ) 1
        if ($timeoutProbe.completed) {
            throw "storage provider timeout self-test did not terminate child"
        }
    }
    if ($EvidenceSelfTestCase -eq "counter_timeout") {
        $counterTimeoutProbe = Start-BoundedPowerShellChild $script:HarnessPath @(
            "-EvidenceDelayWorker",
            "-OutDir", (Quote-ProcessArgument $OutDir)
        ) 1
        if ($counterTimeoutProbe.completed) {
            throw "counter probe timeout self-test did not terminate child"
        }
    }
    if ($EvidenceSelfTestCase -in @("recovery_phase", "recovery_volume",
            "baseline_invalid", "baseline_key_domain", "counter_semantics",
            "counter_metric_semantics", "fresh_outdir",
            "toml_duplicate", "toml_duplicate_table", "online_identity",
            "pagefile_configured")) {
        $guardResult = Invoke-ManufacturedGuardCase $EvidenceSelfTestCase
        if (-not $guardResult.executed -or $guardResult.accepted) {
            throw "manufactured guard case was accepted case=$EvidenceSelfTestCase"
        }
    }
    if ($EvidenceSelfTestCase -eq "pipe_flood") {
        $pipeFloodProbe = Start-BoundedHarnessChild @(
            "-EvidencePipeFloodWorker",
            "-OutDir", (Quote-ProcessArgument $OutDir)
        ) 5
        if (-not $pipeFloodProbe.completed -or $pipeFloodProbe.exit_code -ne 0 -or
            -not $pipeFloodProbe.stdout_drained -or -not $pipeFloodProbe.stderr_drained -or
            $pipeFloodProbe.stdout_bytes -lt 131072 -or
            $pipeFloodProbe.stderr_bytes -lt 131072) {
            throw "redirected pipe flood was not drained"
        }
    }
    $artifactInventoryComplete = $true
    if ($EvidenceSelfTestCase -in @("pass", "missing_artifact")) {
        foreach ($suffix in $requiredCellArtifacts) {
            if ($EvidenceSelfTestCase -eq "missing_artifact" -and
                $suffix -eq "event-153.json") {
                continue
            }
            [IO.File]::WriteAllText(
                (Join-Path $OutDir "manufactured-$suffix"),
                "{}" + [Environment]::NewLine,
                [Text.UTF8Encoding]::new($false))
        }
        try {
            $null = Assert-RequiredArtifactInventory "manufactured"
            if ($EvidenceSelfTestCase -eq "missing_artifact") {
                throw "missing required cell artifact was accepted"
            }
        } catch {
            if ($EvidenceSelfTestCase -ne "missing_artifact" -or
                $_.Exception.Message -notmatch '^missing required cell artifact') {
                throw
            }
            $artifactInventoryComplete = $false
        }
    }
    $event153Count = if ($EvidenceSelfTestCase -eq "event_153") { 1 } else { 0 }
    $event153Refused = $false
    try {
        $manufacturedEvents = if ($event153Count -eq 1) {
            @([pscustomobject]@{ Id = 153 })
        } else { @() }
        Assert-NoDiskRetryEvents "manufactured" $manufacturedEvents
    } catch {
        if ($EvidenceSelfTestCase -ne "event_153" -or
            $_.Exception.Message -notmatch '^disk retry events detected') {
            throw
        }
        $event153Refused = $true
    }
    if (($EvidenceSelfTestCase -eq "event_153") -ne $event153Refused) {
        throw "physical_matrix_rejects_event_153 manufactured assertion failed"
    }
    $testName = switch ($EvidenceSelfTestCase) {
        "pass" { "context_manifest_complete" }
        "baseline" { "unqualified_baseline_is_not_regression_pass" }
        "unqualified" { "unqualified_baseline_is_not_regression_pass" }
        "incomparable" { "baseline_fingerprint_mismatch_is_incomparable" }
        "yellow" { "yellow_never_reports_pass_or_promotes" }
        "red" { "red_regression_returns_nonzero_and_restores_lkg" }
        "missing_rows" { "all_expected_matrix_rows_present" }
        "timeout" { "storage_provider_timeout_is_bounded" }
        "event_153" { "physical_matrix_rejects_event_153" }
        "missing_artifact" { "missing_required_cell_artifact_is_red" }
        "rollback_driver_mismatch" { "rollback_loaded_driver_mismatch_is_red" }
        "counter_timeout" { "counter_probe_timeout_is_bounded" }
        "recovery_phase" { "partial_format_recovery_requires_partition_phase_and_zero_volumes" }
        "recovery_volume" { "partial_format_recovery_requires_partition_phase_and_zero_volumes" }
        "baseline_invalid" { "baseline_schema_cardinality_and_metrics_are_validated" }
        "baseline_key_domain" { "baseline_key_domain_is_validated" }
        "counter_semantics" { "counter_jsonl_semantics_are_required" }
        "counter_metric_semantics" { "counter_jsonl_all_metrics_are_finite" }
        "fresh_outdir" { "fresh_outdir_is_required" }
        "pagefile_configured" { "configured_pagefile_union_refusal" }
        "toml_duplicate" { "toml_unique_scalars_are_verified_before_seal" }
        "toml_duplicate_table" { "toml_duplicate_table_is_refused" }
        "online_identity" { "worker_binds_current_online_serial_before_mutation" }
        "pipe_flood" { "bounded_child_drains_redirected_streams" }
    }
    $rates = [double[]]@(90, 100, 110)
    $stddev = Get-SampleStandardDeviation $rates
    $platformFields = [ordered]@{
        windows_build = "manufactured"
        gpu = "manufactured"
        driver_interface = "1"
        sector = 4096
        qd = 4
        max_io_bytes = 1048576
        workload_revision = $harnessBehaviorRevision
        runs = 3
    }
    [int]$verdictExitCode = Get-VerdictExitCode ([string]$verdict)
    $record = [ordered]@{
        test_name = $testName
        case = $EvidenceSelfTestCase
        verdict = $verdict
        selected_final = if ($verdict -eq "PASS") { "operator" } else { "rollback" }
        schema_version = $evidenceSchemaVersion
        platform_fingerprint = Get-PlatformFingerprint $platformFields
        artifact_inventory = @("manufactured-context.json")
        artifact_inventory_complete = $artifactInventoryComplete
        event_153_count = $event153Count
        rollback_binary_match = ($EvidenceSelfTestCase -ne "rollback_driver_mismatch")
        loaded_driver_sha256 = "MANUFACTURED"
        loaded_winsvc_sha256 = "MANUFACTURED"
        loaded_broker_sha256 = "MANUFACTURED"
        stddev_mib_per_sec = $stddev
        min_mib_per_sec = ($rates | Measure-Object -Minimum).Minimum
        max_mib_per_sec = ($rates | Measure-Object -Maximum).Maximum
        expected_rows = $expectedRows
        observed_rows = $observedRows
        context_complete = $true
        binary_match_persisted = $true
        deviation_present = ($stddev -gt 0)
        qualified = ($EvidenceSelfTestCase -eq "pass")
        failure_phase = if ($EvidenceSelfTestCase -eq "timeout") {
            "storage_provider_timeout"
        } elseif ($EvidenceSelfTestCase -eq "counter_timeout") {
            "counter_probe_timeout"
        } elseif ($guardResult) {
            "manufactured_guard_refusal"
        } else { $null }
        cleanup_attempted = ($verdict -ne "PASS")
        orchestration_child_terminated = if ($timeoutProbe) {
            -not $timeoutProbe.completed
        } elseif ($counterTimeoutProbe) {
            -not $counterTimeoutProbe.completed
        } else { $false }
        process_tree_terminated = if ($counterTimeoutProbe) {
            $counterTimeoutProbe.process_tree_terminated
        } else { $false }
        guard_executed = if ($guardResult) { [bool]$guardResult.executed } elseif ($pipeFloodProbe) { $true } else { $false }
        guard_accepted = if ($guardResult) { [bool]$guardResult.accepted } else { $false }
        guard_error = if ($guardResult) { [string]$guardResult.error } else { $null }
        stdout_drained = if ($pipeFloodProbe) { [bool]$pipeFloodProbe.stdout_drained } else { $false }
        stderr_drained = if ($pipeFloodProbe) { [bool]$pipeFloodProbe.stderr_drained } else { $false }
        exit_code = $verdictExitCode
    }
    Write-Json $record (Join-Path $OutDir "evidence-self-test.json")
    Write-Host "STATUS=$verdict ARTIFACT=$OutDir"
    exit $verdictExitCode
}

if (-not $EvidenceSelfTestCase) {
    Assert-FreshOutDir $OutDir
}
New-Item -ItemType Directory $OutDir | Out-Null
Write-Json ([ordered]@{ cells = $cells; workloads = $workloads; runs = $Runs;
        throughput_regression_red_pct = $throughput_regression_red_pct
        throughput_regression_yellow_pct = $throughput_regression_yellow_pct }) `
    (Join-Path $OutDir "plan.json")
if ($SelfTestWorkload) {
    $selfTestRoot = Join-Path $OutDir "workload-self-test"
    New-Item -ItemType Directory -Force $selfTestRoot | Out-Null
    $selfRows = @(
        Invoke-Workload (Join-Path $selfTestRoot "qd1.bin") "seq1m" 1 1 256MB
        Invoke-Workload (Join-Path $selfTestRoot "qd4.bin") "mixed70r30w" 1 4 256MB
        Invoke-Workload (Join-Path $selfTestRoot "qd16.bin") "flush" 1 16 256MB
    )
    if (@($selfRows).Count -ne 3 -or
        [int]$selfRows[0].effective_queue_depth -ne 1 -or
        [int]$selfRows[1].effective_queue_depth -ne 4 -or
        [int]$selfRows[2].effective_queue_depth -ne 16 -or
        @($selfRows | Where-Object {
                $_.verdict -ne "PASS" -or $_.bytes -le 0 -or
                $_.latency_p99_ms -lt $_.latency_p50_ms
            }).Count -ne 0) {
        throw "workload self-test failed"
    }
    $regressionProbes = @(
        Get-RegressionVerdict $null $null 100 1
        Get-RegressionVerdict 100 1 95 1.1
        Get-RegressionVerdict 100 1 85 1.1
        Get-RegressionVerdict 100 1 79 1.1
        Get-RegressionVerdict 100 1 100 2.01
    )
    if (($regressionProbes -join ",") -ne "BASELINE,PASS,YELLOW,RED,RED") {
        throw "regression threshold self-test failed: $($regressionProbes -join ',')"
    }
    Write-Json $selfRows (Join-Path $OutDir "workload-self-test.json")
    Write-Host "WORKLOAD_SELF_TEST=PASS OUT_DIR=$OutDir"
    exit 0
}
if ($PreparePackages) {
    New-MatrixPackages
    Write-Host "PACKAGES_PREPARED=$PackageRoot"
}
if (-not $Run) { Write-Host "PLAN_ONLY=1 OUT_DIR=$OutDir"; exit 0 }
if (-not $ApprovePhysicalHost) { throw "-ApprovePhysicalHost is required" }
Assert-Admin
if (-not (Test-Path $RollbackManifest -PathType Leaf)) { throw "rollback manifest missing" }
if (-not (Test-Path $Controller -PathType Leaf)) { throw "controller missing" }
if (-not (Test-Path $CounterProbeScript -PathType Leaf)) {
    throw "counter/direct-I/O probe missing"
}
if (-not (Test-Path (Join-Path $PackageRoot "minimum\product-manifest.json"))) {
    New-MatrixPackages
}
$pagefilePreflight = Get-ProductVolumePagefilePreflight
$script:pagefilePreflight = $pagefilePreflight
Write-Json $pagefilePreflight (Join-Path $OutDir "pagefile-preflight.json") 10
Assert-ProductVolumePagefileEntries $pagefilePreflight | Out-Null
Assert-TargetLetterAvailable
Write-Json ([ordered]@{
        schema_version = $evidenceSchemaVersion
        pagefiles = $pagefilePreflight
        target_letter = $script:targetLetterPreflight
    }) (Join-Path $OutDir "preflight.json") 10
$baseline = @{}
$baselineQualified = $false
$baselineSupplied = -not [string]::IsNullOrWhiteSpace($BaselineSummary)
if ($baselineSupplied) {
    if (-not (Test-Path $BaselineSummary -PathType Leaf)) {
        throw "baseline summary missing"
    }
    $baselineDocument = Get-Content $BaselineSummary -Raw | ConvertFrom-Json -ErrorAction Stop
    $baseline = Assert-BaselineDocument $baselineDocument
    $baselineQualified = $true
}

$runId = [guid]::NewGuid().ToString("N")
$repository = Get-RepositoryContext
$invocation = Get-SanitizedInvocation
$hostContext = Get-HostContext
$rows = @()
Arm-Watchdog
Update-WatchdogHeartbeat
$contexts = @{}
$cellEvidenceManifests = @{}
$completed = $false
$primaryError = $null
$cleanupError = $null
$currentContext = $null
$currentContextPath = $null
try {
    Stop-Product
    foreach ($cell in $cells) {
        Update-WatchdogHeartbeat
        $name, $size, $sector, $qd, $maxIo = $cell
        $cellStartUtc = (Get-Date).ToUniversalTime()
        $before = Get-ProductState
        Assert-GpuReserve $size
        $manifestPath = Join-Path (Join-Path $PackageRoot $name) "product-manifest.json"
        if (-not (Test-Path $manifestPath -PathType Leaf)) {
            throw "matrix manifest missing cell=$name path=$manifestPath"
        }
        $manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
        $effectiveConfig = Get-ManifestStorageConfig $manifestPath
        if ($effectiveConfig.size -ne $size -or $effectiveConfig.sector -ne $sector) {
            throw "matrix manifest effective config mismatch cell=$name"
        }
        $fingerprint = Get-CellFingerprint $hostContext $name $size $sector $qd $maxIo
        $currentContextPath = Join-Path $OutDir "$name-context.json"
        $currentContext = [ordered]@{
            schema_version = $evidenceSchemaVersion
            harness_behavior_revision = $harnessBehaviorRevision
            run_id = $runId
            cell = $name
            status = "IN_PROGRESS"
            started_utc = $cellStartUtc.ToString("o")
            completed_utc = $null
            invocation = $invocation
            repository = $repository
            campaign_revision = [string]$manifest.version
            harness_sha256 = Get-Sha256 $script:HarnessPath
            platform = $hostContext
            platform_fingerprint = $fingerprint.sha256
            platform_fingerprint_fields = $fingerprint.fields
            plan = [ordered]@{
                size = $size; sector = $sector; qd = $qd
                max_io_bytes = $maxIo; workloads = $workloads; runs = $Runs
            }
            manifest_path = $manifestPath
            manifest_sha256 = Get-Sha256 $manifestPath
            package_artifacts = @($manifest.artifacts | ForEach-Object {
                    [ordered]@{
                        role = $_.role
                        relative_path = $_.relative_path
                        sha256 = ([string]$_.sha256).ToUpperInvariant()
                    }
                })
            loaded_driver_sha256 = $null
            loaded_winsvc_sha256 = $null
            loaded_broker_sha256 = $null
            pagefile_preflight = $pagefilePreflight
            target_letter_preflight = $script:targetLetterPreflight
            current_online = $null
            before = $before
            identity = $null
            after = $null
            commands = @(
                "$Controller install --manifest <cell-manifest>",
                "Start-Service RamSharedWinSvc",
                "Measure-RamSharedDiskIo.ps1 -Seconds 5 -ChecksumRounds 3",
                "five workloads x three runs",
                "consumer-first supported stop"
            )
            artifact_inventory = @()
            error = $null
        }
        Write-Json $currentContext $currentContextPath 12
        Assert-BinaryMatch $manifest (Split-Path $manifestPath -Parent)
        Invoke-BoundedController @("install", "--manifest", (Quote-ProcessArgument $manifestPath)) | Out-Null
        $serviceStartUtc = (Get-Date).ToUniversalTime()
        Start-Service RamSharedWinSvc -ErrorAction Stop
        (Get-Service RamSharedWinSvc -ErrorAction Stop).WaitForStatus(
            "Running", [timespan]::FromSeconds(30))
        $onlineEvidence = Get-CurrentRunOnlineEvidence $serviceStartUtc $size
        $liveHashes = Assert-LiveBinaryMatch $manifest `
            (Split-Path $manifestPath -Parent)
        $currentContext.loaded_driver_sha256 = $liveHashes.loaded_driver_sha256
        $currentContext.loaded_winsvc_sha256 = $liveHashes.loaded_winsvc_sha256
        $currentContext.loaded_broker_sha256 = $liveHashes.loaded_broker_sha256
        $currentContext.loaded_paths = $liveHashes
        $currentContext.current_online = $onlineEvidence
        Write-Json $currentContext $currentContextPath 12
        $storage = Invoke-BoundedStorageProvider $name $size $sector $onlineEvidence
        $currentContext.identity = $storage
        Write-Json $currentContext $currentContextPath 12
        Write-Json ([ordered]@{
                cell = $name; size = [UInt64]$storage.size
                logical_sector = [UInt32]$storage.logical_sector
                physical_sector = [UInt32]$storage.physical_sector
                bus = [string]$storage.bus
                volume_size = [UInt64]$storage.volume_size
                media = [string]$storage.media
                spindle_speed = [UInt64]$storage.spindle_speed
                enumeration_ms = [double]$storage.enumeration_ms
            }) (Join-Path $OutDir "$name-properties.json")
        $counterLog = Join-Path $OutDir "$name-counter-direct.log"
        $counterJsonl = Join-Path $OutDir "$name-counter-direct.jsonl"
        $counterRun = Start-BoundedPowerShellChild $CounterProbeScript @(
            "-Seconds", "5",
            "-DriveLetter", $Letter,
            "-ChecksumRounds", "3",
            "-ProbeMiB", "8",
            "-ExpectedSerial", ([string]$storage.serial),
            "-ExpectedSizeBytes", ([string][uint64]$storage.size),
            "-JsonlOut", (Quote-ProcessArgument $counterJsonl)
        ) $counterProbeTimeoutSeconds
        [IO.File]::WriteAllText($counterLog,
            $counterRun.stdout + [Environment]::NewLine + $counterRun.stderr,
            [Text.UTF8Encoding]::new($false))
        if (-not $counterRun.completed) {
            throw "counter probe timeout cell=$name seconds=$counterProbeTimeoutSeconds child_terminated=true"
        }
        if ($counterRun.exit_code -ne 0) {
            throw "counter/direct-I/O probe failed cell=$name exit=$($counterRun.exit_code)"
        }
        Assert-CounterJsonlSemantics $counterJsonl ([string]$storage.serial) `
            ([UInt64]$storage.size) | Out-Null
        for ($repetition = 1; $repetition -le $Runs; $repetition++) {
            foreach ($workload in $workloads) {
                Update-WatchdogHeartbeat
                $row = Invoke-BoundedWorkload `
                    "$Letter`:\matrix-$name-$repetition-$workload.bin" `
                    $workload $repetition $qd `
                    ([UInt64]$storage.volume_size_remaining)
                $row.cell = $name; $row.size = $size; $row.sector = $sector
                $row.qd = $qd; $row.max_io_bytes = $maxIo
                $rows += [pscustomobject]$row
                [IO.File]::AppendAllText((Join-Path $OutDir "samples.jsonl"),
                    (($row | ConvertTo-Json -Compress -Depth 6) + [Environment]::NewLine),
                    [Text.UTF8Encoding]::new($false))
                [IO.File]::AppendAllText((Join-Path $OutDir "$name-samples.jsonl"),
                    (($row | ConvertTo-Json -Compress -Depth 6) + [Environment]::NewLine),
                    [Text.UTF8Encoding]::new($false))
                Update-WatchdogHeartbeat
            }
        }
        Stop-Product
        Update-WatchdogHeartbeat
        $cellEndUtc = (Get-Date).ToUniversalTime()
        $currentContext.disk_event_153 = Get-CellDiskRetryEvents `
            $name $cellStartUtc $cellEndUtc
        $currentContext.status = "PASS"
        $currentContext.completed_utc = $cellEndUtc.ToString("o")
        $currentContext.after = Get-ProductState
        $currentContext.artifact_inventory = Assert-RequiredArtifactInventory $name
        Write-Json $currentContext $currentContextPath 12
        $cellEvidenceManifests[$name] = Write-CellEvidenceManifest `
            $name $currentContextPath $currentContext.artifact_inventory
        $contexts[$name] = $currentContext
        $currentContext = $null
        $currentContextPath = $null
    }
    $completed = $true
}
catch {
    $primaryError = $_
    if ($currentContext -and $currentContextPath) {
        $currentContext.status = "RED"
        $currentContext.completed_utc = (Get-Date).ToUniversalTime().ToString("o")
        $currentContext.error = $_.Exception.Message
        $currentContext.after = Get-ProductState
        $currentContext.artifact_inventory = Get-ArtifactInventory $currentContext.cell
        Write-Json $currentContext $currentContextPath 12
        $contexts[$currentContext.cell] = $currentContext
    }
}

$summary = @($rows | Group-Object cell, workload | ForEach-Object {
    $rates = @($_.Group.mib_per_sec | Sort-Object)
    $latencies = @($_.Group.latency_p99_ms | Sort-Object)
    $key = "$([string]$_.Group[0].cell)|$([string]$_.Group[0].workload)"
    $median = [double]$rates[[int][math]::Floor($rates.Count / 2)]
    $p99Latency = [double]$latencies[-1]
    $stddev = Get-SampleStandardDeviation ([double[]]$rates)
    $minimum = [double]($rates | Measure-Object -Minimum).Minimum
    $maximum = [double]($rates | Measure-Object -Maximum).Maximum
    $fingerprint = [string]$contexts[[string]$_.Group[0].cell].platform_fingerprint
    $prior = $baseline[$key]
    $throughputRegressionPct = if ($prior -and
        [double]$prior.median_mib_per_sec -gt 0) {
        [math]::Round(
            (([double]$prior.median_mib_per_sec - $median) /
                [double]$prior.median_mib_per_sec) * 100, 3)
    } else { $null }
    $latencyRatio = if ($prior -and [double]$prior.p99_latency_ms -gt 0) {
        [math]::Round($p99Latency / [double]$prior.p99_latency_ms, 3)
    } else { $null }
    $verdict = if ($rates.Count -ne $Runs) { "RED" } else {
        Get-ComparisonVerdict $baselineSupplied $baselineQualified $prior `
            $fingerprint $median $p99Latency
    }
    [ordered]@{
        key = $key; runs = $rates.Count
        median_mib_per_sec = $median
        stddev_mib_per_sec = [math]::Round($stddev, 3)
        min_mib_per_sec = $minimum
        max_mib_per_sec = $maximum
        p99_latency_ms = $p99Latency
        throughput_regression_pct = $throughputRegressionPct
        latency_ratio = $latencyRatio
        platform_fingerprint = $fingerprint
        verdict = $verdict
    }
})
$uniqueRows = @($rows | Group-Object {
        "$($_.cell)|$($_.workload)|$($_.run)"
    })
$cardinalityOk = ($rows.Count -eq $expectedRows -and
    $uniqueRows.Count -eq $expectedRows -and
    @($uniqueRows | Where-Object Count -ne 1).Count -eq 0 -and
    $summary.Count -eq $expectedSummaries)
$artifactEvidenceOk = ($cellEvidenceManifests.Count -eq $cells.Count -and
    @($cellEvidenceManifests.Values | Where-Object {
            [UInt64]$_.bytes -eq 0 -or
            [string]$_.sha256 -notmatch '^[0-9A-F]{64}$'
        }).Count -eq 0)
$measurementVerdict = if ($primaryError -or -not $completed -or
    -not $cardinalityOk -or -not $artifactEvidenceOk -or
    (Test-Path $watchdogTimeout -PathType Leaf)) {
    "RED"
} else {
    Get-OverallVerdict $summary
}
$selectedFinal = if ($measurementVerdict -eq "PASS") { "operator" } else { "rollback" }
$finalManifest = if ($selectedFinal -eq "operator") {
    Join-Path $PackageRoot "operator\product-manifest.json"
} else { $RollbackManifest }
$cleanupAttempted = $true
$cleanupState = $null
$finalBinaryMatch = $null
$finalOnlineEvidence = $null
$finalActiveIdentity = $null
$terminalStopError = $null
$initialTerminalSelection = $selectedFinal
try {
    Stop-Product
    $terminal = Start-ManifestProduct $finalManifest
    # rollback_binary_match_includes_loaded_driver: Start-ManifestProduct requires
    # the same driver + broker + consumer live hash conjunction for every selection.
    $finalBinaryMatch = $terminal.live_hashes
    $finalOnlineEvidence = $terminal.online
    $finalActiveIdentity = $terminal.identity
    $cleanupState = Get-ProductState
} catch {
    $candidateFailure = $_
    $cleanupError = $candidateFailure
    $measurementVerdict = "RED"
    $selectedFinal = "rollback"
    try {
        # terminal_fallback_stops_partial_candidate: stop the candidate even if
        # its install/start/identity verification failed before it returned.
        Stop-Product
    } catch {
        $terminalStopError = $_
    }
    try {
        if ($initialTerminalSelection -ne "rollback" -and -not $terminalStopError) {
            $finalManifest = $RollbackManifest
            $fallback = Start-ManifestProduct $RollbackManifest
            $finalBinaryMatch = $fallback.live_hashes
            $finalOnlineEvidence = $fallback.online
            $finalActiveIdentity = $fallback.identity
        }
    } catch {
        try {
            # terminal_fallback_stops_partial_candidate: a failed LKG attempt is
            # itself a candidate and must not remain active.
            Stop-Product
        } catch {
            $terminalStopError = $_
        }
        $cleanupError = [Management.Automation.ErrorRecord]::new(
            $_.Exception, "fallback_cleanup_failed",
            [Management.Automation.ErrorCategory]::OperationStopped, $null)
    }
    try {
        $cleanupState = Get-ProductState
    } catch {
        $cleanupState = [ordered]@{
            utc = (Get-Date).ToUniversalTime().ToString("o")
            observation_error = $_.Exception.Message
        }
    }
} finally {
    Remove-Item $watchdogMarker -Force -ErrorAction SilentlyContinue
}
$qualified = ($cardinalityOk -and $artifactEvidenceOk -and
    -not $primaryError -and -not $cleanupError -and
    -not (Test-Path $watchdogTimeout -PathType Leaf) -and
    $measurementVerdict -in @("PASS", "BASELINE"))
$summaryDocument = [ordered]@{
    schema_version = $evidenceSchemaVersion
    harness_behavior_revision = $harnessBehaviorRevision
    run_id = $runId
    qualified = $qualified
    verdict = $measurementVerdict
    selected_final = $selectedFinal
    expected_rows = $expectedRows
    observed_rows = $rows.Count
    expected_summaries = $expectedSummaries
    observed_summaries = $summary.Count
    unique_sample_keys = $uniqueRows.Count
    baseline_supplied = $baselineSupplied
    baseline_qualified = $baselineQualified
    cleanup_attempted = $cleanupAttempted
    cleanup_state = $cleanupState
    artifact_evidence_complete = $artifactEvidenceOk
    cell_evidence_manifests = $cellEvidenceManifests
    selected_final_manifest = [ordered]@{
        selection = $selectedFinal
        path = $finalManifest
        sha256 = if (Test-Path $finalManifest -PathType Leaf) {
            Get-Sha256 $finalManifest
        } else { $null }
        loaded_driver_sha256 = if ($finalBinaryMatch) {
            $finalBinaryMatch.loaded_driver_sha256
        } else { $null }
        loaded_winsvc_sha256 = if ($finalBinaryMatch) {
            $finalBinaryMatch.loaded_winsvc_sha256
        } else { $null }
        loaded_broker_sha256 = if ($finalBinaryMatch) {
            $finalBinaryMatch.loaded_broker_sha256
        } else { $null }
    }
    primary_error = if ($primaryError) { $primaryError.Exception.Message } else { $null }
    cleanup_error = if ($cleanupError) { $cleanupError.Exception.Message } else { $null }
    terminal_stop_error = if ($terminalStopError) { $terminalStopError.Exception.Message } else { $null }
    watchdog_timeout = Test-Path $watchdogTimeout -PathType Leaf
    final_online_evidence = $finalOnlineEvidence
    final_active_identity = $finalActiveIdentity
    entries = $summary
}
Write-Json $summaryDocument (Join-Path $OutDir "matrix-summary.json") 12
$exitCode = Get-VerdictExitCode $measurementVerdict
Write-Host "STATUS=$measurementVerdict ARTIFACT=$OutDir SELECTED_FINAL=$selectedFinal"
exit $exitCode
