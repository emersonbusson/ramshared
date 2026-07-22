#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Invoke-Win11Wsl2FreezeCampaign.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

foreach ($needle in @(
    'win11-drill',
    'PowerShell Direct',
    'GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)',
    'function Invoke-GuestWithRetry',
    'PowerShell Direct did not become ready',
    'foreach ($featureName in @("Microsoft-Windows-Subsystem-Linux", "VirtualMachinePlatform"))',
    'Start-Job -ScriptBlock {',
    '& $WslExe -l -v',
    'WSL_LIST_TIMEOUT',
    'DAILY_HOST_USED',
    'RAMSHARED_ISOLATED_LAB=1',
    'RAMSHARED_FORCE_ISOLATED_LAB=1',
    'wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated',
    'validate-wsl2-freeze-campaign-artifact.sh',
    'WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS',
    'STATUS=PARTIAL',
    'powershell_direct_failed',
    'wsl2_features_not_enabled',
    'guest_wsl_runtime_unavailable',
    'CLASSNOTREG|WSL_LIST_TIMEOUT',
    'guest_wsl_distro_missing'
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("win11_wsl2_freeze_static: missing " + $needle)
    }
}

if ($text -match 'RAMSHARED_DRILL_PASSWORD.*Set-Content|Password.*ConvertTo-Json|Initialize-Disk|Format-Volume|Resize-VHD|Convert-VHD') {
    throw "win11_wsl2_freeze_static: secrets or disk mutation are forbidden"
}

Write-Output "PASS Test-Win11Wsl2FreezeCampaignStatic"
