#Requires -Version 5.1
<#
.SYNOPSIS
  Recover access to the approved disposable Windows WSL lab.

.DESCRIPTION
  `plan` is the default and only reads Hyper-V/VHDX identity. `repair` makes a
  VHDX backup of the exact offline SAM hive, uses the already-installed Linux
  `sampasswd` tool to reset only the sealed lab account to a blank password,
  and verifies the copied-back SAM hash. It never persists a replacement
  password. `restore` writes back the exact recorded SAM backup if the bounded
  blank-password recovery cannot be completed. The VM is never started,
  stopped, checkpointed, or force-powered off by this script.
#>
[CmdletBinding()]
param(
    [ValidateSet("plan", "status", "repair", "restore")]
    [string]$Action = "plan",
    [string]$VMName = "win11-wsl2-lab",
    [string]$VhdPath = "C:\ramshared-hyperv\win11-wsl2-lab\Virtual Hard Disks\win11-wsl2-lab.vhdx",
    [string]$ExpectedVMId = "",
    [string]$ExpectedVhdSha256 = "",
    [ValidatePattern('^[A-Za-z0-9._-]{1,20}$')]
    [string]$LabUser = "drilladmin",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [string]$RollbackArtifact = "",
    [ValidateRange(30, 1800)]
    [int]$HashDeadlineSeconds = 600,
    [ValidateRange(15, 600)]
    [int]$CredentialOperationTimeoutSeconds = 180,
    [switch]$Run,
    [switch]$ApproveGuestAccessRepair,
    [switch]$AllowBlankPasswordReset
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Quote-OfflineRepairChildArgument {
    param([string]$Value)
    if ($Value -notmatch '[\s"]') { return $Value }
    '"' + ($Value -replace '(\\*)"', '$1$1\\"' -replace '(\\+)$', '$1$1') + '"'
}

function Stop-OfflineRepairProcessInstanceSafely {
    param([Parameter(Mandatory = $true)][object]$Process)
    # Bind the original process handle before creation-time revalidation. The
    # Process object's Kill() uses that handle, avoiding a reused numeric PID.
    try {
        $Process.Refresh()
        if ($Process.HasExited) { return [pscustomobject]@{ stopped = $true; reason = "process_already_exited" } }
        $boundHandle = $Process.Handle
        $originalStart = $Process.StartTime.ToUniversalTime().Ticks
        $Process.Refresh()
        if ($Process.HasExited) { return [pscustomobject]@{ stopped = $true; reason = "process_already_exited" } }
        if ($Process.StartTime.ToUniversalTime().Ticks -ne $originalStart) {
            return [pscustomobject]@{ stopped = $false; reason = "process_instance_identity_changed" }
        }
        $Process.Kill()
        if (-not $Process.WaitForExit(5000)) { return [pscustomobject]@{ stopped = $false; reason = "process_instance_kill_unreaped" } }
        return [pscustomobject]@{ stopped = $true; reason = "process_instance_handle_terminated" }
    } catch {
        return [pscustomobject]@{ stopped = $false; reason = "process_instance_identity_unproven" }
    }
}

function Invoke-OfflineRepairBoundedChild {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [Parameter(Mandatory = $true)]
        [string]$Arguments,
        [Parameter(Mandatory = $true)]
        [ValidateRange(1, 1800)]
        [int]$TimeoutSeconds,
        [hashtable]$Environment = @{}
    )

    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $FilePath
    $info.Arguments = $Arguments
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($key in @($Environment.Keys)) {
        $info.EnvironmentVariables[[string]$key] = [string]$Environment[$key]
    }

    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) { throw "offline_repair_child_start_failed" }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $completed = $process.WaitForExit($TimeoutSeconds * 1000)
        if (-not $completed) {
            $stopped = Stop-OfflineRepairProcessInstanceSafely -Process $process
            if (-not $stopped.stopped -or -not $process.WaitForExit(5000)) {
                throw "offline_repair_child_termination_unresolved:$($stopped.reason)"
            }
        }
        if (-not [Threading.Tasks.Task]::WaitAll(
                [Threading.Tasks.Task[]]@($stdoutTask, $stderrTask), 5000)) {
            throw "offline_repair_child_stream_drain_failed"
        }
        if (-not $completed) { throw "offline_repair_child_deadline_exceeded" }
        [pscustomobject]@{
            completed = [bool]$completed
            exit_code = [int]$process.ExitCode
            stdout = [string]$stdoutTask.Result
        }
    }
    finally {
        $process.Dispose()
    }
}

function Get-OfflineRepairPowerShellPath {
    $path = Join-Path $PSHOME "powershell.exe"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "offline_repair_powerShell_host_unavailable"
    }
    return $path
}

