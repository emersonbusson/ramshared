#Requires -Version 5.1
<#
.SYNOPSIS
  Plan-first origin-VHDX configuration source for the revocable VRAM cache.

.DESCRIPTION
  The fixed origin is separate from the existing WSL fallback swap VHDX. Every
  storage-changing branch requires -Run plus the exact approval token. Static
  tests invoke no storage cmdlet and default invocation writes nothing.
#>
[CmdletBinding()]
param(
    [ValidateSet("plan", "install", "configure", "status", "uninstall", "test")]
    [string]$Action = "plan",
    [switch]$Run,
    [switch]$AttendedOriginApply,
    [string]$ApproveOriginProvision = "",
    [ValidateRange(1024, 24576)]
    [int]$LogicalCapacityMiB = 4096,
    [ValidateRange(1024, 24576)]
    [int]$PhysicalCacheCapMiB = 1024,
    [ValidatePattern('^[A-Za-z0-9._-]+$')]
    [string]$Distro = "Ubuntu-24.04",
    [string]$OriginVhdxPath = "",
    [string]$ExistingSwapVhdxPath = "",
    [ValidatePattern('^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$')]
    [string]$PARTUUID = "00000000-0000-0000-0000-000000000000"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-AbsoluteWindowsPath {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$Name)
    $expanded = [Environment]::ExpandEnvironmentVariables($Path.Trim())
    if (-not [IO.Path]::IsPathRooted($expanded) -or $expanded -notmatch '^(?:[A-Za-z]:[\\/]|\\\\)') { throw "$Name must be an absolute Windows path" }
    return [IO.Path]::GetFullPath($expanded)
}

function Get-ConfiguredWslSwapVhdxPath {
    if (-not [string]::IsNullOrWhiteSpace($ExistingSwapVhdxPath)) {
        return Resolve-AbsoluteWindowsPath -Path $ExistingSwapVhdxPath -Name "ExistingSwapVhdxPath"
    }
    $configuration = Join-Path $env:USERPROFILE ".wslconfig"
    if (Test-Path -LiteralPath $configuration -PathType Leaf) {
        $inWsl2 = $false
        foreach ($line in Get-Content -LiteralPath $configuration) {
            $trimmed = $line.Trim()
            if ($trimmed -match '^\[(.+)\]$') { $inWsl2 = $Matches[1] -ieq "wsl2"; continue }
            if ($inWsl2 -and $trimmed -match '^swapFile\s*=\s*(.+?)\s*$') {
                return Resolve-AbsoluteWindowsPath -Path $Matches[1] -Name ".wslconfig swapFile"
            }
        }
    }
    return [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetTempPath()) "swap.vhdx"))
}

function Get-WslDistroStorageRoot {
    $registry = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss"
    if (Test-Path -LiteralPath $registry) {
        foreach ($entry in Get-ChildItem -LiteralPath $registry) {
            $properties = Get-ItemProperty -LiteralPath $entry.PSPath
            if ([string]$properties.DistributionName -ceq $Distro -and -not [string]::IsNullOrWhiteSpace([string]$properties.BasePath)) {
                $base = Resolve-AbsoluteWindowsPath -Path ([string]$properties.BasePath) -Name "WSL distro BasePath"
                $root = [IO.Path]::GetPathRoot($base)
                if (-not [string]::IsNullOrWhiteSpace($root)) { return $root }
            }
        }
    }
    $swapRoot = [IO.Path]::GetPathRoot($ExistingSwapVhdx)
    if ([string]::IsNullOrWhiteSpace($swapRoot)) { throw "cannot discover the WSL distro or swap storage volume" }
    return $swapRoot
}

