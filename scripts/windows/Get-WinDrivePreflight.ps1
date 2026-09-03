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
$start = Get-Date
$script:results = @()

function Ok([string]$Name, [string]$Msg) { $script:results += [PSCustomObject]@{ CheckName = $Name; Status = "Pass"; Detail = $Msg } }
function Warn([string]$Name, [string]$Msg) { $script:results += [PSCustomObject]@{ CheckName = $Name; Status = "Skip"; Detail = $Msg } }
function Bad([string]$Name, [string]$Msg) {
    $script:results += [PSCustomObject]@{ CheckName = $Name; Status = "Fail"; Detail = $Msg }
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
                Bad "PagingFilesConfig" "Ambiguous/malformed PagingFiles entry blocks storage-only teardown: $($badConfigured -join ', ')"
            } else {
                Warn "PagingFilesConfig" "Ambiguous/malformed PagingFiles entry: $($badConfigured -join ', ')"
            }
        } else {
            Ok "PagingFilesConfig" "Configured PagingFiles entries are concrete"
        }
    } catch {
        if ($StorageOnly) { Bad "PagingFilesConfig" "PagingFiles registry query failed (fail-closed): $_" }
        else { Warn "PagingFilesConfig" "PagingFiles registry query failed: $_" }
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
    if (-not [Environment]::Is64BitOperatingSystem) { Bad "OS" "x64 OS required" } else { Ok "OS" "x64 OS" }
} catch {
    Bad "OS" "Could not read OS version: $_"
}

# NVIDIA / nvcuda
$nvsmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
if ($nvsmi) {
    try {
        $gpu = & nvidia-smi --query-gpu=name,memory.total,memory.free,driver_version --format=csv,noheader 2>$null
        Ok "NVIDIA" "nvidia-smi: $gpu"
    } catch {
        Warn "NVIDIA" "nvidia-smi present but query failed: $_"
    }
} else {
    if ($StorageOnly) { Bad "NVIDIA" "nvidia-smi required for storage-only CUDA product" }
    else { Warn "NVIDIA" "nvidia-smi not in PATH" }
}

$dllCandidates = @(
    "$env:SystemRoot\System32\nvcuda.dll",
    "$env:SystemRoot\SysWOW64\nvcuda.dll"
)
$foundDll = $false
foreach ($p in $dllCandidates) {
    if (Test-Path $p) {
        Ok "NVIDIA" "Found $p"
        $foundDll = $true
        break
    }
}
if (-not $foundDll) {
    if ($StorageOnly) { Bad "NVIDIA" "nvcuda.dll missing (product probe-cuda will fail)" }
    else { Warn "NVIDIA" "nvcuda.dll not found" }
}

# Admin
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if ($isAdmin) { Ok "Admin" "Running elevated" }
else {
    if ($StorageOnly) { Warn "Admin" "Not elevated - product install/SCM needs admin" }
    else { Warn "Admin" "Not elevated" }
}

# Test-signing
try {
    $bcd = bcdedit /enum '{current}' 2>$null | Out-String
    if ($bcd -match 'testsigning\s+Yes') { Ok "TestSigning" "testsigning Yes (lab driver load)" }
    else { Warn "TestSigning" "testsigning not Yes (signed package or lab policy required)" }
} catch {
    Warn "TestSigning" "bcdedit not queryable"
}

# Active pagefiles
try {
    $pf = @(Get-CimInstance Win32_PageFileUsage -EA Stop)
    $rs = @($pf | Where-Object { $_.Name -match 'RamShared|VRAM' })
    if ($rs.Count -gt 0) {
        if ($StorageOnly) {
            Bad "ActivePagefiles" "RamShared/VRAM pagefile active: $($rs.Name -join ', ') — PREFLIGHT_STORAGE_ONLY refuse"
        } else {
            Warn "ActivePagefiles" "pagefile on VRAM volume present"
        }
    } else {
        Ok "ActivePagefiles" "No RamShared pagefile in Win32_PageFileUsage"
    }
} catch {
    if ($StorageOnly) { Bad "ActivePagefiles" "pagefile WMI query failed (fail-closed): $_" }
    else { Warn "ActivePagefiles" "pagefile WMI query failed: $_" }
}

# Existing RamShared disks
try {
    $disks = @(Get-Disk -EA SilentlyContinue | Where-Object {
            $_.FriendlyName -match 'RAMSHARE|VRAMDISK|RamShared'
        })
    if ($disks.Count -gt 0) {
        if ($StorageOnly) {
            Bad "ExistingDisks" "Existing RamShared disk(s): $($disks.Number -join ',') — clear before campaign"
        } else {
            Ok "ExistingDisks" "RamShared disk present: N=$($disks.Number -join ',')"
        }
    } else {
        Ok "ExistingDisks" "No RamShared disk currently enumerated"
    }
} catch {
    Warn "ExistingDisks" "Get-Disk failed: $_"
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
            Bad "Win32Disks" "Residual RamShared Win32_DiskDrive node(s): $ids"
        } else {
            Warn "Win32Disks" "Residual RamShared Win32_DiskDrive node(s): $ids"
        }
    } else {
        Ok "Win32Disks" "No residual RamShared Win32_DiskDrive nodes"
    }
} catch {
    if ($StorageOnly) { Bad "Win32Disks" "Win32_DiskDrive query failed (fail-closed): $_" }
    else { Warn "Win32Disks" "Win32_DiskDrive query failed: $_" }
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
            Bad "GhostDisks" "Stale RamShared PnP disk node(s) present: $ids"
        } else {
            Warn "GhostDisks" "Stale RamShared PnP disk node(s): $ids"
        }
    } else {
        Ok "GhostDisks" "No stale RamShared PnP disk nodes"
    }
} catch {
    if ($StorageOnly) { Bad "GhostDisks" "PnP ghost disk query failed (fail-closed): $_" }
    else { Warn "GhostDisks" "PnP ghost disk query failed: $_" }
}

