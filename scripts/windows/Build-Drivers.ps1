[CmdletBinding()]
param(
    [ValidateNotNullOrEmpty()]
    [string]$RepoRoot = "C:\ramshared\src",

    [ValidateNotNullOrEmpty()]
    [string]$KitVersion = "10.0.26100.0",

    [switch]$CodeAnalysis
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $RepoRoot -PathType Container)) {
    Write-Error "RepoRoot does not exist: $RepoRoot" -ErrorId "RepoRootNotFound" -ErrorAction Continue
    throw [System.IO.DirectoryNotFoundException]::new("RepoRoot does not exist: $RepoRoot")
}

Set-Location -LiteralPath $RepoRoot

$log = Join-Path $RepoRoot "artifacts\build-drivers.log"
New-Item -ItemType Directory -Force -Path (Split-Path $log) | Out-Null
Start-Transcript -Path $log -Force

function Find-VcVar {
    $p = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) {
        Write-Error "vcvars64.bat not found: $p" -ErrorId "VcVarsNotFound" -ErrorAction Continue
        throw [System.IO.FileNotFoundException]::new("vcvars64.bat not found: $p")
    }

    $msbuild = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe"
    if (-not (Test-Path -LiteralPath $msbuild -PathType Leaf)) {
        Write-Error "MSBuild.exe not found: $msbuild" -ErrorId "MSBuildNotFound" -ErrorAction Continue
        throw [System.IO.FileNotFoundException]::new("MSBuild.exe not found: $msbuild")
    }

    return $p
}

function Invoke-CmdBat {
    [CmdletBinding()]
    param(
        [ValidateNotNullOrEmpty()]
        [string]$Bat,

        [ValidateNotNullOrEmpty()]
        [string]$Extra
    )
    $cmd = "`"$Bat`" && $Extra"
    Write-Output "CMD> $Extra"
    & cmd.exe /c $cmd
    if ($LASTEXITCODE -ne 0) {
        Write-Error "command failed exit=$LASTEXITCODE : $Extra" -ErrorId "CmdBatFailed" -ErrorAction Continue
        throw [System.Management.Automation.RuntimeException]::new("command failed exit=$LASTEXITCODE : $Extra")
    }
}

$vcvars = Find-VcVar
$kit = "C:\Program Files (x86)\Windows Kits\10"
$incKm = "$kit\Include\$KitVersion\km"
$incShared = "$kit\Include\$KitVersion\shared"
$incKmCrt = "$kit\Include\$KitVersion\km\crt"
$libKm = "$kit\Lib\$KitVersion\km\x64"

if (-not (Test-Path -LiteralPath $incKm -PathType Container)) {
    Write-Error "WDK KM Include missing: $incKm" -ErrorId "WdkKmIncludeNotFound" -ErrorAction Continue
    throw [System.IO.DirectoryNotFoundException]::new("WDK KM Include missing: $incKm")
}

if (-not (Test-Path -LiteralPath $incShared -PathType Container)) {
    Write-Error "Platform SDK Shared Include missing: $incShared" -ErrorId "SdkSharedIncludeNotFound" -ErrorAction Continue
    throw [System.IO.DirectoryNotFoundException]::new("Platform SDK Shared Include missing: $incShared")
}

if (-not (Test-Path -LiteralPath "$incKm\storport.h" -PathType Leaf)) {
    Write-Error "storport.h missing under $incKm" -ErrorId "StorportHeaderNotFound" -ErrorAction Continue
    throw [System.IO.FileNotFoundException]::new("storport.h missing under $incKm")
}

if (-not (Test-Path -LiteralPath "$libKm\storport.lib" -PathType Leaf)) {
    Write-Error "storport.lib missing under $libKm" -ErrorId "StorportLibNotFound" -ErrorAction Continue
    throw [System.IO.FileNotFoundException]::new("storport.lib missing under $libKm")
}

$cflags = @(
    "/nologo", "/c", "/kernel", "/GS-", "/W4", "/WX", "/wd4324", "/O2", "/Z7",
    "/D_WIN64", "/D_AMD64_", "/DAMD64", "/DDEPRECATE_DDK_FUNCTIONS=1",
    "/D_WIN32_WINNT=0x0A00", "/DWINVER=0x0A00", "/DNTDDI_VERSION=0xA000010",
    "/I`"$incShared`"", "/I`"$incKm`"", "/I`"$incKmCrt`""
) -join " "
if ($CodeAnalysis) {
    # Analyze project sources with /WX while excluding WDK-owned headers.
    # WDK 10.0.26100 otherwise emits analyzer findings from wdm.h itself.
    $cflags += " /analyze /analyze:external- /external:W0" +
        " /external:I`"$incShared`" /external:I`"$incKm`"" +
        " /external:I`"$incKmCrt`""
}

$ldflagsCommon = @(
    "/nologo", "/driver", "/entry:GsDriverEntry", "/subsystem:NATIVE",
    "/nodefaultlib", "/incremental:no", "/debug",
    "/libpath:`"$libKm`"",
    "ntoskrnl.lib", "hal.lib", "wmilib.lib", "BufferOverflowFastFailK.lib", "ntstrsafe.lib"
) -join " "

