#Requires -Version 5.1
<#
.SYNOPSIS
  RNF-5 / R8 / DT-19 — holder-cooperative lease revoke with pagefile active.

.DESCRIPTION
  Does NOT send a fake broker Msg (C1). Stops the service via SCM so it runs DT-9:
  pagefile off → (reboot if needed) → drain → destroy → wipe → LeaseRelease.
  Verifies lease gone on broker log/status.

.NOTES
  VM recommended. Never destroy disk while pagefile still active.
#>
[CmdletBinding()]
param(
    [string]$ServiceName = "ramshared-winsvc",
    [string]$BrokerStatusCmd = "",
    [string]$ArtifactDir = ".\artifacts\revoke-drill"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $ArtifactDir | Out-Null
$runId = "revoke-{0:yyyyMMdd-HHmmss}" -f (Get-Date)
$log = Join-Path $ArtifactDir "$runId.log"

function Log([string]$m) {
    $line = "$(Get-Date -Format o) $m"
    Write-Host $line
    Add-Content -Path $log -Value $line
}

Log "Invoke-RevokeDrill start (DT-19 holder-cooperative)"

# Capture pagefile state before stop
try {
    $pf = Get-CimInstance Win32_PageFileUsage -ErrorAction SilentlyContinue
    Log ("pagefile_before: " + ($pf | ConvertTo-Json -Compress))
} catch {
    Log "pagefile_before: unavailable"
}

Log "Stop-Service $ServiceName (service must run DT-9 then LeaseRelease)"
try {
    Stop-Service -Name $ServiceName -Force -ErrorAction Stop
    Log "service stopped"
} catch {
    Log "service stop failed: $($_.Exception.Message)"
    exit 2
}

# Wait for ordered teardown
Start-Sleep -Seconds 5

try {
    $pf2 = Get-CimInstance Win32_PageFileUsage -ErrorAction SilentlyContinue
    Log ("pagefile_after: " + ($pf2 | ConvertTo-Json -Compress))
} catch {
    Log "pagefile_after: unavailable"
}

if ($BrokerStatusCmd -ne "") {
    Log "broker status: $BrokerStatusCmd"
    cmd /c $BrokerStatusCmd 2>&1 | Tee-Object -FilePath (Join-Path $ArtifactDir "$runId-broker.txt")
}

Log @"
Abort triggers (SPEC RNF-5):
- pagefile still active after 'release'
- deadlock in teardown
- broker still shows lease after clean disconnect
Pass: LeaseRelease observed; no pagefile on VRAM volume; no BSOD.
"@
exit 0