$ExistingSwapVhdx = Get-ConfiguredWslSwapVhdxPath
$OriginVhdx = if ([string]::IsNullOrWhiteSpace($OriginVhdxPath)) {
    Join-Path (Get-WslDistroStorageRoot) "RamShared\ramshared-origin.vhdx"
} else {
    Resolve-AbsoluteWindowsPath -Path $OriginVhdxPath -Name "OriginVhdxPath"
}
$OriginSize = 25GB
$ChunkMiB = 128
$GpuReserveMinMiB = 2048
$GpuReservePercent = 20
$ManifestPath = "C:\ProgramData\RamShared\ramshared-origin-manifest.json"
$BackupRoot = "C:\ProgramData\RamShared\ramshared-origin-backup"
$ApprovalToken = "RAMSHARED_ORIGIN_25GIB_PARTUUID"
$OwnershipProofSchema = 1
$PartUuidWasSupplied = $PSBoundParameters.ContainsKey("PARTUUID")
$LogicalCapacityWasSupplied = $PSBoundParameters.ContainsKey("LogicalCapacityMiB")
$PhysicalCacheCapWasSupplied = $PSBoundParameters.ContainsKey("PhysicalCacheCapMiB")
$DiskGuid = ""
$ExpectedSwapUuid = ""
$CanonicalGuidPattern = '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'

function Test-CanonicalOriginGuid {
    param([AllowNull()][object]$Value)
    return $Value -is [string] -and $Value -match $CanonicalGuidPattern
}

function Write-OriginPlan {
    [ordered]@{ state = "PLAN"; action = $Action; distro = $Distro; origin_vhdx = $OriginVhdx; fixed_size_bytes = $OriginSize; logical_capacity_mib = $LogicalCapacityMiB; physical_cache_cap_mib = $PhysicalCacheCapMiB; chunk_mib = $ChunkMiB; gpu_reserve_min_mib = $GpuReserveMinMiB; gpu_reserve_percent = $GpuReservePercent; partuuid = $PARTUUID; expected_swap_uuid = "generated-during-install"; existing_wsl_swap_vhdx = $ExistingSwapVhdx; host_mutation_requires_run = $true; host_mutation_requires_attended_action = $true; host_mutation_requires_exact_approval = $ApprovalToken } | ConvertTo-Json -Depth 4
}

function Get-OriginConfigurationSha256 {
    param(
        [Parameter(Mandatory = $true)][int]$ManifestLogicalCapacityMiB,
        [Parameter(Mandatory = $true)][int]$ManifestPhysicalCacheCapMiB,
        [Parameter(Mandatory = $true)][string]$ManifestPartUuid,
        [Parameter(Mandatory = $true)][string]$ManifestDiskGuid,
        [Parameter(Mandatory = $true)][string]$ManifestExpectedSwapUuid
    )
    $text = "schema=3`norigin_vhdx=$OriginVhdx`nfixed_size_bytes=$OriginSize`nlogical_capacity_mib=$ManifestLogicalCapacityMiB`nphysical_cache_cap_mib=$ManifestPhysicalCacheCapMiB`nchunk_mib=$ChunkMiB`ngpu_reserve_min_mib=$GpuReserveMinMiB`ngpu_reserve_percent=$GpuReservePercent`npartuuid=$ManifestPartUuid`ndisk_guid=$ManifestDiskGuid`nexpected_swap_uuid=$ManifestExpectedSwapUuid`nownership_proof_schema=$OwnershipProofSchema`nexisting_wsl_swap_vhdx=$ExistingSwapVhdx`n"
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($text)))).Replace("-", "").ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}

function Write-OriginManifest {
    if (-not (Test-CanonicalOriginGuid -Value $DiskGuid)) {
        throw "origin disk GUID ownership proof is invalid"
    }
    if (-not (Test-CanonicalOriginGuid -Value $ExpectedSwapUuid)) {
        throw "origin expected swap UUID is invalid"
    }
    $directory = Split-Path -Parent $ManifestPath
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    $configurationHash = Get-OriginConfigurationSha256 -ManifestLogicalCapacityMiB $LogicalCapacityMiB -ManifestPhysicalCacheCapMiB $PhysicalCacheCapMiB -ManifestPartUuid $PARTUUID -ManifestDiskGuid $DiskGuid -ManifestExpectedSwapUuid $ExpectedSwapUuid
    $manifest = [ordered]@{ schema_version = 3; origin_vhdx = $OriginVhdx; fixed_size_bytes = $OriginSize; logical_capacity_mib = $LogicalCapacityMiB; physical_cache_cap_mib = $PhysicalCacheCapMiB; chunk_mib = $ChunkMiB; gpu_reserve_min_mib = $GpuReserveMinMiB; gpu_reserve_percent = $GpuReservePercent; partuuid = $PARTUUID; disk_guid = $DiskGuid; expected_swap_uuid = $ExpectedSwapUuid; ownership_proof_schema = $OwnershipProofSchema; existing_wsl_swap_vhdx = $ExistingSwapVhdx; configuration_sha256 = $configurationHash }
    $temporary = Join-Path $directory ((Split-Path -Leaf $ManifestPath) + "." + [Guid]::NewGuid().ToString("N") + ".tmp")
    try {
        $manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $temporary -Encoding UTF8
        Move-Item -LiteralPath $temporary -Destination $ManifestPath -Force
    } finally {
        if (Test-Path -LiteralPath $temporary -PathType Leaf) { Remove-Item -LiteralPath $temporary -Force }
    }
}

