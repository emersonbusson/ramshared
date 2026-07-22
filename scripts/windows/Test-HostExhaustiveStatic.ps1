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
    "existing partition refused before private mount/write",
    "RAMSHARED",
    "refuse non-raw RAMSHARE disk without mounted RAMSHARED volume",
    "`$p.Refresh()",
    "exit_code recovered from RuntimeSummary",
    "Win32_DiskDrive",
    "remove stale pnp=",
    "PNP_GONE",
    "WIN32_GONE",
    "[UInt64]`$SizeBytes",
    "ExternalWorkloadMiB",
    "MinFreeAfterPlanMiB",
    "insufficient VRAM headroom",
    "winsvc-run.toml",
    "Start-CudaVramWorkload.ps1",
    "EXTERNAL_WORKLOAD_OK",
    "WslPressureMiB",
    "ApproveSharedDesktopWsl",
    "Invoke-SharedWslPressureCampaign.ps1",
    'Join-Path $PSScriptRoot "Invoke-SharedWslPressureCampaign.ps1"',
    "Add-PartitionAccessPath",
    "C:\ProgramData\RamShared\mounts",
    '"-AccessPath", $mountPath',
    "WSL_PRESSURE_OK",
    "`$completed = `$wp.WaitForExit",
    "external workload exit_code recovered from success marker",
    "\[cuda-vram-workload\] released",
    "Measure-RamSharedDiskIo.ps1",
    "DISK_IO_MEASURE_OK",
    "`$measureCompleted = `$mp.WaitForExit",
    "disk I/O measure exit_code recovered from direct checksum",
    '$_.Size -eq $SizeBytes'
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
if ($text -notmatch '\$online -and \$all -and \$externalOk -and \$diskIoOk -and \$stopped -and \$exitCode -eq 0 -and \$leaseReleased' -or
    $text -notmatch '\$pnpLeft\.Count -eq 0') {
    throw "complete_pass_gate: host harness must require lease release for PASS"
}
if ($text -notmatch '\$online -and \$all -and \$externalOk -and \$diskIoOk -and \$stopped') {
    throw "complete_pass_gate: host harness must require external workload result for PASS"
}
if ($text -match 'C:\\ProgramData\\RamShared\\winsvc-product\.toml"\s*\|?\s*Set-Content') {
    throw "product_config_mutation_forbidden: host harness must use an artifact-local run config"
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
if ($text -match 'New-Partition[^\r\n]*-DriveLetter') {
    throw "explorer_drive_forbidden: temporary RamShared LUN must use a private mount path"
}
foreach ($destructive in @("Clear-Disk", "Remove-Disk", "Remove-Partition", "Set-Disk -IsOffline", "Set-Disk -IsReadOnly")) {
    if ($text -match [regex]::Escape($destructive)) {
        throw ("physical_disk_mutation_forbidden: " + $destructive)
    }
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
