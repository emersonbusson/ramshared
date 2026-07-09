#Requires -Version 5.1
<#
.SYNOPSIS
  Build C# lab SCM service, install delayed-auto, optional start (VM only).
#>
[CmdletBinding()]
param(
    [string]$SourceCs = "C:\ramshared\bin\RamSharedWinSvc.cs",
    [string]$OutExe = "C:\ramshared\bin\RamSharedWinSvc.exe",
    [switch]$StartNow
)

$ErrorActionPreference = "Stop"
New-Item -Force -ItemType Directory C:\ramshared\bin, C:\ramshared\package | Out-Null

$csc = (Get-ChildItem "C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe" -EA Stop).FullName
if (-not (Test-Path $SourceCs)) {
    throw "missing $SourceCs"
}

& $csc /nologo /target:exe /platform:x64 `
    /r:System.ServiceProcess.dll `
    /out:$OutExe `
    $SourceCs
if ($LASTEXITCODE -ne 0) { throw "csc failed $LASTEXITCODE" }
Write-Host "BUILT $OutExe size=$((Get-Item $OutExe).Length)"

$svc = Get-Service -Name RamSharedWinSvc -EA SilentlyContinue
if ($svc) {
    Stop-Service RamSharedWinSvc -Force -EA SilentlyContinue
    Start-Sleep 2
    sc.exe delete RamSharedWinSvc | Out-Null
    Start-Sleep 2
}

# LocalSystem, delayed auto-start
sc.exe create RamSharedWinSvc binPath= "`"$OutExe`"" start= delayed-auto DisplayName= "RamShared VRAM Disk Service"
if ($LASTEXITCODE -ne 0) { throw "sc create failed $LASTEXITCODE" }
sc.exe description RamSharedWinSvc "Lab SCM: Start/Stop-RamSharedLab (DT-9). VM only until host-real gate."
sc.exe failure RamSharedWinSvc reset= 86400 actions= //////
# no auto-restart on failure during lab (make crashes visible)

if ($StartNow) {
    Start-Service RamSharedWinSvc
    Start-Sleep 10
}

Get-Service RamSharedWinSvc | Format-List Name, Status, StartType
Write-Host "INSTALL_OK"
exit 0