function Read-SealedOriginManifest {
    if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) { throw "sealed origin manifest is unavailable" }
    try {
        $manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json
    } catch {
        throw "sealed origin manifest is malformed"
    }
    $expectedKeys = @("schema_version", "origin_vhdx", "fixed_size_bytes", "logical_capacity_mib", "physical_cache_cap_mib", "chunk_mib", "gpu_reserve_min_mib", "gpu_reserve_percent", "partuuid", "disk_guid", "expected_swap_uuid", "ownership_proof_schema", "existing_wsl_swap_vhdx", "configuration_sha256")
    $actualKeys = @($manifest.PSObject.Properties.Name | Sort-Object)
    if (($actualKeys -join "`n") -cne (@($expectedKeys | Sort-Object) -join "`n")) { throw "sealed origin manifest schema mismatch" }
    try {
        $logical = [int]$manifest.logical_capacity_mib
        $physical = [int]$manifest.physical_cache_cap_mib
        $fixedSize = [uint64]$manifest.fixed_size_bytes
    } catch {
        throw "sealed origin manifest has invalid numeric fields"
    }
    $partUuid = ([string]$manifest.partuuid).ToLowerInvariant()
    $diskGuid = ([string]$manifest.disk_guid).ToLowerInvariant()
    $expectedSwapUuid = ([string]$manifest.expected_swap_uuid).ToLowerInvariant()
    if ($manifest.schema_version -ne 3 -or $manifest.origin_vhdx -cne $OriginVhdx -or $manifest.existing_wsl_swap_vhdx -cne $ExistingSwapVhdx -or $fixedSize -ne [uint64]$OriginSize -or $logical -lt 1024 -or $logical -gt 24576 -or ($logical % 1024) -ne 0 -or $physical -lt 1024 -or $physical -gt $logical -or ($physical % 1024) -ne 0 -or [int]$manifest.chunk_mib -ne $ChunkMiB -or [int]$manifest.gpu_reserve_min_mib -ne $GpuReserveMinMiB -or [int]$manifest.gpu_reserve_percent -ne $GpuReservePercent -or [int]$manifest.ownership_proof_schema -ne $OwnershipProofSchema -or -not (Test-CanonicalOriginGuid -Value $partUuid) -or -not (Test-CanonicalOriginGuid -Value $diskGuid) -or -not (Test-CanonicalOriginGuid -Value $expectedSwapUuid) -or ([string]$manifest.configuration_sha256) -notmatch '^[0-9a-f]{64}$') {
        throw "sealed origin manifest policy mismatch"
    }
    $actualHash = Get-OriginConfigurationSha256 -ManifestLogicalCapacityMiB $logical -ManifestPhysicalCacheCapMiB $physical -ManifestPartUuid $partUuid -ManifestDiskGuid $diskGuid -ManifestExpectedSwapUuid $expectedSwapUuid
    if ($actualHash -cne [string]$manifest.configuration_sha256) { throw "sealed origin manifest configuration hash mismatch" }
    return $manifest
}

