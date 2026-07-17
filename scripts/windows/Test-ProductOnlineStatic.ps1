#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$RepoRoot
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
}

$path = Join-Path $RepoRoot "crates\ramshared-winsvc\src\product_online.rs"
$text = Get-Content -LiteralPath $path -Raw
$hostPath = Join-Path $RepoRoot "crates\ramshared-winsvc\src\windows_host.rs"
$hostText = Get-Content -LiteralPath $hostPath -Raw
$registerIdx = $text.IndexOf('link.register_queue(&reg)')
$findIdx = $text.IndexOf('WindowsHostState::find_lun')
$onlineLogIdx = $text.IndexOf('product Online: run_id={run_id}')

if ($registerIdx -lt 0 -or $findIdx -lt 0 -or $onlineLogIdx -lt 0) {
    throw "startup_lun_required_before_online: expected register/find_lun/online log markers are missing"
}
if ($findIdx -lt $registerIdx -or $findIdx -gt $onlineLogIdx) {
    throw "startup_lun_required_before_online: product logs Online before observing the Windows LUN"
}
if ($text -notmatch 'startup LUN identity did not appear') {
    throw "startup_lun_required_before_online: missing bounded fail-closed startup identity error"
}
if ($text -notmatch 'startup LUN identity wait must pump I/O') {
    throw "startup_lun_required_before_online: startup identity wait does not document I/O pumping"
}
$pumpIdx = $text.IndexOf('readonly_host_call_with_io_pump(')
if ($pumpIdx -lt 0 -or $pumpIdx -gt $onlineLogIdx) {
    throw "startup_lun_required_before_online: startup identity wait does not pump COMMIT before Online"
}
if ($hostText.Contains('Write-Output ($d[0].Number+''|''+$n')) {
    throw "volume_identity_query: disk number must be cast to string before pipe concatenation"
}
if (-not $hostText.Contains('$wantSize={size_bytes}')) {
    throw "volume_identity_query: product stop identity must bind expected size in Get-Disk query"
}
if (-not $hostText.Contains('([uint64]$_.Size -eq $wantSize)')) {
    throw "volume_identity_query: product stop identity must filter Get-Disk by exact size"
}
if (-not $hostText.Contains('[string]$d[0].Number+''|''+$n+''|''+([string]$d[0].SerialNumber).Trim()+''|''+[string]$d[0].Size')) {
    throw "volume_identity_query: missing string-safe product identity output"
}
if ($hostText.Contains('IOCTL_DISK_GET_LENGTH_INFO')) {
    throw "volume_identity_query: product stop must not depend on PhysicalDrive length IOCTL"
}

Write-Output "PASS Test-ProductOnlineStatic"
