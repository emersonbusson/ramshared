#Requires -Version 5.1

Set-StrictMode -Version Latest
$script:GuestPsDirectMutationBlocked = $false

function Quote-GuestProcessArgument {
    param([string]$Value)
    if ($Value -notmatch '[\s"]') { return $Value }
    '"' + ($Value -replace '(\\*)"', '$1$1\\"' -replace '(\\+)$', '$1$1') + '"'
}

function Limit-GuestChildDiagnostics {
    param([AllowNull()][string]$Text)
    if ($null -eq $Text) { return "" }
    $limit = 32768
    if ($Text.Length -le $limit) { return $Text }
    $Text.Substring(0, $limit) + "`n[diagnostic truncated]"
}

function Stop-GuestProcessInstanceSafely {
    param([Parameter(Mandatory = $true)][object]$Process)
    # Bind the process handle before revalidating its creation time. Kill() on
    # this Process object targets that handle, never a reopened numeric PID.
    try {
        $Process.Refresh()
        if ($Process.HasExited) { return [pscustomobject]@{ stopped = $true; reason = "process_already_exited" } }
        $boundHandle = $Process.Handle
        $originalStart = $Process.StartTime.ToUniversalTime().Ticks
        $Process.Refresh()
        if ($Process.HasExited) { return [pscustomobject]@{ stopped = $true; reason = "process_already_exited" } }
        if ($Process.StartTime.ToUniversalTime().Ticks -ne $originalStart) {
            return [pscustomobject]@{ stopped = $false; reason = "process_instance_identity_changed" }
        }
        $Process.Kill()
        if (-not $Process.WaitForExit(5000)) { return [pscustomobject]@{ stopped = $false; reason = "process_instance_kill_unreaped" } }
        return [pscustomobject]@{ stopped = $true; reason = "process_instance_handle_terminated" }
    } catch {
        return [pscustomobject]@{ stopped = $false; reason = "process_instance_identity_unproven" }
    }
}

function Invoke-BoundedGuestProcess {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [Parameter(Mandatory = $true)]
        [string]$Arguments,
        [ValidateRange(1, 3600)]
        [int]$TimeoutSeconds,
        [hashtable]$Environment = @{}
    )

    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $FilePath
    $info.Arguments = $Arguments
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($key in @($Environment.Keys)) {
        $info.EnvironmentVariables[[string]$key] = [string]$Environment[$key]
    }

    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) {
            throw "failed to start bounded PowerShell Direct child"
        }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $completed = $process.WaitForExit($TimeoutSeconds * 1000)
        $processTreeTerminated = $false
        $terminationReason = "not_needed"
        if (-not $completed) {
            $stopped = Stop-GuestProcessInstanceSafely -Process $process
            $terminationReason = $stopped.reason
            if ($stopped.stopped -and $process.WaitForExit(5000)) {
                $processTreeTerminated = $true
            }
        }
        $streamsDrained = [Threading.Tasks.Task]::WaitAll(
            [Threading.Tasks.Task[]]@($stdoutTask, $stderrTask), 5000)
        [pscustomobject]@{
            completed               = [bool]$completed
            exit_code               = if ($completed) { [int]$process.ExitCode } else { $null }
            stdout                  = if ($streamsDrained) { Limit-GuestChildDiagnostics $stdoutTask.Result } else { "" }
            stderr                  = if ($streamsDrained) { Limit-GuestChildDiagnostics $stderrTask.Result } else { "" }
            process_tree_terminated = [bool]$processTreeTerminated
            termination_reason      = $terminationReason
        }
    }
    finally {
        $process.Dispose()
    }
}

function Assert-GuestPsDirectChildSucceeded {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Execution,
        [Parameter(Mandatory = $true)]
        [string]$Operation
    )

    if (-not $Execution.completed -and -not $Execution.process_tree_terminated) {
        throw "PowerShell Direct $Operation child-tree termination unresolved; refusing any later mutation"
    }
    if (-not $Execution.completed) {
        throw "PowerShell Direct $Operation outer deadline exceeded; process_tree_terminated=$($Execution.process_tree_terminated); stderr=$($Execution.stderr)"
    }
    if ($Execution.exit_code -ne 0) {
        throw "PowerShell Direct $Operation child failed exit=$($Execution.exit_code): $($Execution.stderr)"
    }
    $true
}

