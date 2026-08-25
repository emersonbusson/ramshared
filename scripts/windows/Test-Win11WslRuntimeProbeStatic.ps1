#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Invoke-Win11WslRuntimeProbe.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

foreach ($needle in @(
    'win11-drill',
    'win11-wsl2-lab',
    'ValidateSet("plan", "status", "probe")',
    'ValidateSet("win11-drill", "win11-wsl2-lab")',
    'ExpectedVMId',
    'ApproveGuestWslProbe',
    'UseBlankPassword',
    'blank_password_probe_not_approved_vm',
    'blank_password_probe_explicit_password_forbidden',
    'Invoke-GuestPsDirectBounded.ps1',
    'Invoke-GuestPsDirectBounded',
    'Invoke-BoundedGuestProcess',
    'Get-VMSnapshot',
    'Get-VMHardDiskDrive',
    'ExposeVirtualizationExtensions',
    'expected_vm_id_required',
    'vm_identity_mismatch',
    'snapshot_residue',
    'nested_virtualization_unavailable',
    'PLAN',
    'PowerShell Direct',
    'GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)',
    'Microsoft-Windows-Subsystem-Linux',
    'VirtualMachinePlatform',
    'Where-Object { $_.Name -eq "WslService" }',
    'guest_wsl_service_missing',
    'guest_wsl_service_not_running',
    'Invoke-WslWithTimeout',
    'StandardOutputEncoding = [Text.Encoding]::Unicode',
    'StandardErrorEncoding = [Text.Encoding]::Unicode',
    'status_timeout',
    'list_timeout',
    'guest_wsl_runtime_unavailable',
    'guest_shutdown_timeout',
    'restored_off_host_fallback',
    'host_graceful_shutdown_timeout',
    'powershell_direct_auth_failed',
    'powershell_direct_unavailable',
    'GuestPasswordMayBeEmpty',
    '-AllowEmptyPassword:$UseBlankPassword',
    'probe_reason',
    'cleanup_reason',
    'DISK_MUTATION = $false'
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("win11_wsl_runtime_probe_static: missing " + $needle)
    }
}

if ($text -notmatch 'if\s*\(\$UseBlankPassword\s+-and\s+\$VMName\s+-cne\s+"win11-wsl2-lab"\)') {
    throw "win11_wsl_runtime_probe_static: blank-password recovery must be limited to the approved lab"
}
if ($text -notmatch 'if\s*\(\$UseBlankPassword\s+-and\s+-not\s+\[string\]::IsNullOrEmpty\(\$Password\)\)') {
    throw "win11_wsl_runtime_probe_static: blank-password recovery must reject an explicit competing password"
}

if ($text -match 'Invoke-Command\s+-VMName|Register-ScheduledTask|Start-ScheduledTask|Start-Job|Stop-Job') {
    throw "win11_wsl_runtime_probe_static: remote work must use the bounded PowerShell Direct helper without guest scheduled tasks or jobs"
}

if ($text -match 'Stop-VM\s+.*-(TurnOff|Force|Save)|Initialize-Disk|Format-Volume|Resize-VHD|Convert-VHD|New-VHD|Set-Content.*RAMSHARED_DRILL_PASSWORD|ConvertTo-Json.*Password') {
    throw "win11_wsl_runtime_probe_static: disk mutation or secret persistence is forbidden"
}

$fallbackStart = $text.IndexOf('function Invoke-GracefulHostShutdownFallback')
$fallbackEnd = $text.IndexOf('function Restore-StartedLabVmOff')
if ($fallbackStart -lt 0 -or $fallbackEnd -le $fallbackStart) {
    throw "win11_wsl_runtime_probe_static: graceful fallback function boundary is missing"
}
$fallbackText = $text.Substring($fallbackStart, $fallbackEnd - $fallbackStart)
if ($fallbackText -notmatch '\bdo\b[\s\S]*\bwhile\b' -or
    $fallbackText -notmatch 'VmShutdownTimeoutSeconds' -or
    $fallbackText -notmatch 'Start-Sleep\s+-Seconds\s+3') {
    throw "win11_wsl_runtime_probe_static: graceful fallback must retry while the guest shutdown channel initializes"
}

$approvalOffset = $text.IndexOf('if (-not $ApproveGuestWslProbe)')
$startOffset = $text.IndexOf('Start-VM')
if ($approvalOffset -lt 0 -or $startOffset -lt 0 -or $approvalOffset -ge $startOffset) {
    throw "win11_wsl_runtime_probe_static: approval must be checked before VM start"
}

Write-Output "PASS Test-Win11WslRuntimeProbeStatic"
