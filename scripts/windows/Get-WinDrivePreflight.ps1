#Requires -Version 5.1
<#
.SYNOPSIS
  Preflight checks for windows-storport-cuda-vram storage-only product path.

.DESCRIPTION
  Read-only queries. Does not install drivers, create pagefiles, or thrash the host.
  With -StorageOnly: requires no RamShared pagefile/disk, product binary/config hash
  fields, test-signing/driver package state, CUDA probe prereqs, latest dump identity.
  Returns structured PSCustomObject array with CheckName, Status (Pass/Fail/Skip), and Detail.

.EXAMPLE
  .\Get-WinDrivePreflight.ps1 -StorageOnly
#>
[CmdletBinding()]
param(
    [switch]$StorageOnly,
    [ValidateNotNullOrEmpty()]
    [string]$ProductExe = "C:\ramshared\bin\ramshared-winsvc.exe",
    [ValidateNotNullOrEmpty()]
    [string]$ConfigPath = "C:\ProgramData\RamShared\winsvc.toml",
    [ValidateRange(1, 600)]
    [int]$TimeoutSec = 30
)

$ErrorActionPreference = 'Stop'

if ($TimeoutSec -gt 600) {
    Write-Error "TimeoutSec exceeds maximum limit" -ErrorId "TimeoutSecOutOfBounds" -ErrorAction Continue
    throw [System.ArgumentOutOfRangeException]::new("TimeoutSec", "TimeoutSec must be <= 600")
}

$script:fail = 0
$start = Get-Date

$script:PreflightResults = [System.Collections.Generic.List[PSCustomObject]]::new()

function Pass-Check([string]$CheckName, [string]$Detail) {
    $script:PreflightResults.Add([PSCustomObject]@{ CheckName = $CheckName; Status = 'Pass'; Detail = $Detail })
}
function Fail-Check([string]$CheckName, [string]$Detail) {
    $script:PreflightResults.Add([PSCustomObject]@{ CheckName = $CheckName; Status = 'Fail'; Detail = $Detail })
    $script:fail++
}
function Skip-Check([string]$CheckName, [string]$Detail) {
    $script:PreflightResults.Add([PSCustomObject]@{ CheckName = $CheckName; Status = 'Skip'; Detail = $Detail })
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
        $configured = @((Get-ItemProperty -LiteralPath $mm -Name PagingFiles -ErrorAction Stop).PagingFiles)
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
                Fail-Check "PagingFilesConcrete" "Ambiguous/malformed PagingFiles entry blocks storage-only teardown: $($badConfigured -join ', ')"
            } else {
                Pass-Check "PagingFilesConcrete" "Ambiguous/malformed PagingFiles entry: $($badConfigured -join ', ')"
            }
        } else {
            Pass-Check "PagingFilesConcrete" "Configured PagingFiles entries are concrete"
        }
    } catch {
        if ($StorageOnly) { Fail-Check "PagingFilesConcrete" "PagingFiles registry query failed (fail-closed): $_" }
        else { Pass-Check "PagingFilesConcrete" "PagingFiles registry query failed: $_" }
    }
}

if ($StorageOnly) {
    Test-ConfiguredPagingFilesConcrete
} else {
    Skip-Check "PagingFilesConcrete" "MODE != storage-only"
}

# OS
try {
    $cv = Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction Stop
    $build = $cv.CurrentBuildNumber
    $ubr = $cv.UBR
    $osDetail = "OS build: $build.$ubr (ProductName=$($cv.ProductName))"
    if (-not [Environment]::Is64BitOperatingSystem) { Fail-Check "OSArchitecture" "x64 OS required. $osDetail" } else { Pass-Check "OSArchitecture" "x64 OS. $osDetail" }
} catch {
    Fail-Check "OSArchitecture" "Could not read OS version: $_"
}

# NVIDIA / nvcuda
$nvsmi = Get-Command -Name nvidia-smi -ErrorAction SilentlyContinue
if ($nvsmi) {
    try {
        $gpu = & $nvsmi.Path --query-gpu=name,memory.total,memory.free,driver_version --format=csv,noheader 2>$null
        if ($LASTEXITCODE -ne 0) {
            Pass-Check "NvidiaSMI" "nvidia-smi present but query failed"
        } else {
            Pass-Check "NvidiaSMI" "nvidia-smi: $gpu"
        }
    } catch {
        Pass-Check "NvidiaSMI" "nvidia-smi present but query failed: $_"
    }
} else {
    if ($StorageOnly) { Fail-Check "NvidiaSMI" "nvidia-smi required for storage-only CUDA product" }
    else { Pass-Check "NvidiaSMI" "nvidia-smi not in PATH" }
}

$dllCandidates = @(
    "$env:SystemRoot\System32\nvcuda.dll",
    "$env:SystemRoot\SysWOW64\nvcuda.dll"
)
$foundDll = $false
foreach ($p in $dllCandidates) {
    if (Test-Path -LiteralPath $p) {
        Pass-Check "NvcudaDll" "Found $p"
        $foundDll = $true
        break
    }
}
if (-not $foundDll) {
    if ($StorageOnly) { Fail-Check "NvcudaDll" "nvcuda.dll missing (product probe-cuda will fail)" }
    else { Pass-Check "NvcudaDll" "nvcuda.dll not found" }
}

