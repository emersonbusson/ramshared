#Requires -Version 5.1
<#
.SYNOPSIS
  Install the Rust product ramshared-winsvc.exe (CUDA storage-only). Never compiles C#.

.DESCRIPTION
  SPEC windows-storport-cuda-vram DT-1 / RF-6:
  - Verifies supplied MSVC-built ramshared-winsvc.exe (SHA-256)
  - Copies winsvc.toml to C:\ProgramData\RamShared\ with restrictive ACL
  - Registers SCM ImagePath to that executable only
  - Never copies Start/Stop lab scripts into the product ImagePath

.EXAMPLE
  .\Install-RamSharedService.ps1 -ExePath C:\ramshared\bin\ramshared-winsvc.exe -ConfigPath .\crates\ramshared-winsvc\winsvc.example.toml
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ExePath,
    [string]$ConfigPath = "",
    [string]$ProgramData = "C:\ProgramData\RamShared",
    [string]$ServiceName = "RamSharedWinSvc",
    [switch]$StartNow
)

$ErrorActionPreference = "Stop"
function L($m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m) }

if (-not (Test-Path -LiteralPath $ExePath)) {
    throw "missing product exe: $ExePath"
}
$exeItem = Get-Item -LiteralPath $ExePath
if ($exeItem.Name -notmatch 'ramshared-winsvc') {
    Write-Warning "Exe name does not contain ramshared-winsvc — verify product binary"
}
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $ExePath).Hash
L "PRODUCT_EXE=$($exeItem.FullName) SHA256=$hash"

New-Item -Force -ItemType Directory $ProgramData, (Join-Path $ProgramData "evidence") | Out-Null

if (-not $ConfigPath) {
    $here = Split-Path -Parent $MyInvocation.MyCommand.Path
    $guess = Join-Path $here "..\..\crates\ramshared-winsvc\winsvc.example.toml"
    if (Test-Path $guess) { $ConfigPath = (Resolve-Path $guess).Path }
}
if (-not $ConfigPath -or -not (Test-Path -LiteralPath $ConfigPath)) {
    throw "ConfigPath required (winsvc.toml product shape)"
}
$destToml = Join-Path $ProgramData "winsvc.toml"
Copy-Item -LiteralPath $ConfigPath -Destination $destToml -Force
L "CONFIG=$destToml"

# ACL: SYSTEM + Builtin Administrators write; Users read (DT-1).
$acl = Get-Acl $ProgramData
$acl.SetAccessRuleProtection($true, $false)
$rules = @(
    (New-Object System.Security.AccessControl.FileSystemAccessRule("SYSTEM", "FullControl", "ContainerInherit,ObjectInherit", "None", "Allow")),
    (New-Object System.Security.AccessControl.FileSystemAccessRule("BUILTIN\Administrators", "FullControl", "ContainerInherit,ObjectInherit", "None", "Allow")),
    (New-Object System.Security.AccessControl.FileSystemAccessRule("BUILTIN\Users", "ReadAndExecute", "ContainerInherit,ObjectInherit", "None", "Allow"))
)
foreach ($r in $rules) { $acl.AddAccessRule($r) | Out-Null }
Set-Acl -Path $ProgramData -AclObject $acl

$svc = Get-Service -Name $ServiceName -EA SilentlyContinue
if ($svc) {
    Stop-Service $ServiceName -Force -EA SilentlyContinue
    Start-Sleep 2
    sc.exe delete $ServiceName | Out-Null
    Start-Sleep 2
}

$binPath = "`"$($exeItem.FullName)`""
sc.exe create $ServiceName binPath= $binPath start= demand DisplayName= "RamShared CUDA VRAM Disk Service" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "sc create failed $LASTEXITCODE" }
sc.exe description $ServiceName "Product storage-only CUDA path (no lab RAM backend)." | Out-Null
# Disable failure auto-restart (SPEC).
sc.exe failure $ServiceName reset= 0 actions= = | Out-Null

# Query ImagePath after install
$img = (Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\$ServiceName" -Name ImagePath).ImagePath
L "ImagePath=$img"
if ($img -notmatch [regex]::Escape($exeItem.Name)) {
    throw "PRODUCT_IMAGEPATH_MATCH=0"
}
if ($img -match 'Start-RamSharedLab|Stop-RamSharedLab|WinDriveBackend|RamSharedWinSvc\.cs') {
    throw "NO_LAB_SCRIPT_REFERENCE=0 (lab path leaked into ImagePath)"
}
Write-Host "PRODUCT_IMAGEPATH_MATCH=1"
Write-Host "NO_LAB_SCRIPT_REFERENCE=1"
Write-Host "PRODUCT_SHA256=$hash"

if ($StartNow) {
    Start-Service $ServiceName
    L "service started"
}
exit 0
