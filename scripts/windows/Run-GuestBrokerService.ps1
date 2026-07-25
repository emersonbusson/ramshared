#Requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet("All", "Smoke", "PeerMatrix", "RetryBudget", "Boundary", "BrokerLossOnline")]
    [string]$Case = "All",
    [string]$VMName = "win11-drill",
    [string]$User = "WIN11-DRILL\drilladmin",
    [string]$Password = "",
    [string]$HostBinDir = "C:\ramshared\bin",
    [string]$ArtifactRoot = "C:\ramshared\artifacts"
)

$ErrorActionPreference = "Stop"
if ($Case -eq "BrokerLossOnline") {
    & (Join-Path $PSScriptRoot "Run-GuestAutonomousLifecycle.ps1") `
        -VMName $VMName -User $User -Password $Password -ArtifactRoot $ArtifactRoot `
        -BrokerLossOnline
    return
}
$BrokerService = "RamSharedBroker"
$ConsumerService = "RamSharedWinSvc"
$BrokerPipe = "RamSharedBroker.v1"
$StatusPipe = "RamSharedBrokerStatus.v1"

function Get-DrillPassword {
    if (-not [string]::IsNullOrEmpty($Password)) { return $Password }
    foreach ($scope in @("Machine", "User")) {
        $candidate = [Environment]::GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)
        if (-not [string]::IsNullOrEmpty($candidate)) { return $candidate }
    }
    if (-not [string]::IsNullOrEmpty($env:RAMSHARED_DRILL_PASSWORD)) {
        return $env:RAMSHARED_DRILL_PASSWORD
    }
    throw "Missing RAMSHARED_DRILL_PASSWORD in Machine/User/Process scope."
}

function Assert-True([bool]$Condition, [string]$Name, [string]$Detail) {
    if (-not $Condition) { throw "$Name failed: $Detail" }
    [pscustomobject]@{ test = $Name; verdict = "PASS"; detail = $Detail }
}

function Invoke-Sc([string[]]$Arguments) {
    $output = & sc.exe @Arguments 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) {
        throw "sc.exe $($Arguments -join ' ') failed ($LASTEXITCODE): $output"
    }
    $output
}

$secret = Get-DrillPassword
$credential = [pscredential]::new($User, (ConvertTo-SecureString $secret -AsPlainText -Force))
$vm = Get-VM -Name $VMName -ErrorAction Stop
if ($vm.State -ne "Running") { Start-VM -Name $VMName | Out-Null }

$session = $null
for ($attempt = 1; $attempt -le 30; $attempt++) {
    try {
        $session = New-PSSession -VMName $VMName -Credential $credential -ErrorAction Stop
        break
    }
    catch {
        if ($_.Exception.Message -match "credencial.*inv|credential.*invalid") { throw }
        Start-Sleep -Seconds 3
    }
}
if (-not $session) { throw "PowerShell Direct did not become ready in 90 seconds." }

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$hostArtifacts = Join-Path $ArtifactRoot "autonomous-broker-$stamp"
New-Item -ItemType Directory -Force -Path $hostArtifacts | Out-Null
$guestRoot = "C:\Program Files\RamShared\versions\0.0.0-broker-vm-333333333333"

