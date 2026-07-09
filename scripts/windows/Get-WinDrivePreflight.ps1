#Requires -Version 5.1
<#
.SYNOPSIS
  Preflight checks for windows-swap-driver IMPL (no driver load).

.DESCRIPTION
  SPEC: docs/specs/no-milestone/windows-swap-driver/
  Safe to run on host or VM. Does not install drivers or create pagefiles.

.NOTES
  Exit 0 = environment looks ready for ITEM-1/3 userspace work.
  Exit 1 = blockers listed on stderr/stdout.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Continue'
$fail = 0

function Ok([string]$msg) { Write-Host "[OK]  $msg" -ForegroundColor Green }
function Warn([string]$msg) { Write-Host "[WARN] $msg" -ForegroundColor Yellow }
function Bad([string]$msg) {
    Write-Host "[FAIL] $msg" -ForegroundColor Red
    $script:fail++
}

Write-Host "=== RamShared WinDrive preflight ===" -ForegroundColor Cyan

# OS
try {
    $v = [System.Environment]::OSVersion.Version
    $cv = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction Stop)
    $build = $cv.CurrentBuildNumber
    $ubr = $cv.UBR
    Write-Host "OS build: $build.$ubr (ProductName=$($cv.ProductName))"
    if ($build -like '26200*') {
        Ok "Build series 26200.* (SPEC DT-24 allow-list for NtCreatePagingFile MVP)"
    } else {
        Warn "Build $build not in DT-24 allow-list 26200.* - pagefile activation degrades gracefully (SPEC DT-24)"
    }
    if (-not [Environment]::Is64BitOperatingSystem) { Bad "x64 OS required" } else { Ok "x64 OS" }
} catch {
    Bad "Could not read OS version: $_"
}

# NVIDIA
$nvsmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
if ($nvsmi) {
    try {
        $gpu = & nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv,noheader 2>$null
        Ok "nvidia-smi: $gpu"
    } catch {
        Warn "nvidia-smi present but query failed: $_"
    }
} else {
    Warn "nvidia-smi not in PATH (required for ITEM-1 CUDA on this machine; VM may not need GPU until ITEM-6)"
}

# nvcuda.dll
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
    Warn "nvcuda.dll not found under System32 - ITEM-1 Windows CUDA load will fail here"
}

# Admin (informational)
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if ($isAdmin) { Ok "Running elevated (needed for service/driver later)" }
else { Warn "Not elevated - fine for preflight; ITEM-5+ needs admin in VM" }

# Hyper-V host capability (best-effort)
try {
    $hv = Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V -ErrorAction SilentlyContinue
    if ($hv -and $hv.State -eq 'Enabled') { Ok "Hyper-V feature Enabled (good for disposable VMs)" }
    else { Warn "Hyper-V not enabled or not queryable - required for RNF-6 VM drills" }
} catch {
    Warn "Could not query Hyper-V feature (normal inside guest VM)"
}

Write-Host ""
Write-Host "SPEC gates before host-real driver load:" -ForegroundColor Cyan
Write-Host "  - ITEM-8 kernel-page drill PASS with residency (DT-21) in VM"
Write-Host "  - DEGRADATION-MATRIX updated for B1/B2"
Write-Host "  - ITEM-11 attestation policy (R9) decided"
Write-Host "Docs: docs/specs/no-milestone/windows-swap-driver/PREFLIGHT.md"

if ($fail -gt 0) {
    Write-Host "Preflight finished with $fail failure(s)." -ForegroundColor Red
    exit 1
}
Write-Host "Preflight finished with no hard failures." -ForegroundColor Green
exit 0
