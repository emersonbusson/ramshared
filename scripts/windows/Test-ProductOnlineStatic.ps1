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
$mainPath = Join-Path $RepoRoot "crates\ramshared-winsvc\src\main.rs"
$mainText = Get-Content -LiteralPath $mainPath -Raw
$hostPath = Join-Path $RepoRoot "crates\ramshared-winsvc\src\windows_host.rs"
$hostText = Get-Content -LiteralPath $hostPath -Raw
$driverPath = Join-Path $RepoRoot "crates\ramshared-winsvc\src\windows_driver.rs"
$driverText = Get-Content -LiteralPath $driverPath -Raw
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
if ($text -notmatch 'STARTUP_LUN_PROVIDER_TIMEOUT:\s*Duration\s*=\s*Duration::from_secs\(12\)' -or
    $text -notmatch 'STARTUP_LUN_PUMP_TIMEOUT:\s*Duration\s*=\s*Duration::from_secs\(15\)' -or
    $text -notmatch 'readonly_host_call_with_io_pump\([\s\S]{0,300}STARTUP_LUN_PUMP_TIMEOUT' -or
    $text -notmatch 'find_lun\([\s\S]{0,150}STARTUP_LUN_PROVIDER_TIMEOUT' -or
    $hostText -notmatch 'find_lun\([\s\S]{0,160}provider_timeout:\s*Duration' -or
    $hostText -notmatch 'run_powershell_bounded\([\s\S]{0,120}provider_timeout') {
    throw "startup_lun_observation_budgets_are_nested failed"
}
Write-Output "PASS startup_lun_observation_budgets_are_nested"
$pumpIdx = $text.IndexOf('readonly_host_call_with_io_pump(')
if ($pumpIdx -lt 0 -or $pumpIdx -gt $onlineLogIdx) {
    throw "startup_lun_required_before_online: startup identity wait does not pump COMMIT before Online"
}
if ($hostText.Contains('Write-Output ($d[0].Number+''|''+$n')) {
    throw "volume_identity_query: disk number must be cast to string before pipe concatenation"
}
if (-not $hostText.Contains('$wantSize=[uint64]$env:RAMSHARED_SIZE')) {
    throw "volume_identity_query: product stop identity must bind expected size through the child environment"
}
if (-not $hostText.Contains('([uint64]$_.Size -eq $wantSize)')) {
    throw "volume_identity_query: product stop identity must filter Get-Disk by exact size"
}
if (-not $hostText.Contains('[string]$d[0].Number+''|''+$n+''|''+([string]$d[0].SerialNumber).Trim()+''|''+[string]$d[0].Size+''|''+$vp')) {
    throw "volume_identity_query: missing string-safe product identity output"
}
foreach ($needle in @('$wantMount', 'AccessPaths', 'Get-Volume -ErrorAction Stop', 'lock_product_volume_path')) {
    if (-not $hostText.Contains($needle) -and -not $text.Contains($needle)) {
        throw ("private_mount_identity_required: missing " + $needle)
    }
}
foreach ($needle in @('$env:RAMSHARED_LETTER', '$env:RAMSHARED_MOUNT', '$env:RAMSHARED_SERIAL', '$env:RAMSHARED_SIZE', 'validate_powershell_environment', 'command.env(key, value)')) {
    if (-not $hostText.Contains($needle)) {
        throw ("powershell_dynamic_values_use_environment_only: missing " + $needle)
    }
}
foreach ($forbidden in @('$wantSerial=''', '$wantLetter=''', '$wantMount=''')) {
    if ($hostText.Contains($forbidden)) {
        throw ("powershell_dynamic_values_use_environment_only: interpolated value marker " + $forbidden)
    }
}
Write-Output "PASS powershell_dynamic_values_use_environment_only"
if ($hostText.Contains('IOCTL_DISK_GET_LENGTH_INFO')) {
    throw "volume_identity_query: product stop must not depend on PhysicalDrive length IOCTL"
}
if ($mainText -notmatch 'set_service_status\(ServiceStatus\s*\{[\s\S]{0,500}current_state:\s*ServiceState::StopPending[\s\S]{0,1000}SCM StopPending status failed') {
    throw "scm_stop_pending_status_error_is_logged failed"
}
Write-Output "PASS scm_stop_pending_status_error_is_logged"

