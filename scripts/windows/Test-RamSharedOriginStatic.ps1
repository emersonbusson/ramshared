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
    'ValidateSet("plan", "install", "configure", "status", "uninstall", "test")',
    'Get-WslDistroStorageRoot',
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
    'Write-Error -ErrorId "ServiceNotStopped"',
    'New-VHD',
    'Mount-VHD',
    'Dismount-VHD -Path $VhdxPath',
    'Initialize-Disk',
    'Windows assigns the GPT PARTUUID',
    'Get-Partition -DiskNumber',
    'Set-Partition',
    'Get-OriginVhdxOwnershipProof',
    'Test-CanonicalOriginGuid',
    'Get-DiskImage',
    '$OriginVhdx -ieq $ExistingSwapVhdx',
    'if ($Action -eq "plan" -or (-not $Run -and $Action -ne "status" -and $Action -ne "test"))'
)) {
    if (-not $source.Contains($required)) {
        throw "ramshared_origin: missing contract $required"
    }
}
foreach ($forbidden in @('Clear-Disk', 'Remove-Partition', 'Remove-Item -Recurse', 'Get-Disk |')) {
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
)) {
    if (-not ($manufactured -join "`n").Contains($required)) {
        throw "ramshared_origin: manufactured output missing $required"
    }
}