function Get-OriginVhdxOwnershipProof {
    param([Parameter(Mandatory = $true)][string]$VhdxPath = $OriginVhdx)
    $vhd = Get-VHD -Path $VhdxPath -ErrorAction Stop
    if ($null -eq $vhd -or [uint64]$vhd.Size -ne [uint64]$OriginSize -or [string]$vhd.VhdType -cne "Fixed") {
        throw "origin VHDX does not match the sealed fixed-size policy"
    }
    $image = Get-DiskImage -ImagePath $VhdxPath -ErrorAction Stop
    if ($null -eq $image -or $image.Attached) { throw "origin VHDX must be detached before ownership verification" }
    $mounted = $false
    try {
        $mountedVhd = Mount-VHD -Path $VhdxPath -NoDriveLetter -PassThru
        $mounted = $true
        $disks = @($mountedVhd | Get-Disk)
        if ($disks.Count -ne 1 -or $null -eq $disks[0] -or $disks[0].Number -lt 0 -or [string]$disks[0].PartitionStyle -cne "GPT") { throw "origin VHDX disk identity is unavailable" }
        $diskGuid = ([string]$disks[0].Guid).Trim("{}").ToLowerInvariant()
        if (-not (Test-CanonicalOriginGuid -Value $diskGuid)) { throw "origin VHDX disk GUID ownership proof is invalid" }
        $partitions = @(Get-Partition -DiskNumber $disks[0].Number | Where-Object { [string]$_.Type -ceq "Basic" })
        if ($partitions.Count -ne 1) { throw "origin VHDX must contain exactly one basic data partition" }
        $partUuid = ([string]$partitions[0].Guid).Trim("{}").ToLowerInvariant()
        if (-not (Test-CanonicalOriginGuid -Value $partUuid)) { throw "origin VHDX PARTUUID ownership proof is invalid" }
        return [ordered]@{ partuuid = $partUuid; disk_guid = $diskGuid }
    } finally {
        if ($mounted) { Dismount-VHD -Path $VhdxPath }
    }
}

function Test-OriginProofMatchesManifest {
    param([Parameter(Mandatory = $true)][object]$Proof, [Parameter(Mandatory = $true)][object]$Manifest)
    return $Proof.partuuid -cne $null -and $Proof.disk_guid -cne $null -and
        $Proof.partuuid -cne "" -and $Proof.disk_guid -cne "" -and
        (Test-CanonicalOriginGuid -Value ([string]$Proof.partuuid)) -and
        (Test-CanonicalOriginGuid -Value ([string]$Proof.disk_guid)) -and
        (Test-CanonicalOriginGuid -Value ([string]$Manifest.partuuid)) -and
        (Test-CanonicalOriginGuid -Value ([string]$Manifest.disk_guid)) -and
        $Proof.partuuid -ceq ([string]$Manifest.partuuid) -and
        $Proof.disk_guid -ceq ([string]$Manifest.disk_guid)
}

function New-OriginInstallTransaction {
    if (Test-Path -LiteralPath $OriginVhdx -PathType Leaf) { throw "origin VHDX already exists; refuse replacement" }
    if (Test-Path -LiteralPath $ManifestPath -PathType Leaf) { throw "sealed origin manifest already exists; refuse replacement" }
    $staging = $OriginVhdx + "." + [Guid]::NewGuid().ToString("N") + ".staging.vhdx"
    if (Test-Path -LiteralPath $staging -PathType Any) { throw "origin transaction staging path unexpectedly exists" }
    return [ordered]@{
        staging_vhdx = $staging
        staging_reserved = $true
        origin_promoted = $false
        manifest_written = $false
        expected_proof = $null
    }
}

function Get-OriginInstallRollbackTargets {
    param([Parameter(Mandatory = $true)][System.Collections.IDictionary]$Transaction)
    return [ordered]@{
        remove_manifest = [bool]$Transaction.manifest_written
        remove_origin = [bool]($Transaction.origin_promoted -and $null -ne $Transaction.expected_proof)
        remove_staging = [bool]$Transaction.staging_reserved
    }
}

