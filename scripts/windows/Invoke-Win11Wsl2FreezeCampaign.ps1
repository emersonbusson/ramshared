#Requires -Version 5.1
<#
.SYNOPSIS
  Run the WSL2 freeze-elimination campaign inside the isolated win11-drill VM.

.DESCRIPTION
  This harness is the safe path for WSL2 freeze proof. It refuses to use the
  daily host WSL2 desktop. The only live target is a Windows guest VM reached by
  PowerShell Direct. It emits PASS only when the in-guest WSL2 campaign artifact
  validates with the repository validator.

  If credentials, WSL2, a distro, or the repo path are missing, it emits
  STATUS=PARTIAL with a JSON artifact instead of guessing or touching the daily
  WSL2 host.
#>
[CmdletBinding()]
param(
    [string]$VMName = "win11-drill",
    [string]$User = "WIN11-DRILL\drilladmin",
    [string]$Password = "",
    [string]$PasswordFile = "C:\ramshared\bin\.drill-pw",
    [string]$GuestRepo = "C:\ramshared\src",
    [string]$GuestDistro = "",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [int]$PsDirectReadyTimeoutSec = 900,
    [int]$PsDirectRetrySec = 10,
    [switch]$Start,
    [switch]$Run
)

$ErrorActionPreference = "Stop"

function New-ArtifactDir {
    param([string]$Root)
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $dir = Join-Path $Root "win11-wsl2-freeze-campaign-$stamp"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    return $dir
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
        DAILY_HOST_USED = $false
    }
    foreach ($k in $Extra.Keys) { $summary[$k] = $Extra[$k] }
    $summary | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $Dir "summary.json")
    Write-Host "STATUS=$Status"
    Write-Host "REASON=$Reason"
    Write-Host "ARTIFACT_DIR=$Dir"
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

function Get-GuestWslExe {
    $packaged = "C:\Program Files\WSL\wsl.exe"
    if (Test-Path -LiteralPath $packaged) {
        return $packaged
    }
    return "wsl.exe"
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
    $identity = Invoke-GuestWithRetry -Credential $cred -ScriptBlock {
        [pscustomobject]@{ host = $env:COMPUTERNAME; whoami = (whoami) }
    }
    $features = Invoke-GuestWithRetry -Credential $cred -ScriptBlock {
        @(
            foreach ($featureName in @("Microsoft-Windows-Subsystem-Linux", "VirtualMachinePlatform")) {
                $state = "Unknown"
                try {
                    $state = (Get-WindowsOptionalFeature -Online -FeatureName $featureName -ErrorAction Stop).State.ToString()
                } catch {
                    $state = "Error"
                }
                [pscustomobject]@{ FeatureName = $featureName; State = $state }
            }
        )
    }
    $repoExists = Invoke-GuestWithRetry -Credential $cred -ScriptBlock {
        param($GuestRepo)
        Test-Path -LiteralPath $GuestRepo
    } -ArgumentList $GuestRepo
    $wslList = Invoke-GuestWithRetry -Credential $cred -ScriptBlock {
        function Get-GuestWslExe {
            $packaged = "C:\Program Files\WSL\wsl.exe"
            if (Test-Path -LiteralPath $packaged) {
                return $packaged
            }
            return "wsl.exe"
        }
        $wslExe = Get-GuestWslExe
        $job = Start-Job -ScriptBlock {
            param($WslExe)
            & $WslExe -l -v 2>&1 | Out-String
        } -ArgumentList $wslExe
        if (Wait-Job $job -Timeout 20) {
            Receive-Job $job | Out-String
        } else {
            Stop-Job $job -Force -ErrorAction SilentlyContinue
            "WSL_LIST_TIMEOUT"
        }
        Remove-Job $job -Force -ErrorAction SilentlyContinue
    }
    $distro = $GuestDistro
    if ([string]::IsNullOrWhiteSpace($distro)) {
        $line = ($wslList -split "`r?`n" | Where-Object { $_ -match '\S+\s+Running|\S+\s+Stopped' } | Select-Object -First 1)
        if ($line) {
            $distro = (($line -replace '^\s*\*?\s*', '') -split '\s+')[0]
        }
    }
    $probe = [pscustomobject]@{
        host = $identity.host
        whoami = $identity.whoami
        wsl_list = ($wslList | Out-String)
        features = $features
        repo_exists = [bool]$repoExists
        distro = $distro
    }
    $probe | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $artifactDir "probe.json")
} catch {
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "powershell_direct_failed" -Extra @{
        error = $_.Exception.Message
    }
    exit 2
}

$wslFeature = @($probe.features | Where-Object { $_.FeatureName -eq "Microsoft-Windows-Subsystem-Linux" -and $_.State -eq "Enabled" }).Count -gt 0
$vmpFeature = @($probe.features | Where-Object { $_.FeatureName -eq "VirtualMachinePlatform" -and $_.State -eq "Enabled" }).Count -gt 0
if (-not ($wslFeature -and $vmpFeature)) {
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "wsl2_features_not_enabled" -Extra @{
        wsl_feature = $wslFeature
        vmp_feature = $vmpFeature
    }
    exit 2
}
if (-not $probe.repo_exists) {
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "guest_repo_missing" -Extra @{
        guest_repo = $GuestRepo
    }
    exit 2
}
if ($probe.wsl_list -match "not installed|REGDB_E_CLASSNOTREG|Wsl/CallMsi|CLASSNOTREG|WSL_LIST_TIMEOUT") {
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "guest_wsl_runtime_unavailable"
    exit 2
}
if ([string]::IsNullOrWhiteSpace($probe.distro)) {
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "guest_wsl_distro_missing"
    exit 2
}
if (-not $Run) {
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "plan_only" -Extra @{
        distro = $probe.distro
    }
    exit 2
}

$guestResult = Invoke-GuestWithRetry -Credential $cred -ScriptBlock {
    param($GuestRepo, $Distro)
    function Get-GuestWslExe {
        $packaged = "C:\Program Files\WSL\wsl.exe"
        if (Test-Path -LiteralPath $packaged) {
            return $packaged
        }
        return "wsl.exe"
    }
    $wslExe = Get-GuestWslExe
    $cmd = @"
set -euo pipefail
cd /mnt/c/ramshared/src
export RAMSHARED_ISOLATED_LAB=1
export RAMSHARED_FORCE_ISOLATED_LAB=1
./scripts/safety/wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated --artifact-dir /tmp/ramshared-wsl2-freeze-win11 --rounds 2 --json
./scripts/safety/validate-wsl2-freeze-campaign-artifact.sh /tmp/ramshared-wsl2-freeze-win11
"@
    & $wslExe -d $Distro -- bash -lc $cmd 2>&1 | Out-String
} -ArgumentList $GuestRepo, $probe.distro
$guestResult | Set-Content -Encoding UTF8 (Join-Path $artifactDir "guest-campaign.out")

if ($guestResult -match 'WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS') {
    Write-Summary -Dir $artifactDir -Status "PASS" -Reason "validated_isolated_guest_campaign" -Extra @{
        distro = $probe.distro
    }
    exit 0
}

Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "guest_campaign_validation_missing" -Extra @{
    distro = $probe.distro
}
exit 2
