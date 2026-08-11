#Requires -Version 5.1
<#
.SYNOPSIS
  Build a Windows installer ISO that boots without the EFI "press any key" prompt.

.DESCRIPTION
  The source Windows ISO already contains efisys_noprompt.bin. This helper
  copies the ISO contents to a new staging directory, injects a sealed
  Autounattend.xml file at the ISO root, and calls oscdimg.exe with the
  no-prompt EFI boot sector. It never mounts VHDs, formats disks, modifies VMs,
  or overwrites an existing output ISO unless -Force is supplied.
#>
[CmdletBinding()]
param(
    [string]$SourceIso = "E:\Hyper-V\iso\Win11_25H2_English_x64_v2.iso",
    [string]$AutounattendXml = "E:\Hyper-V\iso\unattend-staging\Autounattend.xml",
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Fa-f0-9]{64}$')]
    [string]$SealedAutounattendSha256,
    [string]$OutputIso = "E:\Hyper-V\iso\Win11_25H2_English_x64_v2_noprompt_unattend.iso",
    [string]$StagingRoot = "E:\Hyper-V\iso\staging\noprompt-win11",
    [string]$Oscdimg = "",
    [ValidateRange(10, 300)]
    [int]$MediaProbeTimeoutSeconds = 90,
    [ValidateRange(300, 1800)]
    [int]$MediaStageTimeoutSeconds = 900,
    [ValidateRange(300, 1800)]
    [int]$OscdimgTimeoutSeconds = 900,
    [switch]$Force
)

$ErrorActionPreference = "Stop"

$mediaContractPath = Join-Path $PSScriptRoot "Win11LabMediaContract.ps1"
if (-not (Test-Path -LiteralPath $mediaContractPath -PathType Leaf)) {
    Write-Error "Win11 lab media contract helper is missing"
    exit 2
}
. $mediaContractPath

function Fail([string]$Message) {
    Write-Error $Message
    exit 2
}

function Resolve-Oscdimg {
    param([string]$Candidate)
    if (-not [string]::IsNullOrWhiteSpace($Candidate)) {
        if (Test-Path -LiteralPath $Candidate) {
            return (Resolve-Path -LiteralPath $Candidate).Path
        }
        Fail "oscdimg.exe not found at explicit path: $Candidate"
    }
    $roots = @(
        "C:\Program Files (x86)\Windows Kits",
        "C:\Program Files\Windows Kits",
        "C:\Program Files (x86)",
        "C:\Program Files"
    )
    foreach ($root in $roots) {
        if (-not (Test-Path -LiteralPath $root)) {
            continue
        }
        $found = Get-ChildItem -LiteralPath $root -Filter oscdimg.exe -Recurse -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($found) {
            return $found.FullName
        }
    }
    Fail "oscdimg.exe not found. Install Windows ADK Deployment Tools or pass -Oscdimg."
}

if (-not (Test-Path -LiteralPath $SourceIso)) {
    Fail "Source ISO not found: $SourceIso"
}
if (-not (Test-Path -LiteralPath $AutounattendXml)) {
    Fail "Autounattend.xml not found: $AutounattendXml"
}
if ((Test-Path -LiteralPath $OutputIso) -and -not $Force) {
    Fail "Output ISO already exists: $OutputIso"
}

try {
    $sealedAutounattendContract = Get-Win11LabAutounattendContract -Path $AutounattendXml
    Assert-Win11LabSealedAutounattendContract `
        -Contract $sealedAutounattendContract `
        -ExpectedSha256 $SealedAutounattendSha256
} catch {
    Fail $_.Exception.Message
}

if (Test-Path -LiteralPath $StagingRoot) {
    $children = @(Get-ChildItem -LiteralPath $StagingRoot -Force -ErrorAction SilentlyContinue)
    if ($children.Count -gt 0) {
        if (-not $Force) {
            Fail "Staging root exists and is not empty: $StagingRoot"
        }
        Remove-Item -LiteralPath $StagingRoot -Recurse -Force
        New-Item -ItemType Directory -Force -Path $StagingRoot | Out-Null
    }
} else {
    New-Item -ItemType Directory -Force -Path $StagingRoot | Out-Null
}

$oscdimgPath = Resolve-Oscdimg -Candidate $Oscdimg
try {
    Invoke-Win11LabSourceIsoStage -IsoPath $SourceIso -StagingRoot $StagingRoot -TimeoutSeconds $MediaStageTimeoutSeconds | Out-Null
    Copy-Item -LiteralPath $AutounattendXml -Destination (Join-Path $StagingRoot "Autounattend.xml") -Force
    $stagedAutounattendContract = Get-Win11LabAutounattendContract -Path (Join-Path $StagingRoot "Autounattend.xml")
    Assert-Win11LabSealedAutounattendContract `
        -Contract $stagedAutounattendContract `
        -ExpectedSha256 $SealedAutounattendSha256

    $stagedBiosBoot = Join-Path $StagingRoot "boot\etfsboot.com"
    $stagedEfiNoPrompt = Join-Path $StagingRoot "efi\microsoft\boot\efisys_noprompt.bin"
    $bootData = "2#p0,e,b$stagedBiosBoot#pEF,e,b$stagedEfiNoPrompt"
    Invoke-Win11LabExternalProcessBounded `
        -FilePath $oscdimgPath `
        -ArgumentValues @("-m", "-o", "-u2", "-udfver102", "-bootdata:$bootData", $StagingRoot, $OutputIso) `
        -TimeoutSeconds $OscdimgTimeoutSeconds | Out-Null

    $embeddedAutounattendContract = Invoke-Win11LabPrimaryIsoContract -IsoPath $OutputIso -TimeoutSeconds $MediaProbeTimeoutSeconds
    $mediaContractReceipt = Assert-Win11LabPrimaryIsoContract `
        -SealedContract $sealedAutounattendContract `
        -EmbeddedContract $embeddedAutounattendContract `
        -SealedAutounattendSha256 $SealedAutounattendSha256 `
        -EfiNoPromptPresent ([bool]$embeddedAutounattendContract.efi_noprompt_present)

    [pscustomobject]@{
        output_iso = $OutputIso
        source_iso = $SourceIso
        autounattend_xml = $AutounattendXml
        sealed_autounattend_sha256 = $mediaContractReceipt.sealed_sha256
        embedded_autounattend_sha256 = $mediaContractReceipt.embedded_sha256
        primary_iso_efi_noprompt = $mediaContractReceipt.efi_noprompt_present
        staging_root = $StagingRoot
        oscdimg = $oscdimgPath
        efi_boot = "efisys_noprompt.bin"
        disk_mutation = $false
    } | ConvertTo-Json -Depth 4
} catch {
    Fail $_.Exception.Message
}