function Invoke-OfflineRepairBoundedPowerShell {
    param(
        [Parameter(Mandatory = $true)][string]$Query,
        [Parameter(Mandatory = $true)][ValidateRange(1, 1800)][int]$TimeoutSeconds,
        [hashtable]$Environment = @{}
    )
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes(
            '$ErrorActionPreference = "Stop"; ' + $Query))
    try {
        $execution = Invoke-OfflineRepairBoundedChild -FilePath (Get-OfflineRepairPowerShellPath) `
            -Arguments ("-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand " + $encoded) `
            -TimeoutSeconds $TimeoutSeconds -Environment $Environment
    } catch {
        if ($_.Exception.Message -eq "offline_repair_child_deadline_exceeded") {
            throw "offline_storage_operation_deadline_exceeded"
        }
        throw
    }
    if (-not $execution.completed) { throw "offline_storage_operation_deadline_exceeded" }
    if ($execution.exit_code -ne 0) { throw "offline_storage_operation_failed" }
    try { return ($execution.stdout | ConvertFrom-Json -ErrorAction Stop) }
    catch { throw "offline_storage_operation_output_invalid" }
}

function Test-OfflineMountReceipt {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$VhdPath
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $false }
    try {
        $receipt = Get-Content -LiteralPath $Path -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        return [int]$receipt.schema -eq 1 -and [bool]$receipt.mounted -and
            [string]$receipt.vhd_path -ceq $VhdPath
    } catch {
        return $false
    }
}

function Mount-OfflineVhdBounded {
    param(
        [Parameter(Mandatory = $true)][string]$VhdPath,
        [Parameter(Mandatory = $true)][string]$MountReceiptPath
    )
    if (Test-Path -LiteralPath $MountReceiptPath) { throw "offline_vhd_mount_receipt_path_already_exists" }
    # The child records ownership immediately after Mount-VHD and before it
    # serializes output.  If its output/drain/deadline subsequently fails, the
    # caller still owns a dismount attempt and preserves this receipt as
    # forensic evidence.
    try {
        Invoke-OfflineRepairBoundedPowerShell -TimeoutSeconds $CredentialOperationTimeoutSeconds `
            -Environment @{ RAMSHARED_OFFLINE_VHD = $VhdPath; RAMSHARED_MOUNT_RECEIPT = $MountReceiptPath } `
            -Query '$ErrorActionPreference = "Stop"; Mount-VHD -Path $env:RAMSHARED_OFFLINE_VHD -Passthru -ErrorAction Stop | Out-Null; $receipt = [ordered]@{ schema = 1; mounted = $true; vhd_path = $env:RAMSHARED_OFFLINE_VHD; recorded_utc = [DateTime]::UtcNow.ToString("o") }; $temporary = $env:RAMSHARED_MOUNT_RECEIPT + ".tmp"; [IO.File]::WriteAllText($temporary, ($receipt | ConvertTo-Json -Compress), [Text.Encoding]::UTF8); Move-Item -LiteralPath $temporary -Destination $env:RAMSHARED_MOUNT_RECEIPT -ErrorAction Stop; [pscustomobject]@{ mounted = $true } | ConvertTo-Json -Compress' | Out-Null
        if (-not (Test-OfflineMountReceipt -Path $MountReceiptPath -VhdPath $VhdPath)) {
            return [pscustomobject]@{ mounted = $false; dismount_required = $true; observation_complete = $false; reason = "offline_vhd_mount_receipt_missing_after_mount" }
        }
        return [pscustomobject]@{ mounted = $true; dismount_required = $true; observation_complete = $true; reason = "mounted" }
    } catch {
        if (Test-OfflineMountReceipt -Path $MountReceiptPath -VhdPath $VhdPath) {
            return [pscustomobject]@{ mounted = $true; dismount_required = $true; observation_complete = $false; reason = "offline_vhd_mount_receipt_preserves_dismount_ownership" }
        }
        # The child may have mounted immediately before an unobservable
        # provider/serialization failure.  A bounded dismount attempt is safer
        # than leaving an unknown attachment; report the uncertainty terminally.
        return [pscustomobject]@{ mounted = $false; dismount_required = $true; observation_complete = $false; reason = "offline_vhd_mount_outcome_unknown" }
    }
}

function Observe-OfflineVhdBounded {
    param([Parameter(Mandatory = $true)][string]$VhdPath)
    return Invoke-OfflineRepairBoundedPowerShell -TimeoutSeconds $CredentialOperationTimeoutSeconds `
        -Environment @{ RAMSHARED_OFFLINE_VHD = $VhdPath } `
        -Query 'Get-VHD -Path $env:RAMSHARED_OFFLINE_VHD -ErrorAction Stop | Select-Object DiskNumber, Attached | ConvertTo-Json -Compress'
}

function Dismount-OfflineVhdBounded {
    param([Parameter(Mandatory = $true)][string]$VhdPath)
    Invoke-OfflineRepairBoundedPowerShell -TimeoutSeconds $CredentialOperationTimeoutSeconds `
        -Environment @{ RAMSHARED_OFFLINE_VHD = $VhdPath } `
        -Query 'Dismount-VHD -Path $env:RAMSHARED_OFFLINE_VHD -ErrorAction Stop; [pscustomobject]@{ dismounted = $true } | ConvertTo-Json -Compress' | Out-Null
}

