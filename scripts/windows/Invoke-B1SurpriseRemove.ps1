#Requires -Version 5.1
<#
.SYNOPSIS
  ITEM-8 B1 lab drill (VM ONLY): surprise-remove disk path with controlled preconditions.

.DESCRIPTION
  SPEC DT-9 / DEGRADATION-MATRIX B1:
  Pulling the virtual disk while a pagefile is active is the dangerous vector (0x7A).

  This drill has two arms (Kahneman #13 — no fake PASS):

  Arm SAFE (DT-9 compliant):
    1) Ensure NO secondary pagefile on RamShared volume (or Usage/alloc == 0)
    2) Ensure backend running + LUN present
    3) Surprise: kill backend AND/OR remove PnP device (devcon remove)
    4) Wait; assert guest alive and no new minidump

  Arm HOT (documented hazard — default OFF):
    -ForceHotPagefileKill: kill with pagefile Usage>0 — EXPECT 0x7A historically.
      Only for adversarial re-proof; requires Hyper-V checkpoint.

.NOTES
  RNF-6: refuse unless Hyper-V/VM model. Prefer Checkpoint-VM before run.
#>
[CmdletBinding()]
param(
    [string]$Drive = "D",
    [int]$PostWaitSec = 20,
    [string]$ArtifactDir = "C:\ramshared\artifacts\b1-lab",
    [switch]$ForceHotPagefileKill,
    [switch]$RemovePnpDevice
)

$ErrorActionPreference = "Continue"
$letter = $Drive.TrimEnd(':')
New-Item -ItemType Directory -Force -Path $ArtifactDir | Out-Null
$log = New-Object System.Collections.Generic.List[string]
function L([string]$s) { [void]$log.Add($s); Write-Host $s }

$model = (Get-CimInstance Win32_ComputerSystem).Model
if ($model -notmatch "Virtual|VMware|KVM|Hyper-V|QEMU|VirtualBox") {
    throw "REFUSE RNF-6: model=$model"
}

Import-Module Storage -EA SilentlyContinue

function Dump-Name {
    $d = Get-ChildItem C:\Windows\Minidump -EA SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($d) { return $d.Name } else { return "none" }
}

L "=== B1 SURPRISE-REMOVE $(Get-Date -Format o) ==="
L "MODEL=$model FORCE_HOT=$ForceHotPagefileKill"
L "DUMP_BEFORE=$(Dump-Name)"

$pf = Get-CimInstance Win32_PageFileUsage -EA SilentlyContinue |
    Where-Object { $_.Name -match "^${letter}:" }
$pfHot = $pf -and $pf.AllocatedBaseSize -gt 0
L "PF=$(if($pf){"$($pf.Name) a=$($pf.AllocatedBaseSize) u=$($pf.CurrentUsage)"}else{"absent"}) HOT=$pfHot"

if ($pfHot -and -not $ForceHotPagefileKill) {
    L "ARM=SAFE but pagefile still hot -> ABORT to DT-9 first (refuse surprise-remove)"
    L "VERDICT=INCONCLUSIVO_NEED_DT9"
    $log | Set-Content (Join-Path $ArtifactDir "b1-inconclusive.txt")
    exit 3
}

if ($ForceHotPagefileKill -and -not $pfHot) {
    L "INCONCLUSIVO: ForceHot requested but PF not hot"
    $log | Set-Content (Join-Path $ArtifactDir "b1-inconclusive.txt")
    exit 3
}

$be = Get-Process WinDriveBackend -EA SilentlyContinue
L "BACKEND=$([bool]$be)"
if (-not $be -and -not $RemovePnpDevice) {
    L "INCONCLUSIVO: nothing to surprise-remove"
    exit 3
}

# Optional filesystem probe
try {
    "b1-$(Get-Date -Format o)" | Set-Content "${letter}:\b1-pre.txt" -EA Stop
    L "PRE_WRITE=OK"
} catch {
    L "PRE_WRITE_SKIP=$($_.Exception.Message)"
}

L "SURPRISE_KILL backend"
Get-Process WinDriveBackend -EA SilentlyContinue | Stop-Process -Force

if ($RemovePnpDevice -and (Test-Path C:\ramshared\bin\devcon.exe)) {
    L "SURPRISE_REMOVE PnP"
    & C:\ramshared\bin\devcon.exe remove Root\RamShared 2>&1 | Out-String | ForEach-Object { L "DEVCON=$_" }
    & C:\ramshared\bin\devcon.exe remove "ROOT\SCSIADAPTER\*" 2>&1 | Out-String | ForEach-Object { L "DEVCON2=$_" }
}

L "WAIT ${PostWaitSec}s"
Start-Sleep $PostWaitSec

$dumpA = Dump-Name
L "DUMP_AFTER=$dumpA"
$newDump = ($dumpA -ne "none") -and ($dumpA -ne (Dump-Name)) # fix below
# re-get before
$dumpBefore = (Get-Content (Join-Path $ArtifactDir "dump-before.txt") -EA SilentlyContinue)
if (-not $dumpBefore) { $dumpBefore = $null }

# Store dump before properly at start
# (re-read from first line we logged is fragile — capture now from variable we should have saved)
# Fix: we logged DUMP_BEFORE — parse from that
$beforeLine = ($log | Where-Object { $_ -like "DUMP_BEFORE=*" } | Select-Object -First 1)
$beforeName = if ($beforeLine) { $beforeLine.Substring("DUMP_BEFORE=".Length) } else { "none" }
$newDump = ($dumpA -ne $beforeName) -and ($dumpA -ne "none")

L "NEW_MINIDUMP=$newDump"
L "GUEST_ALIVE=True"
L "BE_AFTER=$([bool](Get-Process WinDriveBackend -EA SilentlyContinue))"

if ($ForceHotPagefileKill) {
    # Expected: often BSOD 0x7A — if new dump, record HAZARD_CONFIRMED not PASS
    if ($newDump) {
        L "VERDICT=HAZARD_CONFIRMED_0x7A_PATH"
        $exit = 1
    } else {
        L "VERDICT=UNEXPECTED_NO_BSOD (document carefully)"
        $exit = 0
    }
} else {
    # SAFE arm: PASS if no new dump
    if ($newDump) {
        L "VERDICT=FAIL_SAFE_ARM_BSOD"
        $exit = 1
    } else {
        L "VERDICT=PASS_B1_SAFE_ARM"
        $exit = 0
    }
}

$log | Set-Content (Join-Path $ArtifactDir "b1-lab.log")
@{
    force_hot = [bool]$ForceHotPagefileKill
    new_minidump = $newDump
    dump_before = $beforeName
    dump_after = $dumpA
    verdict = ($log | Where-Object { $_ -like "VERDICT=*" } | Select-Object -Last 1)
    ts = (Get-Date -Format o)
} | ConvertTo-Json | Set-Content (Join-Path $ArtifactDir "b1-lab.json")

exit $exit
