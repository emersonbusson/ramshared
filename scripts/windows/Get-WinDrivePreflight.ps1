#Requires -Version 5.1
<#
.SYNOPSIS
  Preflight checks for windows-storport-cuda-vram storage-only product path.

.DESCRIPTION
  Read-only queries. Does not install drivers, create pagefiles, or thrash the host.
  With -StorageOnly: requires no RamShared pagefile/disk, product binary/config hash
  fields, test-signing/driver package state, CUDA probe prereqs, latest dump identity.

.EXAMPLE
  .\Get-WinDrivePreflight.ps1 -StorageOnly
#>
[CmdletBinding()]
param(
    [switch]$StorageOnly,
    [string]$ProductExe = "C:\ramshared\bin\ramshared-winsvc.exe",
    [string]$ConfigPath = "C:\ProgramData\RamShared\winsvc.toml",
    [int]$TimeoutSec = 30
)

$ErrorActionPreference = 'Continue'
$fail = 0
$global:PreflightResults = @()
$start = Get-Date

function Record-Result([string]$CheckName, [string]$Status, [string]$Detail) {
    $global:PreflightResults += [PSCustomObject]@{
        CheckName = $CheckName
        Status = $Status
        Detail = $Detail
    }
}

function Ok([string]$CheckName, [string]$msg) {
    Record-Result $CheckName 'Pass' $msg
    Write-Host "[OK]  $msg" -ForegroundColor Green
}

function Warn([string]$CheckName, [string]$msg) {
    Record-Result $CheckName 'Skip' $msg
    Write-Host "[WARN] $msg" -ForegroundColor Yellow
}

