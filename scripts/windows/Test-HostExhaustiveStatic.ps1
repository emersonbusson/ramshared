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
    "RamSharedCtlOpen",
    "CreateFile",
    "control path OK",
    "ramshared service RUNNING but RamSharedCtl absent",
    "disk identity refused before format/write",
    "^RAMSHARE\s+VRAMDISK$",
    "IsBoot",
    "IsSystem",
    "existing letter refused before write",
    "RAMSHARED",
    "refuse non-raw RAMSHARE disk without mounted RAMSHARED volume",
    "`$p.Refresh()",
    "exit_code recovered from RuntimeSummary",
    "Win32_DiskDrive",
    "remove stale pnp=",
    "PNP_GONE",
    "WIN32_GONE"
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
if ($text -notmatch '\$online -and \$all -and \$stopped -and \$exitCode -eq 0 -and \$leaseReleased' -or
    $text -notmatch '\$pnpLeft\.Count -eq 0') {
    throw "complete_pass_gate: host harness must require lease release for PASS"
}
if ($text -notmatch 'Copy-Item \$brokerLog \(Join-Path \$art "broker-lab.log"\)') {
    throw "broker_log_artifact: host harness must preserve broker log evidence"
}
if ($text -match 'Stop-Process -Id \$p\.Id -Force[^\r\n]*exit 0') {
    throw "force_kill_cannot_pass: force-killed product must not be a passing path"
}
if ($text -match 'Format-Volume\s+-DriveLetter') {
    throw "format_by_letter_forbidden: host harness must format the partition object, not a free letter"
}
if ($text -notmatch '\$np \| Format-Volume') {
    throw "format_partition_object_required: host harness must pipe New-Partition result into Format-Volume"
}
if ($text -notmatch '\[int\]\$disk\.Number -ne 0') {
    throw "disk_zero_forbidden: host harness must refuse disk 0 before format/write"
}
if ($text -match 'Test-Path \$ctl') {
    throw "control_path_testpath_forbidden: device namespace must be opened with CreateFile"
}

Write-Output "PASS Test-HostExhaustiveStatic"
