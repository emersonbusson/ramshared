#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$VMName = "win11-drill",
    [string]$User = "WIN11-DRILL\drilladmin",
    [string]$Password = "",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [UInt64]$ExpectedSizeBytes = 67108864,
    [string]$Letter = "R",
    [switch]$BrokerLossOnline
)

$ErrorActionPreference = "Stop"
function Get-DrillPassword {
    if ($Password) { return $Password }
    foreach ($scope in @("Machine", "User")) {
        $value = [Environment]::GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)
        if ($value) { return $value }
    }
    if ($env:RAMSHARED_DRILL_PASSWORD) { return $env:RAMSHARED_DRILL_PASSWORD }
    throw "Missing RAMSHARED_DRILL_PASSWORD."
}
function New-GuestSession([pscredential]$Credential) {
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        try { return New-PSSession -VMName $VMName -Credential $Credential -ErrorAction Stop }
        catch { Start-Sleep -Seconds 3 }
    }
    throw "PowerShell Direct unavailable after 180 seconds"
}

$credential = [pscredential]::new($User,
    (ConvertTo-SecureString (Get-DrillPassword) -AsPlainText -Force))
$packageHarness = Join-Path $PSScriptRoot "Run-GuestProductPackage.ps1"
& $packageHarness -VMName $VMName -User $User -Password (Get-DrillPassword) `
    -ArtifactRoot $ArtifactRoot -Case FreshInstall | Out-Host

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$hostRun = Join-Path $ArtifactRoot "autonomous-lifecycle-$stamp"
New-Item $hostRun -ItemType Directory -Force | Out-Null
$session = New-GuestSession $credential
try {
    $before = Invoke-Command -Session $session -ScriptBlock {
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
    Invoke-Command -Session $session -ScriptBlock { shutdown.exe /s /t 0 }
    Remove-PSSession $session -ErrorAction SilentlyContinue
    $session = $null
    $offDeadline = (Get-Date).AddSeconds(120)
    while ((Get-VM $VMName).State -ne "Off" -and (Get-Date) -lt $offDeadline) {
        Start-Sleep -Seconds 2
    }
    if ((Get-VM $VMName).State -ne "Off") {
        throw "guest did not complete a supported shutdown within 120 seconds"
    }
    Start-VM -Name $VMName | Out-Null
    Start-Sleep -Seconds 8
    $session = New-GuestSession $credential

    $results = Invoke-Command -Session $session -ScriptBlock {
        param($expectedSize, $letter, $brokerLossOnline)
        $ErrorActionPreference = "Stop"
        $rows = [Collections.Generic.List[object]]::new()
        function Pass([string]$Name, [string]$Detail) {
            $rows.Add([pscustomobject]@{ test = $Name; verdict = "PASS"; detail = $Detail })
        }
        if ((Get-Service RamSharedBroker).Status -ne "Stopped" -or
            (Get-Service RamSharedWinSvc).Status -ne "Stopped") {
            throw "demand services did not remain stopped after cold boot"
        }
        Pass "cold_boot_no_login" "demand services remained stopped; no user startup dependency"

        $startWatch = [Diagnostics.Stopwatch]::StartNew()
        Start-Service RamSharedWinSvc
        (Get-Service RamSharedWinSvc).WaitForStatus("Running", [timespan]::FromSeconds(30))
        $disk = $null
        for ($sample = 0; $sample -lt 120; $sample++) {
            $matches = @(Get-Disk -ErrorAction Stop | Where-Object {
                    $_.FriendlyName -match "RAMSHARE" -and [uint64]$_.Size -eq $expectedSize
                })
            if ($matches.Count -eq 1) { $disk = $matches[0]; break }
            if ($matches.Count -gt 1) { throw "ambiguous RamShared disk identity" }
            Start-Sleep -Milliseconds 250
        }
        if (-not $disk) { throw "product disk did not appear within 30 seconds" }
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

        if ($disk.PartitionStyle -eq "RAW") {
            $formatJob = Start-Job -ScriptBlock {
                param($diskNumber, $driveLetter)
                $ErrorActionPreference = "Stop"
                Initialize-Disk -Number $diskNumber -PartitionStyle GPT -PassThru |
                    New-Partition -UseMaximumSize -DriveLetter $driveLetter |
                    Format-Volume -FileSystem NTFS -NewFileSystemLabel RAMSHARE `
                        -Confirm:$false -Force | Out-Null
            } -ArgumentList $disk.Number, $letter
            try {
                if (-not (Wait-Job $formatJob -Timeout 60)) {
                    Stop-Job $formatJob -ErrorAction SilentlyContinue
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

        $hashes = @()
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
            Stop-Service RamSharedBroker
            (Get-Service RamSharedBroker).WaitForStatus("Stopped", [timespan]::FromSeconds(15))
            $productStopMs = [int]$stopWatch.Elapsed.TotalMilliseconds
            Pass "consumer_first_stop" `
                "consumer_stop_ms=$consumerStopMs; product_stop_ms=$productStopMs"
        }

        $residueJob = Start-Job -ScriptBlock {
            @(Get-CimInstance Win32_DiskDrive -ErrorAction Stop | Where-Object {
                    $_.Model -match "RAMSHARE|VRAMDISK" -or
                    $_.Caption -match "RAMSHARE|VRAMDISK"
                }).Count
        }
        try {
            if (-not (Wait-Job $residueJob -Timeout 10)) {
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
    } -ArgumentList $ExpectedSizeBytes, $Letter, ([bool]$BrokerLossOnline)

    $results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $hostRun "results.json")
    $results.rows
    Write-Host "EVIDENCE=$hostRun"
}
finally {
    if ($session) { Remove-PSSession $session }
}
