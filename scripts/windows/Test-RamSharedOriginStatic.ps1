#Requires -Version 5.1
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$target = Join-Path $PSScriptRoot "Manage-RamSharedOrigin.ps1"
if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
    throw "ramshared_origin: target script is missing"
}
$source = Get-Content -Raw -LiteralPath $target
foreach ($required in @(
    'ValidateSet("plan", "install", "configure", "status", "uninstall", "attach", "test")',
    'Get-WslDistroStorageRoot',
    'Do not infer distro storage from the independent WSL swap VHDX path.',
    'Source = "c_default"',
    '$volumes.Count -ne 1',
    '[IO.DriveType]::Fixed',
    '@("NTFS", "ReFS")',
    'Assert-OriginPathFreeSpace -Path $OriginVhdx',
    'Purpose "before explicit origin allocation"',
    'source = "manufactured_test"',
    'Get-ConfiguredWslSwapVhdxPath',
    'OriginVhdxPath',
    'ExistingSwapVhdxPath',
    '25GB',
    'PARTUUID',
    'ramshared-origin-manifest.json',
    'configuration_sha256',
    'disk_guid',
    'expected_swap_uuid',
    'logical_capacity_mib',
    'physical_cache_cap_mib',
    'chunk_mib',
    'gpu_reserve_min_mib',
    'gpu_reserve_percent',
    'SHA256',
    'ramshared-origin-backup',
    'New-OriginInstallTransaction',
    'Rollback-OriginInstallTransaction',
    'New-OriginUninstallTransaction',
    'Rollback-OriginUninstallTransaction',
    'origin uninstall transaction failed; restored authority',
    'origin install transaction failed; rolled back only current-run artifacts',
    'origin VHDX already exists; refuse replacement',
    'sealed origin manifest already exists; refuse replacement',
    'Remove-Item -LiteralPath $transaction.staging_vhdx -Force',
    'Remove-Item -LiteralPath $OriginVhdx -Force',
    'origin uninstall requires exact sealed ownership proof',
    'New-VHD',
    'Mount-VHD',
    'Dismount-VHD -Path $VhdxPath',
    'Initialize-Disk',
    'Windows assigns the GPT PARTUUID',
    'Get-Partition -DiskNumber',
    'Set-Partition',
    'Get-OriginVhdxOwnershipProof',
    'Get-OriginAttachmentDecision',
    'Invoke-OriginGuestPartUuidProbe',
    'Invoke-OriginBoundedProcess',
    'origin attach requires a whitespace-free sealed VHDX path',
    'origin attach requires an administrator token',
    'Test-CanonicalOriginGuid',
    'Get-DiskImage',
    '$OriginVhdx -ieq $ExistingSwapVhdx',
    'if ($Action -eq "plan" -or (-not $Run -and $Action -ne "status" -and $Action -ne "test"))'
)) {
    if (-not $source.Contains($required)) {
        throw "ramshared_origin: missing contract $required"
    }
}
foreach ($forbidden in @('Clear-Disk', 'Remove-Partition', 'Remove-Item -Recurse', 'Get-Disk |', '--shutdown', '--unmount', 'configured_swap_volume', '$swapRoot')) {
    if ($source.Contains($forbidden)) {
        throw "ramshared_origin: forbidden storage action $forbidden"
    }
}

$installStart = $source.IndexOf('"install" {')
$configureStart = $source.IndexOf('"configure" {')
if ($installStart -lt 0 -or $configureStart -le $installStart) {
    throw "ramshared_origin: install/configure branch boundaries are missing"
}
$installText = $source.Substring($installStart, $configureStart - $installStart)
if ($installText -notmatch 'try\s*\{' -or $installText -notmatch 'finally\s*\{' -or
    $installText -notmatch 'Dismount-VHD\s+-Path\s+\$transaction\.staging_vhdx') {
    throw "ramshared_origin: provisioned VHDX must be detached in an install finally block"
}
$preflightIndex = $installText.IndexOf('Purpose "before fixed origin allocation"')
$transactionIndex = $installText.IndexOf('New-OriginInstallTransaction')
$postAllocationIndex = $installText.IndexOf('Purpose "post-allocation origin reserve"')
$proofIndex = $installText.IndexOf('Get-OriginVhdxOwnershipProof -VhdxPath $transaction.staging_vhdx')
$promotionIndex = $installText.IndexOf('$transaction.origin_promoted = $true')
$manifestIndex = $installText.IndexOf('Write-OriginManifest')
if ($preflightIndex -lt 0 -or $transactionIndex -le $preflightIndex -or
    $postAllocationIndex -lt 0 -or $proofIndex -le $postAllocationIndex -or
    $promotionIndex -le $postAllocationIndex -or $manifestIndex -le $postAllocationIndex) {
    throw "ramshared_origin: reserve checks must bracket staging allocation before proof, promotion, and manifest publication"
}
$resolutionStart = $source.IndexOf('$requestedOriginPath =')
$testModeIndex = $source.IndexOf('if ($Action -eq "test") {', $resolutionStart)
$manifestReadIndex = $source.IndexOf('$sealedOriginPath = Get-SealedOriginVhdxPath', $resolutionStart)
$explicitReserveIndex = $source.IndexOf('Purpose "before explicit origin allocation"', $resolutionStart)
$explicitSelectionIndex = $source.IndexOf('source = "explicit_path"', $resolutionStart)
if ($resolutionStart -lt 0 -or $testModeIndex -le $resolutionStart -or
    $manifestReadIndex -le $testModeIndex -or $explicitReserveIndex -lt 0 -or
    $explicitSelectionIndex -le $explicitReserveIndex) {
    throw "ramshared_origin: tests must bypass live host discovery and explicit plans must prove volume headroom"
}
Write-Output "PASS origin_test_mode_skips_live_host_discovery"
foreach ($required in @(
    'New-OriginInstallTransaction',
    'Rollback-OriginInstallTransaction -Transaction $transaction',
    '$transaction.expected_proof = $proof',
    '$transaction.origin_promoted = $true',
    '$transaction.manifest_written = $true',
    'Move-Item -LiteralPath $transaction.staging_vhdx -Destination $OriginVhdx'
)) {
    if (-not $installText.Contains($required)) {
        throw "ramshared_origin: install transaction lacks $required"
    }
}