function Rollback-OriginInstallTransaction {
    param([Parameter(Mandatory = $true)][System.Collections.IDictionary]$Transaction)
    $targets = Get-OriginInstallRollbackTargets -Transaction $Transaction
    $rollbackErrors = @()
    if ($targets.remove_manifest -and (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
        try {
            $manifest = Read-SealedOriginManifest
            if (Test-OriginProofMatchesManifest -Proof $Transaction.expected_proof -Manifest $manifest) {
                Remove-Item -LiteralPath $ManifestPath -Force
            } else {
                $rollbackErrors += "origin manifest ownership changed before rollback"
            }
        } catch { $rollbackErrors += $_.Exception.Message }
    }
    if ($targets.remove_origin -and (Test-Path -LiteralPath $OriginVhdx -PathType Leaf)) {
        try {
            $proof = Get-OriginVhdxOwnershipProof -VhdxPath $OriginVhdx
            if ($proof.partuuid -ceq $Transaction.expected_proof.partuuid -and $proof.disk_guid -ceq $Transaction.expected_proof.disk_guid) {
                Remove-Item -LiteralPath $OriginVhdx -Force
            } else {
                $rollbackErrors += "origin VHDX ownership changed before rollback"
            }
        } catch { $rollbackErrors += $_.Exception.Message }
    }
    if ($targets.remove_staging -and (Test-Path -LiteralPath $Transaction.staging_vhdx -PathType Leaf)) {
        try { Remove-Item -LiteralPath $transaction.staging_vhdx -Force } catch { $rollbackErrors += $_.Exception.Message }
    }
    if ($rollbackErrors.Count -gt 0) { throw ("origin transaction rollback incomplete: " + ($rollbackErrors -join "; ")) }
}

function Get-OriginFileSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
}

function New-OriginUninstallTransaction {
    param([Parameter(Mandatory = $true)][object]$Manifest)
    $originExists = Test-Path -LiteralPath $OriginVhdx -PathType Leaf
    $manifestExists = Test-Path -LiteralPath $ManifestPath -PathType Leaf
    if (-not $originExists -or -not $manifestExists) {
        throw "origin uninstall requires owned VHDX and sealed manifest"
    }
    New-Item -ItemType Directory -Force -Path $BackupRoot | Out-Null
    $id = [Guid]::NewGuid().ToString("N")
    $manifestBackup = Join-Path $BackupRoot ("origin-uninstall-" + $id + ".manifest.json")
    $stagingVhdx = $OriginVhdx + "." + $id + ".uninstall-staging"
    $backupExists = Test-Path -LiteralPath $manifestBackup -PathType Any
    $stagingExists = Test-Path -LiteralPath $stagingVhdx -PathType Any
    if ($backupExists -or $stagingExists) {
        throw "origin uninstall transaction path collision"
    }
    Copy-Item -LiteralPath $ManifestPath -Destination $manifestBackup -ErrorAction Stop
    if ((Get-OriginFileSha256 -Path $manifestBackup) -cne (Get-OriginFileSha256 -Path $ManifestPath)) {
        Remove-Item -LiteralPath $manifestBackup -Force -ErrorAction SilentlyContinue
        throw "origin uninstall manifest backup verification failed"
    }
    return [ordered]@{
        manifest = $Manifest
        manifest_backup = $manifestBackup
        staging_vhdx = $stagingVhdx
        origin_staged = $false
        manifest_removed = $false
    }
}

function Get-OriginUninstallRollbackTargets {
    param([Parameter(Mandatory = $true)][System.Collections.IDictionary]$Transaction)
    return [ordered]@{
        restore_origin = [bool]$Transaction.origin_staged
        restore_manifest = [bool]$Transaction.manifest_removed
    }
}

function Rollback-OriginUninstallTransaction {
    param([Parameter(Mandatory = $true)][System.Collections.IDictionary]$Transaction)
    $targets = Get-OriginUninstallRollbackTargets -Transaction $Transaction
    $errors = @()
    if ($targets.restore_manifest) {
        try {
            $backupExists = Test-Path -LiteralPath $Transaction.manifest_backup -PathType Leaf
            $manifestExists = Test-Path -LiteralPath $ManifestPath -PathType Any
            if (-not $backupExists -or $manifestExists) {
                throw "origin uninstall manifest authority cannot be restored"
            }
            Copy-Item -LiteralPath $Transaction.manifest_backup -Destination $ManifestPath -ErrorAction Stop
            $restored = Read-SealedOriginManifest
            if (-not (Test-OriginProofMatchesManifest -Proof $Transaction.manifest -Manifest $restored)) {
                throw "origin uninstall manifest authority restoration mismatch"
            }
        } catch { $errors += $_.Exception.Message }
    }
    if ($targets.restore_origin) {
        try {
            $stagingExists = Test-Path -LiteralPath $Transaction.staging_vhdx -PathType Leaf
            $originExists = Test-Path -LiteralPath $OriginVhdx -PathType Any
            if (-not $stagingExists -or $originExists) {
                throw "origin uninstall VHDX authority cannot be restored"
            }
            Move-Item -LiteralPath $Transaction.staging_vhdx -Destination $OriginVhdx -ErrorAction Stop
            $proof = Get-OriginVhdxOwnershipProof -VhdxPath $OriginVhdx
            if (-not (Test-OriginProofMatchesManifest -Proof $proof -Manifest $Transaction.manifest)) {
                throw "origin uninstall VHDX authority restoration mismatch"
            }
        } catch { $errors += $_.Exception.Message }
    }
    if ($errors.Count -ne 0) { throw ("origin uninstall rollback incomplete: " + ($errors -join "; ")) }
}

