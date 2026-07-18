#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Run-HostExhaustive.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

foreach ($needle in @(
    "Lab-LeaseBroker.ps1",
    "RS_BROKER_PORT",
    "broker-lab.log",
    "broker-lab.stop",
    "lease_release",
    "lease {0} liberado",
    "LEASE_RELEASED",
    "\\.\RamSharedCtl",
    "\\.\GLOBALROOT\Device\RamSharedCtl",
    "ramshared service RUNNING but RamSharedCtl absent"
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("host_broker_required: missing " + $needle)
    }
}

if ($text -notmatch 'product Online:') {
    throw "online_marker_must_be_strong: host harness must wait for the full Online line"
}
if ($text -match 'if \(\$txt -match "product Online"\)') {
    throw "online_marker_must_be_strong: host harness still accepts startup banner as Online"
}
if ($text -notmatch 'if \(\$online -and \$all -and \$stopped -and \$exitCode -eq 0 -and \$leaseReleased\)') {
    throw "complete_pass_gate: host harness must require lease release for PASS"
}
if ($text -notmatch 'Copy-Item \$brokerLog \(Join-Path \$art "broker-lab.log"\)') {
    throw "broker_log_artifact: host harness must preserve broker log evidence"
}
if ($text -match 'Stop-Process -Id \$p\.Id -Force[^\r\n]*exit 0') {
    throw "force_kill_cannot_pass: force-killed product must not be a passing path"
}

Write-Output "PASS Test-HostExhaustiveStatic"
