#Requires -Version 5.1
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$target = Join-Path $PSScriptRoot "Repair-Win11Wsl2LabOfflineAccess.ps1"
if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
    throw "win11_wsl_offline_access: target script is missing"
}
$source = Get-Content -LiteralPath $target -Raw

function Import-RepairFunction([string]$Name) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile(
        $target, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "win11_wsl_offline_access: target parser errors"
    }
    $definition = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -ceq $Name
        }, $true)
    if ($null -eq $definition) {
        throw "win11_wsl_offline_access: missing production function $Name"
    }
    $body = $definition.Body.Extent.Text
    $body = $body.Substring(1, $body.Length - 2)
    Set-Item -Path ("Function:\script:{0}" -f $Name) `
        -Value ([scriptblock]::Create($body))
}

foreach ($needle in @(
    'ValidateSet("plan", "status", "repair", "restore")',
    'ExpectedVMId',
    'ExpectedVhdSha256',
    'ApproveGuestAccessRepair',
    'AllowBlankPasswordReset',
    'RollbackArtifact',
    'Get-VM',
    'Get-VMSnapshot',
    'Get-VMHardDiskDrive',
    'Get-VHD',
    'Get-FileHash',
    'Mount-OfflineVhdBounded -VhdPath $canonicalVhd',
    'Get-Disk -Number $diskNumber',
    'Dismount-OfflineVhdBounded',
    'SAM-original',
    'SAM-work',
    'SAM-after',
    'backup-manifest.json',
    'robocopy.exe',
    'sampasswd',
    'wsl.exe',
    'VHDX backup',
    'sam_original_sha256',
    'sam_reset_sha256',
    'sam_restore_sha256',
    'no_guest_or_vhd_mutation',
    'expected_vm_id_required',
    'expected_vhd_sha256_required',
    'vm_identity_mismatch',
    'vhd_hash_mismatch',
    'snapshot_residue',
    'DISK_MUTATION',
    'New-PrivateArtifactDir',
    'Set-PrivateArtifactAcl',
    'SetAccessRuleProtection($true, $false)',
    'Invoke-OfflineRepairBoundedChild',
    'offline_repair_child_deadline_exceeded',
    'HashDeadlineSeconds',
    'CredentialOperationTimeoutSeconds',
    'vhd_disk_identifier',
    'vhd_generation_drift',
    'sam_generation_drift',
    'Invoke-OfflineRepairBoundedPowerShell',
    'Mount-OfflineVhdBounded',
    'Dismount-OfflineVhdBounded',
    'offline_storage_operation_deadline_exceeded'
    'Observe-OfflineVhdBounded'
    'sam_mutation_intent_recorded'
    'sam_rollback_byte_verification_failed'
    'offline_vhd_observation_failed_after_mount'
    'mount_receipt_path'
    'offline_vhd_mount_receipt_preserves_dismount_ownership'
    'Stop-OfflineRepairProcessInstanceSafely'
    'Invoke-OfflineSamRestoreTransaction'
    'sam_restore_rollback_byte_verification_failed'
)) {
    if (-not $source.Contains($needle)) {
        throw "win11_wsl_offline_access: missing contract $needle"
    }
}

foreach ($forbidden in @(
    'Stop-VM -Name $VMName -TurnOff',
    'Stop-VM -Name $VMName -Force',
    'Get-Disk |',
    'reg.exe load',
    'reg.exe unload',
    'RamSharedBootstrap-',
    'RAMSHARED_OFFLINE_BOOTSTRAP',
    'AutoAdminLogon',
    'DefaultPassword',
    'DefaultUserName',
    'net accounts',
    'ConvertTo-Json.*Password',
    'Remove-Item -Recurse',
    'Clear-Disk',
    'Format-Volume',
    'Initialize-Disk',
    'Resize-VHD'
)) {
    if ($source.Contains($forbidden)) {
        throw "win11_wsl_offline_access: forbidden contract $forbidden"
    }
}

$approvalOffset = $source.IndexOf('if (-not $ApproveGuestAccessRepair)')
$mountOffset = $source.IndexOf('Mount-OfflineVhdBounded -VhdPath $canonicalVhd')
if ($approvalOffset -lt 0 -or $mountOffset -lt 0 -or $approvalOffset -ge $mountOffset) {
    throw "win11_wsl_offline_access: approval must be checked before VHD mount"
}

$mountedOwnershipOffset = $source.IndexOf('$mounted = [bool]$mount.dismount_required', $mountOffset)
$observeOffset = $source.IndexOf('Observe-OfflineVhdBounded -VhdPath $canonicalVhd', $mountOffset)
if ($mountedOwnershipOffset -lt 0 -or $observeOffset -lt 0 -or $mountedOwnershipOffset -ge $observeOffset) {
    throw 'win11_wsl_offline_access: a post-mount observation failure can bypass finally dismount ownership'
}

# R4-HOST-07 rerun: run the production mount helper against a child fixture
# that writes its post-mount receipt and then times out.  The return object
# must retain dismount ownership; both repair and restore call sites must
# acquire that ownership before their observation calls.
Import-RepairFunction "Test-OfflineMountReceipt"
Import-RepairFunction "Mount-OfflineVhdBounded"
Import-RepairFunction "Stop-OfflineRepairProcessInstanceSafely"
$script:CredentialOperationTimeoutSeconds = 30
$mountFixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ("ramshared-mounted-receipt-" + [guid]::NewGuid().ToString("N"))
try {
    New-Item -ItemType Directory -Path $mountFixtureRoot -ErrorAction Stop | Out-Null
    $fixtureVhd = Join-Path $mountFixtureRoot "fixture.vhdx"
    $fixtureReceipt = Join-Path $mountFixtureRoot "mount-receipt.json"
    function Invoke-OfflineRepairBoundedPowerShell {
        param([string]$Query, [int]$TimeoutSeconds, [hashtable]$Environment)
        $record = [ordered]@{ schema = 1; mounted = $true; vhd_path = [string]$Environment.RAMSHARED_OFFLINE_VHD }
        [IO.File]::WriteAllText([string]$Environment.RAMSHARED_MOUNT_RECEIPT, ($record | ConvertTo-Json -Compress), [Text.UTF8Encoding]::new($false))
        throw "offline_storage_operation_deadline_exceeded"
    }
    $mountOutcome = Mount-OfflineVhdBounded -VhdPath $fixtureVhd -MountReceiptPath $fixtureReceipt
    if (-not $mountOutcome.mounted -or -not $mountOutcome.dismount_required -or
        $mountOutcome.observation_complete -or $mountOutcome.reason -ne "offline_vhd_mount_receipt_preserves_dismount_ownership") {
        throw "win11_wsl_offline_access: mounted child timeout did not retain dismount ownership"
    }
    $repairMount = $source.IndexOf('$mount = Mount-OfflineVhdBounded -VhdPath $canonicalVhd -MountReceiptPath $mountReceiptPath')
    $repairObserve = $source.IndexOf('Observe-OfflineVhdBounded -VhdPath $canonicalVhd', $repairMount)
    $restoreTransaction = $source.IndexOf('function Invoke-OfflineSamRestoreTransaction')
    $restoreMount = $source.IndexOf('Mount-OfflineVhdBounded -VhdPath $CanonicalVhd', $restoreTransaction)
    $restoreObserve = $source.IndexOf('Observe-OfflineVhdBounded -VhdPath $CanonicalVhd', $restoreTransaction)
    $restoreCall = $source.IndexOf('Invoke-OfflineSamRestoreTransaction -CanonicalVhd $canonicalVhd', $repairMount + 1)
    foreach ($pair in @(@($repairMount, $repairObserve), @($restoreTransaction, $restoreMount), @($restoreMount, $restoreObserve), @($restoreCall, $source.Length))) {
        if ($pair[0] -lt 0 -or $pair[1] -lt 0 -or $pair[0] -ge $pair[1]) {
            throw "win11_wsl_offline_access: repair or restore does not acquire mount ownership before observation"
        }
    }
}
finally {
    Remove-Item -LiteralPath $mountFixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Output "PASS offline_repair_and_restore_retain_dismount_ownership_after_mount_child_timeout"

$offlinePidReuseModel = [pscustomobject]@{ HasExited = $false; StartTime = [DateTime]::UtcNow.AddMinutes(-1); Handle = [IntPtr]1; foreign_signal_count = 0 }
$offlinePidReuseModel | Add-Member -MemberType ScriptMethod -Name Refresh -Value { $this.StartTime = [DateTime]::UtcNow }
$offlinePidReuse = Stop-OfflineRepairProcessInstanceSafely -Process $offlinePidReuseModel
if ($offlinePidReuse.stopped -or $offlinePidReuse.reason -ne 'process_instance_identity_changed' -or $offlinePidReuseModel.foreign_signal_count -ne 0) {
    throw 'offline_repair_pid_reuse_never_signals_foreign_process failed: reused PID was not refused'
}
if ($source -match '(?m)^\s*(?:Stop-Process\s+-Id|taskkill\.exe)') {
    throw 'offline_repair_pid_reuse_never_signals_foreign_process failed: numeric PID termination remains'
}
Write-Output "PASS offline_repair_pid_reuse_never_signals_foreign_process"

# R4-HOST-05: invoke the real restore transaction with filesystem-only mocks.
# A deliberately corrupt copy after durable intent must roll back the exact
# pre-restore bytes, hash, and dismount ownership; it cannot report PARTIAL
# success.
Import-RepairFunction "Test-FileBytesEqual"
Import-RepairFunction "Invoke-OfflineSamRestoreTransaction"
$restoreFixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ("ramshared-restore-transaction-" + [guid]::NewGuid().ToString("N"))
try {
    $configDirectory = Join-Path $restoreFixtureRoot "config"
    $rollbackDirectory = Join-Path $restoreFixtureRoot "rollback"
    $restoreArtifact = Join-Path $restoreFixtureRoot "artifact"
    New-Item -ItemType Directory -Force -Path $configDirectory, $rollbackDirectory, $restoreArtifact | Out-Null
    $sourceSam = Join-Path $rollbackDirectory "SAM-original"
    $liveSam = Join-Path $configDirectory "SAM"
    [IO.File]::WriteAllText($sourceSam, "ORIGINAL", [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText($liveSam, "PRE-RESTORE", [Text.UTF8Encoding]::new($false))
    $script:restoreFixtureCopyCount = 0
    $script:restoreFixtureDismountCount = 0
    function Mount-OfflineVhdBounded { param($VhdPath, $MountReceiptPath) [pscustomobject]@{ dismount_required = $true; observation_complete = $true } }
    function Observe-OfflineVhdBounded { param($VhdPath) [pscustomobject]@{} }
    function Dismount-OfflineVhdBounded { param($VhdPath) $script:restoreFixtureDismountCount++ }
    function Get-WindowsPartitionPaths { param($CanonicalVhd) [pscustomobject]@{ config_directory = $configDirectory; sam_path = $liveSam } }
    function Assert-RollbackGenerationBinding { param($Preflight, $Manifest, $CurrentSamSha256) }
    function Get-Sha256 { param($Path) (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToUpperInvariant() }
    function Invoke-RobocopyFile {
        param($SourceDirectory, $DestinationDirectory, $FileName)
        $script:restoreFixtureCopyCount++
        New-Item -ItemType Directory -Force -Path $DestinationDirectory | Out-Null
        if ($script:restoreFixtureCopyCount -eq 2) {
            [IO.File]::WriteAllText((Join-Path $DestinationDirectory $FileName), "CORRUPTED", [Text.UTF8Encoding]::new($false))
            return
        }
        Copy-Item -LiteralPath (Join-Path $SourceDirectory $FileName) -Destination (Join-Path $DestinationDirectory $FileName) -Force
    }
    $restoreResult = Invoke-OfflineSamRestoreTransaction -CanonicalVhd (Join-Path $restoreFixtureRoot "fixture.vhdx") `
        -Preflight ([pscustomobject]@{ vm_id = "fixture-vm" }) -Manifest ([pscustomobject]@{}) `
        -SamOriginal $sourceSam -RollbackDirectory $rollbackDirectory -RestoreArtifact $restoreArtifact
    if ($restoreResult.ok -or -not $restoreResult.rollback_restored -or $restoreResult.reason -ne "sam_restore_hash_mismatch" -or
        -not (Test-FileBytesEqual -ExpectedPath (Join-Path $restoreArtifact "pre-restore\SAM") -ActualPath $liveSam) -or
        $script:restoreFixtureDismountCount -ne 1 -or -not (Test-Path -LiteralPath (Join-Path $restoreArtifact "restore-mutation-intent.json"))) {
        throw "offline_restore_failure_did_not_rollback_exact_pre_restore_snapshot"
    }
}
finally {
    Remove-Item -LiteralPath $restoreFixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Output "PASS offline_restore_copy_verification_failure_rolls_back_exact_snapshot_or_no_go"

Import-RepairFunction "Assert-RollbackGenerationBinding"
$generationFixture = [pscustomobject]@{
    vm_id = "76024A7E-9EA8-4D1B-8FA1-31572FDD6596"
    vhd_disk_identifier = "ACC90721-5CA4-4E95-A0B3-799EF3EBF7A3"
}
$manifestFixture = [pscustomobject]@{
    schema = 2
    vm_id = $generationFixture.vm_id
    vhd_disk_identifier = $generationFixture.vhd_disk_identifier
    sam_reset_sha256 = ("A" * 64)
}

Assert-RollbackGenerationBinding -Preflight $generationFixture -Manifest $manifestFixture `
    -CurrentSamSha256 ("A" * 64)