function Get-Sha256 {
    param([string]$Path)
    $worker = '$ErrorActionPreference = "Stop"; (Get-FileHash -LiteralPath $env:RAMSHARED_HASH_PATH -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()'
    $encodedWorker = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($worker))
    $execution = Invoke-OfflineRepairBoundedChild -FilePath (Get-OfflineRepairPowerShellPath) `
        -Arguments ("-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand " + $encodedWorker) `
        -TimeoutSeconds $HashDeadlineSeconds -Environment @{ RAMSHARED_HASH_PATH = $Path }
    if (-not $execution.completed) { throw "offline_repair_hash_deadline_exceeded" }
    if ($execution.exit_code -ne 0) { throw "offline_repair_hash_failed" }
    $hash = ([string]$execution.stdout).Trim()
    if ($hash -notmatch '^[0-9A-Fa-f]{64}$') { throw "offline_repair_hash_output_invalid" }
    return $hash.ToUpperInvariant()
}

function Test-FileBytesEqual {
    param([Parameter(Mandatory = $true)][string]$ExpectedPath, [Parameter(Mandatory = $true)][string]$ActualPath)
    $expected = [IO.File]::ReadAllBytes($ExpectedPath)
    $actual = [IO.File]::ReadAllBytes($ActualPath)
    if ($expected.Length -ne $actual.Length) { return $false }
    for ($index = 0; $index -lt $expected.Length; $index++) {
        if ($expected[$index] -ne $actual[$index]) { return $false }
    }
    return $true
}

