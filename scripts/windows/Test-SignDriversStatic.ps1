#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$RepoRoot
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
}

$script = Get-Content (Join-Path $RepoRoot "scripts\windows\Sign-Drivers.ps1") -Raw

if ($script -notmatch 'CertSubject') {
    throw "store_signing_fallback: Sign-Drivers.ps1 does not accept a certificate subject"
}
if ($script -notmatch 'CertStore') {
    throw "store_signing_fallback: Sign-Drivers.ps1 does not select CurrentUser/LocalMachine store"
}
if ($script -notmatch '/sm') {
    throw "store_signing_fallback: SignTool LocalMachine store flag is absent"
}
if ($script -match 'throw "Set -PfxPassword or RAMSHARED_TESTSIGN_PFX_PASSWORD"') {
    throw "store_signing_fallback: missing PFX password still blocks store signing"
}
if ($script -notmatch 'function Invoke-SignTool') {
    throw "store_signing_fallback: signing arguments are not centralized"
}
foreach ($token in @("PackageWorkDir", "UNC package staging", 'Copy-Item $pkgLocal\* $pkg')) {
    if ($script -notmatch [regex]::Escape($token)) {
        throw "unc_package_staging_missing: $token"
    }
}

Write-Output "PASS Test-SignDriversStatic"
