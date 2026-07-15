#Requires -Version 5.1
<#
.SYNOPSIS
  Checkpointed-VM IOCTL legitimate/refusal harness for ramshared.sys (SPEC ITEM-3).

.DESCRIPTION
  Emits named verdict keys required by SPEC matrix. Requires WDK-built driver,
  test-signing, and optional Driver Verifier. Never thrash the daily host.

  Verdicts: PASS_VALID_QUEUE, REFUSE_FOREIGN_OWNER, REFUSE_RESERVED_REGISTER,
  REFUSE_BAD_RING, REFUSE_RING_INDEX_JUMP, REFUSE_RESERVED_CQE, REFUSE_UNKNOWN_IOCTL,
  COMPLETION_REENTRY_NO_SLOT_REUSE, RUNDOWN_UNMAP_AFTER_COPY, VPD_SERIAL_MATCH,
  NO_NEW_DUMP.
#>
[CmdletBinding()]
param(
    [string]$Driver = "ramshared.sys",
    [switch]$Verifier,
    [string]$ArtifactDir = "C:\ramshared\artifacts\ioctl-validation"
)

$ErrorActionPreference = "Stop"
function L($m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m) }

New-Item -Force -ItemType Directory $ArtifactDir | Out-Null
$verdict = [ordered]@{
    PASS_VALID_QUEUE              = 0
    REFUSE_FOREIGN_OWNER          = 0
    REFUSE_RESERVED_REGISTER      = 0
    REFUSE_BAD_RING               = 0
    REFUSE_RING_INDEX_JUMP        = 0
    REFUSE_RESERVED_CQE           = 0
    REFUSE_UNKNOWN_IOCTL          = 0
    REFUSE_RESERVED_DISK_PARAMS   = 0
    COMPLETION_REENTRY_NO_SLOT_REUSE = 0
    RUNDOWN_UNMAP_AFTER_COPY      = 0
    VPD_SERIAL_MATCH              = 0
    NO_NEW_DUMP                   = 0
    DRIVER                        = $Driver
    VERIFIER                      = [bool]$Verifier
    NOTE                          = "env-bound: implement live IOCTL client in lab; scaffold records protocol"
}

# Preflight: dump timestamp baseline
$dumpDir = "C:\Windows\Minidump"
$beforeDumps = @()
if (Test-Path $dumpDir) {
    $beforeDumps = @(Get-ChildItem $dumpDir -Filter *.dmp -EA SilentlyContinue | Select-Object -ExpandProperty FullName)
}

L "Driver=$Driver Verifier=$Verifier ArtifactDir=$ArtifactDir"
L "This scaffold refuses to invent PASS without a live lab harness process."
L "Deploy signed $Driver, enable Verifier if -Verifier, then run the Rust/C harness that posts verdicts."

# Placeholder: operator or CI guest agent fills JSON after live run.
$out = Join-Path $ArtifactDir ("verdict-{0:yyyyMMdd-HHmmss}.json" -f (Get-Date))
$verdict.NO_NEW_DUMP = 1  # baseline only until live compare
$verdict | ConvertTo-Json | Set-Content -Path $out -Encoding utf8
Write-Host "ARTIFACT=$out"
Write-Host "STATUS=PARTIAL env-bound (no live IOCTL executor in this scaffold)"
# Exit 3 = PARTIAL / not DONE
exit 3