# DT-4 / DT-5 / DT-6 `windows_driver.rs` adapter contract: the mapped queue is
# Windows-only and isolates every unsafe block; IOCTL codes derive from the
# shared ABI constants; one pending COMMIT_AND_FETCH is cancelled and drained
# through a single OVERLAPPED; ring indices publish with Release and observe
# with Acquire; `IoctlError` Display carries stable classes and no pointers;
# allocation error paths free in reverse order with no leak.
if ($driverText -notmatch '#!\[cfg\(windows\)\]' -or
    $driverText -notmatch '#!\[allow\(unsafe_code\)\]') {
    throw "windows_driver_mapped_queue_contract: adapter is not Windows-gated unsafe-confined"
}
foreach ($token in @('FILE_DEVICE_MASS_STORAGE: u32 = 0x0000_002d',
                     'METHOD_BUFFERED: u32 = 0',
                     'IOCTL_REGISTER: u32 = ioctl_code(0)',
                     'IOCTL_UNREGISTER: u32 = ioctl_code(1)',
                     'IOCTL_COMMIT: u32 = ioctl_code(2)',
                     'IOCTL_CREATE: u32 = ioctl_code(3)',
                     'IOCTL_DESTROY: u32 = ioctl_code(4)')) {
    if (-not $driverText.Contains($token)) {
        throw ("windows_driver_mapped_queue_contract: missing ABI token " + $token)
    }
}
if ($driverText -notmatch 'if self\.pending' -or
    $driverText -notmatch 'commit already pending' -or
    $driverText -notmatch 'CancelIoEx' -or
    $driverText -notmatch 'GetOverlappedResult' -or
    $driverText -notmatch 'fn cancel_and_drain\([\s\S]{0,400}GetOverlappedResult') {
    throw "windows_driver_mapped_queue_contract: single pending COMMIT is not cancelled and drained"
}
if ($driverText -notmatch 'AtomicU32' -or
    $driverText -notmatch 'Ordering::Acquire' -or
    $driverText -notmatch 'Ordering::Release' -or
    $driverText -notmatch 'fn load_idx\([\s\S]{0,300}Ordering::Acquire' -or
    $driverText -notmatch 'fn store_idx\([\s\S]{0,300}Ordering::Release') {
    throw "windows_driver_mapped_queue_contract: ring indices do not publish Acquire/Release"
}
if ($driverText -match '\{:p\}' -or
    $driverText -notmatch 'impl std::fmt::Display for IoctlError' -or
    $driverText -notmatch 'IoctlError::Timeout => write!\(f, "timeout"\)') {
    throw "windows_driver_mapped_queue_contract: IoctlError Display is not pointer-free stable classes"
}
if ($driverText -notmatch 'Err\(error\) => \{\s*free_region\(sq, sq_bytes\)' -or
    $driverText -notmatch 'Err\(error\) => \{\s*free_region\(cq, cq_bytes\);\s*free_region\(sq, sq_bytes\)' -or
    $driverText -notmatch 'impl Drop for WindowsMappedQueue' -or
    $driverText -notmatch 'free_region\(self\.sq, self\.sq_bytes\);\s*free_region\(self\.cq, self\.cq_bytes\);\s*free_region\(self\.data, self\.data_bytes\)') {
    throw "windows_driver_mapped_queue_contract: mapped regions are not freed on every exit path"
}
Write-Output "PASS windows_driver_mapped_queue_contract"

Write-Output "PASS Test-ProductOnlineStatic"