function New-GuestPsDirectWorkerText {
@'
$ErrorActionPreference = "Stop"
$session = $null
$failure = $null
try {
    $payloadPath = [Environment]::GetEnvironmentVariable("RAMSHARED_PSDIRECT_PAYLOAD")
    $resultPath = [Environment]::GetEnvironmentVariable("RAMSHARED_PSDIRECT_RESULT")
    $password = [Environment]::GetEnvironmentVariable("RAMSHARED_PSDIRECT_PASSWORD")
    $allowEmptyPassword = [Environment]::GetEnvironmentVariable(
        "RAMSHARED_PSDIRECT_ALLOW_EMPTY_PASSWORD") -ceq "1"
    if ([string]::IsNullOrWhiteSpace($payloadPath) -or
        [string]::IsNullOrWhiteSpace($resultPath) -or
        ([string]::IsNullOrWhiteSpace($password) -and -not $allowEmptyPassword) -or
        -not (Test-Path -LiteralPath $payloadPath -PathType Leaf)) {
        throw "PowerShell Direct worker input is incomplete"
    }
    if ($allowEmptyPassword -and -not [string]::IsNullOrEmpty($password) -and
        [string]::IsNullOrWhiteSpace($password)) {
        throw "PowerShell Direct blank-password recovery requires an empty password"
    }
    $payload = Import-Clixml -LiteralPath $payloadPath
    if ([int]$payload.schema -ne 1 -or
        [string]::IsNullOrWhiteSpace([string]$payload.vm_name) -or
        [string]::IsNullOrWhiteSpace([string]$payload.user) -or
        [int]$payload.connect_timeout_seconds -lt 1) {
        throw "PowerShell Direct worker payload is invalid"
    }
    $securePassword = if ($allowEmptyPassword -and [string]::IsNullOrEmpty($password)) {
        New-Object System.Security.SecureString
    }
    else {
        ConvertTo-SecureString $password -AsPlainText -Force
    }
    $credential = [pscredential]::new([string]$payload.user, $securePassword)
    $connectDeadline = (Get-Date).AddSeconds([int]$payload.connect_timeout_seconds)
    $lastConnectionError = ""
    do {
        try {
            $session = New-PSSession -VMName ([string]$payload.vm_name) `
                -Credential $credential -ErrorAction Stop
            break
        }
        catch {
            $lastConnectionError = $_.Exception.Message
            if ((Get-Date) -ge $connectDeadline) { break }
            Start-Sleep -Seconds 3
        }
    } while ($true)
    if (-not $session) {
        throw "PowerShell Direct unavailable after $($payload.connect_timeout_seconds) seconds: $lastConnectionError"
    }

    $value = @()
    switch ([string]$payload.operation) {
        "invoke" {
            if ([string]::IsNullOrWhiteSpace([string]$payload.script_text)) {
                throw "PowerShell Direct invoke payload has no script"
            }
            $remoteScript = [scriptblock]::Create([string]$payload.script_text)
            $value = @(Invoke-Command -Session $session -ScriptBlock $remoteScript `
                    -ArgumentList @($payload.argument_list) -ErrorAction Stop)
        }
        "copy_to" {
            if ([bool]$payload.recurse) {
                Copy-Item -LiteralPath ([string]$payload.source_path) `
                    -Destination ([string]$payload.destination_path) -ToSession $session `
                    -Recurse -Force -ErrorAction Stop
            }
            else {
                Copy-Item -LiteralPath ([string]$payload.source_path) `
                    -Destination ([string]$payload.destination_path) -ToSession $session `
                    -Force -ErrorAction Stop
            }
        }
        "copy_from" {
            if ([bool]$payload.recurse) {
                Copy-Item -LiteralPath ([string]$payload.source_path) `
                    -Destination ([string]$payload.destination_path) -FromSession $session `
                    -Recurse -Force -ErrorAction Stop
            }
            else {
                Copy-Item -LiteralPath ([string]$payload.source_path) `
                    -Destination ([string]$payload.destination_path) -FromSession $session `
                    -Force -ErrorAction Stop
            }
        }
        default {
            throw "PowerShell Direct worker operation is invalid: $($payload.operation)"
        }
    }
    [pscustomobject]@{
        schema = 1
        status = "ok"
        value = @($value)
    } | Export-Clixml -LiteralPath $resultPath -Force
}
catch {
    $failure = $_
}
finally {
    if ($session) {
        try {
            Remove-PSSession -Session $session -ErrorAction Stop
        }
        catch {
            if ($null -eq $failure) {
                $failure = $_
            }
            else {
                [Console]::Error.WriteLine("PowerShell Direct session cleanup failure: $($_.Exception.Message)")
            }
        }
    }
    Remove-Item Env:\RAMSHARED_PSDIRECT_PASSWORD -ErrorAction SilentlyContinue
}
if ($failure) {
    [Console]::Error.WriteLine("PowerShell Direct worker failure: $($failure.Exception.Message)")
    exit 1
}
'@
}

