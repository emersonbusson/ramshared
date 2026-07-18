#Requires -Version 5.1
<#
.SYNOPSIS
  Probe WSL runtime readiness inside the isolated win11-drill VM.

.DESCRIPTION
  This helper starts the lab VM if requested, reaches it through PowerShell
  Direct, and runs a highest-privilege scheduled-task probe for wsl.exe. It does
  not initialize, format, resize, or attach disks. It emits an artifact with the
  regular PowerShell Direct view and the elevated scheduled-task view so WSL
  runtime failures are not misclassified as credential failures.
#>
[CmdletBinding()]
param(
    [string]$VMName = "win11-drill",
    [string]$User = "WIN11-DRILL\drilladmin",
    [string]$Password = "",
    [string]$PasswordFile = "C:\ramshared\bin\.drill-pw",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [int]$PsDirectReadyTimeoutSec = 900,
    [int]$PsDirectRetrySec = 10,
    [switch]$Start
)

$ErrorActionPreference = "Stop"

function New-ArtifactDir {
    param([string]$Root)
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $dir = Join-Path $Root "win11-wsl-runtime-probe-$stamp"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    return $dir
}

function Get-LocalDrillPassword {
    param(
        [string]$InitialPassword,
        [string]$LocalPasswordFile
    )
    if (-not [string]::IsNullOrEmpty($InitialPassword)) {
        return $InitialPassword
    }
    foreach ($scope in @("Machine", "User")) {
        $value = [Environment]::GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)
        if (-not [string]::IsNullOrEmpty($value)) {
            return $value
        }
    }
    if (-not [string]::IsNullOrEmpty($env:RAMSHARED_DRILL_PASSWORD)) {
        return $env:RAMSHARED_DRILL_PASSWORD
    }
    if (Test-Path -LiteralPath $LocalPasswordFile) {
        return (Get-Content -LiteralPath $LocalPasswordFile -Raw).Trim()
    }
    return ""
}

function Invoke-GuestWithRetry {
    param(
        [pscredential]$Credential,
        [scriptblock]$ScriptBlock,
        [object[]]$ArgumentList = @()
    )
    $deadline = (Get-Date).AddSeconds($PsDirectReadyTimeoutSec)
    $attempt = 0
    $lastError = ""
    do {
        $attempt += 1
        try {
            return Invoke-Command -VMName $VMName -Credential $Credential -ScriptBlock $ScriptBlock -ArgumentList $ArgumentList -ErrorAction Stop
        } catch {
            $lastError = $_.Exception.Message
            if ($lastError -match "credencial.*inv|credential.*invalid|logon failure|senha.*incorreta") {
                throw
            }
            Start-Sleep -Seconds $PsDirectRetrySec
        }
    } while ((Get-Date) -lt $deadline)

    throw "PowerShell Direct did not become ready after $attempt attempts over ${PsDirectReadyTimeoutSec}s. Last error: $lastError"
}

function Write-Summary {
    param(
        [string]$Dir,
        [string]$Status,
        [string]$Reason,
        [hashtable]$Extra = @{}
    )
    $summary = [ordered]@{
        STATUS = $Status
        PASS = ($Status -eq "PASS")
        REASON = $Reason
        VM = $VMName
        USER = $User
        ARTIFACT = $Dir
        DISK_MUTATION = $false
    }
    foreach ($k in $Extra.Keys) {
        $summary[$k] = $Extra[$k]
    }
    $summary | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $Dir "summary.json")
    Write-Host "STATUS=$Status"
    Write-Host "REASON=$Reason"
    Write-Host "ARTIFACT_DIR=$Dir"
}

$artifactDir = New-ArtifactDir -Root $ArtifactRoot
$Password = Get-LocalDrillPassword -InitialPassword $Password -LocalPasswordFile $PasswordFile
if ([string]::IsNullOrEmpty($Password)) {
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "missing_guest_credential"
    exit 2
}

