#Requires -Version 5.1
<#
.SYNOPSIS
  ITEM-8 B2 lab drill (VM ONLY): kill backend with pagefile-VRAM Usage > 0.

.DESCRIPTION
  SPEC windows-swap-driver DT-10 / DEGRADATION-MATRIX B2.
  Precondition: RamShared LUN mounted, pagefile on that volume with CurrentUsage > 0,
  WinDriveBackend (lab) holding the disk.

  Steps (Kahneman #1/#3/#13):
  1) Snapshot PF_USE / disks / minidump timestamp
  2) Abort INCONCLUSIVO if D: (or target) pagefile usage <= 0
  3) Kill WinDriveBackend (lab stand-in for ramshared-winsvc crash)
  4) Attempt I/O on the volume - expect failure, not hang
  5) Wait bounded window; refuse if new minidump appears
  6) Optional restart backend (recovery probe)

.NOTES
  RNF-6: never on daily host. Prefer Hyper-V checkpoint before run.
#>
[CmdletBinding()]
param(
    [string]$PagefileDrive = "D",
    [int]$PostKillWaitSec = 20,
    [string]$ArtifactDir = "C:\ramshared\artifacts\b2-lab",
    [switch]$RestartBackend,
    [string]$BackendPath = "C:\ramshared\bin\WinDriveBackend.exe",
    [UInt64]$BackendSize = 67108864
)

$ErrorActionPreference = "Continue"
$drive = $PagefileDrive.TrimEnd(':')
New-Item -ItemType Directory -Force -Path $ArtifactDir | Out-Null

function Get-PfUse {
    Get-CimInstance Win32_PageFileUsage -EA SilentlyContinue |
        ForEach-Object { "$($_.Name) a=$($_.AllocatedBaseSize) u=$($_.CurrentUsage)" }
}

function Get-LastDump {
    Get-ChildItem C:\Windows\Minidump -EA SilentlyContinue |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
}

$model = (Get-CimInstance Win32_ComputerSystem).Model
$vmHints = @("Virtual", "VMware", "KVM", "Hyper-V", "QEMU", "VirtualBox")
$isVm = $false
foreach ($h in $vmHints) { if ($model -like "*$h*") { $isVm = $true; break } }
if (-not $isVm) {
    throw "REFUSE: model='$model' does not look like a VM (RNF-6)."
}

$log = @()
function L([string]$s) { $script:log += $s; Write-Host $s }

L "=== B2 LAB DRILL $(Get-Date -Format o) ==="
L "MODEL=$model"
L "DRIVE=${drive}:"

$pf = Get-CimInstance Win32_PageFileUsage -EA SilentlyContinue |
    Where-Object { $_.Name -match "^${drive}:" }
if (-not $pf -or $pf.AllocatedBaseSize -le 0) {
    L "INCONCLUSIVO: no active pagefile on ${drive}: (alloc missing)"
    $log | Set-Content (Join-Path $ArtifactDir "b2-inconclusive.txt")
    exit 3
}
$usagePct = if ($pf.AllocatedBaseSize -gt 0) { 100.0 * $pf.CurrentUsage / $pf.AllocatedBaseSize } else { 0 }
L "PF_PRE=$($pf.Name) a=$($pf.AllocatedBaseSize) u=$($pf.CurrentUsage) pct=$([math]::Round($usagePct,2))"
if ($pf.CurrentUsage -le 0) {
    L "INCONCLUSIVO: pagefile alloc>0 but CurrentUsage=0 (DT-21 not met for B2)"
    $log | Set-Content (Join-Path $ArtifactDir "b2-inconclusive.txt")
    exit 3
}

$be = Get-Process -Name WinDriveBackend -EA SilentlyContinue
if (-not $be) {
    L "INCONCLUSIVO: WinDriveBackend not running (nothing to kill for lab B2)"
    $log | Set-Content (Join-Path $ArtifactDir "b2-inconclusive.txt")
    exit 3
}
L "BACKEND_PID=$($be.Id)"

$dumpBefore = Get-LastDump
L "DUMP_BEFORE=$(if($dumpBefore){"$($dumpBefore.Name) $($dumpBefore.LastWriteTime)"}else{"none"})"
L "DISKS_PRE=$((Get-Disk | ForEach-Object { "N=$($_.Number) $($_.FriendlyName) $($_.Size)" }) -join '; ')"

# Touch volume once while alive (proves path)
$smokePath = "${drive}:\b2-pre.txt"
try {
    "alive-$(Get-Date -Format o)" | Set-Content $smokePath -EA Stop
    L "PRE_WRITE=OK"
} catch {
    L "PRE_WRITE_FAIL=$($_.Exception.Message)"
}