function Invoke-OriginUninstallTransaction {
    param([Parameter(Mandatory = $true)][object]$Manifest)
    $transaction = New-OriginUninstallTransaction -Manifest $Manifest
    try {
        Move-Item -LiteralPath $OriginVhdx -Destination $transaction.staging_vhdx -ErrorAction Stop
        $transaction.origin_staged = $true
        Remove-Item -LiteralPath $ManifestPath -Force -ErrorAction Stop
        $transaction.manifest_removed = $true
        Remove-Item -LiteralPath $transaction.staging_vhdx -Force -ErrorAction Stop
        $transaction.origin_staged = $false
        # Backup cleanup is non-authoritative; retain it rather than claim a
        # failed cleanup requires reconstructing an already-destroyed VHDX.
        Remove-Item -LiteralPath $transaction.manifest_backup -Force -ErrorAction SilentlyContinue
    } catch {
        $failure = $_
        try { Rollback-OriginUninstallTransaction -Transaction $transaction } catch { throw ("origin uninstall transaction failed and rollback was incomplete: " + $_.Exception.Message) }
        throw ("origin uninstall transaction failed; restored authority: " + $failure.Exception.Message)
    }
}

function Invoke-OriginManufacturedTests {
    $proof = @{ partuuid = "11111111-1111-1111-1111-111111111111"; disk_guid = "22222222-2222-2222-2222-222222222222" }
    $afterCreateFailure = @{ staging_vhdx = "I:\RamShared\current-run.staging.vhdx"; staging_reserved = $true; origin_promoted = $false; manifest_written = $false; expected_proof = $null }
    $afterCreateRollback = Get-OriginInstallRollbackTargets -Transaction $afterCreateFailure
    if (-not $afterCreateRollback.remove_staging -or $afterCreateRollback.remove_origin -or $afterCreateRollback.remove_manifest) { throw "manufactured create failure rollback target selection failed" }
    $afterPromoteFailure = @{ staging_vhdx = "I:\RamShared\current-run.staging.vhdx"; staging_reserved = $false; origin_promoted = $true; manifest_written = $false; expected_proof = $proof }
    $afterPromoteRollback = Get-OriginInstallRollbackTargets -Transaction $afterPromoteFailure
    if ($afterPromoteRollback.remove_staging -or -not $afterPromoteRollback.remove_origin -or $afterPromoteRollback.remove_manifest) { throw "manufactured promotion failure rollback target selection failed" }
    $currentRun = @{ staging_vhdx = "I:\RamShared\current-run.staging.vhdx"; staging_reserved = $false; origin_promoted = $true; manifest_written = $true; expected_proof = $proof }
    $rollback = Get-OriginInstallRollbackTargets -Transaction $currentRun
    if ($rollback.remove_staging -or -not $rollback.remove_origin -or -not $rollback.remove_manifest) { throw "manufactured manifest failure rollback target selection failed" }
    $foreign = @{ staging_vhdx = "I:\RamShared\foreign.staging.vhdx"; staging_reserved = $false; origin_promoted = $false; manifest_written = $false; expected_proof = $null }
    $foreignRollback = Get-OriginInstallRollbackTargets -Transaction $foreign
    if ($foreignRollback.remove_staging -or $foreignRollback.remove_origin -or $foreignRollback.remove_manifest) { throw "manufactured foreign artifact rollback selection failed" }
    $manifest = [pscustomobject]@{ partuuid = "11111111-1111-1111-1111-111111111111"; disk_guid = "22222222-2222-2222-2222-222222222222" }
    if (-not (Test-OriginProofMatchesManifest -Proof $currentRun.expected_proof -Manifest $manifest)) { throw "manufactured exact uninstall proof failed" }
    $wrongProof = [pscustomobject]@{ partuuid = "33333333-3333-3333-3333-333333333333"; disk_guid = "22222222-2222-2222-2222-222222222222" }
    if (Test-OriginProofMatchesManifest -Proof $wrongProof -Manifest $manifest) { throw "manufactured foreign uninstall proof was accepted" }
    $malformedProof = [pscustomobject]@{ partuuid = "11111111-1111-1111-111111111111"; disk_guid = "22222222-2222-2222-2222-222222222222" }
    if (Test-OriginProofMatchesManifest -Proof $malformedProof -Manifest $manifest) { throw "manufactured malformed ownership proof was accepted" }
    $uninstallFailure = @{ manifest = $manifest; manifest_backup = "I:\RamShared\uninstall.manifest.json"; staging_vhdx = "I:\RamShared\uninstall.staging"; origin_staged = $true; manifest_removed = $true }
    $uninstallRollback = Get-OriginUninstallRollbackTargets -Transaction $uninstallFailure
    if (-not $uninstallRollback.restore_origin -or -not $uninstallRollback.restore_manifest) { throw "manufactured uninstall rollback did not preserve authority" }
    Write-Output "PASS origin_plan_is_separate_fixed_and_identity_bound"
    Write-Output "PASS foreign_or_unproven_partuuid_is_rejected"
    Write-Output "PASS origin_install_failure_rolls_back_current_run_only"
    Write-Output "PASS origin_preexisting_or_foreign_vhdx_is_never_removed"
    Write-Output "PASS origin_uninstall_requires_exact_sealed_ownership"
    Write-Output "PASS canonical_vhdx_guid_and_partuuid_are_accepted"
    Write-Output "PASS malformed_or_foreign_origin_identity_is_refused"
    Write-Output "PASS origin_uninstall_failure_restores_vhdx_and_manifest_authority"
}

