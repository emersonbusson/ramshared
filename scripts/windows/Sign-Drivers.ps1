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
    if (-not (Test-Path $f)) { throw "missing $f - run Build-Drivers.ps1 first" }
    Write-Host "SIGN $f"
    & $signtool sign /fd SHA256 /f $PfxPath /p $PfxPassword $f
    if ($LASTEXITCODE -ne 0) { throw "signtool failed $LASTEXITCODE" }
    & $signtool verify /pa $f
}

# DT-25: Inf2Cat + sign catalog so pnputil accepts the package under testsigning
$pkg = Join-Path $RepoRoot "drivers\windows\ramshared\x64\Release\package"
New-Item -ItemType Directory -Force -Path $pkg | Out-Null
Copy-Item (Join-Path $RepoRoot "drivers\windows\ramshared\x64\Release\ramshared.sys") (Join-Path $pkg "ramshared.sys") -Force
Copy-Item (Join-Path $RepoRoot "drivers\windows\ramshared\ramshared.inf") (Join-Path $pkg "ramshared.inf") -Force

$inf2cat = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x86\Inf2Cat.exe"
if (-not (Test-Path $inf2cat)) {
    $inf2cat = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter Inf2Cat.exe -EA SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $inf2cat -or -not (Test-Path $inf2cat)) {
    Write-Warning "Inf2Cat.exe not found - package will lack .cat (pnputil may reject)"
} else {
    Write-Host "INF2CAT $inf2cat"
    Push-Location $pkg
    & $inf2cat /driver:. /os:10_X64 /verbose
    $cat = Join-Path $pkg "ramshared.cat"
    if (Test-Path $cat) {
        & $signtool sign /fd SHA256 /f $PfxPath /p $PfxPassword $cat
        & $signtool verify /pa $cat
        Write-Host "CAT_OK $cat"
    } else {
        Write-Warning "ramshared.cat not produced"
        Get-ChildItem $pkg | Format-Table Name, Length
    }
    Pop-Location
}
Write-Host "SIGN_OK package=$pkg"
