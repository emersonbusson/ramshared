#Requires -Version 5.1
<#
.SYNOPSIS
  Build ramshared.sys + poolstress.sys with VS Build Tools + WDK (host elevated).

.DESCRIPTION
  Uses vcvars64 + cl/link (KM). Output under drivers/windows/*/x64/Release.
#>
[CmdletBinding()]
param(
    [string]$RepoRoot = "C:\Users\emedev\ramshared-src",
    [string]$KitVersion = "10.0.26100.0"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot
$log = Join-Path $RepoRoot "artifacts\build-drivers.log"
New-Item -ItemType Directory -Force -Path (Split-Path $log) | Out-Null
Start-Transcript -Path $log -Force

function Find-VcVars {
    $p = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
    if (-not (Test-Path $p)) { throw "vcvars64.bat not found: $p" }
    return $p
}

function Invoke-CmdBat {
    param([string]$Bat, [string]$Extra)
    $cmd = "`"$Bat`" && $Extra"
    Write-Output "CMD> $Extra"
    & cmd.exe /c $cmd
    if ($LASTEXITCODE -ne 0) { throw "command failed exit=$LASTEXITCODE : $Extra" }
}

$vcvars = Find-VcVars
$kit = "C:\Program Files (x86)\Windows Kits\10"
$incKm = "$kit\Include\$KitVersion\km"
$incShared = "$kit\Include\$KitVersion\shared"
$incKmCrt = "$kit\Include\$KitVersion\km\crt"
$libKm = "$kit\Lib\$KitVersion\km\x64"
$libUcrt = "$kit\Lib\$KitVersion\ucrt\x64"

if (-not (Test-Path "$incKm\storport.h")) { throw "storport.h missing under $incKm" }
if (-not (Test-Path "$libKm\storport.lib")) { throw "storport.lib missing under $libKm" }

$cflags = @(
    "/nologo", "/c", "/kernel", "/GS-", "/W4", "/WX", "/wd4324", "/O2", "/Z7",
    "/D_WIN64", "/D_AMD64_", "/DAMD64", "/DDEPRECATE_DDK_FUNCTIONS=1",
    "/D_WIN32_WINNT=0x0A00", "/DWINVER=0x0A00", "/DNTDDI_VERSION=0xA000010",
    "/I`"$incShared`"", "/I`"$incKm`"", "/I`"$incKmCrt`""
) -join " "

$ldflagsCommon = @(
    "/nologo", "/driver", "/entry:GsDriverEntry", "/subsystem:NATIVE",
    "/nodefaultlib", "/incremental:no", "/debug",
    "/libpath:`"$libKm`"",
    "ntoskrnl.lib", "hal.lib", "wmilib.lib", "BufferOverflowFastFailK.lib", "ntstrsafe.lib"
) -join " "

# --- ramshared.sys (StorPort) ---
$srcDir = Join-Path $RepoRoot "drivers\windows\ramshared"
$outDir = Join-Path $srcDir "x64\Release"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$srcs = @("driver.c", "virtdisk.c", "queue.c", "control.c")
$objs = @()
foreach ($s in $srcs) {
    $obj = Join-Path $outDir ($s -replace '\.c$', '.obj')
    $objs += "`"$obj`""
    $src = Join-Path $srcDir $s
    Invoke-CmdBat -Bat $vcvars -Extra "cl $cflags /Fo`"$obj`" `"$src`""
}
$sys = Join-Path $outDir "ramshared.sys"
$objList = $objs -join " "
Invoke-CmdBat -Bat $vcvars -Extra "link $ldflagsCommon /out:`"$sys`" $objList storport.lib wdmsec.lib"
Write-Output "BUILT $sys"
Get-Item $sys | Format-List FullName, Length, LastWriteTime | Out-String | Write-Output

# --- poolstress.sys ---
$psDir = Join-Path $RepoRoot "drivers\windows\tools\poolstress"
$psOut = Join-Path $psDir "x64\Release"
New-Item -ItemType Directory -Force -Path $psOut | Out-Null
$psObj = Join-Path $psOut "poolstress.obj"
$psSrc = Join-Path $psDir "poolstress.c"
$psSys = Join-Path $psOut "poolstress.sys"
Invoke-CmdBat -Bat $vcvars -Extra "cl $cflags /Fo`"$psObj`" `"$psSrc`""
Invoke-CmdBat -Bat $vcvars -Extra "link $ldflagsCommon /out:`"$psSys`" `"$psObj`" cng.lib"
Write-Output "BUILT $psSys"
Get-Item $psSys | Format-List FullName, Length, LastWriteTime | Out-String | Write-Output

Write-Output "BUILD_DRIVERS_OK"
Stop-Transcript
