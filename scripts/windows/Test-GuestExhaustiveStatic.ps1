#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Run-GuestExhaustive.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

if ($text -notmatch 'function Wait-RamSharedRootOk') {
    throw "pnp_ready_before_ioctl: exhaustive harness has no bounded ROOT\RAMSHARED readiness wait"
}
if ($text -notmatch 'function Ensure-RamSharedRootDevice' -or
    $text -notmatch 'rootRecreateAfterReboot') {
    throw "root_recreate_after_reboot: exhaustive harness must recreate missing ROOT\RAMSHARED after post-deploy reboot"
}
if ($text -notmatch 'pnpReady') {
    throw "pnp_ready_before_ioctl: exhaustive harness does not record PnP readiness verdict"
}
if ($text -notmatch 'RamShared PnP gate failed before IOCTL pass1') {
    throw "pnp_ready_before_ioctl: pass1 does not fail before IOCTL when miniport is not ready"
}
if ($text -notmatch 'RamShared PnP gate failed before IOCTL verifier pass') {
    throw "pnp_ready_before_ioctl: verifier pass does not fail before IOCTL when miniport is not ready"
}
if ($text -notmatch 'pnputil /delete-driver \$publishedInf /uninstall /force') {
    throw "driverstore_purge: exhaustive harness must purge stale ramshared.inf packages before install"
}
if ($text -notmatch '\$load = Invoke-GuestBounded -TimeoutSec 240') {
    throw "load_timeout_budget: initial package install/load needs a budget that covers DriverStore purge plus PnP wait"
}

Write-Output "PASS Test-GuestExhaustiveStatic"
