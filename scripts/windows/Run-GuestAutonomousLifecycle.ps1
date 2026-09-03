#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$VMName = "win11-drill",
    [string]$User = "WIN11-DRILL\drilladmin",
    [string]$Password = "",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [string]$HostBinDir = "C:\ramshared\bin",
    [string]$DriverPackage = "C:\ramshared\artifacts\driver-package-build",
    [UInt64]$ExpectedSizeBytes = 67108864,
    [ValidateSet(1, 4, 16)]
    [uint32]$QueueDepth = 4,
    [uint32]$MaxIoBytes = 1048576,
    [string]$Letter = "R",
    [ValidateRange(15, 30)]
    [int]$GuestShutdownDelaySeconds = 15,
    [ValidateRange(10, 300)]
    [int]$PhaseTimeoutSeconds = 60,
    [switch]$BrokerLossOnline,
    [switch]$RawStopOnly,
    [switch]$ManufacturedRefusalThenStop,
    [switch]$ManufacturedWorkloadFailure
)

$ErrorActionPreference = "Stop"
if ($ManufacturedWorkloadFailure -and $QueueDepth -ne 1) {
    throw "ManufacturedWorkloadFailure requires QueueDepth=1"
}
. (Join-Path $PSScriptRoot "Invoke-GuestPsDirectBounded.ps1")
function Get-DrillPassword {
    if ($Password) { return $Password }
    foreach ($scope in @("Machine", "User")) {
        $value = [Environment]::GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)
        if ($value) { return $value }
    }
    if ($env:RAMSHARED_DRILL_PASSWORD) { return $env:RAMSHARED_DRILL_PASSWORD }
    throw "Missing RAMSHARED_DRILL_PASSWORD."
}
function Assert-DeferredGuestShutdownReceipt {
    [CmdletBinding()]
    param(
        [object]$Receipt,
        [int]$ExpectedDelaySeconds
    )
    if ($ExpectedDelaySeconds -lt 15 -or $ExpectedDelaySeconds -gt 30) {
        throw "deferred guest shutdown delay is outside 15-30 seconds"
    }
    if ($null -eq $Receipt -or
        $Receipt.PSObject.Properties.Match("shutdown_scheduled").Count -ne 1 -or
        $Receipt.PSObject.Properties.Match("delay_seconds").Count -ne 1) {
        throw "deferred guest shutdown receipt is incomplete"
    }
    $scheduled = $Receipt.PSObject.Properties["shutdown_scheduled"].Value
    $delaySeconds = $Receipt.PSObject.Properties["delay_seconds"].Value
    if ($scheduled -isnot [bool] -or $delaySeconds -isnot [int]) {
        throw "deferred guest shutdown receipt has invalid types"
    }
    if (-not $scheduled) {
        throw "deferred guest shutdown receipt did not confirm scheduling"
    }
    if ($delaySeconds -ne $ExpectedDelaySeconds) {
        throw "deferred guest shutdown receipt delay mismatch"
    }
    $true
}