$configureEnd = $source.IndexOf('"uninstall" {', $configureStart)
if ($configureEnd -le $configureStart) {
    throw "ramshared_origin: configure/uninstall branch boundaries are missing"
}
$configureText = $source.Substring($configureStart, $configureEnd - $configureStart)
foreach ($required in @(
    'configure does not accept a caller PARTUUID',
    'Get-OriginVhdxOwnershipProof',
    'Read-SealedOriginManifest',
    'PARTUUID ownership proof does not match the sealed origin VHDX',
    'disk GUID ownership proof does not match the sealed origin VHDX'
)) {
    if (-not $configureText.Contains($required)) {
        throw "ramshared_origin: configure lacks VHDX-bound ownership proof $required"
    }
}
if ($configureText -match 'Write-OriginManifest\s*\r?\n') {
    throw "ramshared_origin: configure must not rewrite a manifest from caller-supplied identity"
}

$uninstallStart = $source.IndexOf('"uninstall" {', $configureEnd)
if ($uninstallStart -lt 0) { throw "ramshared_origin: uninstall branch is missing" }
$uninstallText = $source.Substring($uninstallStart)
foreach ($required in @(
    'Read-SealedOriginManifest',
    'Get-OriginVhdxOwnershipProof -VhdxPath $OriginVhdx',
    'Test-OriginProofMatchesManifest',
    'origin uninstall requires exact sealed ownership proof',
    'Invoke-OriginUninstallTransaction -Manifest $manifest'
)) {
    if (-not $uninstallText.Contains($required)) {
        throw "ramshared_origin: uninstall lacks exact-owned cleanup $required"
    }
}

$powershell = Join-Path $PSHOME "powershell.exe"
if (-not (Test-Path -LiteralPath $powershell -PathType Leaf)) { $powershell = "powershell.exe" }
$manufactured = @(& $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $target -Action test -Run 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw ("ramshared_origin: manufactured transaction cases failed: " + ($manufactured -join "`n"))
}
foreach ($required in @(
    'PASS origin_plan_is_separate_fixed_and_identity_bound',
    'PASS foreign_or_unproven_partuuid_is_rejected',
    'PASS origin_install_failure_rolls_back_current_run_only',
    'PASS origin_preexisting_or_foreign_vhdx_is_never_removed',
    'PASS origin_uninstall_requires_exact_sealed_ownership'
    'PASS canonical_vhdx_guid_and_partuuid_are_accepted',
    'PASS malformed_or_foreign_origin_identity_is_refused'
    'PASS origin_uninstall_failure_restores_vhdx_and_manifest_authority'
    'PASS origin_attach_decision_is_idempotent_and_fail_closed',
    'PASS origin_volume_prefers_distro_volume_with_reserve',
    'PASS origin_volume_falls_back_to_c_when_preferred_lacks_reserve',
    'PASS origin_single_volume_c_satisfies_default',
    'PASS origin_volume_refuses_when_all_candidates_below_reserve',
    'PASS origin_volume_rejects_removable_and_unsupported_filesystem',
    'PASS origin_existing_manifest_path_survives_distro_volume_change',
    'PASS origin_existing_manifest_override_mismatch_is_refused',
    'PASS origin_install_rechecks_post_create_reserve_before_manifest'
)) {
    if (-not ($manufactured -join "`n").Contains($required)) {
        throw "ramshared_origin: manufactured output missing $required"
    }
}
