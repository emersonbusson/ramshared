#Requires -Version 5.1
<#
.SYNOPSIS
  DT-9 ordered kill lab (VM ONLY): pagefile-off BEFORE backend kill.

.DESCRIPTION
  Proves the product order that avoids BugCheck 0x7A (KERNEL_DATA_INPAGE_ERROR /
  c0000185) when the pagefile sits on the RamShared volume.

  Order (SPEC DT-9):
    1) Snapshot Usage / dumps
    2) Remove secondary pagefile setting (CIM) + best-effort pending delete
    3) Re-check Win32_PageFileUsage for the volume
       - if still alloc>0 / present → DT9_REBOOT_REQUIRED, **do not kill** (fail-closed PASS)
       - if gone → kill backend, wait, assert no new minidump (PASS)
    4) Contrast is documented separately: kill-with-usage → 0x7A

.NOTES
  RNF-6: Hyper-V guest only.
#>
[CmdletBinding()]
param(
    [string]$Drive = "D",
    [int]$PostKillWaitSec = 20,
    [string]$ArtifactDir = "C:\ramshared\artifacts\dt9",
    [string]$HelperDll = "C:\ramshared\bin\NtPagefileHelper.dll"
)

$ErrorActionPreference = "Continue"
$letter = $Drive.TrimEnd(':')
New-Item -ItemType Directory -Force -Path $ArtifactDir | Out-Null
$log = @()
function L([string]$s) { $script:log += $s; Write-Host $s }

$model = (Get-CimInstance Win32_ComputerSystem).Model
if ($model -notmatch "Virtual|VMware|KVM|Hyper-V|QEMU|VirtualBox") {
    throw "REFUSE: not a VM (RNF-6). model=$model"
}

L "=== DT-9 ORDERED KILL $(Get-Date -Format o) ==="
L "MODEL=$model DRIVE=${letter}:"

function Get-DumpName {
    $d = Get-ChildItem C:\Windows\Minidump -EA SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($d) { return "$($d.Name)@$($d.LastWriteTime.ToString('s'))" }
    return "none"
}

function Get-PfLine {
    (Get-CimInstance Win32_PageFileUsage -EA SilentlyContinue |
        ForEach-Object { "$($_.Name) a=$($_.AllocatedBaseSize) u=$($_.CurrentUsage)" }) -join "; "
}

$dumpBefore = Get-DumpName
L "DUMP_BEFORE=$dumpBefore"
L "PF_BEFORE=$(Get-PfLine)"
L "BACKEND_BEFORE=$([bool](Get-Process WinDriveBackend -EA SilentlyContinue))"

$pf = Get-CimInstance Win32_PageFileUsage -EA SilentlyContinue |
    Where-Object { $_.Name -match "^${letter}:" }

if (-not $pf -or $pf.AllocatedBaseSize -le 0) {
    L "INCONCLUSIVO: no active pagefile on ${letter}: - cannot exercise DT-9 off-path"
    $log | Set-Content (Join-Path $ArtifactDir "dt9-inconclusive.txt")
    exit 3
}

L "PF_TARGET a=$($pf.AllocatedBaseSize) u=$($pf.CurrentUsage)"

# --- DT-9 step: pagefile OFF (best effort) ---
L "DT9_STEP=PagefileOff"
$cs = Get-CimInstance Win32_ComputerSystem
if ($cs.AutomaticManagedPagefile) {
    Set-CimInstance -InputObject $cs -Property @{ AutomaticManagedPagefile = $false }
    L "AUTO_PF=false"
}

$setting = Get-CimInstance Win32_PageFileSetting -EA SilentlyContinue |
    Where-Object { $_.Name -match "^${letter}:" }
if ($setting) {
    try {
        Remove-CimInstance -InputObject $setting -EA Stop
        L "CIM_REMOVE_SETTING=OK"
    } catch {
        L "CIM_REMOVE_SETTING_FAIL=$($_.Exception.Message)"
    }
} else {
    L "CIM_SETTING=absent"
}