function Invoke-OfflineSamRestoreTransaction {
    param(
        [Parameter(Mandatory = $true)][string]$CanonicalVhd,
        [Parameter(Mandatory = $true)][object]$Preflight,
        [Parameter(Mandatory = $true)][object]$Manifest,
        [Parameter(Mandatory = $true)][string]$SamOriginal,
        [Parameter(Mandatory = $true)][string]$RollbackDirectory,
        [Parameter(Mandatory = $true)][string]$RestoreArtifact
    )
    $mounted = $false
    $mountReceiptPath = Join-Path $RestoreArtifact "mount-receipt.json"
    $paths = $null
    $preRestoreSam = $null
    $restoreMutationIntent = $false
    $rollbackRestored = $false
    $failure = ""
    $result = $null
    try {
        $mount = Mount-OfflineVhdBounded -VhdPath $CanonicalVhd -MountReceiptPath $mountReceiptPath
        $mounted = [bool]$mount.dismount_required
        if (-not $mount.observation_complete) { throw [string]$mount.reason }
        $null = Observe-OfflineVhdBounded -VhdPath $CanonicalVhd
        $paths = Get-WindowsPartitionPaths -CanonicalVhd $CanonicalVhd
        Assert-RollbackGenerationBinding -Preflight $Preflight -Manifest $Manifest `
            -CurrentSamSha256 (Get-Sha256 -Path $paths.sam_path)

        # Snapshot the exact known pre-restore SAM before recording a durable
        # intent or changing the offline hive. That snapshot is rollback
        # authority if the copy or verification phase fails.
        $preRestoreDirectory = Join-Path $RestoreArtifact "pre-restore"
        New-Item -ItemType Directory -Path $preRestoreDirectory -ErrorAction Stop | Out-Null
        Invoke-RobocopyFile -SourceDirectory $paths.config_directory -DestinationDirectory $preRestoreDirectory -FileName "SAM"
        $preRestoreSam = Join-Path $preRestoreDirectory "SAM"
        $preRestoreSha256 = Get-Sha256 -Path $preRestoreSam
        if (-not (Test-FileBytesEqual -ExpectedPath $preRestoreSam -ActualPath $paths.sam_path)) {
            throw "sam_restore_pre_snapshot_byte_verification_failed"
        }

        [ordered]@{
            schema = 2
            status = "sam_restore_mutation_intent_recorded"
            source_sha256 = Get-Sha256 -Path $SamOriginal
            pre_restore_sha256 = $preRestoreSha256
            vm_id = $Preflight.vm_id
            rollback_artifact = $RollbackDirectory
        } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $RestoreArtifact "restore-mutation-intent.json") -Encoding UTF8
        $restoreMutationIntent = $true

        $restoreStage = Join-Path $RestoreArtifact "work"
        New-Item -ItemType Directory -Path $restoreStage -ErrorAction Stop | Out-Null
        Copy-Item -LiteralPath $SamOriginal -Destination (Join-Path $restoreStage "SAM") -ErrorAction Stop
        Invoke-RobocopyFile -SourceDirectory $restoreStage -DestinationDirectory $paths.config_directory -FileName "SAM"
        $afterDirectory = Join-Path $RestoreArtifact "after"
        Invoke-RobocopyFile -SourceDirectory $paths.config_directory -DestinationDirectory $afterDirectory -FileName "SAM"
        $samAfter = Join-Path $afterDirectory "SAM"
        if ((Get-Sha256 -Path $samAfter) -cne (Get-Sha256 -Path $SamOriginal) -or
            -not (Test-FileBytesEqual -ExpectedPath $SamOriginal -ActualPath $samAfter)) {
            throw "sam_restore_hash_mismatch"
        }
        [ordered]@{
            schema = 1
            sam_restore_sha256 = Get-Sha256 -Path $samAfter
            rollback_artifact = $RollbackDirectory
        } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $RestoreArtifact "restore-manifest.json") -Encoding UTF8
        $result = [pscustomobject]@{ ok = $true; reason = "sam_restored"; rollback_restored = $false; artifact = $RestoreArtifact }
    }
    catch {
        $failure = [string]$_.Exception.Message
        if ($restoreMutationIntent -and $null -ne $paths -and $null -ne $preRestoreSam -and
            (Test-Path -LiteralPath $preRestoreSam -PathType Leaf)) {
            try {
                $rollbackStage = Join-Path $RestoreArtifact "rollback-work"
                New-Item -ItemType Directory -Path $rollbackStage -ErrorAction Stop | Out-Null
                Copy-Item -LiteralPath $preRestoreSam -Destination (Join-Path $rollbackStage "SAM") -ErrorAction Stop
                Invoke-RobocopyFile -SourceDirectory $rollbackStage -DestinationDirectory $paths.config_directory -FileName "SAM"
                $rollbackAfter = Join-Path $RestoreArtifact "rollback-after"
                Invoke-RobocopyFile -SourceDirectory $paths.config_directory -DestinationDirectory $rollbackAfter -FileName "SAM"
                $rollbackSam = Join-Path $rollbackAfter "SAM"
                if ((Get-Sha256 -Path $rollbackSam) -cne (Get-Sha256 -Path $preRestoreSam) -or
                    -not (Test-FileBytesEqual -ExpectedPath $preRestoreSam -ActualPath $rollbackSam)) {
                    throw "sam_restore_rollback_byte_verification_failed"
                }
                $rollbackRestored = $true
            } catch {
                $rollbackRestored = $false
                $failure = "sam_restore_rollback_byte_verification_failed"
            }
        }
        $result = [pscustomobject]@{ ok = $false; reason = $failure; rollback_restored = $rollbackRestored; artifact = $RestoreArtifact }
    }
    finally {
        if ($mounted) {
            try { Dismount-OfflineVhdBounded -VhdPath $CanonicalVhd }
            catch {
                if ($null -eq $result -or $result.ok) {
                    $result = [pscustomobject]@{ ok = $false; reason = "vhd_dismount_failed"; rollback_restored = $rollbackRestored; artifact = $RestoreArtifact }
                }
            }
        }
    }
    return $result
}

function Get-CanonicalVhdPath {
    param([string]$Path)
    $canonical = [IO.Path]::GetFullPath($Path)
    $approvedRoot = "C:\ramshared-hyperv\win11-wsl2-lab\"
    if (-not $canonical.StartsWith($approvedRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw "vhd_path_outside_approved_lab"
    }
    if ([IO.Path]::GetExtension($canonical) -cne ".vhdx") {
        throw "vhd_extension_invalid"
    }
    if (-not (Test-Path -LiteralPath $canonical -PathType Leaf)) {
        throw "vhd_missing"
    }
    return $canonical
}

function Get-LabPreflight {
    param([string]$Name, [string]$CanonicalVhd)
    if ($Name -cne "win11-wsl2-lab") { throw "vm_name_not_approved" }
    $vmRows = @(Get-VM -Name $Name -ErrorAction Stop)
    if ($vmRows.Count -ne 1 -or [string]$vmRows[0].Name -cne $Name) {
        throw "vm_identity_ambiguous"
    }
    $vm = $vmRows[0]
    $snapshots = @(Get-VMSnapshot -VMName $Name -ErrorAction Stop)
    $vmDisks = @(Get-VMHardDiskDrive -VMName $Name -ErrorAction Stop)
    $vhd = Get-VHD -Path $CanonicalVhd -ErrorAction Stop
    $vhdDiskIdentifier = ([guid]$vhd.DiskIdentifier).ToString("D").ToUpperInvariant()
    $vmDiskMatches = $false
    if ($vmDisks.Count -eq 1 -and -not [string]::IsNullOrWhiteSpace([string]$vmDisks[0].Path)) {
        $vmDiskMatches = [IO.Path]::GetFullPath([string]$vmDisks[0].Path) -ceq $CanonicalVhd
    }
    [pscustomobject]@{
        vm_name = [string]$vm.Name
        vm_id = ([guid]$vm.Id).ToString("D").ToUpperInvariant()
        vm_state = [string]$vm.State
        vm_generation = [int]$vm.Generation
        automatic_checkpoints_enabled = [bool]$vm.AutomaticCheckpointsEnabled
        checkpoint_type = [string]$vm.CheckpointType
        snapshot_count = [int]$snapshots.Count
        vm_disk_count = [int]$vmDisks.Count
        vm_disk_matches = [bool]$vmDiskMatches
        vhd_attached = [bool]$vhd.Attached
        vhd_size_bytes = [Int64]$vhd.Size
        vhd_file_size_bytes = [Int64]$vhd.FileSize
        vhd_disk_identifier = $vhdDiskIdentifier
        vhd_sha256 = Get-Sha256 -Path $CanonicalVhd
    }
}

function Assert-RollbackGenerationBinding {
    param([object]$Preflight, [object]$Manifest, [string]$CurrentSamSha256)
    if ([int]$Manifest.schema -ne 2 -or
        [string]$Manifest.vm_id -cne [string]$Preflight.vm_id) {
        throw "rollback_artifact_identity_mismatch"
    }
    if ([string]$Manifest.vhd_disk_identifier -cne [string]$Preflight.vhd_disk_identifier) {
        throw "vhd_generation_drift"
    }
    if ([string]$Manifest.sam_reset_sha256 -notmatch '^[0-9A-F]{64}$' -or
        $CurrentSamSha256 -cne [string]$Manifest.sam_reset_sha256) {
        throw "sam_generation_drift"
    }
}

function Assert-RepairPreflight {
    param([object]$Preflight, [switch]$RequireExpectedVhd)
    if ([string]::IsNullOrWhiteSpace($ExpectedVMId)) { throw "expected_vm_id_required" }
    $expectedVmCanonical = ([guid]$ExpectedVMId).ToString("D").ToUpperInvariant()
    if ($Preflight.vm_id -cne $expectedVmCanonical) { throw "vm_identity_mismatch" }
    if ($RequireExpectedVhd) {
        if ([string]::IsNullOrWhiteSpace($ExpectedVhdSha256)) { throw "expected_vhd_sha256_required" }
        $expectedVhdCanonical = $ExpectedVhdSha256.Trim().ToUpperInvariant()
        if ($expectedVhdCanonical -notmatch '^[0-9A-F]{64}$') { throw "expected_vhd_sha256_invalid" }
        if ($Preflight.vhd_sha256 -cne $expectedVhdCanonical) { throw "vhd_hash_mismatch" }
    }
    if ($Preflight.vm_state -cne "Off") { throw "vm_must_be_off" }
    if ($Preflight.vm_generation -ne 2 -or $Preflight.snapshot_count -ne 0 -or
        $Preflight.automatic_checkpoints_enabled -or $Preflight.checkpoint_type -cne "Disabled") {
        throw "snapshot_residue"
    }
    if ($Preflight.vm_disk_count -ne 1 -or -not $Preflight.vm_disk_matches -or $Preflight.vhd_attached) {
        throw "vhd_identity_or_attachment_invalid"
    }
}

function Write-TypedResult {
    param([string]$Status, [string]$Reason, [hashtable]$Extra = @{}, [bool]$DiskMutation = $false)
    $record = [ordered]@{
        schema = 2
        status = $Status
        reason = $Reason
        action = $Action
        vm_name = $VMName
    }
    $record["DISK_MUTATION"] = if ($DiskMutation) { $true } else { $false }
    foreach ($key in @($Extra.Keys)) { $record[$key] = $Extra[$key] }
    $record | ConvertTo-Json -Depth 8
}

function Set-PrivateArtifactAcl {
    param([string]$Path)
    $currentSid = [Security.Principal.WindowsIdentity]::GetCurrent().User
    if ($null -eq $currentSid) { throw "artifact_acl_identity_unavailable" }
    $systemSid = New-Object Security.Principal.SecurityIdentifier("S-1-5-18")
    $acl = Get-Acl -LiteralPath $Path -ErrorAction Stop
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($sid in @($currentSid, $systemSid)) {
        $rule = New-Object Security.AccessControl.FileSystemAccessRule(
            $sid, "FullControl", "ContainerInherit,ObjectInherit", "None", "Allow")
        [void]$acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $Path -AclObject $acl -ErrorAction Stop
    if (-not (Get-Acl -LiteralPath $Path -ErrorAction Stop).AreAccessRulesProtected) {
        throw "artifact_acl_not_private"
    }
}

function Assert-PrivateArtifactAcl {
    param([string]$Path)
    if (-not (Get-Acl -LiteralPath $Path -ErrorAction Stop).AreAccessRulesProtected) {
        throw "rollback_artifact_not_private"
    }
}

function New-PrivateArtifactDir {
    param([string]$Root, [string]$Prefix)
    $directory = Join-Path $Root ($Prefix + "-" + [guid]::NewGuid().ToString("N"))
    if (Test-Path -LiteralPath $directory) { throw "artifact_directory_already_exists" }
    New-Item -ItemType Directory -Path $directory -ErrorAction Stop | Out-Null
    Set-PrivateArtifactAcl -Path $directory
    return $directory
}

function Invoke-RobocopyFile {
    param([string]$SourceDirectory, [string]$DestinationDirectory, [string]$FileName)
    New-Item -ItemType Directory -Path $DestinationDirectory -Force -ErrorAction Stop | Out-Null
    $arguments = @($SourceDirectory, $DestinationDirectory, $FileName, "/B", "/COPY:DAT", "/R:0", "/W:0", "/NFL", "/NDL", "/NJH", "/NJS", "/NP") |
        ForEach-Object { Quote-OfflineRepairChildArgument ([string]$_) }
    try {
        $execution = Invoke-OfflineRepairBoundedChild -FilePath "robocopy.exe" -Arguments ($arguments -join " ") `
            -TimeoutSeconds $CredentialOperationTimeoutSeconds
    } catch {
        if ($_.Exception.Message -eq "offline_repair_child_deadline_exceeded") { throw "offline_storage_operation_deadline_exceeded" }
        throw
    }
    if (-not $execution.completed) { throw "offline_storage_operation_deadline_exceeded" }
    if ($execution.exit_code -ge 8) { throw "robocopy_file_failed" }
}