function Bad([string]$CheckName, [string]$msg) {
    Record-Result $CheckName 'Fail' $msg
    Write-Host "[FAIL] $msg" -ForegroundColor Red
    $script:fail++
}
function Test-ControlPath([string]$Path) {
    if (-not ("RamSharedCtlOpen" -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class RamSharedCtlOpen {
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  static extern IntPtr CreateFile(string path, uint access, uint share, IntPtr sec, uint creation, uint flags, IntPtr template);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern bool CloseHandle(IntPtr h);

  public static int TryOpen(string path) {
    IntPtr h = CreateFile(path, 0x80000000u | 0x40000000u, 0, IntPtr.Zero, 3, 0, IntPtr.Zero);
    long v = h.ToInt64();
    if (v == -1 || v == 0) return Marshal.GetLastWin32Error();
    CloseHandle(h);
    return 0;
  }
}
'@
    }
    return [RamSharedCtlOpen]::TryOpen($Path)
}
function Test-ConfiguredPagingFilesConcrete {
    try {
        $mm = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management'
        $configured = @((Get-ItemProperty -LiteralPath $mm -Name PagingFiles -EA Stop).PagingFiles)
        $badConfigured = @()
        foreach ($entry in $configured) {
            $line = [string]$entry
            if ([string]::IsNullOrWhiteSpace($line)) {
                continue
            }
            $parts = @($line.Trim() -split '\s+')
            $path = [string]$parts[0]
            if ($path.StartsWith('?:\') -or $path -notmatch '^[A-Za-z]:\\' -or $parts.Count -lt 3) {
                $badConfigured += $line
            }
        }
        if ($badConfigured.Count -gt 0) {
            if ($StorageOnly) {
                Bad 'PagingFiles' "Ambiguous/malformed PagingFiles entry blocks storage-only teardown: $($badConfigured -join ', ')"
            } else {
                Warn 'PagingFiles' "Ambiguous/malformed PagingFiles entry: $($badConfigured -join ', ')"
            }
        } else {
            Ok 'PagingFiles' "Configured PagingFiles entries are concrete"
        }
    } catch {
        if ($StorageOnly) { Bad 'PagingFiles' "PagingFiles registry query failed (fail-closed): $_" }
        else { Warn 'PagingFiles' "PagingFiles registry query failed: $_" }
    }
}

Write-Host "=== RamShared WinDrive preflight ===" -ForegroundColor Cyan
if ($StorageOnly) {
    Write-Host "MODE=storage-only (no pagefile campaign)" -ForegroundColor Cyan
    Test-ConfiguredPagingFilesConcrete
}

# OS
try {
    $cv = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction Stop)
    $build = $cv.CurrentBuildNumber
    $ubr = $cv.UBR
    Write-Host "OS build: $build.$ubr (ProductName=$($cv.ProductName))"
    if (-not [Environment]::Is64BitOperatingSystem) { Bad 'OS' "x64 OS required" } else { Ok 'OS' "x64 OS" }
} catch {
    Bad 'OS' "Could not read OS version: $_"
}

# NVIDIA / nvcuda
$nvsmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
if ($nvsmi) {
    try {
        $gpu = & nvidia-smi --query-gpu=name,memory.total,memory.free,driver_version --format=csv,noheader 2>$null
        Ok 'NVIDIA_SMI' "nvidia-smi: $gpu"
    } catch {
        Warn 'NVIDIA_SMI' "nvidia-smi present but query failed: $_"
    }
} else {
    if ($StorageOnly) { Bad 'NVIDIA_SMI' "nvidia-smi required for storage-only CUDA product" }
    else { Warn 'NVIDIA_SMI' "nvidia-smi not in PATH" }
}

$dllCandidates = @(
    "$env:SystemRoot\System32\nvcuda.dll",
    "$env:SystemRoot\SysWOW64\nvcuda.dll"
)
$foundDll = $false
foreach ($p in $dllCandidates) {
    if (Test-Path $p) {
        Ok 'NVCUDA' "Found $p"
        $foundDll = $true
        break
    }
}
if (-not $foundDll) {
    if ($StorageOnly) { Bad 'NVCUDA' "nvcuda.dll missing (product probe-cuda will fail)" }
    else { Warn 'NVCUDA' "nvcuda.dll not found" }
}

# Admin
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if ($isAdmin) { Ok 'Admin' "Running elevated" }
else {
    if ($StorageOnly) { Warn 'Admin' "Not elevated - product install/SCM needs admin" }
    else { Warn 'Admin' "Not elevated" }
}

# Test-signing
try {
    $bcd = bcdedit /enum '{current}' 2>$null | Out-String
    if ($bcd -match 'testsigning\s+Yes') { Ok 'TestSigning' "testsigning Yes (lab driver load)" }
    else { Warn 'TestSigning' "testsigning not Yes (signed package or lab policy required)" }
} catch {
    Warn 'TestSigning' "bcdedit not queryable"
}

# Active pagefiles
try {
    $pf = @(Get-CimInstance Win32_PageFileUsage -EA Stop)
    $rs = @($pf | Where-Object { $_.Name -match 'RamShared|VRAM' })
    if ($rs.Count -gt 0) {
        if ($StorageOnly) {
            Bad 'ActivePagefile' "RamShared/VRAM pagefile active: $($rs.Name -join ', ') — PREFLIGHT_STORAGE_ONLY refuse"
        } else {
            Warn 'ActivePagefile' "pagefile on VRAM volume present"
        }
    } else {
        Ok 'ActivePagefile' "No RamShared pagefile in Win32_PageFileUsage"
    }
} catch {
    if ($StorageOnly) { Bad 'ActivePagefile' "pagefile WMI query failed (fail-closed): $_" }
    else { Warn 'ActivePagefile' "pagefile WMI query failed: $_" }
}

# Existing RamShared disks
try {
    $disks = @(Get-Disk -EA SilentlyContinue | Where-Object {
            $_.FriendlyName -match 'RAMSHARE|VRAMDISK|RamShared'
        })
    if ($disks.Count -gt 0) {
        if ($StorageOnly) {
            Bad 'ExistingRamSharedDisks' "Existing RamShared disk(s): $($disks.Number -join ',') — clear before campaign"
        } else {
            Ok 'ExistingRamSharedDisks' "RamShared disk present: N=$($disks.Number -join ',')"
        }
    } else {
        Ok 'ExistingRamSharedDisks' "No RamShared disk currently enumerated"
    }
} catch {
    Warn 'ExistingRamSharedDisks' "Get-Disk failed: $_"
}

# Redundant Win32 disk inventory catches residual class-stack devices that may
# still be visible to Task Manager even if the first Get-Disk pass races clean.
try {
    $win32Disks = @(Get-CimInstance Win32_DiskDrive -EA Stop | Where-Object {
            $_.Model -match 'RAMSHARE|VRAMDISK|RamShared'
        })
    if ($win32Disks.Count -gt 0) {
        $ids = @($win32Disks | ForEach-Object { "Index=$($_.Index) Model=$($_.Model) Serial=$($_.SerialNumber)" }) -join '; '
        if ($StorageOnly) {
            Bad 'ResidualWin32Disks' "Residual RamShared Win32_DiskDrive node(s): $ids"
        } else {
            Warn 'ResidualWin32Disks' "Residual RamShared Win32_DiskDrive node(s): $ids"
        }
    } else {
        Ok 'ResidualWin32Disks' "No residual RamShared Win32_DiskDrive nodes"
    }
} catch {
    if ($StorageOnly) { Bad 'ResidualWin32Disks' "Win32_DiskDrive query failed (fail-closed): $_" }
    else { Warn 'ResidualWin32Disks' "Win32_DiskDrive query failed: $_" }
}

# Ghost/stale PnP disk nodes can survive after surprise removal even when
# Get-Disk is clean. They poison identity checks, so storage-only preflight
# refuses until the operator removes them or reboots.
try {
    $ghostDisks = @(Get-PnpDevice -PresentOnly:$false -EA SilentlyContinue | Where-Object {
            $_.InstanceId -like 'SCSI\DISK&VEN_RAMSHARE&PROD_VRAMDISK*' -or
            $_.FriendlyName -match 'RAMSHARE|VRAMDISK|RamShared'
        })
    if ($ghostDisks.Count -gt 0) {
        $ids = @($ghostDisks | ForEach-Object { $_.InstanceId }) -join ', '
        if ($StorageOnly) {
            Bad 'GhostPnPDisks' "Stale RamShared PnP disk node(s) present: $ids"
        } else {
            Warn 'GhostPnPDisks' "Stale RamShared PnP disk node(s): $ids"
        }
    } else {
        Ok 'GhostPnPDisks' "No stale RamShared PnP disk nodes"
    }
} catch {
    if ($StorageOnly) { Bad 'GhostPnPDisks' "PnP ghost disk query failed (fail-closed): $_" }
    else { Warn 'GhostPnPDisks' "PnP ghost disk query failed: $_" }
}

# Product binary / config
if ($StorageOnly) {
    if (Test-Path -LiteralPath $ProductExe) {
        $h = (Get-FileHash -Algorithm SHA256 -LiteralPath $ProductExe).Hash
        Ok 'ProductBinary' "Product exe $ProductExe SHA256=$h"
        if ($ProductExe -match 'WinDriveBackend|RamSharedWinSvc\.cs|Start-RamSharedLab') {
            Bad 'ProductBinary' "Product path looks like lab backend (false RAM green risk)"
        }
    } else {
        Bad 'ProductBinary' "Product exe missing: $ProductExe"
    }
    if (Test-Path -LiteralPath $ConfigPath) {
        $ch = (Get-FileHash -Algorithm SHA256 -LiteralPath $ConfigPath).Hash
        Ok 'ProductConfig' "Config $ConfigPath SHA256=$ch"
        $raw = Get-Content -LiteralPath $ConfigPath -Raw
        if ($raw -match 'backend\s*=') {
            Bad 'ProductConfig' "Config contains backend= (product forbid)"
        } else {
            Ok 'ProductConfig' "Config has no backend selector"
        }
    } else {
        Warn 'ProductConfig' "Config missing: $ConfigPath (install will copy example)"
    }
}

# Driver package presence (optional)
$serviceImage = $null
try {
    $rawImage = [string](Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\ramshared" -Name ImagePath -EA Stop).ImagePath
    $serviceImage = $rawImage.Trim('"') -replace '^\\SystemRoot', $env:SystemRoot -replace '^\\\?\?\\', ''
} catch {}
$sys = @($serviceImage, "C:\ramshared\package\ramshared.sys") | Where-Object { $_ }
$drv = $false
foreach ($s in $sys) {
    if (Test-Path $s) {
        Ok 'DriverPackage' "Driver package candidate: $s"
        $drv = $true
    }
}
if (-not $drv) {
    Warn 'DriverPackage' "ramshared.sys not found in default paths (build/sign/deploy first)"
}
if ($StorageOnly -and
    $serviceImage -and
    (Test-Path -LiteralPath $serviceImage) -and
    (Test-Path "C:\ramshared\package\ramshared.sys")) {
    $serviceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $serviceImage).Hash
    $packageHash = (Get-FileHash -Algorithm SHA256 -LiteralPath "C:\ramshared\package\ramshared.sys").Hash
    if ($serviceHash -eq $packageHash) {
        Ok 'DriverPackageHash' "Driver image matches package SHA256=$serviceHash"
    } else {
        Bad 'DriverPackageHash' "Driver image/package mismatch: service=$serviceHash package=$packageHash"
    }
}

# Loaded miniport health. A running service without the control device means a
# previous PnP/remove path left the physical host in a stale loaded state; fail
# closed before a storage campaign tries to create a LUN.
try {
    $svcText = sc.exe query ramshared 2>$null | Out-String
    $svcRunning = $svcText -match 'RUNNING'
    $ctlPaths = @("\\.\RamSharedCtl", "\\.\GLOBALROOT\Device\RamSharedCtl")
    $ctlOk = $false
    foreach ($ctl in $ctlPaths) {
        try {
            $err = Test-ControlPath $ctl
            if ($err -eq 0) {
                $ctlOk = $true
                Ok 'ControlPath' "Control path $ctl"
                break
            } else {
                Warn 'ControlPath' "Control path $ctl open failed err=$err"
            }
        } catch {
            Warn 'ControlPath' "Control path $ctl query failed: $_"
        }
    }
    if ($svcRunning -and -not $ctlOk) {
        if ($StorageOnly) {
            Bad 'LoadedMiniportHealth' "ramshared service is RUNNING but RamSharedCtl is absent; reboot/unload/redeploy before physical Online"
        } else {
            Warn 'LoadedMiniportHealth' "ramshared service is RUNNING but RamSharedCtl is absent"
        }
    } elseif (-not $svcRunning) {
        Warn 'LoadedMiniportHealth' "ramshared service not running yet; campaign must start it before Online"
    }
} catch {
    Warn 'LoadedMiniportHealth' "ramshared service/control query failed: $_"
}

# Latest dump identity (no contents)
$dumpDir = "C:\Windows\Minidump"
if (Test-Path $dumpDir) {
    $latest = Get-ChildItem $dumpDir -Filter *.dmp -EA SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latest) {
        Ok 'Minidump' "Latest dump: $($latest.Name) @ $($latest.LastWriteTimeUtc.ToString('u')) size=$($latest.Length)"
    } else {
        Ok 'Minidump' "No minidumps present"
    }
} else {
    Ok 'Minidump' "Minidump directory absent"
}

$elapsed = ((Get-Date) - $start).TotalSeconds
if ($elapsed -gt $TimeoutSec) {
    Warn 'Timeout' "Preflight exceeded TimeoutSec=$TimeoutSec (elapsed=$([int]$elapsed)s)"
}
Write-Host ("PREFLIGHT_ELAPSED_SEC={0:n1}" -f $elapsed)

if ($StorageOnly) {
    if ($fail -eq 0) {
        Write-Host "PREFLIGHT_STORAGE_ONLY=PASS" -ForegroundColor Green
    } else {
        Write-Host "PREFLIGHT_STORAGE_ONLY=FAIL" -ForegroundColor Red
    }
}

Write-Output $global:PreflightResults
if ($fail -gt 0) {
    Write-Host "Preflight finished with $fail failure(s)." -ForegroundColor Red
    exit 1
}
Write-Host "Preflight finished with no hard failures." -ForegroundColor Green
exit 0
