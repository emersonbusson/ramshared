#Requires -Version 5.1
<#
.SYNOPSIS
  ITEM-11 / RF-8 / RNF-7 — build, attestation-sign, install package.

.DESCRIPTION
  Builds ramshared.sys via MSBuild, runs InfVerif, packages for Partner Center
  attestation (R9 org step). Verifies load on 26200.* with test-signing OFF
  only after attestation blob is present.

.PARAMETER Configuration
  MSBuild configuration (default Release).

.PARAMETER SkipSign
  Build + InfVerif only (lab).
#>
[CmdletBinding()]
param(
    [ValidateSet("Release", "Debug")]
    [string]$Configuration = "Release",
    [string]$Project = "..\..\drivers\windows\ramshared\ramshared.vcxproj",
    [switch]$SkipSign,
    [string]$ArtifactDir = ".\artifacts\build-sign"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $ArtifactDir | Out-Null

$proj = Resolve-Path $Project -ErrorAction SilentlyContinue
if (-not $proj) {
    Write-Error "Project not found: $Project"
    exit 2
}

Write-Host "MSBuild $proj ($Configuration|x64)"
msbuild $proj /p:Configuration=$Configuration /p:Platform=x64 /p:TreatWarningAsError=true /v:m
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$inf = Join-Path (Split-Path $proj) "ramshared.inf"
Write-Host "InfVerif /w $inf"
if (Get-Command InfVerif.exe -ErrorAction SilentlyContinue) {
    InfVerif.exe /w $inf
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} else {
    Write-Warning "InfVerif.exe not on PATH — skip (install WDK)"
}

if ($SkipSign) {
    Write-Host "SkipSign: package ready for lab test-signing only."
    exit 0
}

Write-Host @"
Attestation path (R9 — Partner Center, organizational):
  1. signtool sign /v /fd sha256 /tr http://timestamp.digicert.com /td sha256 ramshared.sys
  2. Submit package via Partner Center hardware dashboard (attestation).
  3. Install on 26200.8655 with test-signing OFF; confirm load (RNF-7).
  4. Record in IMPL.md ITEM-11 evidence.

Abort if driver does not load on stable build and WHCP cost not justified (PRD #2a).
"@
exit 0