function Convert-ToHostWslPath {
    param([string]$WindowsPath)
    $canonical = [IO.Path]::GetFullPath($WindowsPath)
    $prefix = "C:\"
    if (-not $canonical.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "host_wsl_path_requires_c_drive"
    }
    return "/mnt/c/" + (($canonical.Substring($prefix.Length)) -replace '\\', '/')
}

function Invoke-SamPasswordReset {
    param([string]$SamPath, [string]$User)
    $wslExe = "C:\Program Files\WSL\wsl.exe"
    if (-not (Test-Path -LiteralPath $wslExe -PathType Leaf)) {
        $wslExe = Join-Path $env:SystemRoot "System32\wsl.exe"
    }
    $linuxPath = Convert-ToHostWslPath -WindowsPath $SamPath
    $arguments = @("-d", "Ubuntu-24.04", "-u", "root", "--", "/usr/sbin/sampasswd", "-r", "-u", $User, "-N", "-E", $linuxPath) |
        ForEach-Object { Quote-OfflineRepairChildArgument ([string]$_) }
    $execution = Invoke-OfflineRepairBoundedChild -FilePath $wslExe `
        -Arguments ($arguments -join " ") -TimeoutSeconds $CredentialOperationTimeoutSeconds
    if (-not $execution.completed) { throw "sampasswd_reset_deadline_exceeded" }
    if ($execution.exit_code -ne 0) { throw "sampasswd_reset_failed" }
}

function Get-WindowsPartitionPaths {
    param([string]$CanonicalVhd)
    $paths = Invoke-OfflineRepairBoundedPowerShell -TimeoutSeconds $CredentialOperationTimeoutSeconds `
        -Environment @{ RAMSHARED_OFFLINE_VHD = $CanonicalVhd } `
        -Query '$mountedVhd = Get-VHD -Path $env:RAMSHARED_OFFLINE_VHD -ErrorAction Stop; $diskNumber = [int]$mountedVhd.DiskNumber; if ($diskNumber -lt 0) { throw "mounted_vhd_has_no_disk_number" }; $disk = Get-Disk -Number $diskNumber -ErrorAction Stop; if ($null -eq $disk) { throw "mounted_vhd_disk_not_found" }; $windowsPartitions = @(Get-Partition -DiskNumber $diskNumber -ErrorAction Stop | Where-Object { $_.DriveLetter -and (Test-Path -LiteralPath ("$($_.DriveLetter):\Windows\System32\config\SAM")) }); if ($windowsPartitions.Count -ne 1) { throw "offline_windows_partition_ambiguous" }; [pscustomobject]@{ config_directory = "$($windowsPartitions[0].DriveLetter):\Windows\System32\config" } | ConvertTo-Json -Compress'
    $configDirectory = [string]$paths.config_directory
    if ([string]::IsNullOrWhiteSpace($configDirectory)) { throw "offline_windows_partition_ambiguous" }
    return [pscustomobject]@{
        config_directory = $configDirectory
        sam_path = Join-Path $configDirectory "SAM"
    }
}

function Get-CanonicalRollbackArtifact {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) { throw "rollback_artifact_required" }
    $canonical = [IO.Path]::GetFullPath($Path)
    $root = "C:\ramshared\artifacts\"
    if (-not $canonical.StartsWith($root, [StringComparison]::OrdinalIgnoreCase) -or
        -not (Test-Path -LiteralPath $canonical -PathType Container)) {
        throw "rollback_artifact_invalid"
    }
    Assert-PrivateArtifactAcl -Path $canonical
    return $canonical
}

$canonicalVhd = ""
try {
    $canonicalVhd = Get-CanonicalVhdPath -Path $VhdPath
    $preflight = Get-LabPreflight -Name $VMName -CanonicalVhd $canonicalVhd
}
catch {
    Write-TypedResult -Status "PARTIAL" -Reason "host_preflight_failed"
    exit 2
}

if ($Action -ceq "status") {
    Write-TypedResult -Status "STATUS" -Reason "host_observation_complete" -Extra @{ preflight = $preflight }
    exit 0
}
if ($Action -ceq "plan" -or (($Action -cin @("repair", "restore")) -and -not $Run)) {
    Write-TypedResult -Status "PLAN" -Reason "no_guest_or_vhd_mutation" -Extra @{
        preflight = $preflight
        requires_run = $true
        requires_approval = "ApproveGuestAccessRepair"
        repair_requires_blank_password_approval = $true
        repair_requires_expected_vhd_sha256 = $true
        restore_requires_rollback_artifact = $true
    }
    exit 0
}
if (-not $ApproveGuestAccessRepair) {
    Write-TypedResult -Status "PARTIAL" -Reason "guest_access_repair_approval_required"
    exit 2
}

if ($Action -ceq "repair") {
    if (-not $AllowBlankPasswordReset) {
        Write-TypedResult -Status "PARTIAL" -Reason "blank_password_reset_approval_required"
        exit 2
    }
    try {
        Assert-RepairPreflight -Preflight $preflight -RequireExpectedVhd
    }
    catch {
        Write-TypedResult -Status "PARTIAL" -Reason ([string]$_.Exception.Message)
        exit 2
    }

    $artifactDir = New-PrivateArtifactDir -Root $ArtifactRoot -Prefix "win11-wsl2-lab-sam-repair"
    $backupDirectory = Join-Path $artifactDir "backup"
    # Keep the mutable copy visibly separate from the immutable SAM-original.
    $workDirectory = Join-Path $artifactDir "SAM-work"
    $afterDirectory = Join-Path $artifactDir "after"
    $samOriginal = Join-Path $artifactDir "SAM-original"
    $samWork = Join-Path $workDirectory "SAM"
    $samAfter = Join-Path $artifactDir "SAM-after"
    $mounted = $false
    $mountReceiptPath = Join-Path $artifactDir "mount-receipt.json"
    $guestSamMutationIntent = $false
    $guestSamChanged = $false
    $committed = $false
    $rollbackRestored = $false
    $failureReason = ""
    $paths = $null

    try {
        $mount = Mount-OfflineVhdBounded -VhdPath $canonicalVhd -MountReceiptPath $mountReceiptPath
        # offline_vhd_observation_failed_after_mount: from this point forward
        # finally owns a known mounted handle even if a storage/provider
        # observation hangs or fails.
        $mounted = [bool]$mount.dismount_required
        if (-not $mount.observation_complete) { throw [string]$mount.reason }
        $null = Observe-OfflineVhdBounded -VhdPath $canonicalVhd
        $paths = Get-WindowsPartitionPaths -CanonicalVhd $canonicalVhd
        Invoke-RobocopyFile -SourceDirectory $paths.config_directory -DestinationDirectory $backupDirectory -FileName "SAM"
        Move-Item -LiteralPath (Join-Path $backupDirectory "SAM") -Destination $samOriginal -ErrorAction Stop
        $originalHash = Get-Sha256 -Path $samOriginal
        New-Item -ItemType Directory -Path $workDirectory -ErrorAction Stop | Out-Null
        Copy-Item -LiteralPath $samOriginal -Destination $samWork -ErrorAction Stop
        Invoke-SamPasswordReset -SamPath $samWork -User $LabUser
        $resetHash = Get-Sha256 -Path $samWork
        if ($resetHash -ceq $originalHash) { throw "sam_reset_made_no_change" }
        [ordered]@{
            schema = 2
            kind = "VHDX backup"
            vm_id = $preflight.vm_id
            pre_repair_vhd_sha256 = $preflight.vhd_sha256
            vhd_disk_identifier = $preflight.vhd_disk_identifier
            sam_original_sha256 = $originalHash
            sam_reset_sha256 = $resetHash
            lab_user = $LabUser
            blank_password_reset = $true
        } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $artifactDir "backup-manifest.json") -Encoding UTF8
        # Intent is durable before copyback: a deadline/nonzero after a partial
        # robocopy is treated as a possible guest SAM mutation and rolls back.
        [ordered]@{
            schema = 1; status = "sam_mutation_intent_recorded"; vm_id = $preflight.vm_id
            vhd_disk_identifier = $preflight.vhd_disk_identifier; sam_original_sha256 = $originalHash; sam_reset_sha256 = $resetHash
        } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $artifactDir "sam-mutation-intent.json") -Encoding UTF8
        $guestSamMutationIntent = $true
        $guestSamChanged = $true
        Invoke-RobocopyFile -SourceDirectory $workDirectory -DestinationDirectory $paths.config_directory -FileName "SAM"
        Invoke-RobocopyFile -SourceDirectory $paths.config_directory -DestinationDirectory $afterDirectory -FileName "SAM"
        Move-Item -LiteralPath (Join-Path $afterDirectory "SAM") -Destination $samAfter -ErrorAction Stop
        if ((Get-Sha256 -Path $samAfter) -cne $resetHash) { throw "sam_copyback_hash_mismatch" }
        $committed = $true
    }
    catch {
        $failureReason = [string]$_.Exception.Message
    }
    finally {
        if (-not $committed -and $guestSamMutationIntent -and $guestSamChanged -and $null -ne $paths -and
            (Test-Path -LiteralPath $samOriginal -PathType Leaf)) {
            try {
                $restoreDirectory = Join-Path $artifactDir "restore"
                New-Item -ItemType Directory -Path $restoreDirectory -ErrorAction Stop | Out-Null
                Copy-Item -LiteralPath $samOriginal -Destination (Join-Path $restoreDirectory "SAM") -ErrorAction Stop
                Invoke-RobocopyFile -SourceDirectory $restoreDirectory -DestinationDirectory $paths.config_directory -FileName "SAM"
                $rollbackAfterDirectory = Join-Path $artifactDir "rollback-after"
                Invoke-RobocopyFile -SourceDirectory $paths.config_directory -DestinationDirectory $rollbackAfterDirectory -FileName "SAM"
                $rollbackSam = Join-Path $rollbackAfterDirectory "SAM"
                if ((Get-Sha256 -Path $rollbackSam) -cne $originalHash -or
                    -not (Test-FileBytesEqual -ExpectedPath $samOriginal -ActualPath $rollbackSam)) {
                    throw "sam_rollback_byte_verification_failed"
                }
                $rollbackRestored = $true
            }
            catch {
                $rollbackRestored = $false
                $failureReason = "sam_rollback_byte_verification_failed"
            }
        }
        if ($mounted) {
            try { Dismount-OfflineVhdBounded -VhdPath $canonicalVhd }
            catch { if ([string]::IsNullOrWhiteSpace($failureReason)) { $failureReason = "vhd_dismount_failed" } }
        }
    }

    if (-not $committed -or -not [string]::IsNullOrWhiteSpace($failureReason)) {
        Write-TypedResult -Status "PARTIAL" -Reason $failureReason -DiskMutation $true -Extra @{
            artifact = $artifactDir
            rollback_sam_restored = [bool]$rollbackRestored
        } | Set-Content -LiteralPath (Join-Path $artifactDir "summary.json") -Encoding UTF8
        Get-Content -LiteralPath (Join-Path $artifactDir "summary.json") -Raw
        exit 2
    }
    Write-TypedResult -Status "PASS" -Reason "sam_blank_password_reset" -DiskMutation $true -Extra @{
        artifact = $artifactDir
        vm_id = $preflight.vm_id
        sam_original_sha256 = Get-Sha256 -Path $samOriginal
        sam_reset_sha256 = Get-Sha256 -Path $samWork
        next_action = "Use the explicitly permitted blank-password probe, then restore the original password inside the guest or run restore."
    } | Set-Content -LiteralPath (Join-Path $artifactDir "summary.json") -Encoding UTF8
    Get-Content -LiteralPath (Join-Path $artifactDir "summary.json") -Raw
    exit 0
}

try {
    Assert-RepairPreflight -Preflight $preflight
    $rollbackDirectory = Get-CanonicalRollbackArtifact -Path $RollbackArtifact
    $manifestPath = Join-Path $rollbackDirectory "backup-manifest.json"
    $samOriginal = Join-Path $rollbackDirectory "SAM-original"
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $samOriginal -PathType Leaf)) {
        throw "rollback_artifact_incomplete"
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ([int]$manifest.schema -ne 2 -or [string]$manifest.vm_id -cne $preflight.vm_id -or
        [string]$manifest.sam_original_sha256 -cne (Get-Sha256 -Path $samOriginal)) {
        throw "rollback_artifact_identity_mismatch"
    }
    if ([string]$manifest.vhd_disk_identifier -cne $preflight.vhd_disk_identifier) {
        throw "vhd_generation_drift"
    }
}
catch {
    Write-TypedResult -Status "PARTIAL" -Reason ([string]$_.Exception.Message)
    exit 2
}

$restoreArtifact = New-PrivateArtifactDir -Root $ArtifactRoot -Prefix "win11-wsl2-lab-sam-restore"
$restoreResult = Invoke-OfflineSamRestoreTransaction -CanonicalVhd $canonicalVhd -Preflight $preflight `
    -Manifest $manifest -SamOriginal $samOriginal -RollbackDirectory $rollbackDirectory -RestoreArtifact $restoreArtifact
if (-not $restoreResult.ok) {
    # An attempted restore that cannot prove either the target bytes or its
    # rollback is terminally unsafe, never a misleading partial success.
    Write-TypedResult -Status "NO_GO" -Reason $restoreResult.reason -DiskMutation $true -Extra @{
        artifact = $restoreResult.artifact
        rollback_sam_restored = [bool]$restoreResult.rollback_restored
        recovery_evidence_preserved = $true
    }
    exit 2
}
Write-TypedResult -Status "PASS" -Reason "sam_restored" -DiskMutation $true -Extra @{ artifact = $restoreResult.artifact }
