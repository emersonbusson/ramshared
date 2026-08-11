#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path $PSScriptRoot "Run-HostAutonomousLifecycle.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

function Import-ProductionFunction([string]$Name) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput(
        $text, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) { throw "host lifecycle harness does not parse" }
    $functionAst = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $Name
        }, $true)
    if (-not $functionAst) { throw "missing production function $Name" }
    $body = $functionAst.Body.Extent.Text
    $body = $body.Substring(1, $body.Length - 2)
    Set-Item -Path ("Function:\script:{0}" -f $Name) `
        -Value ([scriptblock]::Create($body))
}

foreach ($functionName in @(
        "Get-Sha256Hex",
        "Test-IntendedPayloadMatch",
        "Resolve-ExactOnlineDisk",
        "Resolve-CurrentOnlineEvidence",
        "Get-PackagedDriveConfig",
        "Assert-PagefileLetterFree",
        "Assert-SupportedStop",
        "Quote-ProcessArgument",
        "Invoke-BoundedProcess",
        "Invoke-BoundedPowerShellChild",
        "New-RebootApproval",
        "Use-ResumeApproval",
        "Get-ScheduledResumeArguments",
        "Get-WatchdogTimeoutAction",
        "Get-WatchdogCommand",
        "Invoke-CampaignSafetyCleanup")) {
    Import-ProductionFunction $functionName
}

$intended = [byte[]](1, 2, 3, 4)
$corrupt = [byte[]](9, 2, 3, 4)
if (Test-IntendedPayloadMatch $intended $corrupt) {
    throw "intended_payload_corruption_is_red failed"
}
Write-Output "PASS intended_payload_corruption_is_red"

$onlineDisk = [pscustomobject]@{
    Number = 5
    FriendlyName = "RAMSHARE VRAMDISK"
    SerialNumber = "ABCDEF0123456789"
    Size = [uint64](64MB)
    BusType = "Virtual"
    LogicalSectorSize = 4096
    PhysicalSectorSize = 4096
    PartitionStyle = "RAW"
    NumberOfPartitions = 0
    IsBoot = $false
    IsSystem = $false
}
$resolved = Resolve-ExactOnlineDisk @($onlineDisk) `
    "ABCDEF0123456789" ([uint64](64MB)) 4096
