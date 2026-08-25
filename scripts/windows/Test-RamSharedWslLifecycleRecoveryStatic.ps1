#Requires -Version 5.1
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$target = Join-Path $PSScriptRoot "Recover-RamSharedWslLifecycle.ps1"
if (-not (Test-Path -LiteralPath $target -PathType Leaf)) { throw "lifecycle_recovery_source_missing" }
$source = Get-Content -Raw -LiteralPath $target

foreach ($required in @(
    'ValidateSet("plan", "status", "recover", "test")',
    'lifecycle-recovery',
    'RAMSHARED_NBD_CONTROLLER_APPROVAL=recover:',
    'cascade-controller.sh',
    'systemctl stop --no-block',
    'lifecycle-recovery-status.sh',
    'swapoff_or_detach_not_proven',
    'recover:$Distro',
    'Stop-BoundProcessInstance',
    'Get-RecoveryDecision'
)) {
    if (-not $source.Contains($required)) { throw "lifecycle_recovery_contract_missing: $required" }
}
foreach ($forbidden in @('wsl.exe --terminate', 'wsl.exe --shutdown', '--terminate', '--shutdown', 'Restart-Computer', 'Stop-Computer', 'mkswap', 'nbd-client')) {
    if ($source.Contains($forbidden)) { throw "lifecycle_recovery_forbidden_host_action: $forbidden" }
}
if ($source -match '(?m)^\s*&?\s*swapoff\b') { throw "lifecycle_recovery_may_not_run_swapoff_on_host" }
if ($source -match '\$process\.WaitForExit\(\)') { throw "lifecycle_recovery_unbounded_wait_for_exit" }

$powershell = Join-Path $PSHOME "powershell.exe"
if (-not (Test-Path -LiteralPath $powershell -PathType Leaf)) { $powershell = "powershell.exe" }
$manufactured = @(& $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $target -Action test 2>&1)
if ($LASTEXITCODE -ne 0) { throw ("lifecycle_recovery_manufactured_failed: " + ($manufactured -join "`n")) }
foreach ($required in @(
    'PASS lifecycle_recovery_accepts_only_exact_sealed_marker',
    'PASS lifecycle_recovery_never_starts_or_terminates_stopped_distro',
    'PASS lifecycle_recovery_serializes_behind_active_controller'
)) {
    if (-not (($manufactured -join "`n").Contains($required))) { throw "lifecycle_recovery_manufactured_output_missing: $required" }
}
Write-Output "PASS host_lifecycle_recovery_is_plan_first_bounded_and_fail_closed"
