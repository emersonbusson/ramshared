#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "New-WindowsNoPromptIso.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

foreach ($needle in @(
    'efisys_noprompt.bin',
    'Autounattend.xml',
    'oscdimg.exe not found',
    'Mount-DiskImage -ImagePath $SourceIso -PassThru',
    'Dismount-DiskImage -ImagePath $SourceIso',
    'robocopy $sourceRoot $StagingRoot /MIR',
    '-bootdata:$bootData',
    'disk_mutation = $false',
    'Output ISO already exists',
    'Staging root exists and is not empty'
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("windows_noprompt_iso_static: missing " + $needle)
    }
}

if ($text -match 'New-VHD|Remove-VHD|Format-Volume|Initialize-Disk|Resize-VHD|Convert-VHD|Remove-VM|Set-Content.*RAMSHARED_DRILL_PASSWORD') {
    throw "windows_noprompt_iso_static: disk mutation or secret persistence is forbidden"
}

Write-Output "PASS Test-WindowsNoPromptIsoStatic"
