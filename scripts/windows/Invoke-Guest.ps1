#Requires -Version 5.1
<#
.SYNOPSIS
  Run a script block or file inside Hyper-V guest win11-drill via PowerShell Direct.

.DESCRIPTION
  Must be launched elevated (use scripts/windows/wsl-elevated-ps.sh from WSL).
  Credential defaults match Passo 0 drill (local drilladmin).

.EXAMPLE
  # From WSL:
  ./scripts/windows/wsl-elevated-ps.sh -File C:\Users\emedev\ramshared-src\scripts\windows\Invoke-Guest.ps1 -Command "hostname; whoami"
#>
[CmdletBinding()]
param(
    [string]$VMName = "win11-drill",
    [string]$User = "WIN11-DRILL\drilladmin",
    # Prefer env RAMSHARED_DRILL_PASSWORD (do not commit secrets). Passo 0 lab only.
    [string]$Password = $env:RAMSHARED_DRILL_PASSWORD,
    [string]$Command = "hostname; whoami; [Environment]::OSVersion.VersionString",
    [string]$File = ""
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrEmpty($Password)) {
    throw "Set -Password or env RAMSHARED_DRILL_PASSWORD (lab guest local admin)."
}
$pass = ConvertTo-SecureString $Password -AsPlainText -Force
$cred = New-Object System.Management.Automation.PSCredential($User, $pass)

$vm = Get-VM -Name $VMName -ErrorAction Stop
if ($vm.State -ne "Running") {
    Write-Host "Starting $VMName..."
    Start-VM -Name $VMName
    $t = 0
    while ((Get-VM $VMName).State -ne "Running" -and $t -lt 60) {
        Start-Sleep 2
        $t += 2
    }
}

if ($File -ne "") {
    Invoke-Command -VMName $VMName -Credential $cred -FilePath $File
} else {
    Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock ([scriptblock]::Create($Command))
}
