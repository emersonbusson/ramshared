#Requires -Version 5.1
<#
.SYNOPSIS
  Build a Windows installer ISO that boots without the EFI "press any key" prompt.

.DESCRIPTION
  The source Windows ISO already contains efisys_noprompt.bin. This helper
  copies the ISO contents to a new staging directory, optionally injects an
  Autounattend.xml file at the ISO root, and calls oscdimg.exe with the
  no-prompt EFI boot sector. It never mounts VHDs, formats disks, modifies VMs,
  or overwrites an existing output ISO unless -Force is supplied.
#>
[CmdletBinding()]
param(
    [string]$SourceIso = "E:\Hyper-V\iso\Win11_25H2_English_x64_v2.iso",
    [string]$AutounattendXml = "E:\Hyper-V\iso\unattend-staging\Autounattend.xml",
    [string]$OutputIso = "E:\Hyper-V\iso\Win11_25H2_English_x64_v2_noprompt_unattend.iso",
    [string]$StagingRoot = "E:\Hyper-V\iso\staging\noprompt-win11",
    [string]$Oscdimg = "",
    [switch]$Force
)

$ErrorActionPreference = "Stop"

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
if (Test-Path -LiteralPath $StagingRoot) {
    $children = @(Get-ChildItem -LiteralPath $StagingRoot -Force -ErrorAction SilentlyContinue)
    if ($children.Count -gt 0) {
        Fail "Staging root exists and is not empty: $StagingRoot"
    }
} else {
    New-Item -ItemType Directory -Force -Path $StagingRoot | Out-Null
}

$oscdimgPath = Resolve-Oscdimg -Candidate $Oscdimg
$mounted = $null
try {
    $mounted = Mount-DiskImage -ImagePath $SourceIso -PassThru
    $volume = $mounted | Get-Volume
    if (-not $volume.DriveLetter) {
        Fail "Mounted source ISO has no drive letter"
    }
    $sourceRoot = "$($volume.DriveLetter):\"
    $efiNoPrompt = Join-Path $sourceRoot "efi\microsoft\boot\efisys_noprompt.bin"
    $biosBoot = Join-Path $sourceRoot "boot\etfsboot.com"
    if (-not (Test-Path -LiteralPath $efiNoPrompt)) {
        Fail "Source ISO lacks efisys_noprompt.bin"
    }
    if (-not (Test-Path -LiteralPath $biosBoot)) {
        Fail "Source ISO lacks BIOS boot sector etfsboot.com"
    }

    robocopy $sourceRoot $StagingRoot /MIR /NFL /NDL /NJH /NJS /NP | Out-Null
    $rc = $LASTEXITCODE
    if ($rc -gt 7) {
        Fail "robocopy failed with exit code $rc"
    }
    Copy-Item -LiteralPath $AutounattendXml -Destination (Join-Path $StagingRoot "Autounattend.xml") -Force

    $stagedBiosBoot = Join-Path $StagingRoot "boot\etfsboot.com"
    $stagedEfiNoPrompt = Join-Path $StagingRoot "efi\microsoft\boot\efisys_noprompt.bin"
    $bootData = "2#p0,e,b$stagedBiosBoot#pEF,e,b$stagedEfiNoPrompt"
    & $oscdimgPath -m -o -u2 -udfver102 "-bootdata:$bootData" $StagingRoot $OutputIso
    if ($LASTEXITCODE -ne 0) {
        Fail "oscdimg failed with exit code $LASTEXITCODE"
    }

    [pscustomobject]@{
        output_iso = $OutputIso
        source_iso = $SourceIso
        autounattend_xml = $AutounattendXml
        staging_root = $StagingRoot
        oscdimg = $oscdimgPath
        efi_boot = "efisys_noprompt.bin"
        disk_mutation = $false
    } | ConvertTo-Json -Depth 4
} finally {
    if ($mounted) {
        Dismount-DiskImage -ImagePath $SourceIso -ErrorAction SilentlyContinue | Out-Null
    }
}