if ($resolved.Number -ne 5) {
    throw "exact_online_identity_required_before_format failed: valid identity refused"
}
foreach ($badRows in @(
        @($onlineDisk, $onlineDisk),
        @([pscustomobject]@{
                Number = 5; FriendlyName = "RAMSHARE VRAMDISK"
                SerialNumber = "0000000000000000"; Size = [uint64](64MB)
                BusType = "Virtual"; LogicalSectorSize = 4096
                PhysicalSectorSize = 4096; PartitionStyle = "RAW"
                NumberOfPartitions = 0; IsBoot = $false; IsSystem = $false
            }))) {
    try {
        Resolve-ExactOnlineDisk $badRows "ABCDEF0123456789" `
            ([uint64](64MB)) 4096 | Out-Null
        throw "exact_online_identity_required_before_format failed: bad identity accepted"
    } catch {
        if ($_.Exception.Message -like "exact_online_identity_required_before_format failed:*") {
            throw
        }
    }
}
Write-Output "PASS exact_online_identity_required_before_format"

$onlineRow = [pscustomobject]@{
    schema = 1; pid = 4321; phase = "Online"
    run_id = "run-4321-1000000000-9"
    source_run_id = "run-4321-1000000000-9"
    ts_utc_ms = [int64]5000; lun_serial = "ABCDEF0123456789"
}
if ((Resolve-CurrentOnlineEvidence @($onlineRow) 4321 4000).run_id -cne
    $onlineRow.run_id) {
    throw "exact_online_identity_required_before_format failed: current Online row refused"
}
foreach ($badOnline in @(
        [pscustomobject]@{
            schema = 1; pid = 9999; phase = "Online"
            run_id = "run-9999-1000000000-9"
            source_run_id = "run-9999-1000000000-9"
            ts_utc_ms = [int64]5000; lun_serial = "ABCDEF0123456789"
        },
        [pscustomobject]@{
            schema = 1; pid = 4321; phase = "Online"
            run_id = "run-4321-1000000000-9"
            source_run_id = "run-4321-1000000000-9"
            ts_utc_ms = [int64]1000; lun_serial = "ABCDEF0123456789"
        })) {
    try {
        Resolve-CurrentOnlineEvidence @($badOnline) 4321 4000 | Out-Null
        throw "exact_online_identity_required_before_format failed: stale/foreign Online row accepted"
    } catch {
        if ($_.Exception.Message -like
            "exact_online_identity_required_before_format failed:*") { throw }
    }
}

$configRoot = Join-Path ([IO.Path]::GetTempPath()) `
    ("ramshared-config-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $configRoot | Out-Null
$configPath = Join-Path $configRoot "winsvc.toml"
$manifestPath = Join-Path $configRoot "product-manifest.json"
$configText = @'
[win_drive]
size_bytes = 67108864
block_size = 4096
volume_letter = "S"
evidence_path = "C:\\ProgramData\\RamShared\\evidence"
'@
try {
    [IO.File]::WriteAllText($configPath, $configText,
        [Text.UTF8Encoding]::new($false))
    $manifest = [pscustomobject]@{ artifacts = @([pscustomobject]@{
                role = "winsvc_config"; relative_path = "winsvc.toml"
                sha256 = (Get-FileHash $configPath -Algorithm SHA256).Hash
            }) }
    $parsedConfig = Get-PackagedDriveConfig $manifest $manifestPath
    if ($parsedConfig.size_bytes -ne [uint64](64MB) -or
        $parsedConfig.block_size -ne 4096 -or
        $parsedConfig.volume_letter -cne "S" -or
        $parsedConfig.evidence_path -cne "C:\ProgramData\RamShared\evidence") {
        throw "exact_online_identity_required_before_format failed: packaged config mismatch"
    }
    [IO.File]::AppendAllText($configPath, "`r`nsize_bytes = 67108864`r`n",
        [Text.UTF8Encoding]::new($false))
    $manifest.artifacts[0].sha256 =
        (Get-FileHash $configPath -Algorithm SHA256).Hash
    try {
        Get-PackagedDriveConfig $manifest $manifestPath | Out-Null
        throw "exact_online_identity_required_before_format failed: duplicate config accepted"
    } catch {
        if ($_.Exception.Message -like
            "exact_online_identity_required_before_format failed:*") { throw }
    }
} finally {
    Remove-Item $configRoot -Recurse -Force -ErrorAction SilentlyContinue
}

$nonRaw = $onlineDisk.PSObject.Copy()
$nonRaw.PartitionStyle = "GPT"
$nonRaw.NumberOfPartitions = 1
try {
    Resolve-ExactOnlineDisk @($nonRaw) "ABCDEF0123456789" `
        ([uint64](64MB)) 4096 | Out-Null
    throw "non_raw_lun_refuses_before_mutation failed: GPT accepted"
} catch {
    if ($_.Exception.Message -like "non_raw_lun_refuses_before_mutation failed:*") { throw }
}
Write-Output "PASS non_raw_lun_refuses_before_mutation"

try {
    Assert-PagefileLetterFree "S" @("S:\pagefile.sys") @() $true | Out-Null
    throw "active_pagefile_refuses_before_install failed: active pagefile accepted"
} catch {
    if ($_.Exception.Message -like "active_pagefile_refuses_before_install failed:*") { throw }
}
Write-Output "PASS active_pagefile_refuses_before_install"

try {
    Assert-PagefileLetterFree "S" @() @("S:\pagefile.sys 16 16") $true | Out-Null
    throw "configured_pagefile_refuses_before_install failed: configured pagefile accepted"
} catch {
    if ($_.Exception.Message -like "configured_pagefile_refuses_before_install failed:*") { throw }
}
Write-Output "PASS configured_pagefile_refuses_before_install"

try {
    Assert-PagefileLetterFree "S" @() @() $false | Out-Null
    throw "pagefile_query_failure_refuses_before_install failed: query failure accepted"
} catch {
    if ($_.Exception.Message -like "pagefile_query_failure_refuses_before_install failed:*") { throw }
}
Write-Output "PASS pagefile_query_failure_refuses_before_install"
if (-not (Assert-PagefileLetterFree "S" @("C:\pagefile.sys") `
        @("C:\pagefile.sys 0 0") $true)) {
    throw "pagefile refusals failed: legitimate foreign pagefile was refused"
}

try {
    Assert-SupportedStop "Stop-Service returned 1061" "Stopped" | Out-Null
    throw "stop_request_error_is_red failed: command error accepted"
} catch {
    if ($_.Exception.Message -like "stop_request_error_is_red failed:*") { throw }
}
if (-not (Assert-SupportedStop "" "Stopped")) {
    throw "stop_request_error_is_red failed: clean stop refused"
}
Write-Output "PASS stop_request_error_is_red"

$processTestRoot = Join-Path ([IO.Path]::GetTempPath()) `
    ("ramshared-process-tree-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $processTestRoot | Out-Null
$workerPath = Join-Path $processTestRoot "worker.ps1"
$childPidPath = Join-Path $processTestRoot "child.pid"
$worker = @'
param([string]$ChildPidPath)
$powerShellPath = (Get-Process -Id $PID -ErrorAction Stop).Path
if ([string]::IsNullOrWhiteSpace($powerShellPath) -or
    -not (Test-Path -LiteralPath $powerShellPath -PathType Leaf)) {
    throw "current PowerShell executable is unavailable"
}
$child = Start-Process -FilePath $powerShellPath `
    -ArgumentList @("-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 60") `
    -PassThru
[IO.File]::WriteAllText($ChildPidPath, [string]$child.Id)
1..4096 | ForEach-Object { Write-Output ("bounded-output-" + $_) }
Start-Sleep -Seconds 60
'@
[IO.File]::WriteAllText($workerPath, $worker, [Text.UTF8Encoding]::new($false))
try {
    $bounded = Invoke-BoundedPowerShellChild $workerPath `
        @("-ChildPidPath", (Quote-ProcessArgument $childPidPath)) 2
    if ($bounded.completed -or -not $bounded.process_tree_terminated -or
        -not (Test-Path $childPidPath -PathType Leaf)) {
        throw "bounded_child_terminates_process_tree failed: timeout/tree evidence mismatch"
    }
    $childPid = [int](Get-Content $childPidPath -Raw)
    Start-Sleep -Milliseconds 250
    if (Get-Process -Id $childPid -ErrorAction SilentlyContinue) {
        throw "bounded_child_terminates_process_tree failed: grandchild survived"
    }
} finally {
    Remove-Item $processTestRoot -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Output "PASS bounded_child_terminates_process_tree"
Write-Output "PASS static_child_uses_current_host_executable"

$approval = New-RebootApproval 2
$scheduledArguments = Get-ScheduledResumeArguments `
    "C:\ramshared\Run-HostAutonomousLifecycle.ps1" `
    "C:\ramshared\product-manifest.json" 3 `
    "C:\ramshared\ramshared-winsvc.exe" `
    "C:\ProgramData\RamShared\physical-autonomous-gate" `
    $approval.token $false
if ($scheduledArguments -match '(?i)ApprovePhysicalHost' -or
    $scheduledArguments -notmatch [regex]::Escape($approval.token)) {
    throw "resume_task_has_one_time_token_without_approval_switch failed"
}
Write-Output "PASS resume_task_has_one_time_token_without_approval_switch"

$resumeState = [ordered]@{
    status = "scheduled"
    next_boot = 2
    approval_boot = 2
    approval_token_sha256 = $approval.token_sha256
    approval_expires_utc = $approval.expires_utc
    approval_consumed = $false
}
Use-ResumeApproval $resumeState $approval.token 2 | Out-Null
try {
    Use-ResumeApproval $resumeState $approval.token 2 | Out-Null
    throw "stale_or_replayed_resume_token_is_refused failed: replay accepted"
} catch {
    if ($_.Exception.Message -like "stale_or_replayed_resume_token_is_refused failed:*") {
        throw
    }
}
try {
    $expiredState = [ordered]@{
        status = "scheduled"; next_boot = 2; approval_boot = 2
        approval_token_sha256 = $approval.token_sha256
        approval_expires_utc = [datetime]::UtcNow.AddMinutes(-1).ToString("o")
        approval_consumed = $false
    }
    Use-ResumeApproval $expiredState $approval.token 2 | Out-Null
    throw "stale_or_replayed_resume_token_is_refused failed: expired token accepted"
} catch {
    if ($_.Exception.Message -like "stale_or_replayed_resume_token_is_refused failed:*") {
        throw
    }
}
Write-Output "PASS stale_or_replayed_resume_token_is_refused"

if ((Get-WatchdogTimeoutAction $false) -ne "record_only" -or
    (Get-WatchdogTimeoutAction $true) -ne "shutdown") {
    throw "watchdog_shutdown_requires_separate_approval failed"
}
$recordOnlyCommand = Get-WatchdogCommand `
    "C:\marker" "C:\watchdog.log" "nonce-a" $false
$shutdownCommand = Get-WatchdogCommand `
    "C:\marker" "C:\watchdog.log" "nonce-b" $true
if ($recordOnlyCommand -match '(?i)shutdown\.exe' -or
    $recordOnlyCommand -notmatch 'watchdog_record_only' -or
    $recordOnlyCommand -notmatch "nonce-a" -or
    $shutdownCommand -notmatch '(?i)shutdown\.exe /s /t 0 /f' -or
    $shutdownCommand -notmatch 'watchdog_shutdown' -or
    $shutdownCommand -notmatch "nonce-b") {
    throw "watchdog_shutdown_requires_separate_approval failed: command mismatch"
}
Write-Output "PASS watchdog_shutdown_requires_separate_approval"

$cleanupRoot = Join-Path ([IO.Path]::GetTempPath()) `
    ("ramshared-cleanup-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $cleanupRoot | Out-Null
$marker = Join-Path $cleanupRoot "watchdog.armed"
[IO.File]::WriteAllText($marker, "armed")
$script:taskRemoved = $false
try {
    Invoke-CampaignSafetyCleanup $marker "ManufacturedTask" {
        param($Name)
        if ($Name -eq "ManufacturedTask") { $script:taskRemoved = $true }
    }
    if ((Test-Path $marker) -or -not $script:taskRemoved) {
        throw "failure_cleanup_disarms_watchdog_and_task failed"
    }
} finally {
    Remove-Item $cleanupRoot -Recurse -Force -ErrorAction SilentlyContinue
}
if ($text -notmatch '(?s)finally\s*\{\s*Invoke-CampaignSafetyCleanup') {
    throw "failure_cleanup_disarms_watchdog_and_task failed: production finally is not wired"
}
Write-Output "PASS failure_cleanup_disarms_watchdog_and_task"

Write-Output "PASS Test-HostAutonomousLifecycleStatic"
