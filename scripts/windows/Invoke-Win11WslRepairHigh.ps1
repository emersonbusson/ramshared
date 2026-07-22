#Requires -Version 5.1
<#
.SYNOPSIS
  Repair WSL registration inside an isolated Windows lab VM.

.DESCRIPTION
  Uses PowerShell Direct to register and run a highest-privilege scheduled task
  inside the guest. The task repairs the WSL MSI, registers the installed Appx
  manifest, invokes wslservice registration hooks, and captures bounded output.
#>
[CmdletBinding()]
param(
    [string]$VMName = "win11-wsl2-lab",
    [string]$User = "WIN11-WSL2-LAB\drilladmin",
    [string]$Password = "",
    [string]$PasswordFile = "C:\ramshared\bin\.drill-pw",
    [string]$MsiPath = "C:\ramshared\downloads\wsl.2.7.10.0.x64.msi",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [int]$TimeoutSec = 300
)

$ErrorActionPreference = "Stop"

function Get-LocalDrillPassword {
    param([string]$InitialPassword, [string]$LocalPasswordFile)
    if (-not [string]::IsNullOrEmpty($InitialPassword)) { return $InitialPassword }
    foreach ($scope in @("Machine", "User")) {
        $value = [Environment]::GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)
        if (-not [string]::IsNullOrEmpty($value)) { return $value }
    }
    if (-not [string]::IsNullOrEmpty($env:RAMSHARED_DRILL_PASSWORD)) { return $env:RAMSHARED_DRILL_PASSWORD }
    if (Test-Path -LiteralPath $LocalPasswordFile) { return (Get-Content -LiteralPath $LocalPasswordFile -Raw).Trim() }
    return ""
}

$Password = Get-LocalDrillPassword -InitialPassword $Password -LocalPasswordFile $PasswordFile
if ([string]::IsNullOrEmpty($Password)) { throw "Missing local lab credential." }

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$artifactDir = Join-Path $ArtifactRoot "win11-wsl-repair-high-$stamp"
New-Item -ItemType Directory -Force -Path $artifactDir | Out-Null

$cred = [pscredential]::new($User, (ConvertTo-SecureString $Password -AsPlainText -Force))
$result = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
    param($TaskUser, $TaskPassword, $GuestMsiPath, $TimeoutSec)
    $ErrorActionPreference = "Stop"
    New-Item -ItemType Directory -Force -Path "C:\ramshared\artifacts" | Out-Null
    $script = "C:\ramshared\repair-wsl-high.ps1"
    $out = "C:\ramshared\artifacts\repair-wsl-high.out"
@'
$ErrorActionPreference = "Continue"
$out = "C:\ramshared\artifacts\repair-wsl-high.out"
function Add-Line([string]$Line) {
    $Line | Out-File -Encoding UTF8 -Append -LiteralPath $out
}
Add-Line "start=$(Get-Date -Format o) whoami=$(whoami)"
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
Add-Line "is_admin=$($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))"
Add-Line "msi_repair_start"
$p = Start-Process msiexec.exe -ArgumentList "/fa `"$GuestMsiPath`" /qn /norestart" -Wait -PassThru
Add-Line "msi_repair_exit=$($p.ExitCode)"
Add-Line "appx_register_start"
$pkg = Get-AppxPackage -AllUsers *WindowsSubsystemForLinux* | Select-Object -First 1
if ($pkg) {
    Add-AppxPackage -Register (Join-Path $pkg.InstallLocation "AppxManifest.xml") -DisableDevelopmentMode
    Add-Line "appx=$($pkg.PackageFullName)"
} else {
    Add-Line "appx=missing"
}
Add-Line "wslservice_register_start"
& "C:\Program Files\WSL\wslservice.exe" /install 2>&1 | Out-File -Encoding UTF8 -Append -LiteralPath $out
Add-Line "wslservice_install_exit=$LASTEXITCODE"
& "C:\Program Files\WSL\wslservice.exe" /register 2>&1 | Out-File -Encoding UTF8 -Append -LiteralPath $out
Add-Line "wslservice_register_exit=$LASTEXITCODE"
Get-Service *Lxss*,*wsl* -ErrorAction SilentlyContinue | Format-List | Out-File -Encoding UTF8 -Append -LiteralPath $out
Add-Line "status_start"
$job = Start-Job -ScriptBlock { & "C:\Program Files\WSL\wsl.exe" --status 2>&1 | Out-String; "exit=$LASTEXITCODE" }
if (Wait-Job $job -Timeout 45) {
    Receive-Job $job | Out-File -Encoding UTF8 -Append -LiteralPath $out
} else {
    Stop-Job $job
    Add-Line "status_timeout=true"
}
Remove-Job $job -Force -ErrorAction SilentlyContinue
Add-Line "end=$(Get-Date -Format o)"
'@ | Set-Content -Encoding UTF8 -LiteralPath $script

    $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument ('-NoProfile -ExecutionPolicy Bypass -File "{0}"' -f $script)
    $trigger = New-ScheduledTaskTrigger -Once -At (Get-Date).AddSeconds(10)
    $principal = New-ScheduledTaskPrincipal -UserId $TaskUser -RunLevel Highest -LogonType Password
    $task = New-ScheduledTask -Action $action -Trigger $trigger -Principal $principal
    Register-ScheduledTask -TaskName "RamSharedRepairWslHigh" -InputObject $task -User $TaskUser -Password $TaskPassword -Force | Out-Null
    Start-ScheduledTask -TaskName "RamSharedRepairWslHigh"
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        Start-Sleep -Seconds 5
        $state = (Get-ScheduledTask -TaskName "RamSharedRepairWslHigh").State
        $info = Get-ScheduledTaskInfo -TaskName "RamSharedRepairWslHigh"
    } while ($state -eq "Running" -and (Get-Date) -lt $deadline)
    $content = if (Test-Path -LiteralPath $out) { Get-Content -LiteralPath $out -Raw } else { "NO_OUTPUT" }
    Unregister-ScheduledTask -TaskName "RamSharedRepairWslHigh" -Confirm:$false
    [pscustomobject]@{
        state = $state.ToString()
        last_task_result = $info.LastTaskResult
        output = $content
    }
} -ArgumentList $User, $Password, $MsiPath, $TimeoutSec

$result | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $artifactDir "result.json")
[pscustomobject]@{
    artifact = $artifactDir
    state = $result.state
    last_task_result = $result.last_task_result
    output = $result.output
} | ConvertTo-Json -Depth 6
