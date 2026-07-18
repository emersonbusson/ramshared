#Requires -Version 5.1
<#
.SYNOPSIS
  App-agnostic VRAM reclaim pressure matrix harness.

.DESCRIPTION
  Encodes the GiB-scale cases that prove reclaim behavior. Default mode is a
  plan/preflight only; live pressure needs -Run and the matching approval flags.
  No example application name is part of the contract.
#>
[CmdletBinding()]
param(
    [switch]$Run,
    [switch]$ApprovePhysicalHost,
    [switch]$ApproveSharedDesktopWsl,
    [ValidateSet("all", "windows-smoke", "windows-3gib", "wsl2-1gib", "wsl2-4gib", "split-4gib-1gib")]
    [string]$Case = "all",
    [int]$GpuIndex = 0,
    [int]$ReserveMiB = 1024,
    [string]$OutDir = "C:\ramshared\artifacts\vram-reclaim-matrix-$(Get-Date -Format yyyyMMdd-HHmmss)"
)

$ErrorActionPreference = "Stop"

function L([string]$Message) {
    Write-Host "[vram-reclaim-matrix] $Message"
}

function Read-Gpu {
    $line = & nvidia-smi --id=$GpuIndex --query-gpu=name,memory.total,memory.free,memory.used --format=csv,noheader,nounits 2>$null |
        Select-Object -First 1
    if (-not $line) { throw "nvidia-smi did not return GPU memory data" }
    $p = @($line -split ',' | ForEach-Object { $_.Trim() })
    return [pscustomobject]@{
        name = $p[0]
        total_mib = [int]$p[1]
        free_mib = [int]$p[2]
        used_mib = [int]$p[3]
    }
}

function New-Case([string]$Name, [int]$WindowsMiB, [int]$WslMiB, [int]$ExternalMiB, [string]$Expected) {
    return [pscustomobject]@{
        case = $Name
        windows_lun_mib = $WindowsMiB
        wsl2_vram_mib = $WslMiB
        external_gpu_workload_mib = $ExternalMiB
        expected = $Expected
    }
}

function Add-Result {
    param(
        [System.Collections.ArrayList]$Results,
        [object]$Case,
        [string]$Status,
        [string]$Reason,
        [string]$Artifact = ""
    )
    [void]$Results.Add([pscustomobject]@{
        case = $Case.case
        status = $Status
        reason = $Reason
        windows_lun_mib = [int]$Case.windows_lun_mib
        wsl2_vram_mib = [int]$Case.wsl2_vram_mib
        external_gpu_workload_mib = [int]$Case.external_gpu_workload_mib
        artifact = $Artifact
    })
}

New-Item -Force -ItemType Directory -Path $OutDir | Out-Null
$gpu = Read-Gpu
$cases = @(
    New-Case "windows-smoke" 64 0 0 "Online + checksum + graceful teardown; not reclaim proof"
    New-Case "windows-3gib" 3072 0 1024 "Large LUN survives I/O; external pressure recovers; no dump"
    New-Case "wsl2-1gib" 0 1024 1024 "WSL2 tier demotes/refuses before reserve exhaustion"
    New-Case "wsl2-4gib" 0 4096 1024 "WSL2 cascade returns VRAM via swapoff-first DEMOTE"
    New-Case "split-4gib-1gib" 1024 4096 1024 "One owner releases/refuses growth; external workload gets headroom"
)

if ($Case -ne "all") {
    $cases = @($cases | Where-Object { $_.case -eq $Case })
}

$context = [ordered]@{
    tool = "Invoke-VramReclaimPressureMatrix.ps1"
    run = [bool]$Run
    approve_physical_host = [bool]$ApprovePhysicalHost
    approve_shared_desktop_wsl = [bool]$ApproveSharedDesktopWsl
    gpu = $gpu
    reserve_mib = $ReserveMiB
    cases = @($cases)
    note = "App-agnostic aggregate VRAM pressure; no process attribution is claimed."
}
$context | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $OutDir "matrix-plan.json")
L ("GPU {0}: total={1}MiB free={2}MiB used={3}MiB reserve={4}MiB" -f
    $gpu.name, $gpu.total_mib, $gpu.free_mib, $gpu.used_mib, $ReserveMiB)

foreach ($c in $cases) {
    $needed = [int]$c.windows_lun_mib + [int]$c.wsl2_vram_mib + [int]$c.external_gpu_workload_mib + $ReserveMiB
    if ($needed -gt $gpu.free_mib -and ($c.windows_lun_mib -gt 0 -or $c.wsl2_vram_mib -gt 0)) {
        L ("REFUSE {0}: free VRAM {1}MiB < windows+wsl2+external+reserve {2}MiB" -f
            $c.case, $gpu.free_mib, $needed)
    } else {
        L ("PLAN {0}: windows={1}MiB wsl2={2}MiB external={3}MiB expected={4}" -f
            $c.case, $c.windows_lun_mib, $c.wsl2_vram_mib, $c.external_gpu_workload_mib, $c.expected)
    }
}