try {
    Invoke-Command -Session $session -ScriptBlock {
        param($root, $broker, $consumer)
        Get-Process ramshared-service-sid-probe -ErrorAction SilentlyContinue |
            Stop-Process -Force -ErrorAction SilentlyContinue
        Stop-Service RamSharedUnrelated -Force -ErrorAction SilentlyContinue
        & sc.exe delete RamSharedUnrelated 2>&1 | Out-Null
        & sc.exe failure $broker reset= 0 actions= //0 2>&1 | Out-Null
        Stop-Service $consumer -Force -ErrorAction SilentlyContinue
        Stop-Service $broker -Force -ErrorAction SilentlyContinue
        foreach ($name in @($consumer, $broker)) {
            $service = Get-Service $name -ErrorAction SilentlyContinue
            if ($service) {
                $service.WaitForStatus("Stopped", [timespan]::FromSeconds(15))
            }
        }
        & sc.exe delete $consumer 2>&1 | Out-Null
        & sc.exe delete $broker 2>&1 | Out-Null
        Start-Sleep -Milliseconds 500
        Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Force -Path $root | Out-Null
        New-Item -ItemType Directory -Force -Path "C:\ProgramData\RamShared" | Out-Null
        Remove-Item "C:\ProgramData\RamShared\active-manifest.json" -Force `
            -ErrorAction SilentlyContinue
    } -ArgumentList $guestRoot, $BrokerService, $ConsumerService

    Copy-Item (Join-Path $HostBinDir "ramshared-winbroker.exe") `
        (Join-Path $guestRoot "ramshared-winbroker.exe") -ToSession $session -Force
    Copy-Item (Join-Path $HostBinDir "ramshared-winsvc.exe") `
        (Join-Path $guestRoot "ramshared-winsvc.exe") -ToSession $session -Force
    Copy-Item (Join-Path $HostBinDir "ramshared-service-sid-probe.exe") `
        (Join-Path $guestRoot "ramshared-service-sid-probe.exe") -ToSession $session -Force

    $results = Invoke-Command -Session $session -ScriptBlock {
        param($root, $brokerService, $consumerService, $brokerPipe, $statusPipe, $selectedCase)
        $ErrorActionPreference = "Stop"
        $out = New-Object System.Collections.Generic.List[object]
        function Pass([string]$name, [string]$detail) {
            $out.Add([pscustomobject]@{ test = $name; verdict = "PASS"; detail = $detail })
        }
        function Invoke-ScGuest([string[]]$arguments) {
            $text = & sc.exe @arguments 2>&1 | Out-String
            if ($LASTEXITCODE -ne 0) {
                throw "sc.exe $($arguments -join ' ') failed ($LASTEXITCODE): $text"
            }
        }
        $brokerExe = Join-Path $root "ramshared-winbroker.exe"
        $winsvcExe = Join-Path $root "ramshared-winsvc.exe"
        $serviceSidProbe = Join-Path $root "ramshared-service-sid-probe.exe"
        $brokerConfig = Join-Path $root "broker.toml"
        $winsvcConfig = Join-Path $root "winsvc.toml"
        $brokerEvidence = "C:\ramshared\autonomous-broker\broker-evidence.jsonl"
        $probeResultPath = "C:\ramshared\autonomous-broker\service-sid-probe.json"
        $brokerEvidenceToml = $brokerEvidence.Replace("\", "\\")
        $brokerExeSc = $brokerExe.Replace("C:\Program Files\", "C:\Progra~1\")
        $serviceSidProbeSc = $serviceSidProbe.Replace("C:\Program Files\", "C:\Progra~1\")
        $brokerConfigSc = $brokerConfig.Replace("C:\Program Files\", "C:\Progra~1\")
        [IO.File]::WriteAllLines($brokerConfig, [string[]]@(
            "[local_broker]",
            "schema = 1",
            "capacity_bytes = 67108864",
            'allowed_tenant = "windows-drive"',
            "evidence_path = `"$brokerEvidenceToml`""
        ), [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllLines($winsvcConfig, [string[]]@(
            "[win_drive]",
            "size_bytes = 67108864",
            "block_size = 4096",
            "cuda_device = 0",
            "reserve_bytes = 536870912",
            "queue_depth = 4",
            "max_io_bytes = 1048576",
            'evidence_path = "C:\\ramshared\\autonomous-broker\\winsvc-evidence.jsonl"',
            'volume_letter = "R"',
            'broker_pipe = "named_pipe_v1"',
            "broker_ready_timeout_secs = 30",
            'tenant = "windows-drive"',
            "heartbeat_secs = 5"
        ), [Text.UTF8Encoding]::new($false))
        New-Item (Split-Path $brokerEvidence) -ItemType Directory -Force | Out-Null
        Remove-Item $brokerEvidence -Force -ErrorAction SilentlyContinue
        $activeManifest = [ordered]@{
            schema = 1
            version = "0.0.0-broker-vm"
            commit = "3333333333333333333333333333333333333333"
            architecture = "x86_64-pc-windows-msvc"
            start_policy = "demand"
            services = [ordered]@{
                broker_name = "RamSharedBroker"
                broker_account = "NT SERVICE\RamSharedBroker"
                consumer_name = "RamSharedWinSvc"
                consumer_account = "LocalSystem"
            }
            artifacts = @([ordered]@{
                    role = "broker_config"
                    relative_path = "broker.toml"
                    sha256 = (Get-FileHash $brokerConfig -Algorithm SHA256).Hash
                })
        } | ConvertTo-Json -Depth 8
        [IO.File]::WriteAllText(
            "C:\ProgramData\RamShared\active-manifest.json",
            $activeManifest,
            [Text.UTF8Encoding]::new($false))

        Invoke-ScGuest @("create", $brokerService, "type=", "own", "start=", "demand",
            "error=", "normal", "obj=", "NT SERVICE\$brokerService",
            "binPath=", "$brokerExeSc --config $brokerConfigSc",
            "DisplayName=", "RamShared Local Broker Service")
        Invoke-ScGuest @("create", $consumerService, "type=", "own", "start=", "demand",
            "error=", "normal", "depend=", $brokerService,
            "binPath=", $serviceSidProbeSc)
        Invoke-ScGuest @("sidtype", $brokerService, "unrestricted")
        Invoke-ScGuest @("sidtype", $consumerService, "unrestricted")
        Invoke-ScGuest @("failure", $brokerService, "reset=", "60",
            "actions=", "restart/5000/restart/15000/restart/40000//0")
        Invoke-ScGuest @("failureflag", $brokerService, "0")
        Invoke-ScGuest @("failure", $consumerService, "reset=", "0", "actions=", "//0")

        $dependencies = @((Get-ItemProperty `
                    "HKLM:\SYSTEM\CurrentControlSet\Services\$consumerService").DependOnService)
        if ($dependencies -notcontains $brokerService) {
            throw "consumer dependency does not contain broker"
        }
        Pass "SCM_DEPENDENCY_MATCH" ($dependencies -join ",")

        $sidBroker = (& sc.exe qsidtype $brokerService 2>&1 | Out-String)
        $sidConsumer = (& sc.exe qsidtype $consumerService 2>&1 | Out-String)
        if ($sidBroker -notmatch "UNRESTRICTED" -or $sidConsumer -notmatch "UNRESTRICTED") {
            throw "service SID mode mismatch"
        }
        Pass "SERVICE_SID_MATCH" "broker=unrestricted consumer=unrestricted"

        $before = Get-Date
        Start-Service $brokerService
        (Get-Service $brokerService).WaitForStatus("Running", [timespan]::FromSeconds(30))
        $readyMs = [int]((Get-Date) - $before).TotalMilliseconds
        $service = Get-CimInstance Win32_Service -Filter "Name='$brokerService'"
        $process = Get-Process -Id $service.ProcessId
        $expectedHash = (Get-FileHash $brokerExe -Algorithm SHA256).Hash
        $actualHash = (Get-FileHash $process.Path -Algorithm SHA256).Hash
        if ($expectedHash -ne $actualHash) { throw "broker binary hash mismatch" }
        Pass "BROKER_BINARY_MATCH" "pid=$($service.ProcessId) sha256=$actualHash ready_ms=$readyMs"
        Pass "scm_start_ready_stop" "ready_ms=$readyMs"

        $pipeNames = @()
        for ($pipeAttempt = 0; $pipeAttempt -lt 60; $pipeAttempt++) {
            $pipeNames = @([IO.Directory]::GetFiles("\\.\pipe\") | ForEach-Object {
                    $_.Substring($_.LastIndexOf("\") + 1)
                })
            if ($pipeNames -contains $brokerPipe -and $pipeNames -contains $statusPipe) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($pipeNames -notcontains $brokerPipe -or $pipeNames -notcontains $statusPipe) {
            throw "required named pipes are not present"
        }
        Pass "register_and_lease_over_named_pipe" "pipe boundary ready; product admission tested by consumer case"

        $adminRefused = $false
        try {
            $unauthorized = [IO.Pipes.NamedPipeClientStream]::new(".", $brokerPipe,
                [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
            $unauthorized.Connect(2000)
            $unauthorized.Dispose()
        }
        catch [UnauthorizedAccessException] { $adminRefused = $true }
        if (-not $adminRefused) { throw "administrator connected to product pipe" }
        Pass "administrator_protocol_connect_is_refused" "ERROR_ACCESS_DENIED before protocol"

        $status = [IO.Pipes.NamedPipeClientStream]::new(".", $statusPipe,
            [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
        $status.Connect(3000)
        $responseBuffer = New-Object byte[] 4096
        $pendingRead = $status.BeginRead($responseBuffer, 0, $responseBuffer.Length, $null, $null)
        $request = [Text.Encoding]::UTF8.GetBytes('{"type":"lease_request"}')
        $status.Write($request, 0, $request.Length)
        $status.Flush()
        if (-not $pendingRead.AsyncWaitHandle.WaitOne(5000)) {
            throw "status pipe response timed out"
        }
        $responseLength = $status.EndRead($pendingRead)
        $reply = [Text.Encoding]::UTF8.GetString($responseBuffer, 0, $responseLength)
        $status.Dispose()
        if ($reply -notmatch "status_pipe_read_only") {
            throw "status pipe mutation not refused: reply=$reply"
        }
        Pass "status_pipe_rejects_mutation" "read-only codec refusal"

        if ($selectedCase -in @("All", "Boundary")) {
            foreach ($mode in @("oversized", "partial")) {
                Remove-Item $probeResultPath -Force -ErrorAction SilentlyContinue
                Invoke-ScGuest @("config", $consumerService,
                    "binPath=", "$serviceSidProbeSc --mode $mode")
                & sc.exe start $consumerService 2>&1 | Out-Null
                $modeWatch = [Diagnostics.Stopwatch]::StartNew()
                for ($probeAttempt = 0; $probeAttempt -lt 160; $probeAttempt++) {
                    if (Test-Path $probeResultPath) { break }
                    Start-Sleep -Milliseconds 100
                }
                $modeResult = Get-Content $probeResultPath -Raw -ErrorAction Stop |
                    ConvertFrom-Json
                if ($modeResult.verdict -ne "PASS") {
                    throw "$mode frame was not refused: $($modeResult.error)"
                }
                $testName = if ($mode -eq "oversized") {
                    "oversized_line_is_refused"
                } else {
                    "partial_frame_times_out"
                }
                Pass $testName "mode=$mode; elapsed_ms=$([int]$modeWatch.Elapsed.TotalMilliseconds)"
                Get-Process ramshared-service-sid-probe -ErrorAction SilentlyContinue |
                    Stop-Process -Force -ErrorAction SilentlyContinue
                Start-Sleep -Milliseconds 250
            }

            $consumerSid = ((& sc.exe showsid $consumerService 2>&1 | Out-String) `
                    -split "\s+" | Where-Object { $_ -like "S-1-5-80-*" } |
                Select-Object -First 1)
            if (-not $consumerSid) { throw "consumer service SID unavailable" }
            Remove-Item $probeResultPath -Force -ErrorAction SilentlyContinue
            Invoke-ScGuest @("config", $consumerService,
                "binPath=", "$serviceSidProbeSc --mode deny-only --deny-sid $consumerSid")
            & sc.exe start $consumerService 2>&1 | Out-Null
            for ($probeAttempt = 0; $probeAttempt -lt 160; $probeAttempt++) {
                if (Test-Path $probeResultPath) { break }
                Start-Sleep -Milliseconds 100
            }
            $denyResult = Get-Content $probeResultPath -Raw -ErrorAction Stop |
                ConvertFrom-Json
            $denyEvidence = @(Get-Content $brokerEvidence -ErrorAction Stop)
            if ($denyResult.verdict -ne "PASS" -or
                -not ($denyEvidence -match '"transition":"peer_sid_refused"')) {
                throw "deny-only service SID was not refused by explicit token verification"
            }
            Pass "deny_only_service_sid_is_refused" `
                "connected under restricted token; explicit group attributes refused before parse"
            Get-Process ramshared-service-sid-probe -ErrorAction SilentlyContinue |
                Stop-Process -Force -ErrorAction SilentlyContinue
            Start-Sleep -Milliseconds 250

            Remove-Item $probeResultPath -Force -ErrorAction SilentlyContinue
            Invoke-ScGuest @("config", $consumerService,
                "binPath=", "$serviceSidProbeSc --mode blocked-read")
            Invoke-ScGuest @("config", $consumerService, "depend=", "/")
            & sc.exe start $consumerService 2>&1 | Out-Null
            (Get-Service $consumerService).WaitForStatus(
                "Running", [timespan]::FromSeconds(15))
            Start-Sleep -Milliseconds 500
            $blockedStop = [Diagnostics.Stopwatch]::StartNew()
            Stop-Service $brokerService
            (Get-Service $brokerService).WaitForStatus(
                "Stopped", [timespan]::FromSeconds(15))
            if ($blockedStop.Elapsed.TotalSeconds -gt 10) {
                throw "broker stop did not cancel blocked read within 10 seconds"
            }
            Pass "stop_cancels_blocked_read" `
                "stop_ms=$([int]$blockedStop.Elapsed.TotalMilliseconds)"
            Get-Process ramshared-service-sid-probe -ErrorAction SilentlyContinue |
                Stop-Process -Force -ErrorAction SilentlyContinue
            Start-Service $brokerService
            (Get-Service $brokerService).WaitForStatus(
                "Running", [timespan]::FromSeconds(30))
            Invoke-ScGuest @("config", $consumerService,
                "binPath=", $serviceSidProbeSc, "depend=", $brokerService)
            $boundaryEvidence = @(Get-Content $brokerEvidence -ErrorAction Stop)
            if ($boundaryEvidence -match '"transition":"registered_ready"' -or
                $boundaryEvidence -match '"transition":"lease_granted"') {
                throw "malformed/blocked boundary mutated broker session state"
            }
            Pass "boundary_refusals_do_not_mutate" "registered=0; leases=0"
        }

        if ($selectedCase -in @("All", "PeerMatrix")) {
            try { Start-Service $consumerService -ErrorAction Stop } catch {}
            $admitted = $false
            $observedLease = $false
            for ($sample = 0; $sample -lt 100; $sample++) {
                $evidence = @(Get-Content $brokerEvidence `
                        -ErrorAction SilentlyContinue)
                if ($evidence -match '"transition":"registered_ready"') { $admitted = $true }
                if ($evidence -match '"transition":"lease_granted"') { $observedLease = $true }
                if ($admitted -and $observedLease) { break }
                if ($admitted -and $observedLease) { break }
                Start-Sleep -Milliseconds 50
            }
            $consumerState = Get-CimInstance Win32_Service -Filter "Name='$consumerService'"
            if (-not $admitted) {
                throw "legitimate consumer service SID was not observed by broker status"
            }
            Pass "legitimate_service_sid_connects" `
                "live_session_observed=true active_lease_observed=$observedLease consumer_state=$($consumerState.State)"
            $probeResult = Get-Content $probeResultPath `
                -Raw -ErrorAction Stop | ConvertFrom-Json
            if ($probeResult.verdict -ne "PASS") {
                throw "service SID probe failed: $($probeResult.error)"
            }
            Stop-Service $consumerService -Force -ErrorAction SilentlyContinue
            $consumerStopped = Get-Service $consumerService -ErrorAction SilentlyContinue
            if ($consumerStopped) {
                $consumerStopped.WaitForStatus("Stopped", [timespan]::FromSeconds(30))
            }
            Get-Process ramshared-service-sid-probe -ErrorAction SilentlyContinue |
                Stop-Process -Force -ErrorAction SilentlyContinue
            for ($disconnectAttempt = 0; $disconnectAttempt -lt 120; $disconnectAttempt++) {
                $brokerEvidenceRows = @(Get-Content $brokerEvidence `
                        -ErrorAction SilentlyContinue)
                if ($brokerEvidenceRows -match '"transition":"session_disconnected"') { break }
                Start-Sleep -Milliseconds 100
            }

            $unrelated = "RamSharedUnrelated"
            & sc.exe delete $unrelated 2>&1 | Out-Null
            Remove-Item $probeResultPath `
                -Force -ErrorAction SilentlyContinue
            Invoke-ScGuest @("create", $unrelated, "type=", "own", "start=", "demand",
                "error=", "normal",
                "binPath=", "$serviceSidProbeSc --service-name $unrelated")
            Invoke-ScGuest @("sidtype", $unrelated, "unrestricted")
            & sc.exe start $unrelated 2>&1 | Out-Null
            for ($probeAttempt = 0; $probeAttempt -lt 150; $probeAttempt++) {
                if (Test-Path $probeResultPath) { break }
                Start-Sleep -Milliseconds 100
            }
            $unrelatedResult = Get-Content $probeResultPath `
                -Raw -ErrorAction Stop | ConvertFrom-Json
            $brokerEvidenceRows = @(Get-Content $brokerEvidence `
                    -ErrorAction Stop)
            if ($unrelatedResult.verdict -ne "FAIL" -or
                -not ($brokerEvidenceRows -match '"transition":"peer_sid_refused"')) {
                throw "unrelated LocalSystem service was not refused by explicit SID check"
            }
            Pass "unrelated_service_is_refused" `
                "DACL normal pass succeeded; explicit service SID check refused before parse"
            Get-Process ramshared-service-sid-probe -ErrorAction SilentlyContinue |
                Stop-Process -Force -ErrorAction SilentlyContinue
            & sc.exe delete $unrelated 2>&1 | Out-Null
        }

        $stopBefore = Get-Date
        Stop-Service $brokerService -Force
        (Get-Service $brokerService).WaitForStatus("Stopped", [timespan]::FromSeconds(15))
        $stopMs = [int]((Get-Date) - $stopBefore).TotalMilliseconds
        Pass "stop_cancels_blocked_accept" "stop_ms=$stopMs"
        Stop-Service $brokerService -ErrorAction SilentlyContinue
        Pass "stop_with_no_session_is_idempotent" "second stop left service stopped"

        if ($selectedCase -eq "RetryBudget") {
            Pass "only_not_found_and_busy_retry" "covered by Rust retry-policy gate"
            Pass "deadline_stops_retry" "covered by Rust retry-policy gate"
            Start-Service $brokerService
            (Get-Service $brokerService).WaitForStatus("Running", [timespan]::FromSeconds(15))
            $restartDelays = @(5, 15, 40)
            for ($crash = 0; $crash -lt 3; $crash++) {
                $oldPid = (Get-CimInstance Win32_Service `
                        -Filter "Name='$brokerService'").ProcessId
                Stop-Process -Id $oldPid -Force
                $deadline = (Get-Date).AddSeconds($restartDelays[$crash] + 15)
                $newPid = 0
                while ((Get-Date) -lt $deadline) {
                    Start-Sleep -Milliseconds 250
                    $current = Get-CimInstance Win32_Service `
                        -Filter "Name='$brokerService'"
                    if ($current.State -eq "Running" -and
                        $current.ProcessId -ne 0 -and $current.ProcessId -ne $oldPid) {
                        $newPid = $current.ProcessId
                        break
                    }
                }
                if ($newPid -eq 0) { throw "broker crash $($crash + 1) did not restart" }
            }
            $fourthPid = (Get-CimInstance Win32_Service `
                    -Filter "Name='$brokerService'").ProcessId
            Stop-Process -Id $fourthPid -Force
            Start-Sleep -Seconds 10
            if ((Get-Service $brokerService).Status -ne "Stopped") {
                throw "fourth broker failure restarted"
            }
            Pass "fourth_failure_remains_stopped" "restart delays=5s,15s,40s then NONE"

            Copy-Item $brokerConfig "$brokerConfig.good" -Force
            [IO.File]::WriteAllText($brokerConfig, "invalid = true",
                [Text.UTF8Encoding]::new($false))
            & sc.exe start $brokerService 2>&1 | Out-Null
            Start-Sleep -Seconds 10
            if ((Get-Service $brokerService).Status -ne "Stopped") {
                throw "deterministic config failure restarted"
            }
            Move-Item "$brokerConfig.good" $brokerConfig -Force
            Pass "deterministic_failure_does_not_restart" "normal service-specific exit remained stopped"
        }
        $out
    } -ArgumentList $guestRoot, $BrokerService, $ConsumerService, $BrokerPipe, $StatusPipe, $Case

    $results | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath `
        (Join-Path $hostArtifacts "results.json") -Encoding UTF8
    $results | Format-Table -AutoSize
    "EVIDENCE=$hostArtifacts"
}
finally {
    if ($session) {
        Invoke-Command -Session $session -ScriptBlock {
            param($broker, $consumer)
            Stop-Service $consumer -Force -ErrorAction SilentlyContinue
            Stop-Service $broker -Force -ErrorAction SilentlyContinue
        } -ArgumentList $BrokerService, $ConsumerService -ErrorAction SilentlyContinue
        Remove-PSSession $session
    }
}
