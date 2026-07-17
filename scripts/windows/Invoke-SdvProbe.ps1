#Requires -Version 5.1
<#
.SYNOPSIS
  Probe Static Driver Verifier (SDV) availability for ramshared.sys.

.DESCRIPTION
  Does NOT claim SDV PASS. Detects sdv.exe / StaticDV and optionally invokes
  MSBuild /t:sdv on ramshared.vcxproj. Writes an artifact summary.

.EXAMPLE
  .\Invoke-SdvProbe.ps1 -ArtifactDir C:\ramshared\artifacts\sdv-probe
#>
[CmdletBinding()]
param(
    [string]$ProjectPath = "",
    [string]$ArtifactDir = "C:\ramshared\artifacts\sdv-probe",
    [string]$KitVersion = "10.0.26100.0"
)

$ErrorActionPreference = "Continue"
New-Item -Force -ItemType Directory $ArtifactDir | Out-Null

function L($m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m) }

if ([string]::IsNullOrWhiteSpace($ProjectPath)) {
    $candidates = @(
        (Join-Path $PSScriptRoot "..\..\drivers\windows\ramshared\ramshared.vcxproj"),
        "C:\ramshared\src\drivers\windows\ramshared\ramshared.vcxproj"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { $ProjectPath = (Resolve-Path $c).Path; break }
    }
}

$o = [ordered]@{
    ts = (Get-Date).ToString("o")
    project = $ProjectPath
    projectExists = (Test-Path $ProjectPath)
    sdvOnPath = $false
    sdvPath = $null
    staticDvOnPath = $false
    sdvTargetsPresent = $false
    sdvTargetsPath = $null
    msbuildPath = $null
    msbuildSdvExit = $null
    claim = "NOT_CLAIMED"
    reasons = @()
}

$sdvTargets = "C:\Program Files (x86)\Windows Kits\10\build\$KitVersion\WindowsDriver.Sdv.targets"
if (Test-Path $sdvTargets) {
    $o.sdvTargetsPresent = $true
    $o.sdvTargetsPath = $sdvTargets
} else {
    $o.reasons += "WindowsDriver.Sdv.targets_missing"
}

# where after vcvars
$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
$whereLog = Join-Path $ArtifactDir "where-sdv.txt"
if (Test-Path $vcvars) {
    cmd /c "`"$vcvars`" >nul && where sdv 2>nul && where StaticDV 2>nul" | Set-Content $whereLog
    $lines = @(Get-Content $whereLog -EA SilentlyContinue | Where-Object { $_ -and $_ -notmatch 'INFO:' })
    foreach ($line in $lines) {
        if ($line -match 'sdv\.exe$' -or $line -match '\\sdv\.exe') {
            $o.sdvOnPath = $true
            $o.sdvPath = $line.Trim()
        }
        if ($line -match 'StaticDV') {
            $o.staticDvOnPath = $true
        }
    }
} else {
    $o.reasons += "vcvars64_missing"
}

if (-not $o.sdvOnPath) {
    $o.reasons += "sdv.exe_not_on_path"
}

$msbuildCands = @(
    "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe",
    "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe"
)
foreach ($m in $msbuildCands) {
    if (Test-Path $m) { $o.msbuildPath = $m; break }
}
if (-not $o.msbuildPath) {
    $o.reasons += "msbuild_missing"
}

$msbuildLog = Join-Path $ArtifactDir "msbuild-sdv.log"
if ($o.msbuildPath -and $o.projectExists) {
    $projDir = Split-Path -Parent $ProjectPath
    Push-Location $projDir
    try {
        $args = @(
            $ProjectPath,
            "/t:sdv",
            "/p:Configuration=Release",
            "/p:Platform=x64",
            "/v:minimal"
        )
        L ("MSBuild /t:sdv on " + $ProjectPath)
        $p = Start-Process -FilePath $o.msbuildPath -ArgumentList $args -Wait -PassThru -NoNewWindow `
            -RedirectStandardOutput $msbuildLog -RedirectStandardError (Join-Path $ArtifactDir "msbuild-sdv.err")
        $o.msbuildSdvExit = $p.ExitCode
        if ($p.ExitCode -ne 0) {
            $tail = ""
            if (Test-Path $msbuildLog) { $tail = (Get-Content $msbuildLog -Raw) }
            if ($tail -match 'MSB4057' -or $tail -match 'does not exist') {
                $o.reasons += "msbuild_target_sdv_missing"
            } else {
                $o.reasons += ("msbuild_sdv_exit_" + $p.ExitCode)
            }
        }
    } finally {
        Pop-Location
    }
} else {
    $o.reasons += "msbuild_sdv_skipped"
}

# Claim only if sdv ran successfully (exit 0) and produced no fail - never auto-claim.
if ($o.sdvOnPath -and $o.msbuildSdvExit -eq 0) {
    $o.reasons += "manual_review_required_before_claim"
    # Still NOT_CLAIMED until a human/SPEC gate records PASS with defect count.
    $o.claim = "NOT_CLAIMED"
} else {
    $o.claim = "NOT_CLAIMED"
}

$jsonPath = Join-Path $ArtifactDir "sdv-probe-summary.json"
$o | ConvertTo-Json -Depth 4 | Set-Content -Path $jsonPath -Encoding utf8
L ("ARTIFACT=" + $jsonPath)
L ("SDV_CLAIM=" + $o.claim)
L ("reasons=" + ($o.reasons -join ","))
Write-Host ("SDV_PROBE_READY=" + [bool]$o.sdvOnPath)
Write-Host ("SDV_CLAIM=" + $o.claim)

# Exit 0 always: probe is informational; missing tool is expected env gap.
exit 0