# Admin
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if ($isAdmin) { Pass-Check "AdminPrivilege" "Running elevated" }
else {
    if ($StorageOnly) { Pass-Check "AdminPrivilege" "Not elevated - product install/SCM needs admin" }
    else { Pass-Check "AdminPrivilege" "Not elevated" }
}

# Test-signing
try {
    $bcd = bcdedit.exe /enum '{current}' 2>$null | Out-String
    if ($bcd -match 'testsigning\s+Yes') { Pass-Check "TestSigning" "testsigning Yes (lab driver load)" }
    else { Pass-Check "TestSigning" "testsigning not Yes (signed package or lab policy required)" }
} catch {
    Pass-Check "TestSigning" "bcdedit not queryable"
}

# Active pagefiles
try {
    $pf = @(Get-CimInstance -ClassName Win32_PageFileUsage -ErrorAction Stop)
    $rs = @($pf | Where-Object { $_.Name -match 'RamShared|VRAM' })
    if ($rs.Count -gt 0) {
        if ($StorageOnly) {
            Fail-Check "ActivePagefiles" "RamShared/VRAM pagefile active: $($rs.Name -join ', ') - PREFLIGHT_STORAGE_ONLY refuse"
        } else {
            Pass-Check "ActivePagefiles" "pagefile on VRAM volume present"
        }
    } else {
        Pass-Check "ActivePagefiles" "No RamShared pagefile in Win32_PageFileUsage"
    }
} catch {
    if ($StorageOnly) { Fail-Check "ActivePagefiles" "pagefile WMI query failed (fail-closed): $_" }
    else { Pass-Check "ActivePagefiles" "pagefile WMI query failed: $_" }
}

# Existing RamShared disks
try {
    $disks = @(Get-Disk -ErrorAction SilentlyContinue | Where-Object {
            $_.FriendlyName -match 'RAMSHARE|VRAMDISK|RamShared'
        })
    if ($disks.Count -gt 0) {
        if ($StorageOnly) {
            Fail-Check "RamSharedDisks" "Existing RamShared disk(s): $($disks.Number -join ',') - clear before campaign"
        } else {
            Pass-Check "RamSharedDisks" "RamShared disk present: N=$($disks.Number -join ',')"
        }
    } else {
        Pass-Check "RamSharedDisks" "No RamShared disk currently enumerated"
    }
} catch {
    Pass-Check "RamSharedDisks" "Get-Disk failed: $_"
}

# Redundant Win32 disk inventory catches residual class-stack devices that may
# still be visible to Task Manager even if the first Get-Disk pass races clean.
try {
    $win32Disks = @(Get-CimInstance -ClassName Win32_DiskDrive -ErrorAction Stop | Where-Object {
            $_.Model -match 'RAMSHARE|VRAMDISK|RamShared'
        })
    if ($win32Disks.Count -gt 0) {
        $ids = @($win32Disks | ForEach-Object { "Index=$($_.Index) Model=$($_.Model) Serial=$($_.SerialNumber)" }) -join '; '
        if ($StorageOnly) {
            Fail-Check "ResidualWin32Disks" "Residual RamShared Win32_DiskDrive node(s): $ids"
        } else {
            Pass-Check "ResidualWin32Disks" "Residual RamShared Win32_DiskDrive node(s): $ids"
        }
    } else {
        Pass-Check "ResidualWin32Disks" "No residual RamShared Win32_DiskDrive nodes"
    }
} catch {
    if ($StorageOnly) { Fail-Check "ResidualWin32Disks" "Win32_DiskDrive query failed (fail-closed): $_" }
    else { Pass-Check "ResidualWin32Disks" "Win32_DiskDrive query failed: $_" }
}

# Ghost/stale PnP disk nodes can survive after surprise removal even when
# Get-Disk is clean. They poison identity checks, so storage-only preflight
# refuses until the operator removes them or reboots.
try {
    $ghostDisks = @(Get-PnpDevice -PresentOnly:$false -ErrorAction SilentlyContinue | Where-Object {
            $_.InstanceId -like 'SCSI\DISK&VEN_RAMSHARE&PROD_VRAMDISK*' -or
            $_.FriendlyName -match 'RAMSHARE|VRAMDISK|RamShared'
        })
    if ($ghostDisks.Count -gt 0) {
        $ids = @($ghostDisks | ForEach-Object { $_.InstanceId }) -join ', '
        if ($StorageOnly) {
            Fail-Check "GhostPnpDisks" "Stale RamShared PnP disk node(s) present: $ids"
        } else {
            Pass-Check "GhostPnpDisks" "Stale RamShared PnP disk node(s): $ids"
        }
    } else {
        Pass-Check "GhostPnpDisks" "No stale RamShared PnP disk nodes"
    }
} catch {
    if ($StorageOnly) { Fail-Check "GhostPnpDisks" "PnP ghost disk query failed (fail-closed): $_" }
    else { Pass-Check "GhostPnpDisks" "PnP ghost disk query failed: $_" }
}

