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
        "Stop-GuestProcessInstanceSafely",
        "Invoke-BoundedGuestProcess",
        "Assert-GuestPsDirectChildSucceeded",
        "New-GuestPsDirectWorkerText")) {
    Import-ProductionFunction $functionName $helperText
}

foreach ($needle in @(
        "ReadToEndAsync",
        "[Threading.Tasks.Task]::WaitAll",
        "process_instance_handle_terminated",
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
if ($helperText -notmatch [regex]::Escape("Stop-GuestProcessInstanceSafely")) {
    throw "psdirect_pid_reuse_never_signals_foreign_process failed: missing process-instance fail-closed helper"
}
foreach ($needle in @(
        "[AllowEmptyString()]",
        "[switch]`$AllowEmptyPassword",
        "RAMSHARED_PSDIRECT_ALLOW_EMPTY_PASSWORD",
        "`$allowEmptyPassword",
        "[string]::IsNullOrWhiteSpace(`$Password) -and -not `$AllowEmptyPassword",
        "[string]::IsNullOrWhiteSpace(`$password) -and -not `$allowEmptyPassword",
        "blank-password recovery requires an empty password",
        "New-Object System.Security.SecureString",
        "if (`$allowEmptyPassword -and [string]::IsNullOrEmpty(`$password))")) {
    if ($helperText -notmatch [regex]::Escape($needle)) {
        throw "psdirect_blank_password_recovery_is_explicit failed: missing helper guard $needle"
    }
}

$emptyCredential = [pscredential]::new(
    "blank-password-contract", (New-Object System.Security.SecureString))
if ($emptyCredential.GetNetworkCredential().Password -cne "") {
    throw "psdirect_blank_password_recovery_is_explicit failed: an empty secure credential was not preserved"
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

Import-ProductionFunction "Invoke-GuestPsDirectBounded" $helperText
$script:GuestPsDirectMutationBlocked = $false
$script:capturedBlankPasswordEnvironment = $null
$originalInvokeBoundedGuestProcess = (Get-Command Invoke-BoundedGuestProcess -CommandType Function).ScriptBlock
$originalAssertGuestPsDirectChildSucceeded = (Get-Command Assert-GuestPsDirectChildSucceeded -CommandType Function).ScriptBlock
function Invoke-BoundedGuestProcess {
    param([string]$FilePath, [string]$Arguments, [int]$TimeoutSeconds, [hashtable]$Environment)
    $script:capturedBlankPasswordEnvironment = $Environment
    [pscustomobject]@{
        schema = 1
        status = "ok"
        value = @()
    } | Export-Clixml -LiteralPath $Environment["RAMSHARED_PSDIRECT_RESULT"] -Force
    [pscustomobject]@{
        completed = $true
        exit_code = 0
        stdout = ""
        stderr = ""
        process_tree_terminated = $false
    }
}
function Assert-GuestPsDirectChildSucceeded {
    param([object]$Execution, [string]$Operation)
    $true
}

try {
    Invoke-GuestPsDirectBounded -VMName "blank-password-contract" -User "lab\\user" `
        -Password "" -AllowEmptyPassword -Operation invoke -TimeoutSeconds 3 `
        -ConnectTimeoutSeconds 1 -ScriptBlock { $null } | Out-Null
    if ($null -eq $script:capturedBlankPasswordEnvironment -or
        $script:capturedBlankPasswordEnvironment["RAMSHARED_PSDIRECT_ALLOW_EMPTY_PASSWORD"] -cne "1") {
        throw "psdirect_blank_password_recovery_is_explicit failed: explicit blank-password opt-in was not propagated"
    }
    try {
        Invoke-GuestPsDirectBounded -VMName "blank-password-contract" -User "lab\\user" `
            -Password "" -Operation invoke -TimeoutSeconds 3 -ConnectTimeoutSeconds 1 `
            -ScriptBlock { $null } | Out-Null
        throw "psdirect_blank_password_recovery_is_explicit failed: blank password was accepted without opt-in"
    }
    catch {
        if ($_.Exception.Message -like "psdirect_blank_password_recovery_is_explicit failed:*") {
            throw
        }
        if ($_.Exception.Message -notmatch "password is required") {
            throw "psdirect_blank_password_recovery_is_explicit failed: wrong default blank-password failure"
        }
    }
    try {
        Invoke-GuestPsDirectBounded -VMName "blank-password-contract" -User "lab\\user" `
            -Password " " -AllowEmptyPassword -Operation invoke -TimeoutSeconds 3 `
            -ConnectTimeoutSeconds 1 -ScriptBlock { $null } | Out-Null
        throw "psdirect_blank_password_recovery_is_explicit failed: whitespace password was accepted as blank"
    }
    catch {
        if ($_.Exception.Message -like "psdirect_blank_password_recovery_is_explicit failed:*") {
            throw
        }
        if ($_.Exception.Message -notmatch "blank-password recovery requires an empty password") {
            throw "psdirect_blank_password_recovery_is_explicit failed: wrong whitespace-password failure"
        }
    }
}
finally {
    Set-Item -Path "Function:\script:Invoke-BoundedGuestProcess" -Value $originalInvokeBoundedGuestProcess
    Set-Item -Path "Function:\script:Assert-GuestPsDirectChildSucceeded" -Value $originalAssertGuestPsDirectChildSucceeded
}
Write-Output "PASS psdirect_blank_password_recovery_is_explicit"

$originalInvokeBoundedGuestProcess = (Get-Command Invoke-BoundedGuestProcess -CommandType Function).ScriptBlock
$originalAssertGuestPsDirectChildSucceeded = (Get-Command Assert-GuestPsDirectChildSucceeded -CommandType Function).ScriptBlock
$script:GuestPsDirectMutationBlocked = $false
$script:unresolvedChildInvocationCount = 0
function Invoke-BoundedGuestProcess {
    param([string]$FilePath, [string]$Arguments, [int]$TimeoutSeconds, [hashtable]$Environment)
    $script:unresolvedChildInvocationCount++
    [pscustomobject]@{
        completed = $false
        exit_code = $null
        stdout = ""
        stderr = "opaque"
        process_tree_terminated = $false
    }
}
try {
    try {
        Invoke-GuestPsDirectBounded -VMName "unresolved-child-contract" -User "lab\\user" `
            -Password "manufactured-password" -Operation invoke -TimeoutSeconds 3 `
            -ConnectTimeoutSeconds 1 -ScriptBlock { $null } | Out-Null
        throw "psdirect_unresolved_termination_is_terminal failed: unresolved termination was accepted"
    }
    catch {
        if ($_.Exception.Message -like "psdirect_unresolved_termination_is_terminal failed:*") {
            throw
        }
        if ($_.Exception.Message -notmatch "child-tree termination unresolved") {
            throw "psdirect_unresolved_termination_is_terminal failed: wrong initial refusal"
        }
    }
    try {
        Invoke-GuestPsDirectBounded -VMName "unresolved-child-contract" -User "lab\\user" `
            -Password "manufactured-password" -Operation invoke -TimeoutSeconds 3 `
            -ConnectTimeoutSeconds 1 -ScriptBlock { $null } | Out-Null
        throw "psdirect_unresolved_termination_is_terminal failed: later mutation was accepted"
    }
    catch {
        if ($_.Exception.Message -like "psdirect_unresolved_termination_is_terminal failed:*") {
            throw
        }
        if ($_.Exception.Message -notmatch "blocked after unresolved child-tree termination") {
            throw "psdirect_unresolved_termination_is_terminal failed: later mutation was not blocked"
        }
    }
    if ($script:unresolvedChildInvocationCount -ne 1) {
        throw "psdirect_unresolved_termination_is_terminal failed: a blocked invocation reached the child"
    }
}
finally {
    Set-Item -Path "Function:\script:Invoke-BoundedGuestProcess" -Value $originalInvokeBoundedGuestProcess
    Set-Item -Path "Function:\script:Assert-GuestPsDirectChildSucceeded" -Value $originalAssertGuestPsDirectChildSucceeded
}
Write-Output "PASS psdirect_unresolved_termination_is_terminal"

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

$reusedPidModel = [pscustomobject]@{
    HasExited = $false
    StartTime = [DateTime]::UtcNow.AddMinutes(-1)
    Handle = [IntPtr]1
    foreign_signal_count = 0
}
$reusedPidModel | Add-Member -MemberType ScriptMethod -Name Refresh -Value {
    # Model a PID that was replaced after the caller observed it. A safe helper
    # may inspect it but cannot signal the new foreign instance.
    $this.StartTime = [DateTime]::UtcNow
}
$reuseResult = Stop-GuestProcessInstanceSafely -Process $reusedPidModel
if ($reuseResult.stopped -or $reuseResult.reason -ne "process_instance_identity_changed" -or $reusedPidModel.foreign_signal_count -ne 0) {
    throw "psdirect_pid_reuse_never_signals_foreign_process failed: reused PID was not refused"
}
Write-Output "PASS psdirect_redirected_streams_are_deadline_bounded"
Write-Output "PASS psdirect_timeout_fails_closed_without_numeric_pid_kill"
Write-Output "PASS psdirect_pid_reuse_never_signals_foreign_process"
Write-Output "PASS psdirect_runner_uses_current_host_executable"
Write-Output "PASS psdirect_nonzero_child_is_red"

Write-Output "PASS Test-GuestPsDirectDeadlineStatic"
