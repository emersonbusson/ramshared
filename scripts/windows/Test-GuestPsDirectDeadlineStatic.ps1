#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HelperPath = "",
    [string]$GuestLifecyclePath = "",
    [string]$GuestPackagePath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($HelperPath)) {
    $HelperPath = Join-Path $PSScriptRoot "Invoke-GuestPsDirectBounded.ps1"
}
if ([string]::IsNullOrWhiteSpace($GuestLifecyclePath)) {
    $GuestLifecyclePath = Join-Path $PSScriptRoot "Run-GuestAutonomousLifecycle.ps1"
}
if ([string]::IsNullOrWhiteSpace($GuestPackagePath)) {
    $GuestPackagePath = Join-Path $PSScriptRoot "Run-GuestProductPackage.ps1"
}
if (-not (Test-Path -LiteralPath $HelperPath -PathType Leaf)) {
    throw "psdirect_outer_deadline_is_enforced failed: shared bounded PowerShell Direct helper is missing"
}

function Get-ParsedAst([string]$Path) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile(
        $Path, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "psdirect_outer_deadline_is_enforced failed: parser errors in $Path"
    }
    $ast
}

function Import-ProductionFunction([string]$Name, [string]$Source) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput(
        $Source, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "psdirect_outer_deadline_is_enforced failed: helper parser errors"
    }
    $definition = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $Name
        }, $true)
    if (-not $definition) {
        throw "psdirect_outer_deadline_is_enforced failed: missing production function $Name"
    }
    $body = $definition.Body.Extent.Text
    $body = $body.Substring(1, $body.Length - 2)
    Set-Item -Path ("Function:\script:{0}" -f $Name) `
        -Value ([scriptblock]::Create($body))
}

$helperText = Get-Content -LiteralPath $HelperPath -Raw
$lifecycleText = Get-Content -LiteralPath $GuestLifecyclePath -Raw
$packageText = Get-Content -LiteralPath $GuestPackagePath -Raw
$helperAst = Get-ParsedAst $HelperPath
$lifecycleAst = Get-ParsedAst $GuestLifecyclePath
$packageAst = Get-ParsedAst $GuestPackagePath

foreach ($functionName in @(
        "Quote-GuestProcessArgument",
        "Limit-GuestChildDiagnostics",
        "Invoke-BoundedGuestProcess",
        "Assert-GuestPsDirectChildSucceeded",
        "New-GuestPsDirectWorkerText")) {
    Import-ProductionFunction $functionName $helperText
}

foreach ($needle in @(
        "ReadToEndAsync",
        "[Threading.Tasks.Task]::WaitAll",
        "taskkill.exe",
        "/T /F",
        "Copy-Item",
        "-ToSession",
        "-FromSession",
        "outer deadline must exceed connect deadline",
        "Remove-PSSession",
        "RAMSHARED_PSDIRECT_PASSWORD")) {
    if ($helperText -notmatch [regex]::Escape($needle)) {
        throw "psdirect_outer_deadline_is_enforced failed: missing helper guard $needle"
    }
}
if ($helperText -notmatch 'finally\s*\{[\s\S]{0,800}Remove-PSSession') {
    throw "psdirect_calls_are_session_finally_cleaned failed: worker cleanup is not in finally"
}
if ($helperText -match 'payload\.password|Arguments\s*=.*Password') {
    throw "psdirect_calls_are_session_finally_cleaned failed: password escaped into worker payload or command arguments"
}
$workerTokens = $null
$workerErrors = $null
[void][Management.Automation.Language.Parser]::ParseInput(
    (New-GuestPsDirectWorkerText), [ref]$workerTokens, [ref]$workerErrors)
if ($workerErrors.Count -ne 0) {
    throw "psdirect_outer_deadline_is_enforced failed: generated worker has parser errors"
}

if ($lifecycleText -notmatch 'function\s+Assert-DeferredGuestShutdownReceipt') {
    throw "deferred_guest_shutdown_preserves_psdirect_result failed: lifecycle receipt guard is missing"
}
Import-ProductionFunction "Assert-DeferredGuestShutdownReceipt" $lifecycleText
if ($lifecycleText -match 'shutdown\.exe\s+/s\s+/t\s+0' -or
    $lifecycleText -notmatch '\[ValidateRange\(15,\s*30\)\]\s*\r?\n\s*\[int\]\$GuestShutdownDelaySeconds\s*=\s*15' -or
    $lifecycleText -notmatch 'shutdown\.exe\s+/s\s+/t\s+\$DelaySeconds' -or
    $lifecycleText -notmatch '\$LASTEXITCODE\s+-ne\s+0' -or
    $lifecycleText -notmatch 'shutdown_scheduled' -or
    $lifecycleText -notmatch 'Assert-DeferredGuestShutdownReceipt') {
    throw "deferred_guest_shutdown_preserves_psdirect_result failed: deferred typed shutdown contract is incomplete"
}
$validShutdownReceipt = [pscustomobject]@{
    shutdown_scheduled = $true
    delay_seconds = 15
}
if (-not (Assert-DeferredGuestShutdownReceipt $validShutdownReceipt 15)) {
    throw "deferred_guest_shutdown_preserves_psdirect_result failed: valid receipt was refused"
}
foreach ($invalidShutdownReceipt in @(
        [pscustomobject]@{ shutdown_scheduled = $false; delay_seconds = 15 },
        [pscustomobject]@{ shutdown_scheduled = $true; delay_seconds = 14 },
        [pscustomobject]@{ shutdown_scheduled = $true; delay_seconds = 16 },
        [pscustomobject]@{ shutdown_scheduled = "false"; delay_seconds = 15 },
        [pscustomobject]@{ shutdown_scheduled = $true; delay_seconds = "15" })) {
    try {
        Assert-DeferredGuestShutdownReceipt $invalidShutdownReceipt 15 | Out-Null
        throw "deferred_guest_shutdown_preserves_psdirect_result failed: invalid receipt was accepted"
    }
    catch {
        if ($_.Exception.Message -like
            "deferred_guest_shutdown_preserves_psdirect_result failed:*") {
            throw
        }
    }
}
Write-Output "PASS deferred_guest_shutdown_preserves_psdirect_result"

function Get-DirectHostPsCommandCount($Ast) {
    @($Ast.FindAll({
                param($node)
                $node -is [Management.Automation.Language.CommandAst] -and
                ($node.GetCommandName() -in @(
                        "New-PSSession", "Invoke-Command", "Remove-PSSession"))
            }, $true)).Count
}

if ((Get-DirectHostPsCommandCount $lifecycleAst) -ne 0 -or
    (Get-DirectHostPsCommandCount $packageAst) -ne 0) {
    throw "psdirect_outer_deadline_is_enforced failed: a guest harness still invokes PowerShell Direct outside the bounded child"
}
if ($lifecycleText -match '(?m)^\s*Copy-Item.*-(ToSession|FromSession)' -or
    $packageText -match '(?m)^\s*Copy-Item.*-(ToSession|FromSession)') {
    throw "psdirect_outer_deadline_is_enforced failed: a session copy bypasses the bounded child"
}
$lifecycleCalls = @($lifecycleAst.FindAll({
            param($node)
            $node -is [Management.Automation.Language.CommandAst] -and
            $node.GetCommandName() -eq "Invoke-GuestPsDirectBounded"
        }, $true)).Count
$packageCalls = @($packageAst.FindAll({
            param($node)
            $node -is [Management.Automation.Language.CommandAst] -and
            $node.GetCommandName() -eq "Invoke-GuestPsDirectBounded"
        }, $true)).Count
if ($lifecycleCalls -lt 4 -or $packageCalls -lt 5) {
    throw "psdirect_outer_deadline_is_enforced failed: not every guest operation is bounded lifecycle=$lifecycleCalls package=$packageCalls"
}
Write-Output "PASS psdirect_outer_deadline_is_enforced"
Write-Output "PASS psdirect_calls_are_session_finally_cleaned"

$testRoot = Join-Path ([IO.Path]::GetTempPath()) `
    ("ramshared-psdirect-deadline-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
$workerPath = Join-Path $testRoot "worker.ps1"
$childPidPath = Join-Path $testRoot "grandchild.pid"
$failureWorkerPath = Join-Path $testRoot "failure-worker.ps1"
$worker = @'
param([string]$GrandchildPidPath)
$child = Start-Process -FilePath (Join-Path $PSHOME "powershell.exe") `
    -ArgumentList @("-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 60") `
    -PassThru
[IO.File]::WriteAllText($GrandchildPidPath, [string]$child.Id)
1..1024 | ForEach-Object {
    Write-Output ("bounded-stdout-" + $_)
    [Console]::Error.WriteLine("bounded-stderr-" + $_)
}
Write-Output "bounded-stdout-finished"
[Console]::Error.WriteLine("bounded-stderr-finished")
Start-Sleep -Seconds 60
'@
try {
    [IO.File]::WriteAllText($workerPath, $worker, [Text.UTF8Encoding]::new($false))
    $powershell = Join-Path $PSHOME "powershell.exe"
    $workerArguments = @(
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
        (Quote-GuestProcessArgument $workerPath), "-GrandchildPidPath",
        (Quote-GuestProcessArgument $childPidPath)) -join " "
    $timedOut = Invoke-BoundedGuestProcess $powershell $workerArguments 2 @{}
    if ($timedOut.completed -or -not $timedOut.process_tree_terminated -or
        -not (Test-Path -LiteralPath $childPidPath -PathType Leaf)) {
        throw "psdirect_timeout_terminates_child_tree failed: timeout/tree evidence mismatch"
    }
    if ($timedOut.stdout -notmatch "bounded-stdout-finished" -or
        $timedOut.stderr -notmatch "bounded-stderr-finished") {
        throw "psdirect_redirected_streams_are_drained failed: redirected output was not drained before timeout"
    }
    $grandchildPid = [int](Get-Content -LiteralPath $childPidPath -Raw)
    Start-Sleep -Milliseconds 250
    if (Get-Process -Id $grandchildPid -ErrorAction SilentlyContinue) {
        throw "psdirect_timeout_terminates_child_tree failed: synthetic grandchild survived"
    }
    Write-Output "PASS psdirect_redirected_streams_are_drained"
    Write-Output "PASS psdirect_timeout_terminates_child_tree"

    [IO.File]::WriteAllText($failureWorkerPath, "exit 17", [Text.UTF8Encoding]::new($false))
    $failureArguments = @(
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
        (Quote-GuestProcessArgument $failureWorkerPath)) -join " "
    $failed = Invoke-BoundedGuestProcess $powershell $failureArguments 10 @{}
    try {
        Assert-GuestPsDirectChildSucceeded $failed "manufactured-nonzero" | Out-Null
        throw "psdirect_nonzero_child_is_red failed: nonzero child was accepted"
    }
    catch {
        if ($_.Exception.Message -like "psdirect_nonzero_child_is_red failed:*") {
            throw
        }
    }
    Write-Output "PASS psdirect_nonzero_child_is_red"
}
finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output "PASS Test-GuestPsDirectDeadlineStatic"