function Invoke-GuestPsDirectBounded {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$VMName,
        [Parameter(Mandatory = $true)]
        [string]$User,
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$Password,
        [Parameter(Mandatory = $true)]
        [ValidateSet("invoke", "copy_to", "copy_from")]
        [string]$Operation,
        [scriptblock]$ScriptBlock = $null,
        [object[]]$ArgumentList = @(),
        [string]$SourcePath = "",
        [string]$DestinationPath = "",
        [switch]$Recurse,
        [switch]$AllowEmptyPassword,
        [ValidateRange(2, 3600)]
        [int]$TimeoutSeconds = 210,
        [ValidateRange(1, 180)]
        [int]$ConnectTimeoutSeconds = 180
    )

    if ($script:GuestPsDirectMutationBlocked) {
        throw "PowerShell Direct mutation blocked after unresolved child-tree termination"
    }
    if ($TimeoutSeconds -le $ConnectTimeoutSeconds) {
        throw "PowerShell Direct outer deadline must exceed connect deadline"
    }
    if ([string]::IsNullOrWhiteSpace($Password) -and -not $AllowEmptyPassword) {
        throw "PowerShell Direct password is required"
    }
    if ($AllowEmptyPassword -and -not [string]::IsNullOrEmpty($Password) -and
        [string]::IsNullOrWhiteSpace($Password)) {
        throw "PowerShell Direct blank-password recovery requires an empty password"
    }
    if ($Operation -eq "invoke" -and $null -eq $ScriptBlock) {
        throw "PowerShell Direct invoke requires a script block"
    }
    if ($Operation -ne "invoke" -and
        ([string]::IsNullOrWhiteSpace($SourcePath) -or
            [string]::IsNullOrWhiteSpace($DestinationPath))) {
        throw "PowerShell Direct copy requires source and destination paths"
    }

    $workRoot = Join-Path ([IO.Path]::GetTempPath()) `
        ("ramshared-psdirect-" + [guid]::NewGuid().ToString("N"))
    $result = $null
    $failure = $null
    $cleanupFailure = $null
    try {
        New-Item -ItemType Directory -Force -Path $workRoot | Out-Null
        $payloadPath = Join-Path $workRoot "payload.clixml"
        $resultPath = Join-Path $workRoot "result.clixml"
        [pscustomobject]@{
            schema = 1
            vm_name = $VMName
            user = $User
            operation = $Operation
            script_text = if ($ScriptBlock) { $ScriptBlock.ToString() } else { "" }
            argument_list = @($ArgumentList)
            source_path = $SourcePath
            destination_path = $DestinationPath
            recurse = [bool]$Recurse
            connect_timeout_seconds = $ConnectTimeoutSeconds
        } | Export-Clixml -LiteralPath $payloadPath -Force
        $workerText = New-GuestPsDirectWorkerText
        $encodedWorker = [Convert]::ToBase64String(
            [Text.Encoding]::Unicode.GetBytes($workerText))
        $childArguments = "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encodedWorker"
        $environment = @{
            RAMSHARED_PSDIRECT_PAYLOAD = $payloadPath
            RAMSHARED_PSDIRECT_RESULT = $resultPath
            RAMSHARED_PSDIRECT_PASSWORD = $Password
            RAMSHARED_PSDIRECT_ALLOW_EMPTY_PASSWORD = if ($AllowEmptyPassword) { "1" } else { "0" }
        }
        $execution = Invoke-BoundedGuestProcess `
            (Join-Path $PSHOME "powershell.exe") $childArguments $TimeoutSeconds `
            $environment
        Assert-GuestPsDirectChildSucceeded $execution $Operation | Out-Null
        if (-not (Test-Path -LiteralPath $resultPath -PathType Leaf)) {
            throw "PowerShell Direct $Operation child produced no typed result"
        }
        $result = Import-Clixml -LiteralPath $resultPath
        if ([int]$result.schema -ne 1 -or [string]$result.status -cne "ok") {
            throw "PowerShell Direct $Operation child result is invalid"
        }
    }
    catch {
        $failure = $_
        if ($_.Exception.Message -match "child-tree termination unresolved") {
            $script:GuestPsDirectMutationBlocked = $true
        }
    }
    finally {
        if (Test-Path -LiteralPath $workRoot) {
            try {
                Remove-Item -LiteralPath $workRoot -Recurse -Force -ErrorAction Stop
                if (Test-Path -LiteralPath $workRoot) {
                    throw "PowerShell Direct worker cleanup left its temporary directory"
                }
            }
            catch {
                $cleanupFailure = $_
            }
        }
    }
    if ($failure) { throw $failure }
    if ($cleanupFailure) {
        throw "PowerShell Direct worker cleanup failed: $($cleanupFailure.Exception.Message)"
    }
    @($result.value)
}
