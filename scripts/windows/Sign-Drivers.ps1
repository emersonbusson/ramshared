#Requires -Version 5.1
<#
.SYNOPSIS
  Test-sign ramshared.sys / poolstress.sys for VM load (testsigning ON).

.PARAMETER PfxPath
  Code-signing PFX. Create once with New-SelfSignedCertificate -Type CodeSigningCert.

.PARAMETER PfxPassword
  Or env RAMSHARED_TESTSIGN_PFX_PASSWORD.
#>
[CmdletBinding()]
param(
    [string]$RepoRoot = "C:\Users\emedev\ramshared-src",
    [string]$PfxPath = "C:\Users\emedev\ramshared-drill\certs\ramshared-test.pfx",
    [string]$PfxPassword = $env:RAMSHARED_TESTSIGN_PFX_PASSWORD
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrEmpty($PfxPassword)) {
    throw "Set -PfxPassword or RAMSHARED_TESTSIGN_PFX_PASSWORD"
}
$signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "\\x64\\" } | Select-Object -First 1 -ExpandProperty FullName
if (-not $signtool) { throw "signtool.exe not found (install WDK)" }

$files = @(
    (Join-Path $RepoRoot "drivers\windows\ramshared\x64\Release\ramshared.sys"),
    (Join-Path $RepoRoot "drivers\windows\tools\poolstress\x64\Release\poolstress.sys")
)
foreach ($f in $files) {
    if (-not (Test-Path $f)) { throw "missing $f — run Build-Drivers.ps1 first" }
    Write-Host "SIGN $f"
    & $signtool sign /fd SHA256 /f $PfxPath /p $PfxPassword $f
    if ($LASTEXITCODE -ne 0) { throw "signtool failed $LASTEXITCODE" }
    & $signtool verify /pa $f
}
Write-Host "SIGN_OK"
