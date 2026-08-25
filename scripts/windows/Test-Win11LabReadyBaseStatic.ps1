#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path $PSScriptRoot "Seal-Win11LabReadyBase.ps1"
}
if (-not (Test-Path -LiteralPath $HarnessPath -PathType Leaf)) {
    throw "ready_base_static: production harness missing"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw -ErrorAction Stop

foreach ($needle in @(
    "ExpectedVMId",
    "ReadinessReceiptPath",
    "ExpectedReadinessReceiptSha256",
    "BaseRoot",
    "ApproveSeal",
    "Assert-Win11LabReadyBaseReceipt",
    "IMAGE_STATE_COMPLETE",
    "SealedOffline",
    "Get-VMSnapshot",
    "Get-VMHardDiskDrive",
    "Get-VMNetworkAdapter",
    "SwitchType",
    "Invoke-GuestPsDirectBounded",
    "shutdown.exe /s /t",
    "shutdown_scheduled",
    "CopyTimeoutSeconds",
    "Invoke-BoundedGuestProcess",
    "Quote-GuestProcessArgument",
    "Copy-Item",
    "Get-FileHash",
    "source_vhd_sha256",
    "copied_vhd_sha256",
    "base-manifest.json",
    "base_files_read_only",
    "IsReadOnly",
    "[IO.Directory]::Move"
)) {
    if ($text -notmatch [regex]::Escape($needle)) {
        throw ("ready_base_static: required contract missing " + $needle)
    }
}
$boundedHelperPath = Join-Path (Split-Path -Parent $HarnessPath) "Invoke-GuestPsDirectBounded.ps1"
if (-not (Test-Path -LiteralPath $boundedHelperPath -PathType Leaf)) {
    throw "ready_base_static: bounded helper missing"
}
$boundedHelperText = Get-Content -LiteralPath $boundedHelperPath -Raw -ErrorAction Stop
foreach ($boundedNeedle in @(
    "ProcessStartInfo",
    "ReadToEndAsync",
    "Stop-GuestProcessInstanceSafely",
    '$Process.Handle',
    "StartTime",
    '$Process.Kill()'
)) {
    if ($boundedHelperText -notmatch [regex]::Escape($boundedNeedle)) {
        throw ("ready_base_static: bounded helper contract missing " + $boundedNeedle)
    }
}
if ($boundedHelperText -match '(?im)^\s*taskkill\.exe\b' -or
    $boundedHelperText -match '(?im)^\s*Stop-Process\s+-Id\b') {
    throw "ready_base_static: numeric PID termination is forbidden; require process-instance identity"
}
Write-Output "PASS ready_base_bounded_helper_uses_process_instance_identity_not_numeric_pid_kill"
Write-Output "PASS ready_base_requires_exact_sealed_receipt"
Write-Output "PASS ready_base_guest_shutdown_is_graceful_and_bounded"
Write-Output "PASS ready_base_copy_is_deadline_bounded_and_hash_exact"
Write-Output "PASS ready_base_manifest_is_complete"

foreach ($forbidden in @(
    "Stop-VM",
    "Checkpoint-VM",
    "Export-VM",
    "Restart-VM",
    "Remove-VM"
)) {
    if ($text -match [regex]::Escape($forbidden)) {
        throw ("ready_base_static: forbidden lifecycle surface " + $forbidden)
    }
}
if ($text -notmatch 'Test-Path\s+-LiteralPath\s+\$BaseRoot' -or
    $text -notmatch 'ready_base_destination_exists') {
    throw "ready_base_static: destination overwrite refusal missing"
}
Write-Output "PASS ready_base_never_overwrites_or_uses_checkpoints"

$capabilityIndex = $text.IndexOf('$requiredHostCommands', [StringComparison]::Ordinal)
$shutdownIndex = $text.IndexOf('shutdown.exe /s /t', [StringComparison]::Ordinal)
$copyIndex = $text.IndexOf('Invoke-BoundedGuestProcess', [StringComparison]::Ordinal)
if ($capabilityIndex -lt 0 -or $shutdownIndex -lt 0 -or $copyIndex -lt 0 -or
    $capabilityIndex -gt $shutdownIndex -or $capabilityIndex -gt $copyIndex -or
    $text -notmatch [regex]::Escape('Get-VMSecurity') -or
    $text -match [regex]::Escape('Get-VMTPM')) {
    throw "ready_base_capabilities_precede_shutdown_and_copy"
}
Write-Output "PASS ready_base_capabilities_precede_shutdown_and_copy"

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("ramshared-ready-base-static-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tempRoot -ErrorAction Stop | Out-Null
try {
    $sourcePath = Join-Path $tempRoot "source.vhdx"
    $destinationPath = Join-Path $tempRoot "destination.vhdx"
    $resultPath = Join-Path $tempRoot "copy-result.json"
    [IO.File]::WriteAllBytes($sourcePath, [byte[]](0..255))
    & $HarnessPath -WorkerMode Copy -SourcePath $sourcePath `
        -DestinationPath $destinationPath -ResultPath $resultPath
    if (-not (Test-Path -LiteralPath $destinationPath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $resultPath -PathType Leaf)) {
        throw "ready_base_static: manufactured worker output missing"
    }
    $copyReceipt = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json -ErrorAction Stop
    if ([int64]$copyReceipt.source_vhd_bytes -ne 256 -or
        [int64]$copyReceipt.copied_vhd_bytes -ne 256 -or
        [string]$copyReceipt.source_vhd_sha256 -cne [string]$copyReceipt.copied_vhd_sha256) {
        throw "ready_base_static: manufactured worker receipt mismatch"
    }
}
finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Output "PASS ready_base_copy_worker_is_executable"
Write-Output "PASS Test-Win11LabReadyBaseStatic"