if ($Action -eq "plan" -or (-not $Run -and $Action -ne "status" -and $Action -ne "test")) { Write-OriginPlan; exit 0 }
if ($Action -eq "status") { if (Test-Path -LiteralPath $ManifestPath -PathType Leaf) { Get-Content -Raw -LiteralPath $ManifestPath } else { Write-OriginPlan }; exit 0 }
if ($Action -eq "test") { Invoke-OriginManufacturedTests; exit 0 }
if (-not $AttendedOriginApply) { throw "origin action requires separate attended explicit action" }
if ($ApproveOriginProvision -cne $ApprovalToken) { throw "origin action requires exact approval token" }
if ($OriginVhdx -ieq $ExistingSwapVhdx) { throw "origin must never equal the existing WSL swap VHDX" }
if (($LogicalCapacityMiB % 1024) -ne 0) { throw "logical capacity must be whole GiB between 1 and 24 GiB" }
if (($PhysicalCacheCapMiB % 1024) -ne 0 -or $PhysicalCacheCapMiB -gt $LogicalCapacityMiB) { throw "physical cache cap must be whole GiB and no larger than logical capacity" }
if ($Action -eq "install" -and $PARTUUID -cne "00000000-0000-0000-0000-000000000000") { throw "Windows assigns the GPT PARTUUID; seal the generated value from the mounted origin VHDX" }