$packageHarness = Join-Path $PSScriptRoot "Run-GuestProductPackage.ps1"
& $packageHarness -VMName $VMName -User $User -Password (Get-DrillPassword) `
    -ArtifactRoot $ArtifactRoot -HostBinDir $HostBinDir `
    -DriverPackage $DriverPackage -Case FreshInstall `
    -QueueDepth $QueueDepth -MaxIoBytes $MaxIoBytes | Out-Host

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$hostRun = Join-Path $ArtifactRoot "autonomous-lifecycle-$stamp"
New-Item $hostRun -ItemType Directory -Force | Out-Null
try {
    $before = Invoke-GuestPsDirectBounded -VMName $VMName -User $User `
        -Password (Get-DrillPassword) -Operation invoke -TimeoutSeconds 210 `
        -ScriptBlock {
        $disks = @(Get-Disk | Where-Object FriendlyName -Match "RAMSHARE")
        [pscustomobject]@{
            broker = (Get-Service RamSharedBroker).Status.ToString()
            consumer = (Get-Service RamSharedWinSvc).Status.ToString()
            ramshared_disks = $disks.Count
            paging_files = @((Get-CimInstance Win32_PageFileUsage -ErrorAction Stop).Name)
        }
    }
    if ($before.ramshared_disks -ne 0 -or $before.broker -ne "Stopped" -or
        $before.consumer -ne "Stopped") {
        throw "before state is not clean: $($before | ConvertTo-Json -Compress)"
    }
    $before | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $hostRun "before.json")

    # A demand-start package must remain inert across a cold boot.
    $shutdownReceipt = Invoke-GuestPsDirectBounded -VMName $VMName -User $User `
        -Password (Get-DrillPassword) -Operation invoke -TimeoutSeconds 210 `
        -ScriptBlock {
            param([int]$DelaySeconds)
            & shutdown.exe /s /t $DelaySeconds | Out-Null
            if ($LASTEXITCODE -ne 0) {
                throw "guest shutdown scheduling failed: exit=$LASTEXITCODE"
            }
            [pscustomobject]@{
                shutdown_scheduled = $true
                delay_seconds = $DelaySeconds
            }
        } -ArgumentList @($GuestShutdownDelaySeconds)
    Assert-DeferredGuestShutdownReceipt $shutdownReceipt `
        $GuestShutdownDelaySeconds | Out-Null
    $offDeadline = (Get-Date).AddSeconds(120)
    while ((Get-VM $VMName).State -ne "Off" -and (Get-Date) -lt $offDeadline) {
        Start-Sleep -Seconds 2
    }
    if ((Get-VM $VMName).State -ne "Off") {
        throw "guest did not complete a supported shutdown within 120 seconds"
    }
    Start-VM -Name $VMName | Out-Null
    Start-Sleep -Seconds 8

    $results = Invoke-GuestPsDirectBounded -VMName $VMName -User $User `
        -Password (Get-DrillPassword) -Operation invoke -TimeoutSeconds 900 `
        -ScriptBlock {
        param($expectedSize, $letter, $brokerLossOnline, $rawStopOnly,
            $manufacturedRefusalThenStop, $queueDepth,
            $manufacturedWorkloadFailure, $phaseTimeoutSeconds)
        $ErrorActionPreference = "Stop"
        $rows = [Collections.Generic.List[object]]::new()
        $formatRecoveryPath =
            "C:\ProgramData\RamShared\format-timeout-recovery.json"
        Remove-Item $formatRecoveryPath -Force -ErrorAction SilentlyContinue
        function Pass([string]$Name, [string]$Detail) {
            $rows.Add([pscustomobject]@{ test = $Name; verdict = "PASS"; detail = $Detail })
        }
        function Normalize-IdentityText([object]$Value) {
            return (([string]$Value -replace '\s+', ' ').Trim()).ToUpperInvariant()
        }
        function Get-CurrentRunOnlineEvidence(
            [uint32]$ServicePid,
            [DateTime]$StartedUtc,
            [UInt64]$ConfiguredSize
        ) {
            $evidenceRoot = "C:\ProgramData\RamShared\evidence"
            $deadline = (Get-Date).AddSeconds(75)
            $epoch = [DateTime]::SpecifyKind([DateTime]"1970-01-01", [DateTimeKind]::Utc)
            $startedUtcMs = [int64](
                ($StartedUtc.ToUniversalTime() - $epoch).TotalMilliseconds)
            $runPattern = "^run-$ServicePid-\d+-\d+$"
            do {
                $service = Get-CimInstance Win32_Service `
                    -Filter "Name='RamSharedWinSvc'" -ErrorAction Stop
                if ($service.State -ne "Running" -or
                    [uint32]$service.ProcessId -ne $ServicePid) {
                    throw "current winsvc run did not reach Online before service exit"
                }
                try {
                    $runFiles = @(Get-ChildItem -LiteralPath $evidenceRoot `
                            -Filter "run-$ServicePid-*.jsonl" -File `
                            -ErrorAction Stop | Where-Object {
                                $_.LastWriteTimeUtc -ge
                                    $StartedUtc.ToUniversalTime().AddSeconds(-2)
                            })
                }
                catch {
                    throw "current_online_evidence_failure_is_red: $($_.Exception.Message)"
                }
                if ($runFiles.Count -gt 1) {
                    throw "ambiguous current winsvc run evidence"
                }
                if ($runFiles.Count -eq 1) {
                    try {
                        $events = @(Get-Content -LiteralPath $runFiles[0].FullName `
                                -ErrorAction Stop | Where-Object {
                                    -not [string]::IsNullOrWhiteSpace($_)
                                } | ForEach-Object {
                                    $_ | ConvertFrom-Json -ErrorAction Stop
                                })
                    }
                    catch {
                        throw "current_online_evidence_failure_is_red: $($_.Exception.Message)"
                    }
                    if ($events.Count -eq 0) {
                        throw "current winsvc evidence file is empty"
                    }
                    $foreignRows = @($events | Where-Object {
                            [string]$_.run_id -cne $runFiles[0].BaseName -or
                            [uint32]$_.pid -ne $ServicePid
                        })
                    if ($foreignRows.Count -ne 0) {
                        throw "current winsvc evidence file contains a foreign row"
                    }
                    if (@($events | Where-Object {
                                [string]$_.phase -ceq "FailedSafe"
                            }).Count -ne 0) {
                        throw "current winsvc run entered FailedSafe before Online"
                    }
                    $onlineRows = @($events | Where-Object {
                            [string]$_.phase -ceq "Online"
                        })
                    if ($onlineRows.Count -gt 1) {
                        throw "current winsvc Online evidence is ambiguous"
                    }
                    if ($onlineRows.Count -eq 1) {
                        $online = $onlineRows[0]
                        if ([int]$online.schema -ne 1 -or
                            [string]$online.run_id -notmatch $runPattern -or
                            [string]$online.run_id -cne $runFiles[0].BaseName -or
                            [int64]$online.ts_utc_ms -lt ($startedUtcMs - 2000) -or
                            (Normalize-IdentityText $online.lun_vendor) -cne "RAMSHARE" -or
                            (Normalize-IdentityText $online.lun_product) -cne "VRAMDISK" -or
                            (Normalize-IdentityText $online.lun_serial) -notmatch '^[0-9A-F]{16}$' -or
                            [UInt64]$online.lun_size_bytes -ne $ConfiguredSize) {
                            throw "current winsvc Online identity is invalid"
                        }
                        return [pscustomobject]@{
                            run_id = [string]$online.run_id
                            serial = Normalize-IdentityText $online.lun_serial
                            size_bytes = [UInt64]$online.lun_size_bytes
                            evidence_path = $runFiles[0].FullName
                        }
                    }
                }
                Start-Sleep -Milliseconds 250
            } while ((Get-Date) -lt $deadline)
            throw "current winsvc run did not reach Online within 75 seconds"
        }
        function Resolve-ExactCurrentRunDisk([object]$OnlineEvidence) {
            $disk = $null
            for ($sample = 0; $sample -lt 120; $sample++) {
                $matches = @(Get-Disk -ErrorAction Stop | Where-Object {
                        (Normalize-IdentityText $_.FriendlyName) -ceq
                            "RAMSHARE VRAMDISK" -and
                        (Normalize-IdentityText $_.SerialNumber) -ceq
                            $OnlineEvidence.serial -and
                        [uint64]$_.Size -eq [uint64]$OnlineEvidence.size_bytes -and
                        -not $_.IsBoot -and -not $_.IsSystem
                    })
                if ($matches.Count -eq 1) {
                    $disk = $matches[0]
                    break
                }
                if ($matches.Count -gt 1) {
                    throw "ambiguous current-run RamShared disk identity"
                }
                Start-Sleep -Milliseconds 250
            }
            if (-not $disk) {
                throw "current-run RamShared disk did not appear within 30 seconds"
            }
            return $disk
        }
        function Get-CurrentRunDisk153Events(
            [DateTime]$StartTime,
            [DateTime]$EndTime
        ) {
            try {
                return @(Get-WinEvent -FilterHashtable @{
                            LogName = "System"; ProviderName = "disk"
                            StartTime = $StartTime; EndTime = $EndTime; Id = 153
                        } -ErrorAction Stop)
            }
            catch {
                if ($_.FullyQualifiedErrorId -match '^NoMatchingEventsFound') {
                    return @()
                }
                throw "event153_query_failure_is_red: $($_.Exception.Message)"
            }
        }
        if ((Get-Service RamSharedBroker).Status -ne "Stopped" -or
            (Get-Service RamSharedWinSvc).Status -ne "Stopped") {
            throw "demand services did not remain stopped after cold boot"
        }
        Pass "cold_boot_no_login" "demand services remained stopped; no user startup dependency"

        $startWatch = [Diagnostics.Stopwatch]::StartNew()
        $startUtc = [DateTime]::UtcNow
        Start-Service RamSharedWinSvc
        (Get-Service RamSharedWinSvc).WaitForStatus("Running", [timespan]::FromSeconds(30))
        $consumerService = Get-CimInstance Win32_Service `
            -Filter "Name='RamSharedWinSvc'"
        $consumerPid = [uint32]$consumerService.ProcessId
        if ($consumerPid -eq 0) {
            throw "winsvc reported Running without a process ID"
        }
        $onlineEvidence = Get-CurrentRunOnlineEvidence $consumerPid $startUtc `
            $expectedSize
        Pass "harness_waits_for_current_run_online" `
            "pid=$consumerPid; run=$($onlineEvidence.run_id); serial=$($onlineEvidence.serial); phase=Online"

        $disk = Resolve-ExactCurrentRunDisk $onlineEvidence
        $readyMs = [int]$startWatch.Elapsed.TotalMilliseconds

        $manifest = Get-Content "C:\ProgramData\RamShared\active-manifest.json" -Raw |
            ConvertFrom-Json
        $versionRoot = "C:\Program Files\RamShared\versions\$($manifest.version)-$($manifest.commit.Substring(0,12))"
        foreach ($name in @("RamSharedBroker", "RamSharedWinSvc")) {
            $svc = Get-CimInstance Win32_Service -Filter "Name='$name'"
            $process = Get-Process -Id $svc.ProcessId
            $role = if ($name -eq "RamSharedBroker") { "broker_exe" } else { "winsvc_exe" }
            $relative = ($manifest.artifacts | Where-Object role -eq $role).relative_path
            $expectedHash = (Get-FileHash (Join-Path $versionRoot $relative) -Algorithm SHA256).Hash
            $actualHash = (Get-FileHash $process.Path -Algorithm SHA256).Hash
            if ($expectedHash -ne $actualHash) { throw "$name BINARY_MATCH failed" }
            Pass "$($name)_BINARY_MATCH" "pid=$($svc.ProcessId); sha256=$actualHash"
        }
        $driver = Get-CimInstance Win32_SystemDriver | Where-Object Name -eq "ramshared"
        if (-not $driver -or $driver.State -ne "Running") {
            throw "ramshared driver is not Running"
        }
        $driverPath = ([string]$driver.PathName).Trim('"') -replace '^\\\?\?\\', ''
        if ($driverPath -like "\SystemRoot\*") {
            $driverPath = Join-Path $env:SystemRoot $driverPath.Substring(12)
        }
        $driverRelative = ($manifest.artifacts |
            Where-Object role -eq "driver_sys").relative_path
        $driverExpected = (Get-FileHash (Join-Path $versionRoot $driverRelative) `
                -Algorithm SHA256).Hash
        $driverActual = (Get-FileHash $driverPath -Algorithm SHA256).Hash
        if ($driverExpected -ne $driverActual) { throw "driver BINARY_MATCH failed" }
        Pass "RamSharedDriver_BINARY_MATCH" "sha256=$driverActual; path=$driverPath"
        Pass "broker_ready_to_online" "ready_ms=$readyMs"

        if ($manufacturedWorkloadFailure) {
            throw "MANUFACTURED_WORKLOAD_FAILURE_AFTER_ONLINE"
        }

        if ($manufacturedRefusalThenStop) {
            $pagingKey = "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management"
            $snapshot = @((Get-ItemProperty $pagingKey -Name PagingFiles `
                        -ErrorAction Stop).PagingFiles)
            $entry = "$letter`:\pagefile.sys 16 16"
            $stopJob = $null
            try {
                Remove-Item "C:\ProgramData\RamShared\teardown-diag.log" `
                    -Force -ErrorAction SilentlyContinue
                Set-ItemProperty $pagingKey -Name PagingFiles `
                    -Value (@($snapshot) + @($entry)) -Type MultiString
                $stopJob = Start-Job -ScriptBlock {
                    try {
                        Stop-Service RamSharedWinSvc -ErrorAction Stop
                        [pscustomobject]@{ returned = "success"; error = "" }
                    }
                    catch {
                        [pscustomobject]@{
                            returned = "refused"
                            error = $_.Exception.Message
                        }
                    }
                }
                $refusalDeadline = (Get-Date).AddSeconds(25)
                $refusalObserved = $false
                do {
                    $diag = Get-Content "C:\ProgramData\RamShared\teardown-diag.log" `
                        -Raw -ErrorAction SilentlyContinue
                    if ($diag -match "gate_a_active") {
                        $refusalObserved = $true
                        break
                    }
                    Start-Sleep -Milliseconds 100
                } while ((Get-Date) -lt $refusalDeadline)
                if (-not $refusalObserved) {
                    throw "manufactured pagefile refusal was not observed"
                }
                $pendingDeadline = (Get-Date).AddSeconds(5)
                do {
                    $pending = Get-Service RamSharedWinSvc
                    if ($pending.Status -eq "StopPending") { break }
                    Start-Sleep -Milliseconds 50
                } while ((Get-Date) -lt $pendingDeadline)
                if ($pending.Status -ne "StopPending") {
                    throw "refused STOP did not remain StopPending; observed=$($pending.Status)"
                }
                Pass "refusal_state_observation_is_bounded" `
                    "status=StopPending; deadline_seconds=5"
                $stopCallerState = $stopJob.State.ToString()
            }
            finally {
                Set-ItemProperty $pagingKey -Name PagingFiles `
                    -Value ([string[]]$snapshot) -Type MultiString
            }
            try {
                if (-not (Wait-Job $stopJob -Timeout $phaseTimeoutSeconds)) {
                    Stop-Job $stopJob -ErrorAction SilentlyContinue
                    throw "same STOP did not complete after pagefile restoration"
                }
                $stopResult = Receive-Job $stopJob -ErrorAction Stop
            }
            finally {
                Remove-Job $stopJob -Force -ErrorAction SilentlyContinue
            }
            if ($stopResult.returned -ne "success" -or
                (Get-Service RamSharedWinSvc).Status -ne "Stopped") {
                throw "same STOP did not reach Stopped cleanly"
            }
            Pass "scm_refusal_pending_then_same_stop" `
                "GateA refused; StopPending preserved; caller=$stopCallerState; registry restored; same STOP completed"

            $restartUtc = [DateTime]::UtcNow
            Start-Service RamSharedWinSvc
            (Get-Service RamSharedWinSvc).WaitForStatus(
                "Running", [timespan]::FromSeconds(30))
            $restartService = Get-CimInstance Win32_Service `
                -Filter "Name='RamSharedWinSvc'" -ErrorAction Stop
            $restartPid = [uint32]$restartService.ProcessId
            if ($restartPid -eq 0) {
                throw "winsvc restart reported Running without a process ID"
            }
            $onlineEvidence = Get-CurrentRunOnlineEvidence $restartPid $restartUtc `
                $expectedSize
            $disk = Resolve-ExactCurrentRunDisk $onlineEvidence
            Pass "post_refusal_clean_restart" `
                "service=Running; run=$($onlineEvidence.run_id); exact_disk=$($disk.Number)"
        }

        $hashes = @()
        $ioEventStart = Get-Date
        if ($rawStopOnly) {
            $partitions = @(Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue)
            if ($disk.PartitionStyle -ne "RAW" -or $partitions.Count -ne 0) {
                throw "RAW stop precondition failed: style=$($disk.PartitionStyle); partitions=$($partitions.Count)"
            }
            Pass "exact_raw_lun_before_stop" `
                "disk=$($disk.Number); style=RAW; partitions=0; size=$($disk.Size)"
        }
        else {
            if ($disk.PartitionStyle -eq "RAW") {
                $formatBeganRaw = $true
                $formatDiskNumber = [int]$disk.Number
                $formatSerial = [string]$disk.SerialNumber
                $formatJob = Start-Job -ScriptBlock {
                    param($diskNumber, $driveLetter)
                    $ErrorActionPreference = "Stop"
                    Initialize-Disk -Number $diskNumber -PartitionStyle GPT -PassThru |
                        New-Partition -UseMaximumSize -DriveLetter $driveLetter |
                        Format-Volume -FileSystem NTFS -NewFileSystemLabel RAMSHARE `
                            -Confirm:$false -Force | Out-Null
                } -ArgumentList $disk.Number, $letter
                try {
                    if (-not (Wait-Job $formatJob -Timeout $phaseTimeoutSeconds)) {
                        Stop-Job $formatJob -ErrorAction SilentlyContinue
                        $recoveryJob = Start-Job -ScriptBlock {
                            param($diskNumber, $serial, $size, $driveLetter,
                                $recoveryPath, $beganRaw)
                            $ErrorActionPreference = "Stop"
                            if (-not $beganRaw) {
                                throw "refuse recovery: LUN did not begin RAW"
                            }
                            $exact = @(Get-Disk -ErrorAction Stop | Where-Object {
                                    $_.Number -eq $diskNumber -and
                                    $_.FriendlyName -eq "RAMSHARE VRAMDISK" -and
                                    [string]$_.SerialNumber -eq $serial -and
                                    [uint64]$_.Size -eq [uint64]$size -and
                                    -not $_.IsBoot -and -not $_.IsSystem
                                })
                            if ($exact.Count -ne 1) {
                                throw "exact recovery identity count=$($exact.Count)"
                            }
                            $partitions = @(Get-Partition -DiskNumber $diskNumber `
                                    -ErrorAction Stop)
                            if ($partitions.Count -ne 1 -or
                                [string]$partitions[0].DriveLetter -ne $driveLetter) {
                                throw "recovery partition mismatch"
                            }
                            try {
                                $volumes = @($partitions | Get-Volume `
                                        -ErrorAction Stop)
                            }
                            catch {
                                throw "recovery_volume_query_failure_is_red: $($_.Exception.Message)"
                            }
                            if ($volumes.Count -ne 0) {
                                throw "refuse recovery: volume published"
                            }
                            $record = [ordered]@{
                                schema = 1
                                before = [ordered]@{
                                    disk_number = $diskNumber
                                    serial = $serial
                                    size_bytes = [uint64]$size
                                    partition_style =
                                        $exact[0].PartitionStyle.ToString()
                                    partition_count = $partitions.Count
                                    volume_count = $volumes.Count
                                }
                                action = "Clear-Disk exact volatile scratch LUN"
                                after = $null
                            }
                            Clear-Disk -Number $diskNumber -RemoveData -RemoveOEM `
                                -Confirm:$false
                            $after = Get-Disk -Number $diskNumber -ErrorAction Stop
                            if ($after.PartitionStyle -ne "RAW") {
                                throw "exact scratch recovery did not return RAW"
                            }
                            $record.after = [ordered]@{
                                partition_style = $after.PartitionStyle.ToString()
                                partition_count = @(Get-Partition `
                                        -DiskNumber $diskNumber `
                                        -ErrorAction SilentlyContinue).Count
                            }
                            [IO.File]::WriteAllText($recoveryPath,
                                ($record | ConvertTo-Json -Depth 6),
                                [Text.UTF8Encoding]::new($false))
                        } -ArgumentList $formatDiskNumber, $formatSerial,
                            $expectedSize, $letter, $formatRecoveryPath,
                            $formatBeganRaw
                        try {
                            if (-not (Wait-Job $recoveryJob -Timeout $phaseTimeoutSeconds)) {
                                Stop-Job $recoveryJob -ErrorAction SilentlyContinue
                                throw "exact format recovery exceeded budget"
                            }
                            Receive-Job $recoveryJob -ErrorAction Stop | Out-Null
                        }
                        finally {
                            Remove-Job $recoveryJob -Force `
                                -ErrorAction SilentlyContinue
                        }
                        throw "initialize/partition/format exceeded 60 seconds"
                    }
                    Receive-Job $formatJob -ErrorAction Stop | Out-Null
                }
                finally {
                    Remove-Job $formatJob -Force -ErrorAction SilentlyContinue
                }
            }
            $volume = Get-Volume -DriveLetter $letter -ErrorAction Stop
            $volumeDisk = (Get-Partition -DriveLetter $letter | Get-Disk)
            if ($volumeDisk.Number -ne $disk.Number -or $volume.FileSystemLabel -ne "RAMSHARE") {
                throw "refuse foreign volume binding"
            }

            for ($round = 1; $round -le 3; $round++) {
                $path = "$letter`:\ramshared-round-$round.bin"
                $payload = New-Object byte[] (8MB)
                $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
                try { $rng.GetBytes($payload) } finally { $rng.Dispose() }
                [IO.File]::WriteAllBytes($path, $payload)
                $sha = [Security.Cryptography.SHA256]::Create()
                try {
                    $writeHash = ([BitConverter]::ToString(
                            $sha.ComputeHash($payload))).Replace("-", "")
                }
                finally { $sha.Dispose() }
                $readHash = (Get-FileHash $path -Algorithm SHA256).Hash
                if ($writeHash -ne $readHash) { throw "SHA mismatch round $round" }
                $hashes += $readHash
                Remove-Item $path -Force
            }
            Pass "three_round_sha" (($hashes | ForEach-Object { "sha256=$_" }) -join "; ")

            if ($queueDepth -eq 1) {
                $mixedKind = "mixed70r30w"
                for ($mixedRound = 1; $mixedRound -le 3; $mixedRound++) {
                    $mixedPath = "$letter`:\ramshared-$mixedKind-$mixedRound.bin"
                    $fileBytes = 8MB
                    $expected = New-Object byte[] $fileBytes
                    $small = New-Object byte[] 4096
                    $read = New-Object byte[] 4096
                    $random = [Random]::new(32000 + $mixedRound)
                    $stream = $null
                    try {
                        $stream = [IO.File]::Open($mixedPath, "Create",
                            "ReadWrite", "ReadWrite")
                        $stream.SetLength($fileBytes)
                        for ($operation = 0; $operation -lt 512; $operation++) {
                            $slot = $random.Next(0, $fileBytes / 4096)
                            $offset = $slot * 4096
                            $stream.Position = $offset
                            if ($random.Next(0, 10) -ge 7) {
                                $random.NextBytes($small)
                                $stream.Write($small, 0, $small.Length)
                                [Array]::Copy($small, 0, $expected, $offset,
                                    $small.Length)
                            }
                            else {
                                $got = $stream.Read($read, 0, $read.Length)
                                if ($got -ne $read.Length) {
                                    throw "QD1 mixed short read"
                                }
                                for ($byte = 0; $byte -lt $read.Length; $byte++) {
                                    if ($read[$byte] -ne $expected[$offset + $byte]) {
                                        throw "QD1 mixed read mismatch"
                                    }
                                }
                            }
                        }
                        $stream.Flush($true)
                    }
                    finally {
                        if ($stream) { $stream.Dispose() }
                    }
                    $actual = [IO.File]::ReadAllBytes($mixedPath)
                    $sha = [Security.Cryptography.SHA256]::Create()
                    try {
                        $expectedHash = [BitConverter]::ToString(
                            $sha.ComputeHash($expected))
                        $actualHash = [BitConverter]::ToString(
                            $sha.ComputeHash($actual))
                    }
                    finally { $sha.Dispose() }
                    if ($expectedHash -ne $actualHash) {
                        throw "QD1 mixed final checksum mismatch"
                    }
                    Remove-Item $mixedPath -Force
                }
            }

            $retryEvents = Get-CurrentRunDisk153Events $ioEventStart (Get-Date)
            if ($retryEvents.Count -ne 0) {
                throw "registered QD emitted disk Event 153 retries count=$($retryEvents.Count) qd=$queueDepth"
            }
            Pass "all_registered_depths_have_zero_disk_retries" `
                "queue_depth=$queueDepth; event153=0"
            if ($queueDepth -eq 1) {
                Pass "qd1_mixed_flush_has_zero_retry_events" `
                    "rounds=3; operations_per_round=512; event153=0; checksums=match"
            }
        }

        $stopWatch = [Diagnostics.Stopwatch]::StartNew()
        $stopRequestError = $null
        if ($brokerLossOnline) {
            $brokerService = Get-CimInstance Win32_Service -Filter "Name='RamSharedBroker'"
            Stop-Process -Id $brokerService.ProcessId -Force
            (Get-Service RamSharedWinSvc).WaitForStatus(
                "Stopped", [timespan]::FromSeconds(30))
            $consumerStopMs = [int]$stopWatch.Elapsed.TotalMilliseconds
            $diag = Get-Content "C:\ProgramData\RamShared\teardown-diag.log" -Raw
            if ($diag -notmatch "broker_lost_online" -or
                $diag -notmatch "safe teardown completed after broker loss") {
                throw "broker-loss containment evidence incomplete"
            }
            $brokerNow = Get-Service RamSharedBroker
            if ($brokerNow.Status -ne "Stopped") {
                Stop-Service RamSharedBroker
                $brokerNow.WaitForStatus("Stopped", [timespan]::FromSeconds(15))
            }
            $productStopMs = [int]$stopWatch.Elapsed.TotalMilliseconds
            Pass "BrokerLossOnline" `
                "consumer_stop_ms=$consumerStopMs; no reconnect; safe teardown completed"
        }
        else {
            try { Stop-Service RamSharedWinSvc -ErrorAction Stop }
            catch { $stopRequestError = $_.Exception.Message }
            (Get-Service RamSharedWinSvc).WaitForStatus("Stopped", [timespan]::FromSeconds(30))
            $consumerStopMs = [int]$stopWatch.Elapsed.TotalMilliseconds
            if ($stopRequestError) {
                throw "SCM stop returned an error despite reaching Stopped: $stopRequestError"
            }
            Stop-Service RamSharedBroker
            (Get-Service RamSharedBroker).WaitForStatus("Stopped", [timespan]::FromSeconds(15))
            $productStopMs = [int]$stopWatch.Elapsed.TotalMilliseconds
            Pass "consumer_first_stop" `
                "consumer_stop_ms=$consumerStopMs; product_stop_ms=$productStopMs"
            Pass "scm_stop_returns_cleanly" "Stop-Service returned success; state=Stopped"
        }

        $residueJob = Start-Job -ScriptBlock {
            @(Get-CimInstance Win32_DiskDrive -ErrorAction Stop | Where-Object {
                    $_.Model -match "RAMSHARE|VRAMDISK" -or
                    $_.Caption -match "RAMSHARE|VRAMDISK"
                }).Count
        }
        try {
            if (-not (Wait-Job $residueJob -Timeout $phaseTimeoutSeconds)) {
                Stop-Job $residueJob -ErrorAction SilentlyContinue
                throw "Win32_DiskDrive zero-residue query timed out"
            }
            $remaining = [int](Receive-Job $residueJob -ErrorAction Stop)
        }
        finally { Remove-Job $residueJob -Force -ErrorAction SilentlyContinue }
        if ($remaining -ne 0) { throw "RamShared Win32_DiskDrive residue after stop" }
        $evidence = Get-Content "C:\ProgramData\RamShared\evidence\broker.jsonl" `
            -ErrorAction Stop -Raw
        if (-not $brokerLossOnline) {
            if ($evidence -notmatch '"transition":"lease_released_explicit"') {
                throw "broker evidence has no lease release"
            }
            Pass "lease_release" "broker JSONL contains lease_released"
        }
        Pass "zero_residue" "Win32_DiskDrive_count=0; services=Stopped; stop_request_error=$stopRequestError"

        [pscustomobject]@{
            rows = $rows
            metrics = [ordered]@{
                readiness_ms = $readyMs
                consumer_stop_ms = $consumerStopMs
                product_stop_ms = $productStopMs
                sha256 = $hashes
            }
        }
    } -ArgumentList @(
        $ExpectedSizeBytes, $Letter, ([bool]$BrokerLossOnline),
        ([bool]$RawStopOnly), ([bool]$ManufacturedRefusalThenStop), $QueueDepth,
        ([bool]$ManufacturedWorkloadFailure), $PhaseTimeoutSeconds)

    $results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $hostRun "results.json")
    $results.rows
    Write-Host "EVIDENCE=$hostRun"
}
catch {
    $primaryError = $_
    $cleanupRecord = [ordered]@{
        schema = 1
        primary_error = $primaryError.Exception.Message
        cleanup_error = $null
        cleanup = $null
    }
    try {
        if ((Get-VM $VMName -ErrorAction Stop).State -eq "Running") {
            $cleanupRecord.cleanup = Invoke-GuestPsDirectBounded `
                -VMName $VMName -User $User -Password (Get-DrillPassword) `
                -Operation invoke -TimeoutSeconds 210 -ScriptBlock {
                $ErrorActionPreference = "Stop"
                $consumer = Get-Service RamSharedWinSvc -ErrorAction SilentlyContinue
                if ($consumer -and $consumer.Status -ne "Stopped") {
                    & sc.exe stop RamSharedWinSvc 2>&1 | Out-Null
                    $deadline = (Get-Date).AddSeconds(60)
                    do {
                        $consumer.Refresh()
                        if ($consumer.Status -eq "Stopped") { break }
                        Start-Sleep -Milliseconds 250
                    } while ((Get-Date) -lt $deadline)
                }
                $consumer_state = if ($consumer) {
                    $consumer.Refresh()
                    $consumer.Status.ToString()
                } else { "Missing" }

                $broker = Get-Service RamSharedBroker -ErrorAction SilentlyContinue
                if ($consumer_state -eq "Stopped" -and $broker -and
                    $broker.Status -ne "Stopped") {
                    & sc.exe stop RamSharedBroker 2>&1 | Out-Null
                    $deadline = (Get-Date).AddSeconds(30)
                    do {
                        $broker.Refresh()
                        if ($broker.Status -eq "Stopped") { break }
                        Start-Sleep -Milliseconds 250
                    } while ((Get-Date) -lt $deadline)
                }
                $broker_state = if ($broker) {
                    $broker.Refresh()
                    $broker.Status.ToString()
                } else { "Missing" }
                $liveDisks = @(Get-Disk -ErrorAction Stop | Where-Object {
                        $_.FriendlyName -match "RAMSHARE"
                    })
                $liveWin32 = @(Get-CimInstance Win32_DiskDrive -ErrorAction Stop |
                    Where-Object {
                        $_.Model -match "RAMSHARE|VRAMDISK" -or
                        $_.Caption -match "RAMSHARE|VRAMDISK"
                    })
                [pscustomobject]@{
                    consumer_state = $consumer_state
                    broker_state = $broker_state
                    ramshared_disks = $liveDisks.Count
                    ramshared_win32_disks = $liveWin32.Count
                    format_recovery = if (Test-Path `
                        "C:\ProgramData\RamShared\format-timeout-recovery.json") {
                        Get-Content `
                            "C:\ProgramData\RamShared\format-timeout-recovery.json" `
                            -Raw | ConvertFrom-Json
                    } else { $null }
                    supported_consumer_first = $true
                    force_kill = $false
                }
            }
        }
    }
    catch {
        $cleanupRecord.cleanup_error = $_.Exception.Message
    }
    $cleanupRecord | ConvertTo-Json -Depth 8 |
        Set-Content (Join-Path $hostRun "failure-cleanup.json")
    throw $primaryError
}