# --- B2 kill ---
L "B2_KILL WinDriveBackend"
Stop-Process -Name WinDriveBackend -Force -EA SilentlyContinue
Start-Sleep 2
$still = Get-Process -Name WinDriveBackend -EA SilentlyContinue
L "BACKEND_AFTER_KILL=$([bool]$still)"

# I/O must fail or complete with error - must not hang forever
$ioOutcome = "UNKNOWN"
$sw = [Diagnostics.Stopwatch]::StartNew()
$job = Start-Job -ScriptBlock {
    param($p)
    try {
        Get-Content $p -Raw -EA Stop | Out-Null
        "READ_OK"
    } catch {
        "READ_FAIL:$($_.Exception.Message)"
    }
} -ArgumentList $smokePath

$completed = Wait-Job $job -Timeout 15
$sw.Stop()
if (-not $completed) {
    Stop-Job $job -EA SilentlyContinue
    Remove-Job $job -Force -EA SilentlyContinue
    $ioOutcome = "READ_TIMEOUT_15s"
} else {
    $ioOutcome = Receive-Job $job
    Remove-Job $job -Force -EA SilentlyContinue
}
L "IO_POST_KILL=$ioOutcome elapsed_ms=$($sw.ElapsedMilliseconds)"

# Disk enumeration post-kill
try {
    L "DISKS_POST=$((Get-Disk -EA Stop | ForEach-Object { "N=$($_.Number) $($_.FriendlyName) $($_.OperationalStatus)" }) -join '; ')"
} catch {
    L "DISKS_POST_ERR=$($_.Exception.Message)"
}

L "WAIT ${PostKillWaitSec}s for bugcheck window"
Start-Sleep $PostKillWaitSec

$dumpAfter = Get-LastDump
L "DUMP_AFTER=$(if($dumpAfter){"$($dumpAfter.Name) $($dumpAfter.LastWriteTime)"}else{"none"})"
$newDump = $false
if ($dumpAfter -and (-not $dumpBefore -or $dumpAfter.FullName -ne $dumpBefore.FullName -or $dumpAfter.LastWriteTime -gt $dumpBefore.LastWriteTime)) {
    if ($dumpBefore -eq $null -or $dumpAfter.LastWriteTime -gt $dumpBefore.LastWriteTime) {
        $newDump = $true
    }
}
L "NEW_MINIDUMP=$newDump"

$guestAlive = $true
try { $null = Get-Process -Id $PID; L "GUEST_ALIVE=True" } catch { $guestAlive = $false; L "GUEST_ALIVE=False" }

if ($RestartBackend -and (Test-Path $BackendPath)) {
    L "RESTART_BACKEND"
    Start-Process $BackendPath -ArgumentList @("$BackendSize", "300") `
        -RedirectStandardOutput C:\ramshared\bin\backend.out `
        -RedirectStandardError C:\ramshared\bin\backend.err `
        -WindowStyle Hidden
    Start-Sleep 8
    L "BACKEND_RESTARTED=$([bool](Get-Process WinDriveBackend -EA SilentlyContinue))"
    L "BE_OUT=$((Get-Content C:\ramshared\bin\backend.out -Raw -EA SilentlyContinue) -replace '\s+',' ')"
}

# Verdict
$pass = $true
$reasons = @()
if ($newDump) { $pass = $false; $reasons += "new_minidump" }
if ($ioOutcome -eq "READ_TIMEOUT_15s") { $pass = $false; $reasons += "io_hang" }
if ($ioOutcome -eq "READ_OK") {
    # Volume still readable after backend death can be OK if OS cached; flag as warn
    $reasons += "io_still_ok_cached_or_unexpected"
}
if (-not $guestAlive) { $pass = $false; $reasons += "guest_dead" }

L "=== VERDICT ==="
L "PASS=$pass"
L "REASONS=$(($reasons -join ','))"
L "NOTE=lab B2 uses WinDriveBackend kill; product path is ramshared-winsvc"

$log | Set-Content (Join-Path $ArtifactDir "b2-lab.log") -Encoding UTF8
@{
    pass = $pass
    pagefile_usage_pct = $usagePct
    pagefile_current = $pf.CurrentUsage
    io_post_kill = "$ioOutcome"
    new_minidump = $newDump
    reasons = $reasons
    ts = (Get-Date -Format o)
} | ConvertTo-Json | Set-Content (Join-Path $ArtifactDir "b2-lab.json") -Encoding UTF8

if ($pass) { exit 0 } else { exit 1 }