# Product binary / config
if ($StorageOnly) {
    if (Test-Path -LiteralPath $ProductExe) {
        $h = (Get-FileHash -Algorithm SHA256 -LiteralPath $ProductExe).Hash
        if ($ProductExe -match 'WinDriveBackend|RamSharedWinSvc\.cs|Start-RamSharedLab') {
            Fail-Check "ProductBinary" "Product path looks like lab backend (false RAM green risk)"
        } else {
            Pass-Check "ProductBinary" "Product exe $ProductExe SHA256=$h"
        }
    } else {
        Fail-Check "ProductBinary" "Product exe missing: $ProductExe"
    }
    if (Test-Path -LiteralPath $ConfigPath) {
        $ch = (Get-FileHash -Algorithm SHA256 -LiteralPath $ConfigPath).Hash
        $raw = Get-Content -LiteralPath $ConfigPath -Raw
        if ($raw -match 'backend\s*=') {
            Fail-Check "ProductConfig" "Config contains backend= (product forbid)"
        } else {
            Pass-Check "ProductConfig" "Config $ConfigPath SHA256=$ch has no backend selector"
        }
    } else {
        Pass-Check "ProductConfig" "Config missing: $ConfigPath (install will copy example)"
    }
} else {
    Skip-Check "ProductBinary" "MODE != storage-only"
    Skip-Check "ProductConfig" "MODE != storage-only"
}

# Driver package presence (optional)
$serviceImage = $null
try {
    $rawImage = [string](Get-ItemProperty -LiteralPath "HKLM:\SYSTEM\CurrentControlSet\Services\ramshared" -Name ImagePath -ErrorAction Stop).ImagePath
    $serviceImage = $rawImage.Trim('"') -replace '^\\SystemRoot', $env:SystemRoot -replace '^\\\?\?\\', ''
} catch {}
$sys = @($serviceImage, "C:\ramshared\package\ramshared.sys") | Where-Object { $_ }
$drv = $false
foreach ($s in $sys) {
    if (Test-Path -LiteralPath $s) {
        Pass-Check "DriverPackage" "Driver package candidate: $s"
        $drv = $true
        break
    }
}
if (-not $drv) {
    Pass-Check "DriverPackage" "ramshared.sys not found in default paths (build/sign/deploy first)"
}
if ($StorageOnly -and
    $serviceImage -and
    (Test-Path -LiteralPath $serviceImage) -and
    (Test-Path -LiteralPath "C:\ramshared\package\ramshared.sys")) {
    $serviceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $serviceImage).Hash
    $packageHash = (Get-FileHash -Algorithm SHA256 -LiteralPath "C:\ramshared\package\ramshared.sys").Hash
    if ($serviceHash -eq $packageHash) {
        Pass-Check "DriverPackageHash" "Driver image matches package SHA256=$serviceHash"
    } else {
        Fail-Check "DriverPackageHash" "Driver image/package mismatch: service=$serviceHash package=$packageHash"
    }
} else {
    Skip-Check "DriverPackageHash" "Condition not met for checking driver hash"
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
                Pass-Check "MiniportHealth" "Control path $ctl"
                break
            }
        } catch {}
    }
    if (-not $ctlOk) {
        if ($svcRunning) {
            if ($StorageOnly) {
                Fail-Check "MiniportHealth" "ramshared service is RUNNING but RamSharedCtl is absent; reboot/unload/redeploy before physical Online"
            } else {
                Pass-Check "MiniportHealth" "ramshared service is RUNNING but RamSharedCtl is absent"
            }
        } else {
            Pass-Check "MiniportHealth" "ramshared service not running yet; campaign must start it before Online"
        }
    }
} catch {
    Pass-Check "MiniportHealth" "ramshared service/control query failed: $_"
}

# Latest dump identity (no contents)
$dumpDir = "C:\Windows\Minidump"
if (Test-Path -LiteralPath $dumpDir) {
    $latest = Get-ChildItem -LiteralPath $dumpDir -Filter *.dmp -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latest) {
        Pass-Check "LatestMinidump" "Latest dump: $($latest.Name) @ $($latest.LastWriteTimeUtc.ToString('u')) size=$($latest.Length)"
    } else {
        Pass-Check "LatestMinidump" "No minidumps present"
    }
} else {
    Pass-Check "LatestMinidump" "Minidump directory absent"
}

$elapsed = ((Get-Date) - $start).TotalSeconds
if ($elapsed -gt $TimeoutSec) {
    Fail-Check "Timeout" "Preflight exceeded TimeoutSec=$TimeoutSec (elapsed=$([int]$elapsed)s)"
} else {
    Pass-Check "Timeout" ("Preflight elapsed sec = {0:n1}" -f $elapsed)
}

# Output the array
$script:PreflightResults


if ($script:fail -gt 0) {
    exit 1
}
exit 0
