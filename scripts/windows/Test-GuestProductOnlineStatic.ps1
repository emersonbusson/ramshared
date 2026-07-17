#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Run-GuestProductOnline.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

$requiredVerdictFields = @(
    "CONSOLE_EXIT_ZERO",
    "NO_FORCE_KILL",
    "LEASE_RELEASED",
    "CUDA_RESTORED",
    "NO_NEW_DUMP",
    "TEARDOWN_WITHIN_BUDGET",
    "TERMINAL_SAFE"
)
foreach ($field in $requiredVerdictFields) {
    if ($text -notmatch [regex]::Escape($field)) {
        throw ("verdict_requires_complete_graceful_stop: missing " + $field)
    }
}
if ($text -match "preStopLockProbe" -or $text -match "Pre-stop exclusive lock probe") {
    throw "pre_stop_probe_is_absent: harness still mutates/locks before product stop"
}
# Default product Online is three fresh lifecycle rounds. Manufactured pagefile
# refuse may use a single round via \$lifecycleRounds override only.
if ($text -notmatch '\$lifecycleRounds = 3') {
    throw "three_fresh_rounds_are_required: default lifecycleRounds must be 3"
}
if ($text -notmatch 'for \(\$campaignRound = 1; \$campaignRound -le \$lifecycleRounds; \$campaignRound\+\+\)') {
    throw "three_fresh_rounds_are_required: campaign loop must iterate lifecycleRounds"
}
if ($text -notmatch 'ManufacturedPagefileRefuse') {
    throw "manufactured_pagefile_refuse: harness missing ManufacturedPagefileRefuse switch"
}
if ($text -notmatch 'PAGEFILE_REFUSE_PASS') {
    throw "manufactured_pagefile_refuse: summary missing PAGEFILE_REFUSE_PASS"
}
if ($text -notmatch 'if \(-not \$summary\.PASS\)') {
    throw "verdict_requires_complete_graceful_stop: exit gate does not consume complete PASS"
}
$enableIdx = $text.IndexOf('pnputil /enable-device')
$startIdx = $text.IndexOf('sc.exe start ramshared')
if ($enableIdx -lt 0 -or $startIdx -lt 0 -or $enableIdx -gt $startIdx) {
    throw "pnp_adapter_enabled_before_create: harness starts ramshared before ROOT\RAMSHARED is enabled"
}
if ($text -notmatch 'ramshared PnP not OK before service start') {
    throw "pnp_adapter_enabled_before_create: harness does not fail closed on disabled PnP adapter"
}
if ($text -notmatch 'Driver package added successfully') {
    throw "pnputil_idempotent_install: package already installed/up-to-date is still treated as fatal"
}
if ($text -notmatch 'Device is already enabled') {
    throw "pnputil_idempotent_enable: already-enabled ROOT\RAMSHARED is still treated as fatal"
}
if ($text -notmatch 'driver_store_binary_mismatch') {
    throw "binary_match_gate: harness does not abort before product start on DriverStore/package mismatch"
}
if ($text -notmatch 'function Get-RamSharedPublishedInf') {
    throw "driverstore_purge: harness cannot enumerate stale ramshared.inf DriverStore packages"
}
if ($text -notmatch 'pnputil /delete-driver \$publishedInf /uninstall /force') {
    throw "driverstore_purge: harness does not purge stale ramshared.inf DriverStore packages before install"
}
$purgeIdx = $text.IndexOf('pnputil /delete-driver $publishedInf /uninstall /force')
$installIdx = $text.IndexOf('pnputil /add-driver C:\ramshared\package\ramshared.inf /install')
if ($purgeIdx -lt 0 -or $installIdx -lt 0 -or $purgeIdx -gt $installIdx) {
    throw "driverstore_purge: stale DriverStore purge must run before add-driver"
}
if ($text -match 'Compare-Object \$dumpBefore \$dumpAfter') {
    throw "dump_inventory_empty_safe: empty dump inventory still reaches Compare-Object null binding"
}
$waitExitIdx = $text.IndexOf('$p.WaitForExit()')
$rawExitIdx = $text.IndexOf('try { $rawExit = $p.ExitCode }')
if ($waitExitIdx -lt 0 -or $rawExitIdx -lt 0 -or $waitExitIdx -gt $rawExitIdx) {
    throw "console_exit_capture: harness must wait for process exit before reading ExitCode"
}
if ($text -notmatch 'consoleExitSource = "runtime_summary"' -or $text -notmatch 'console stopped: RuntimeSummary \.\*exit_code: 0') {
    throw "console_exit_capture: harness must fail closed but accept product RuntimeSummary exit_code fallback"
}
if ($text -notmatch '\$cudaRestoreWatch = \[Diagnostics\.Stopwatch\]::StartNew\(\)' -or
    $text -notmatch 'cudaRestoreSamples') {
    throw "cuda_restore_gate: harness must poll CUDA free memory before declaring restoration failure"
}
if ($text -match 'pnputil /disable-device') {
    throw "root_device_cleanup: product harness must not leave ROOT\RAMSHARED disabled"
}
if ($text -notmatch 'pnputil /remove-device' -or $text -notmatch 'RamSharedRootEnum') {
    throw "root_device_cleanup: product harness must remove stale root devices and recreate via SetupAPI"
}

Write-Output "PASS Test-GuestProductOnlineStatic"
