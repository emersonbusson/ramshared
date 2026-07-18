#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ScriptPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Invoke-WindowsDiskCounterAudit.ps1"
}
$text = Get-Content -LiteralPath $ScriptPath -Raw

foreach ($needle in @(
    "PLAN_ONLY=1",
    "-ApprovePhysicalHost is required",
    "Get-WinDrivePreflight.ps1",
    "Run-HostExhaustive.ps1",
    "delegated ARTIFACT recovered from latest exhaustive directory",
    "exhaustive-*",
    "DISK_IO_MEASURE_OK",
    "Direct load during sampling",
    "Direct \d+ MiB write=.* read=.* match=True",
    "PerfDisk match:",
    "NONZERO_ACTIVITY",
    "LUN_GONE",
    "WIN32_GONE",
    "PNP_GONE",
    "Task Manager UI parity is not claimed"
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("windows_disk_counter_audit_static: missing " + $needle)
    }
}

foreach ($forbidden in @(
    "Initialize-Disk",
    "Format-Volume",
    "New-Partition",
    "Clear-Disk",
    "Remove-Partition"
)) {
    if ($text -match [regex]::Escape($forbidden)) {
        throw ("windows_disk_counter_audit_static: forbidden token " + $forbidden)
    }
}

Write-Output "PASS Test-WindowsDiskCounterAuditStatic"