if (-not $Run) {
    L "PLAN_ONLY=1"
    exit 0
}

if (-not $ApprovePhysicalHost) {
    throw "Refusing live matrix: -ApprovePhysicalHost is required"
}

$results = [System.Collections.ArrayList]::new()
foreach ($c in $cases) {
    if ($c.case -eq "windows-smoke") {
        L "RUN windows-smoke via Run-HostExhaustive.ps1"
        $caseOut = & (Join-Path $PSScriptRoot "..\windows\Run-HostExhaustive.ps1") 2>&1 |
            ForEach-Object { $_.ToString() }
        $caseOut | Set-Content -Encoding utf8 (Join-Path $OutDir "windows-smoke.out")
        if ($LASTEXITCODE -eq 0) {
            Add-Result -Results $results -Case $c -Status "PASS" -Reason "host_exhaustive_pass"
        } else {
            Add-Result -Results $results -Case $c -Status "FAIL" -Reason "host_exhaustive_exit_$LASTEXITCODE"
        }
    } elseif ($c.case -eq "windows-3gib") {
        $needed = [int]$c.windows_lun_mib + [int]$c.external_gpu_workload_mib + $ReserveMiB
        $gpuNow = Read-Gpu
        if (($needed + 256) -gt $gpuNow.free_mib) {
            $reason = ("insufficient_vram_headroom_free_{0}_planned_{1}_plus_256" -f
                $gpuNow.free_mib, $needed)
            L ("REFUSE windows-3gib: free VRAM {0}MiB < planned {1}MiB + 256MiB margin" -f
                $gpuNow.free_mib, $needed)
            Add-Result -Results $results -Case $c -Status "PARTIAL" -Reason $reason
            continue
        }
        L "RUN windows-3gib via Run-HostExhaustive.ps1 with external workload"
        $caseOut = & (Join-Path $PSScriptRoot "..\windows\Run-HostExhaustive.ps1") `
            -SizeBytes ([uint64]$c.windows_lun_mib * 1MB) `
            -ExternalWorkloadMiB ([int]$c.external_gpu_workload_mib) `
            -ExternalWorkloadHoldSec 20 `
            -MinFreeAfterPlanMiB ($ReserveMiB + 256) 2>&1 |
            ForEach-Object { $_.ToString() }
        $caseOut | Set-Content -Encoding utf8 (Join-Path $OutDir "windows-3gib.out")
        if ($LASTEXITCODE -eq 0) {
            Add-Result -Results $results -Case $c -Status "PASS" -Reason "host_exhaustive_pass"
        } else {
            Add-Result -Results $results -Case $c -Status "FAIL" -Reason "host_exhaustive_exit_$LASTEXITCODE"
        }
    } elseif ($c.case -like "wsl2-*") {
        if (-not $ApproveSharedDesktopWsl) {
            L "REFUSE $($c.case): WSL2 pressure requires isolated lab or -ApproveSharedDesktopWsl"
            Add-Result -Results $results -Case $c -Status "PARTIAL" -Reason "requires_isolated_lab_or_shared_desktop_wsl_approval"
            continue
        }
        L "PARTIAL $($c.case): use scripts/safety/wsl2-freeze-campaign.sh in an isolated lab"
        Add-Result -Results $results -Case $c -Status "PARTIAL" -Reason "wsl2_orchestration_delegated_to_freeze_campaign"
    } else {
        L "PARTIAL $($c.case): split-owner live orchestration is not enabled"
        Add-Result -Results $results -Case $c -Status "PARTIAL" -Reason "split_owner_orchestration_not_live_enabled"
    }
}

$summary = [ordered]@{
    tool = "Invoke-VramReclaimPressureMatrix.ps1"
    artifact = $OutDir
    gpu = $gpu
    reserve_mib = $ReserveMiB
    results = @($results)
    pass_count = @($results | Where-Object { $_.status -eq "PASS" }).Count
    partial_count = @($results | Where-Object { $_.status -eq "PARTIAL" }).Count
    fail_count = @($results | Where-Object { $_.status -eq "FAIL" }).Count
}
$summary | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $OutDir "matrix-summary.json")
L ("SUMMARY " + ($summary | ConvertTo-Json -Compress -Depth 6))
if ($summary.fail_count -gt 0) { exit 1 }
if ($summary.partial_count -gt 0) { exit 2 }
exit 0