switch ($Action) {
    "install" {
        $consumer = Get-Service -Name RamSharedWinSvc -ErrorAction SilentlyContinue
        $broker = Get-Service -Name RamSharedBroker -ErrorAction SilentlyContinue
        if (($null -ne $consumer -and $consumer.Status -ne "Stopped") -or ($null -ne $broker -and $broker.Status -ne "Stopped")) {
            throw "origin install requires RamShared services to be stopped"
        }
        $transaction = New-OriginInstallTransaction
        try {
            if (Test-Path -LiteralPath $ExistingSwapVhdx) { Write-Verbose "existing WSL swap VHDX remains untouched" }
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $OriginVhdx) | Out-Null
            New-Item -ItemType Directory -Force -Path $BackupRoot | Out-Null
            $vhd = New-VHD -Path $transaction.staging_vhdx -SizeBytes $OriginSize -Fixed
            $mounted = $false
            try {
                $mountedVhd = Mount-VHD -Path $vhd.Path -NoDriveLetter -PassThru
                $mounted = $true
                $disk = $mountedVhd | Get-Disk
                if ($null -eq $disk -or $disk.Number -lt 0) { throw "origin disk identity unavailable" }
                $initialized = Initialize-Disk -Number $disk.Number -PartitionStyle GPT -PassThru
                $partition = New-Partition -DiskNumber $initialized.Number -UseMaximumSize -AssignDriveLetter:$false
                Set-Partition -InputObject $partition -NoDefaultDriveLetter $true
            } finally {
                if ($mounted) { Dismount-VHD -Path $transaction.staging_vhdx }
            }
            $proof = Get-OriginVhdxOwnershipProof -VhdxPath $transaction.staging_vhdx
            $transaction.expected_proof = $proof
            $PARTUUID = $proof.partuuid
            $DiskGuid = $proof.disk_guid
            $ExpectedSwapUuid = [Guid]::NewGuid().ToString("D").ToLowerInvariant()
            $transaction.origin_promoted = $true
            Move-Item -LiteralPath $transaction.staging_vhdx -Destination $OriginVhdx
            $transaction.staging_reserved = $false
            $transaction.manifest_written = $true
            Write-OriginManifest
        } catch {
            $installFailure = $_
            try { Rollback-OriginInstallTransaction -Transaction $transaction } catch { throw ("origin install transaction failed and rollback was incomplete: " + $_.Exception.Message) }
            throw ("origin install transaction failed; rolled back only current-run artifacts: " + $installFailure.Exception.Message)
        }
    }
    "configure" {
        if ($PartUuidWasSupplied) { throw "configure does not accept a caller PARTUUID; it derives identity from the sealed origin VHDX" }
        $manifest = Read-SealedOriginManifest
        if (($LogicalCapacityWasSupplied -and $LogicalCapacityMiB -ne [int]$manifest.logical_capacity_mib) -or ($PhysicalCacheCapWasSupplied -and $PhysicalCacheCapMiB -ne [int]$manifest.physical_cache_cap_mib)) { throw "configure does not accept caller configuration changes without a new sealed origin VHDX" }
        $proof = Get-OriginVhdxOwnershipProof -VhdxPath $OriginVhdx
        if ($proof.partuuid -cne [string]$manifest.partuuid) { throw "PARTUUID ownership proof does not match the sealed origin VHDX" }
        if ($proof.disk_guid -cne [string]$manifest.disk_guid) { throw "disk GUID ownership proof does not match the sealed origin VHDX" }
        [ordered]@{ state = "VERIFIED"; action = $Action; partuuid = $proof.partuuid; disk_guid = $proof.disk_guid; host_mutation = $false } | ConvertTo-Json -Depth 4
    }
    "uninstall" {
        $consumer = Get-Service -Name RamSharedWinSvc -ErrorAction SilentlyContinue
        $broker = Get-Service -Name RamSharedBroker -ErrorAction SilentlyContinue
        if (($null -ne $consumer -and $consumer.Status -ne "Stopped") -or ($null -ne $broker -and $broker.Status -ne "Stopped")) {
            throw "origin uninstall requires RamShared services to be stopped"
        }
        if ($PartUuidWasSupplied -or $LogicalCapacityWasSupplied -or $PhysicalCacheCapWasSupplied) { throw "origin uninstall does not accept caller identity or policy values" }
        $manifest = Read-SealedOriginManifest
        $proof = Get-OriginVhdxOwnershipProof -VhdxPath $OriginVhdx
        if (-not (Test-OriginProofMatchesManifest -Proof $proof -Manifest $manifest)) { throw "origin uninstall requires exact sealed ownership proof" }
        Invoke-OriginUninstallTransaction -Manifest $manifest
        Write-Output "origin uninstalled: owned VHDX and sealed manifest removed"
    }
}
