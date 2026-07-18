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
    [string]$Password = $env:RAMSHARED_DRILL_PASSWORD,
    [string]$PasswordFile = "C:\ramshared\bin\.drill-pw",
    [string]$GuestRepo = "C:\ramshared\src",
    [string]$GuestDistro = "",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
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

$artifactDir = New-ArtifactDir -Root $ArtifactRoot

if ([string]::IsNullOrEmpty($Password) -and (Test-Path -LiteralPath $PasswordFile)) {
    $Password = (Get-Content -LiteralPath $PasswordFile -Raw).Trim()
}
if ([string]::IsNullOrEmpty($Password)) {
    Write-Summary -Dir $artifactDir -Status "PARTIAL" -Reason "missing_guest_credential"
    exit 2
}

try {
    if ($Start) {
        Start-VM -Name $VMName -ErrorAction Stop
        Start-Sleep -Seconds 15
    }
    $sec = ConvertTo-SecureString $Password -AsPlainText -Force
    $cred = [pscredential]::new($User, $sec)
    $identity = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
        [pscustomobject]@{ host = $env:COMPUTERNAME; whoami = (whoami) }
    }
    $features = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
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
    $repoExists = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
        param($GuestRepo)
        Test-Path -LiteralPath $GuestRepo
    } -ArgumentList $GuestRepo
    $wslList = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
        $job = Start-Job -ScriptBlock { wsl.exe -l -v 2>&1 | Out-String }
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

$guestResult = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
    param($GuestRepo, $Distro)
    $cmd = @"
set -euo pipefail
cd /mnt/c/ramshared/src
export RAMSHARED_ISOLATED_LAB=1
export RAMSHARED_FORCE_ISOLATED_LAB=1
./scripts/safety/wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated --artifact-dir /tmp/ramshared-wsl2-freeze-win11 --rounds 2 --json
./scripts/safety/validate-wsl2-freeze-campaign-artifact.sh /tmp/ramshared-wsl2-freeze-win11
"@
    wsl.exe -d $Distro -- bash -lc $cmd 2>&1 | Out-String
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