foreach ($driftCase in @(
        [pscustomobject]@{
            name = "vhd"; preflight = [pscustomobject]@{
                vm_id = $generationFixture.vm_id
                vhd_disk_identifier = "CB2F3C3A-676C-4859-B1B7-8CEDA9F7C0A7"
            }; sam_sha256 = ("A" * 64); reason = "vhd_generation_drift"
        },
        [pscustomobject]@{
            name = "sam"; preflight = $generationFixture; sam_sha256 = ("B" * 64)
            reason = "sam_generation_drift"
        })) {
    try {
        Assert-RollbackGenerationBinding -Preflight $driftCase.preflight `
            -Manifest $manifestFixture -CurrentSamSha256 $driftCase.sam_sha256
        throw "win11_wsl_offline_access: $($driftCase.name) generation drift was accepted"
    }
    catch {
        if ($_.Exception.Message -like "win11_wsl_offline_access:*") { throw }
        if ($_.Exception.Message -cne $driftCase.reason) {
            throw "win11_wsl_offline_access: wrong $($driftCase.name) drift refusal"
        }
    }
}
Write-Output "PASS offline_sam_restore_refuses_vhd_or_sam_generation_drift"

Import-RepairFunction "Set-PrivateArtifactAcl"
Import-RepairFunction "New-PrivateArtifactDir"
$privateArtifactRoot = Join-Path ([IO.Path]::GetTempPath()) `
    ("ramshared-private-sam-static-" + [guid]::NewGuid().ToString("N"))
try {
    New-Item -ItemType Directory -Path $privateArtifactRoot -ErrorAction Stop | Out-Null
    $privateArtifact = New-PrivateArtifactDir -Root $privateArtifactRoot -Prefix "manufactured-sam"
    $privateAcl = Get-Acl -LiteralPath $privateArtifact -ErrorAction Stop
    if (-not $privateAcl.AreAccessRulesProtected) {
        throw "offline_sam_artifacts_are_private failed: inherited artifact ACL was retained"
    }
}
finally {
    Remove-Item -LiteralPath $privateArtifactRoot -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Output "PASS offline_sam_artifacts_are_private"

Write-Output "PASS win11_wsl_offline_access_is_plan_first_exact_and_reversible"
Write-Output "PASS offline_sam_copy_mount_restore_and_verify_are_child_deadline_bounded"
Write-Output "PASS offline_sam_marks_mutation_intent_then_byte_verifies_rollback"
Write-Output "PASS offline_vhd_mount_is_dismounted_when_observation_fails"