# --- ramshared.sys (StorPort) ---
$srcDir = Join-Path $RepoRoot "drivers\windows\ramshared"

if (-not (Test-Path -LiteralPath $srcDir -PathType Container)) {
    Write-Error "Driver source directory missing: $srcDir" -ErrorId "DriverSrcNotFound" -ErrorAction Continue
    throw [System.IO.DirectoryNotFoundException]::new("Driver source directory missing: $srcDir")
}

$outDir = Join-Path $srcDir "x64\Release"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$srcs = @("driver.c", "virtdisk.c", "queue.c", "control.c")
$objs = @()
foreach ($s in $srcs) {
    $obj = Join-Path $outDir ($s -replace '\.c$', '.obj')
    $objs += "`"$obj`""
    $src = Join-Path $srcDir $s

    if (-not (Test-Path -LiteralPath $src -PathType Leaf)) {
        Write-Error "Driver source file missing: $src" -ErrorId "DriverSrcFileNotFound" -ErrorAction Continue
        throw [System.IO.FileNotFoundException]::new("Driver source file missing: $src")
    }

    Invoke-CmdBat -Bat $vcvars -Extra "cl $cflags /Fo`"$obj`" `"$src`""
}
$sys = Join-Path $outDir "ramshared.sys"
$objList = $objs -join " "
Invoke-CmdBat -Bat $vcvars -Extra "link $ldflagsCommon /out:`"$sys`" $objList storport.lib wdmsec.lib"
Write-Output "BUILT $sys"
Get-Item -LiteralPath $sys | Format-List FullName, Length, LastWriteTime | Out-String | Write-Output

# --- poolstress.sys ---
$psDir = Join-Path $RepoRoot "drivers\windows\tools\poolstress"

if (-not (Test-Path -LiteralPath $psDir -PathType Container)) {
    Write-Error "Poolstress directory missing: $psDir" -ErrorId "PoolstressDirNotFound" -ErrorAction Continue
    throw [System.IO.DirectoryNotFoundException]::new("Poolstress directory missing: $psDir")
}

$psOut = Join-Path $psDir "x64\Release"
New-Item -ItemType Directory -Force -Path $psOut | Out-Null
$psObj = Join-Path $psOut "poolstress.obj"
$psSrc = Join-Path $psDir "poolstress.c"
$psSys = Join-Path $psOut "poolstress.sys"

if (-not (Test-Path -LiteralPath $psSrc -PathType Leaf)) {
    Write-Error "Poolstress source file missing: $psSrc" -ErrorId "PoolstressSrcFileNotFound" -ErrorAction Continue
    throw [System.IO.FileNotFoundException]::new("Poolstress source file missing: $psSrc")
}

Invoke-CmdBat -Bat $vcvars -Extra "cl $cflags /Fo`"$psObj`" `"$psSrc`""
Invoke-CmdBat -Bat $vcvars -Extra "link $ldflagsCommon /out:`"$psSys`" `"$psObj`" cng.lib"
Write-Output "BUILT $psSys"
Get-Item -LiteralPath $psSys | Format-List FullName, Length, LastWriteTime | Out-String | Write-Output

Write-Output "BUILD_DRIVERS_OK"
Stop-Transcript
