#Requires -Version 5.1
<#
.SYNOPSIS
  ITEM-8 / DT-11 / DT-21 — kernel page residency drill (VM ONLY).

.DESCRIPTION
  SPEC windows-swap-driver. Loads poolstress (when built), confirms pagefile-VRAM
  % Usage > 0 with incompressible data, then kills the service and records B1 vs B2.
  Abort as INCONCLUSIVO if residency cannot be proven (DT-21).

.PARAMETER Runs
  Number of drill iterations with confirmed residency (default 3).

.NOTES
  NEVER run on the daily physical host (RNF-6). Snapshot the VM first.
#>
[CmdletBinding()]
param(
    [int]$Runs = 3,
    [string]$ArtifactDir = ".\artifacts\kernel-page-drill",
    [string]$PagefileInstance = "*V:*",  # counter instance for VRAM volume
    [switch]$SkipPoolstressLoad,
    [switch]$WhatIfHostCheck
)

$ErrorActionPreference = "Stop"

function Assert-DisposableVm {
    if ($WhatIfHostCheck) {
        Write-Warning "WhatIfHostCheck: skipping VM assertion"
        return
    }
    # Heuristic: Hyper-V / VMware / QEMU guest tools presence.
    $bios = Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction SilentlyContinue
    $model = if ($bios) { $bios.Model } else { "" }
    $vmHints = @("Virtual", "VMware", "KVM", "Hyper-V", "QEMU", "VirtualBox")
    $isVm = $false
    foreach ($h in $vmHints) {
        if ($model -like "*$h*") { $isVm = $true; break }
    }
    if (-not $isVm) {
        throw "REFUSE: host model '$model' does not look like a VM. RNF-6 — abort (use -WhatIfHostCheck only for dry-run docs)."
    }
}

function Get-PagefileVramUsage {
    param([string]$Instance)
    try {
        $c = Get-Counter "\Paging File($Instance)\% Usage" -ErrorAction Stop
        return [double]$c.CounterSamples[0].CookedValue
    } catch {
        # Fallback WMI
        $pf = Get-CimInstance Win32_PageFileUsage -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match 'V:|RamShared|VRAM' }
        if ($pf) {
            if ($pf.AllocatedBaseSize -gt 0) {
                return (100.0 * $pf.CurrentUsage / $pf.AllocatedBaseSize)
            }
        }
        return $null
    }
}

function Write-Artifact {
    param([string]$Name, [string]$Content)
    New-Item -ItemType Directory -Force -Path $ArtifactDir | Out-Null
    $path = Join-Path $ArtifactDir $Name
    Set-Content -Path $path -Value $Content -Encoding UTF8
    return $path
}

Write-Host "Invoke-KernelPageDrill.ps1 — ITEM-8 (DT-11/DT-21)"
Assert-DisposableVm
New-Item -ItemType Directory -Force -Path $ArtifactDir | Out-Null

$results = @()
$confirmed = 0

for ($i = 1; $i -le $Runs; $i++) {
    Write-Host "=== Run $i / $Runs ==="

    if (-not $SkipPoolstressLoad) {
        Write-Host "Load poolstress.sys (test-signing), ALLOC + touch (operator step if not automated)"
        # sc.exe create / start left to operator; document in artifact.
    }

    $usage = Get-PagefileVramUsage -Instance $PagefileInstance
    Write-Host "pagefile-VRAM % Usage = $usage"

    if ($null -eq $usage -or $usage -le 0) {
        $msg = "INCONCLUSIVO run=$i: pagefile-VRAM usage not > 0 (DT-21). Does not count as PASS or BSOD."
        Write-Warning $msg
        $results += [pscustomobject]@{ Run = $i; Outcome = "INCONCLUSIVO"; Usage = $usage }
        Write-Artifact -Name "run-$i-inconclusive.txt" -Content $msg | Out-Null
        continue
    }

    $confirmed++
    # B2 path: stop service cleanly so driver QTeardownOnCrash completes SRBs with error.
    Write-Host "B2: Stop-Service ramshared-winsvc (if installed)"
    try {
        Stop-Service -Name "ramshared-winsvc" -Force -ErrorAction Stop
        $outcome = "B2_SERVICE_STOPPED"
    } catch {
        $outcome = "B2_SERVICE_STOP_FAILED: $($_.Exception.Message)"
    }

    # Optional READBACK via poolstress to force page-in after kill — operator / next automation.
    $results += [pscustomobject]@{ Run = $i; Outcome = $outcome; Usage = $usage }
    Write-Artifact -Name "run-$i.json" -Content ($results[-1] | ConvertTo-Json) | Out-Null
}

$summary = @"
Kernel page drill summary
Runs requested: $Runs
Residency confirmed: $confirmed
Results: $($results | ConvertTo-Json -Compress)

Gate: need >= 3 confirmed runs before host-real (SPEC ITEM-8).
Update DEGRADATION-MATRIX with B1 vs B2 empirical result.
"@
Write-Host $summary
Write-Artifact -Name "summary.txt" -Content $summary | Out-Null

if ($confirmed -lt $Runs) {
    Write-Warning "Fewer confirmed runs than requested — do not promote."
    exit 3
}
exit 0
