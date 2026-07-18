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

function Ok([string]$msg) { Write-Host "[OK]  $msg" -ForegroundColor Green }
function Warn([string]$msg) { Write-Host "[WARN] $msg" -ForegroundColor Yellow }
function Bad([string]$msg) {
    Write-Host "[FAIL] $msg" -ForegroundColor Red
    $script:fail++
}

Write-Host "=== RamShared WinDrive preflight ===" -ForegroundColor Cyan
if ($StorageOnly) {
    Write-Host "MODE=storage-only (no pagefile campaign)" -ForegroundColor Cyan
}

# OS
try {
    $cv = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction Stop)
    $build = $cv.CurrentBuildNumber
    $ubr = $cv.UBR
    Write-Host "OS build: $build.$ubr (ProductName=$($cv.ProductName))"
    if (-not [Environment]::Is64BitOperatingSystem) { Bad "x64 OS required" } else { Ok "x64 OS" }
} catch {
    Bad "Could not read OS version: $_"
}

# NVIDIA / nvcuda
$nvsmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
if ($nvsmi) {
    try {
        $gpu = & nvidia-smi --query-gpu=name,memory.total,memory.free,driver_version --format=csv,noheader 2>$null
        Ok "nvidia-smi: $gpu"
    } catch {
        Warn "nvidia-smi present but query failed: $_"
    }
} else {
    if ($StorageOnly) { Bad "nvidia-smi required for storage-only CUDA product" }
    else { Warn "nvidia-smi not in PATH" }
}

$dllCandidates = @(
    "$env:SystemRoot\System32\nvcuda.dll",
    "$env:SystemRoot\SysWOW64\nvcuda.dll"
)
$foundDll = $false
foreach ($p in $dllCandidates) {
    if (Test-Path $p) {
        Ok "Found $p"
        $foundDll = $true
        break
    }
}
if (-not $foundDll) {
    if ($StorageOnly) { Bad "nvcuda.dll missing (product probe-cuda will fail)" }
    else { Warn "nvcuda.dll not found" }
}

# Admin
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if ($isAdmin) { Ok "Running elevated" }
else {
    if ($StorageOnly) { Warn "Not elevated - product install/SCM needs admin" }
    else { Warn "Not elevated" }
}

# Test-signing
try {
    $bcd = bcdedit /enum '{current}' 2>$null | Out-String
    if ($bcd -match 'testsigning\s+Yes') { Ok "testsigning Yes (lab driver load)" }
    else { Warn "testsigning not Yes (signed package or lab policy required)" }
} catch {
    Warn "bcdedit not queryable"
}

# Active pagefiles
try {
    $pf = @(Get-CimInstance Win32_PageFileUsage -EA Stop)
    $rs = @($pf | Where-Object { $_.Name -match 'RamShared|VRAM' })
    if ($rs.Count -gt 0) {
        if ($StorageOnly) {
            Bad "RamShared/VRAM pagefile active: $($rs.Name -join ', ') — PREFLIGHT_STORAGE_ONLY refuse"
        } else {
            Warn "pagefile on VRAM volume present"
        }
    } else {
        Ok "No RamShared pagefile in Win32_PageFileUsage"
    }
} catch {
    if ($StorageOnly) { Bad "pagefile WMI query failed (fail-closed): $_" }
    else { Warn "pagefile WMI query failed: $_" }
}

# Existing RamShared disks
try {
    $disks = @(Get-Disk -EA SilentlyContinue | Where-Object {
            $_.FriendlyName -match 'RAMSHARE|VRAMDISK|RamShared'
        })
    if ($disks.Count -gt 0) {
        if ($StorageOnly) {
            Warn "Existing RamShared disk(s): $($disks.Number -join ',') — clear before campaign"
        } else {
            Ok "RamShared disk present: N=$($disks.Number -join ',')"
        }
    } else {
        Ok "No RamShared disk currently enumerated"
    }
} catch {
    Warn "Get-Disk failed: $_"
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
            Bad "Stale RamShared PnP disk node(s) present: $ids"
        } else {
            Warn "Stale RamShared PnP disk node(s): $ids"
        }
    } else {
        Ok "No stale RamShared PnP disk nodes"
    }
} catch {
    if ($StorageOnly) { Bad "PnP ghost disk query failed (fail-closed): $_" }
    else { Warn "PnP ghost disk query failed: $_" }
}

# Product binary / config
if ($StorageOnly) {
    if (Test-Path -LiteralPath $ProductExe) {
        $h = (Get-FileHash -Algorithm SHA256 -LiteralPath $ProductExe).Hash
        Ok "Product exe $ProductExe SHA256=$h"
        if ($ProductExe -match 'WinDriveBackend|RamSharedWinSvc\.cs|Start-RamSharedLab') {
            Bad "Product path looks like lab backend (false RAM green risk)"
        }
    } else {
        Bad "Product exe missing: $ProductExe"
    }
    if (Test-Path -LiteralPath $ConfigPath) {
        $ch = (Get-FileHash -Algorithm SHA256 -LiteralPath $ConfigPath).Hash
        Ok "Config $ConfigPath SHA256=$ch"
        $raw = Get-Content -LiteralPath $ConfigPath -Raw
        if ($raw -match 'backend\s*=') {
            Bad "Config contains backend= (product forbid)"
        } else {
            Ok "Config has no backend selector"
        }
    } else {
        Warn "Config missing: $ConfigPath (install will copy example)"
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
        Ok "Driver package candidate: $s"
        $drv = $true
    }
}
if (-not $drv) {
    Warn "ramshared.sys not found in default paths (build/sign/deploy first)"
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
            if (Test-Path $ctl) {
                $ctlOk = $true
                Ok "Control path $ctl"
                break
            }
        } catch {}
    }
    if ($svcRunning -and -not $ctlOk) {
        if ($StorageOnly) {
            Bad "ramshared service is RUNNING but RamSharedCtl is absent; reboot/unload/redeploy before physical Online"
        } else {
            Warn "ramshared service is RUNNING but RamSharedCtl is absent"
        }
    } elseif (-not $svcRunning) {
        Warn "ramshared service not running yet; campaign must start it before Online"
    }
} catch {
    Warn "ramshared service/control query failed: $_"
}

# Latest dump identity (no contents)
$dumpDir = "C:\Windows\Minidump"
if (Test-Path $dumpDir) {
    $latest = Get-ChildItem $dumpDir -Filter *.dmp -EA SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latest) {
        Ok "Latest dump: $($latest.Name) @ $($latest.LastWriteTimeUtc.ToString('u')) size=$($latest.Length)"
    } else {
        Ok "No minidumps present"
    }
} else {
    Ok "Minidump directory absent"
}

$elapsed = ((Get-Date) - $start).TotalSeconds
if ($elapsed -gt $TimeoutSec) {
    Warn "Preflight exceeded TimeoutSec=$TimeoutSec (elapsed=$([int]$elapsed)s)"
}
Write-Host ("PREFLIGHT_ELAPSED_SEC={0:n1}" -f $elapsed)

if ($StorageOnly) {
    if ($fail -eq 0) {
        Write-Host "PREFLIGHT_STORAGE_ONLY=PASS" -ForegroundColor Green
    } else {
        Write-Host "PREFLIGHT_STORAGE_ONLY=FAIL" -ForegroundColor Red
    }
}

if ($fail -gt 0) {
    Write-Host "Preflight finished with $fail failure(s)." -ForegroundColor Red
    exit 1
}
Write-Host "Preflight finished with no hard failures." -ForegroundColor Green
exit 0