# Product binary / config
if ($StorageOnly) {
    if (Test-Path -LiteralPath $ProductExe) {
        $h = (Get-FileHash -Algorithm SHA256 -LiteralPath $ProductExe).Hash
        Ok "ProductConfig" "Product exe $ProductExe SHA256=$h"
        if ($ProductExe -match 'WinDriveBackend|RamSharedWinSvc\.cs|Start-RamSharedLab') {
            Bad "ProductConfig" "Product path looks like lab backend (false RAM green risk)"
        }
    } else {
        Bad "ProductConfig" "Product exe missing: $ProductExe"
    }
    if (Test-Path -LiteralPath $ConfigPath) {
        $ch = (Get-FileHash -Algorithm SHA256 -LiteralPath $ConfigPath).Hash
        Ok "ProductConfig" "Config $ConfigPath SHA256=$ch"
        $raw = Get-Content -LiteralPath $ConfigPath -Raw
        if ($raw -match 'backend\s*=') {
            Bad "ProductConfig" "Config contains backend= (product forbid)"
        } else {
            Ok "ProductConfig" "Config has no backend selector"
        }
    } else {
        Warn "ProductConfig" "Config missing: $ConfigPath (install will copy example)"
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
        Ok "DriverPackage" "Driver package candidate: $s"
        $drv = $true
    }
}
if (-not $drv) {
    Warn "DriverPackage" "ramshared.sys not found in default paths (build/sign/deploy first)"
}
if ($StorageOnly -and
    $serviceImage -and
    (Test-Path -LiteralPath $serviceImage) -and
    (Test-Path "C:\ramshared\package\ramshared.sys")) {
    $serviceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $serviceImage).Hash
    $packageHash = (Get-FileHash -Algorithm SHA256 -LiteralPath "C:\ramshared\package\ramshared.sys").Hash
    if ($serviceHash -eq $packageHash) {
        Ok "DriverPackage" "Driver image matches package SHA256=$serviceHash"
    } else {
        Bad "DriverPackage" "Driver image/package mismatch: service=$serviceHash package=$packageHash"
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
                Ok "MiniportHealth" "Control path $ctl"
                break
            } else {
                Warn "MiniportHealth" "Control path $ctl open failed err=$err"
            }
        } catch {
            Warn "MiniportHealth" "Control path $ctl query failed: $_"
        }
    }
    if ($svcRunning -and -not $ctlOk) {
        if ($StorageOnly) {
            Bad "MiniportHealth" "ramshared service is RUNNING but RamSharedCtl is absent; reboot/unload/redeploy before physical Online"
        } else {
            Warn "MiniportHealth" "ramshared service is RUNNING but RamSharedCtl is absent"
        }
    } elseif (-not $svcRunning) {
        Warn "MiniportHealth" "ramshared service not running yet; campaign must start it before Online"
    }
} catch {
    Warn "MiniportHealth" "ramshared service/control query failed: $_"
}

# Latest dump identity (no contents)
$dumpDir = "C:\Windows\Minidump"
if (Test-Path $dumpDir) {
    $latest = Get-ChildItem $dumpDir -Filter *.dmp -EA SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latest) {
        Ok "DumpIdentity" "Latest dump: $($latest.Name) @ $($latest.LastWriteTimeUtc.ToString('u')) size=$($latest.Length)"
    } else {
        Ok "DumpIdentity" "No minidumps present"
    }
} else {
    Ok "DumpIdentity" "Minidump directory absent"
}

$elapsed = ((Get-Date) - $start).TotalSeconds
if ($elapsed -gt $TimeoutSec) {
    Warn "Timeout" "Preflight exceeded TimeoutSec=$TimeoutSec (elapsed=$([int]$elapsed)s)"
}
Write-Host ("PREFLIGHT_ELAPSED_SEC={0:n1}" -f $elapsed)

if ($StorageOnly) {
    if ($fail -eq 0) {
        Write-Host "PREFLIGHT_STORAGE_ONLY=PASS" -ForegroundColor Green
    } else {
        Write-Host "PREFLIGHT_STORAGE_ONLY=FAIL" -ForegroundColor Red
    }
}

$script:results

if ($fail -gt 0) {
    Write-Error -ErrorId "PreflightFailures" -Message "Preflight finished with $fail failure(s)." -ErrorAction Stop
}