# Registry PagingFiles: drop D: lines
try {
    $key = "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management"
    $cur = @((Get-ItemProperty $key -EA Stop).PagingFiles)
    $new = @($cur | Where-Object { $_ -notmatch "^${letter}:" })
    if (-not $new -or $new.Count -eq 0) {
        $new = @("C:\pagefile.sys 0 0")
    }
    Set-ItemProperty -Path $key -Name PagingFiles -Value $new
    L "REG_PAGINGFILES=$(( $new -join ' | '))"
} catch {
    L "REG_FAIL=$($_.Exception.Message)"
}

if (Test-Path $HelperDll) {
    try {
        Add-Type -Path $HelperDll -EA Stop
        $path = "${letter}:\pagefile.sys"
        L "HELPER=$([NtPagefile]::RemoveBestEffort($path))"
    } catch {
        L "HELPER_FAIL=$($_.Exception.Message)"
    }
}

Start-Sleep 3
$pfAfter = Get-CimInstance Win32_PageFileUsage -EA SilentlyContinue |
    Where-Object { $_.Name -match "^${letter}:" }
L "PF_AFTER_OFF=$(Get-PfLine)"

$stillHot = $false
if ($pfAfter -and $pfAfter.AllocatedBaseSize -gt 0) {
    $stillHot = $true
}

if ($stillHot) {
    L "DT9_REBOOT_REQUIRED=True (pagefile still allocated - refuse kill, fail-closed)"
    L "VERDICT=PASS_DT9_REFUSE_KILL"
    L "NOTE=Product must not destroy backend while secondary pagefile Usage/alloc active"
    $log | Set-Content (Join-Path $ArtifactDir "dt9-refuse.log")
    @{
        verdict = "PASS_DT9_REFUSE_KILL"
        still_hot = $true
        pf_after = (Get-PfLine)
        dump_before = $dumpBefore
        ts = (Get-Date -Format o)
    } | ConvertTo-Json | Set-Content (Join-Path $ArtifactDir "dt9.json")
    exit 0
}

# --- Safe to kill: pagefile off ---
L "DT9_PAGEFILE_CLEAR=True"
if (-not (Get-Process WinDriveBackend -EA SilentlyContinue)) {
    L "INCONCLUSIVO: backend already dead"
    $log | Set-Content (Join-Path $ArtifactDir "dt9-inconclusive.txt")
    exit 3
}

L "DT9_STEP=KillBackend"
Stop-Process -Name WinDriveBackend -Force -EA SilentlyContinue
Start-Sleep 2
L "BACKEND_AFTER=$([bool](Get-Process WinDriveBackend -EA SilentlyContinue))"

# bounded I/O probe
$job = Start-Job {
    param($p)
    try { Get-Content $p -Raw -EA Stop | Out-Null; "READ_OK" }
    catch { "READ_FAIL" }
} -ArgumentList "${letter}:\dt9-probe.txt"
$done = Wait-Job $job -Timeout 10
if (-not $done) {
    Stop-Job $job -EA SilentlyContinue
    Remove-Job $job -Force -EA SilentlyContinue
    L "IO=READ_TIMEOUT"
} else {
    L "IO=$(Receive-Job $job)"
    Remove-Job $job -Force -EA SilentlyContinue
}

L "WAIT ${PostKillWaitSec}s"
Start-Sleep $PostKillWaitSec
$dumpAfter = Get-DumpName
L "DUMP_AFTER=$dumpAfter"
$newDump = ($dumpAfter -ne $dumpBefore) -and ($dumpAfter -ne "none")
L "NEW_MINIDUMP=$newDump"

$pass = -not $newDump
L "VERDICT=$(if($pass){'PASS_DT9_ORDERED_KILL'}else{'FAIL_NEW_DUMP'})"
$log | Set-Content (Join-Path $ArtifactDir "dt9-ordered.log")
@{
    verdict = $(if ($pass) { "PASS_DT9_ORDERED_KILL" } else { "FAIL_NEW_DUMP" })
    still_hot = $false
    new_minidump = $newDump
    dump_before = $dumpBefore
    dump_after = $dumpAfter
    ts = (Get-Date -Format o)
} | ConvertTo-Json | Set-Content (Join-Path $ArtifactDir "dt9.json")

if ($pass) { exit 0 } else { exit 1 }
