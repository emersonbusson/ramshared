#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Invoke-LinuxKernelLabCapabilityAudit.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

if ($text -notmatch '\[string\]\$VMName = "linux-kernel-lab"') {
    throw "default_vm: audit must target linux-kernel-lab by default"
}
if ($text -notmatch 'Get-LinuxKernelLabAccess\.ps1') {
    throw "access_helper: audit must reuse the documented access helper"
}
if ($text -notmatch 'BatchMode=yes' -or $text -notmatch 'ConnectTimeout=10') {
    throw "bounded_ssh: audit must use bounded non-interactive SSH"
}
if ($text -notmatch 'sudo -n true' -or $text -notmatch 'sudo -n modprobe -n -v ublk_drv') {
    throw "capability_probe: audit must check noninteractive sudo and dry-run ublk module loading"
}
if ($text -notmatch '/dev/ublk-control') {
    throw "ublk_control: audit must check the ublk control device"
}
if ($text -notmatch 'RequireGpuSurface' -or $text -notmatch '/dev/dxg' -or $text -notmatch '/dev/nvidiactl' -or $text -notmatch 'nvidia-smi') {
    throw "gpu_surface: audit must support explicit GPU surface gating"
}
if ($text -notmatch 'STATUS=PASS' -or $text -notmatch 'STATUS=PARTIAL') {
    throw "verdicts: audit must emit machine-readable PASS/PARTIAL status"
}
if ($text -notmatch 'access_failed' -or $text -notmatch 'try \{' -or $text -notmatch 'catch \{' -or $text -notmatch 'Get-LinuxKernelLabAccess\.ps1') {
    throw "access_failure: audit must turn access helper failures into PARTIAL artifacts"
}
if ($text -match 'Initialize-Disk|Format-Volume|Resize-VHD|Convert-VHD|New-VHD|Remove-VMHardDiskDrive|Add-VMHardDiskDrive') {
    throw "disk_safety: audit must not mutate disks"
}
if ($text -match 'Password\s*=' -or $text -match 'ConvertTo-SecureString' -or $text -match 'Get-Content .*drill-pw') {
    throw "secret_safety: audit must not read or store credentials"
}
if ($text -match 'swapon|swapoff|stress-ng|fio|dd if=|ramshared up') {
    throw "host_safety: audit must not run swap or pressure workloads"
}

Write-Output "PASS Test-LinuxKernelLabCapabilityAuditStatic"