try {
    if ($Start) {
        Start-VM -Name $VMName -ErrorAction Stop
    }

    $sec = ConvertTo-SecureString $Password -AsPlainText -Force
    $cred = [pscredential]::new($User, $sec)
    $probe = Invoke-GuestWithRetry -Credential $cred -ScriptBlock {
        $ErrorActionPreference = "Continue"
        $features = @(
            foreach ($name in @("Microsoft-Windows-Subsystem-Linux", "VirtualMachinePlatform")) {
                $feature = Get-WindowsOptionalFeature -Online -FeatureName $name
                [pscustomobject]@{
                    FeatureName = $name
                    State = $feature.State.ToString()
                    RestartNeeded = $feature.RestartNeeded
                }
            }
        )
        $appx = Get-AppxPackage -AllUsers *WindowsSubsystemForLinux* |
            Select-Object Name, PackageFullName, Version, InstallLocation
        $wslCommand = Get-Command wsl.exe -ErrorAction SilentlyContinue |
            Select-Object Source, Version
        $wslStatus = wsl.exe --status 2>&1 | Out-String
        [pscustomobject]@{
            host = $env:COMPUTERNAME
            whoami = (whoami)
            features = $features
            appx = $appx
            wsl_command = $wslCommand
            wsl_status = $wslStatus
            wsl_status_exit = $LASTEXITCODE
        }
    }
    $probe | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $artifactDir "psdirect-probe.json")

    $taskProbe = Invoke-GuestWithRetry -Credential $cred -ScriptBlock {
        param($TaskPassword)
        $ErrorActionPreference = "Continue"
        New-Item -ItemType Directory -Force -Path "C:\ramshared\artifacts" | Out-Null
        $script = "C:\ramshared\wsl-high-probe.ps1"
        $out = "C:\ramshared\artifacts\wsl-high-probe.out"
        @'
$ErrorActionPreference = "Continue"
"whoami=$(whoami)"
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
"is_admin=$($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))"
"status_start"
wsl.exe --status 2>&1
"status_exit=$LASTEXITCODE"
"list_start"
wsl.exe -l -v 2>&1
"list_exit=$LASTEXITCODE"
'@ | Set-Content -Encoding UTF8 -LiteralPath $script
        $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument ('-NoProfile -ExecutionPolicy Bypass -File "{0}" *> "{1}"' -f $script, $out)
        $trigger = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1)
        $principal = New-ScheduledTaskPrincipal -UserId "WIN11-DRILL\drilladmin" -RunLevel Highest -LogonType Password
        $task = New-ScheduledTask -Action $action -Trigger $trigger -Principal $principal
        Register-ScheduledTask -TaskName "RamSharedWslHighProbe" -InputObject $task -User "WIN11-DRILL\drilladmin" -Password $TaskPassword -Force | Out-Null
        Start-ScheduledTask -TaskName "RamSharedWslHighProbe"
        Start-Sleep -Seconds 20
        $info = Get-ScheduledTaskInfo -TaskName "RamSharedWslHighProbe"
        $content = if (Test-Path -LiteralPath $out) {
            Get-Content -LiteralPath $out -Raw
        } else {
            "NO_OUTPUT"
        }
        Unregister-ScheduledTask -TaskName "RamSharedWslHighProbe" -Confirm:$false
        [pscustomobject]@{
            last_task_result = $info.LastTaskResult
            output = $content
        }
    } -ArgumentList @($Password)
    $taskProbe | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $artifactDir "scheduled-task-probe.json")
} catch {
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "probe_failed" -Extra @{
        error = $_.Exception.Message
    }
    exit 2
}

$taskOutput = $taskProbe.output | Out-String
$taskNoOutput = ($taskOutput -match "NO_OUTPUT")
$runtimeReady = ($taskOutput -notmatch "not installed|REGDB_E_CLASSNOTREG|Wsl/CallMsi|CLASSNOTREG") -and
    (-not $taskNoOutput) -and
    ($taskOutput -match "status_exit=0|list_exit=0")
if ($runtimeReady) {
    Write-Summary -Dir $artifactDir -Status "PASS" -Reason "wsl_runtime_ready_in_elevated_guest"
    exit 0
}

if ($taskNoOutput) {
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "guest_wsl_elevated_task_no_output" -Extra @{
        last_task_result = $taskProbe.last_task_result
    }
    exit 2
}

Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "guest_wsl_runtime_unavailable"
exit 2
