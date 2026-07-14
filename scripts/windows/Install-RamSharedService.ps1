#Requires -Version 5.1
<#
.SYNOPSIS
  Build C# lab SCM service, install delayed-auto, optional start (VM / lab host).

.DESCRIPTION
  Closes issue #29 install path:
  - Copies Start/Stop lab scripts + RamSharedWinSvc.cs from the repo into C:\ramshared\bin
  - Builds RamSharedWinSvc.exe with csc (ServiceBase)
  - Registers delayed-auto LocalSystem service
  - OnStop is DT-9 fail-closed (pagefile-hot refuse)

  Product CUDA path remains crates/ramshared-winsvc (MSVC). This is the lab SCM that boots today.
#>
[CmdletBinding()]
param(
    [string]$RepoRoot = "",
    [string]$SourceCs = "",
    [string]$OutExe = "C:\ramshared\bin\RamSharedWinSvc.exe",
    [string]$BinDir = "C:\ramshared\bin",
    [switch]$StartNow,
    # Pass -Force through to Start-RamSharedLab format path under SCM.
    [switch]$ForceFormat
)

$ErrorActionPreference = "Stop"

if (-not $RepoRoot) {
    # Prefer repo relative to this script: scripts/windows -> repo root
    $here = Split-Path -Parent $MyInvocation.MyCommand.Path
    $guess = Resolve-Path (Join-Path $here "..\..") -ErrorAction SilentlyContinue
    if ($guess) { $RepoRoot = $guess.Path }
}
if (-not $RepoRoot -or -not (Test-Path $RepoRoot)) {
    throw "RepoRoot required (folder containing scripts\windows and crates)"
}

New-Item -Force -ItemType Directory $BinDir, C:\ramshared\package | Out-Null

$labCs = Join-Path $RepoRoot "scripts\windows\lab\RamSharedWinSvc.cs"
if (-not $SourceCs) { $SourceCs = $labCs }
if (-not (Test-Path $SourceCs)) { throw "missing $SourceCs" }

# Deploy scripts the service will call on start/stop.
$startSrc = Join-Path $RepoRoot "scripts\windows\Start-RamSharedLab.ps1"
$stopSrc = Join-Path $RepoRoot "scripts\windows\Stop-RamSharedLab.ps1"
if (-not (Test-Path $startSrc)) { throw "missing $startSrc" }
if (-not (Test-Path $stopSrc)) { throw "missing $stopSrc" }
Copy-Item $startSrc (Join-Path $BinDir "Start-RamSharedLab.ps1") -Force
Copy-Item $stopSrc (Join-Path $BinDir "Stop-RamSharedLab.ps1") -Force
Copy-Item $SourceCs (Join-Path $BinDir "RamSharedWinSvc.cs") -Force

$csc = (Get-ChildItem "C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe" -EA Stop).FullName
& $csc /nologo /target:exe /platform:x64 `
    /r:System.ServiceProcess.dll `
    /out:$OutExe `
    (Join-Path $BinDir "RamSharedWinSvc.cs")
if ($LASTEXITCODE -ne 0) { throw "csc failed $LASTEXITCODE" }
Write-Host "BUILT $OutExe size=$((Get-Item $OutExe).Length)"

$svc = Get-Service -Name RamSharedWinSvc -EA SilentlyContinue
if ($svc) {
    Stop-Service RamSharedWinSvc -Force -EA SilentlyContinue
    Start-Sleep 2
    sc.exe delete RamSharedWinSvc | Out-Null
    Start-Sleep 2
}

# LocalSystem, delayed auto-start (after storage stack)
$binPath = "`"$OutExe`""
if ($ForceFormat) {
    # Service reads RAMSHARED_WINSVC_FORCE_FORMAT=1
    [Environment]::SetEnvironmentVariable("RAMSHARED_WINSVC_FORCE_FORMAT", "1", "Machine")
}
sc.exe create RamSharedWinSvc binPath= $binPath start= delayed-auto DisplayName= "RamShared VRAM Disk Service"
if ($LASTEXITCODE -ne 0) { throw "sc create failed $LASTEXITCODE" }
sc.exe description RamSharedWinSvc "Lab SCM: Start/Stop-RamSharedLab (DT-9). OnStop refuses pagefile-hot kill."
sc.exe failure RamSharedWinSvc reset= 86400 actions= //////
# no auto-restart on failure during lab (make crashes visible)

if ($StartNow) {
    Start-Service RamSharedWinSvc
    Start-Sleep 10
}

Get-Service RamSharedWinSvc | Format-List Name, Status, StartType
Write-Host "INSTALL_OK RepoRoot=$RepoRoot"
exit 0
