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
    [ValidateSet("plan", "install", "configure", "status", "uninstall", "attach", "test")]
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
    [string]$PARTUUID = "00000000-0000-0000-0000-000000000000",
    [ValidateRange(5GB, 64GB)]
    [uint64]$OriginSizeBytes = 5GB
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-AbsoluteWindowsPath {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$Name)
    $expanded = [Environment]::ExpandEnvironmentVariables($Path.Trim())
    if (-not [IO.Path]::IsPathRooted($expanded) -or $expanded -notmatch '^(?:[A-Za-z]:[\\/]|\\\\)') { throw "$Name must be an absolute Windows path" }
    return [IO.Path]::GetFullPath($expanded)
}

# Manifest policy compares paths, not strings. A sealed `origin_vhdx` and the
# runtime path can be the same file while differing in separator, case, or
# `.`/`..` segments; only a resolved ordinal-ignore-case comparison is sound.
# An unresolvable side is never equal, so this stays fail-closed.
function Test-SameWindowsPath {
    param([AllowEmptyString()][string]$Left = "", [AllowEmptyString()][string]$Right = "")
    if ([string]::IsNullOrWhiteSpace($Left) -and [string]::IsNullOrWhiteSpace($Right)) { return $true }
    if ([string]::IsNullOrWhiteSpace($Left) -or [string]::IsNullOrWhiteSpace($Right)) { return $false }
    try {
        $leftPath = Resolve-AbsoluteWindowsPath -Path $Left -Name "manifest path"
        $rightPath = Resolve-AbsoluteWindowsPath -Path $Right -Name "runtime path"
    } catch {
        return $false
    }
    return [string]::Equals($leftPath, $rightPath, [StringComparison]::OrdinalIgnoreCase)
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
    try {
        if (Test-Path -LiteralPath $registry) {
            foreach ($entry in Get-ChildItem -LiteralPath $registry -ErrorAction Stop) {
                try {
                    $properties = Get-ItemProperty -LiteralPath $entry.PSPath -ErrorAction Stop
                    if ([string]$properties.DistributionName -ceq $Distro -and -not [string]::IsNullOrWhiteSpace([string]$properties.BasePath)) {
                        $base = Resolve-AbsoluteWindowsPath -Path ([string]$properties.BasePath) -Name "WSL distro BasePath"
                        $root = [IO.Path]::GetPathRoot($base)
                        if (-not [string]::IsNullOrWhiteSpace($root)) {
                            return [pscustomobject]@{ Root = $root; Source = "distro_basepath" }
                        }
                    }
                } catch {
                    continue
                }
            }
        }
    } catch {
        # Registry access is observational; C: remains the bounded fallback.
    }
    # Do not infer distro storage from the independent WSL swap VHDX path.
    # If registry discovery is unavailable, C: is the bounded documented fallback.
    return [pscustomobject]@{ Root = "C:\"; Source = "c_default" }
}

$ExistingSwapVhdx = Get-ConfiguredWslSwapVhdxPath
$OriginSize = if ($PSBoundParameters.ContainsKey("OriginSizeBytes")) { [uint64]$OriginSizeBytes } else { 5GB }
if (($OriginSize % 1GB) -ne 0 -or $OriginSize -lt 5GB -or $OriginSize -gt 64GB -or $OriginSize -lt [uint64](($LogicalCapacityMiB + 1024) * 1MB)) {
    throw "origin container size must be whole GiB between 5 GiB and 64 GiB, and at least 1 GiB larger than logical capacity"
}
$OriginSizeGiB = [int]($OriginSize / 1GB)
$ChunkMiB = 128
$GpuReserveMinMiB = 2048
$GpuReservePercent = 20
$ManifestPath = "C:\ProgramData\RamShared\ramshared-origin-manifest.json"
$BackupRoot = "C:\ProgramData\RamShared\ramshared-origin-backup"
$OriginHostFreeSpaceReserveBytes = [uint64]10GB
$ApprovalToken = if ($OriginSize -eq 25GB) { "RAMSHARED_ORIGIN_25GIB_PARTUUID" } else { "RAMSHARED_ORIGIN_${OriginSizeGiB}GIB_PARTUUID" }
$OwnershipProofSchema = 1
$PartUuidWasSupplied = $PSBoundParameters.ContainsKey("PARTUUID")
$LogicalCapacityWasSupplied = $PSBoundParameters.ContainsKey("LogicalCapacityMiB")
$PhysicalCacheCapWasSupplied = $PSBoundParameters.ContainsKey("PhysicalCacheCapMiB")
$DiskGuid = ""
$ExpectedSwapUuid = ""
$CanonicalGuidPattern = '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'

function Get-OriginVolumeSnapshot {
    param([Parameter(Mandatory = $true)][string]$Path)
    $root = [IO.Path]::GetPathRoot($Path)
    if ([string]::IsNullOrWhiteSpace($root) -or $root -notmatch '^[A-Za-z]:\\$') { return $null }
    try {
        $drive = New-Object System.IO.DriveInfo($root)
        if (-not $drive.IsReady -or $drive.DriveType -ne [IO.DriveType]::Fixed) { return $null }
        $volumes = @(Get-Volume -FilePath $root -ErrorAction Stop)
        if ($volumes.Count -ne 1) { return $null }
        $volume = $volumes[0]
        $fileSystemType = [string]$volume.FileSystemType
        $uniqueId = [string]$volume.UniqueId
        if ([string]::IsNullOrWhiteSpace($uniqueId) -or $fileSystemType -notin @("NTFS", "ReFS")) { return $null }
        return [pscustomobject]@{
            Root = $root
            UniqueId = $uniqueId
            DriveType = [string]$drive.DriveType
            FileSystemType = $fileSystemType
            FreeBytes = [uint64]$drive.AvailableFreeSpace
        }
    } catch {
        return $null
    }
}

function Select-OriginStorageVolume {
    param(
        [AllowNull()][object]$DistroVolume,
        [AllowNull()][object]$CVolume,
        [Parameter(Mandatory = $true)][uint64]$RequiredOriginBytes,
        [Parameter(Mandatory = $true)][uint64]$FreeSpaceReserveBytes
    )
    $requiredFreeBytes = $RequiredOriginBytes + $FreeSpaceReserveBytes
    $seenVolumes = @{}
    $observations = @()
    $orderedCandidates = @(
        [pscustomobject]@{ role = "distro"; volume = $DistroVolume },
        [pscustomobject]@{ role = "c_fallback"; volume = $CVolume }
    )
    foreach ($entry in $orderedCandidates) {
        $candidate = $entry.volume
        if ($null -eq $candidate) { continue }
        $root = [string]$candidate.Root
        $uniqueId = [string]$candidate.UniqueId
        if ($entry.role -eq "c_fallback" -and $root -ine "C:\") { continue }
        if ($root -notmatch '^[A-Za-z]:\\$' -or
            [string]$candidate.DriveType -cne "Fixed" -or
            [string]$candidate.FileSystemType -notin @("NTFS", "ReFS") -or
            [string]::IsNullOrWhiteSpace($uniqueId)) {
            continue
        }
        if ($seenVolumes.ContainsKey($uniqueId)) { continue }
        $seenVolumes[$uniqueId] = $true
        $freeBytes = [uint64]$candidate.FreeBytes
        $observations += "${root}=$freeBytes"
        if ($freeBytes -ge $requiredFreeBytes) { return $candidate }
    }
    $observed = if ($observations.Count -gt 0) { $observations -join "," } else { "none" }
    throw "origin_volume_headroom_insufficient: required_free_bytes=$requiredFreeBytes candidates=$observed"
}

function Resolve-OriginVhdxPath {
    param(
        [AllowEmptyString()][string]$RequestedPath = "",
        [AllowEmptyString()][string]$SealedPath = "",
        [AllowNull()][object]$DistroVolume,
        [AllowNull()][object]$CVolume,
        [uint64]$RequiredOriginBytes = $OriginSize,
        [uint64]$FreeSpaceReserveBytes = $OriginHostFreeSpaceReserveBytes
    )
    if (-not [string]::IsNullOrWhiteSpace($SealedPath)) {
        $sealed = Resolve-AbsoluteWindowsPath -Path $SealedPath -Name "sealed origin_vhdx"
        if (-not [string]::IsNullOrWhiteSpace($RequestedPath)) {
            $requested = Resolve-AbsoluteWindowsPath -Path $RequestedPath -Name "OriginVhdxPath"
            if (-not [string]::Equals($sealed, $requested, [StringComparison]::OrdinalIgnoreCase)) {
                throw "OriginVhdxPath conflicts with the sealed origin path"
            }
        }
        return $sealed
    }
    if (-not [string]::IsNullOrWhiteSpace($RequestedPath)) {
        return Resolve-AbsoluteWindowsPath -Path $RequestedPath -Name "OriginVhdxPath"
    }
    $selected = Select-OriginStorageVolume -DistroVolume $DistroVolume -CVolume $CVolume `
        -RequiredOriginBytes $RequiredOriginBytes -FreeSpaceReserveBytes $FreeSpaceReserveBytes
    return Join-Path $selected.Root "RamShared\ramshared-origin.vhdx"
}

function Get-SealedOriginVhdxPath {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return "" }
    try {
        $manifest = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -ErrorAction Stop
    } catch {
        throw "sealed origin manifest is malformed; refusing to choose a new storage path"
    }
    if ([int]$manifest.schema_version -ne 3 -or [string]::IsNullOrWhiteSpace([string]$manifest.origin_vhdx)) {
        throw "sealed origin manifest path is invalid; refusing to choose a new storage path"
    }
    return Resolve-AbsoluteWindowsPath -Path ([string]$manifest.origin_vhdx) -Name "sealed origin_vhdx"
}

function Assert-OriginFreeSpace {
    param(
        [Parameter(Mandatory = $true)][uint64]$AvailableBytes,
        [Parameter(Mandatory = $true)][uint64]$RequiredFreeBytes,
        [Parameter(Mandatory = $true)][string]$Purpose
    )
    if ($AvailableBytes -lt $RequiredFreeBytes) {
        throw "origin_free_space_insufficient: purpose=$Purpose required_free_bytes=$RequiredFreeBytes available_free_bytes=$AvailableBytes"
    }
}

function Assert-OriginPathFreeSpace {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][uint64]$RequiredFreeBytes,
        [Parameter(Mandatory = $true)][string]$Purpose
    )
    $volume = Get-OriginVolumeSnapshot -Path $Path
    if ($null -eq $volume) { throw "origin_volume_unavailable: purpose=$Purpose" }
    Assert-OriginFreeSpace -AvailableBytes $volume.FreeBytes -RequiredFreeBytes $RequiredFreeBytes -Purpose $Purpose
    return $volume
}

$requestedOriginPath = if ($PSBoundParameters.ContainsKey("OriginVhdxPath")) { $OriginVhdxPath } else { "" }
if ($Action -eq "test") {
    # Manufactured tests must not inspect or depend on live host storage state.
    $OriginVhdx = "C:\RamShared\manufactured-origin.vhdx"
    $OriginStorageSelection = [ordered]@{ source = "manufactured_test"; drive_root = "C:\"; free_bytes = $null; required_free_bytes = $null }
} else {
    $sealedOriginPath = Get-SealedOriginVhdxPath -Path $ManifestPath
    if (-not [string]::IsNullOrWhiteSpace($sealedOriginPath)) {
        $OriginVhdx = Resolve-OriginVhdxPath -RequestedPath $requestedOriginPath -SealedPath $sealedOriginPath
        $OriginStorageSelection = [ordered]@{ source = "sealed_manifest"; drive_root = [IO.Path]::GetPathRoot($OriginVhdx); free_bytes = $null; required_free_bytes = $null }
    } elseif (-not [string]::IsNullOrWhiteSpace($requestedOriginPath)) {
        $OriginVhdx = Resolve-OriginVhdxPath -RequestedPath $requestedOriginPath
        $explicitVolume = Assert-OriginPathFreeSpace -Path $OriginVhdx `
            -RequiredFreeBytes ($OriginSize + $OriginHostFreeSpaceReserveBytes) -Purpose "before explicit origin allocation"
        $OriginStorageSelection = [ordered]@{
            source = "explicit_path"
            drive_root = $explicitVolume.Root
            free_bytes = $explicitVolume.FreeBytes
            required_free_bytes = $OriginSize + $OriginHostFreeSpaceReserveBytes
        }
    } else {
        $preferredStorage = Get-WslDistroStorageRoot
        $distroVolume = Get-OriginVolumeSnapshot -Path $preferredStorage.Root
        $cVolume = Get-OriginVolumeSnapshot -Path "C:\"
        $selectedVolume = Select-OriginStorageVolume -DistroVolume $distroVolume -CVolume $cVolume `
            -RequiredOriginBytes $OriginSize -FreeSpaceReserveBytes $OriginHostFreeSpaceReserveBytes
        $OriginVhdx = Resolve-OriginVhdxPath -DistroVolume $distroVolume -CVolume $cVolume `
            -RequiredOriginBytes $OriginSize -FreeSpaceReserveBytes $OriginHostFreeSpaceReserveBytes
        $selectionSource = if ($null -ne $distroVolume -and $selectedVolume.UniqueId -ceq $distroVolume.UniqueId) { [string]$preferredStorage.Source } else { "c_fallback" }
        $OriginStorageSelection = [ordered]@{
            source = $selectionSource
            drive_root = $selectedVolume.Root
            free_bytes = $selectedVolume.FreeBytes
            required_free_bytes = $OriginSize + $OriginHostFreeSpaceReserveBytes
        }
    }
}

function Test-CanonicalOriginGuid {
    param([AllowNull()][object]$Value)
    return $Value -is [string] -and $Value -match $CanonicalGuidPattern
}

function Write-OriginPlan {
    [ordered]@{ state = "PLAN"; action = $Action; distro = $Distro; origin_vhdx = $OriginVhdx; origin_storage_selection = $OriginStorageSelection; fixed_size_bytes = $OriginSize; logical_capacity_mib = $LogicalCapacityMiB; physical_cache_cap_mib = $PhysicalCacheCapMiB; chunk_mib = $ChunkMiB; gpu_reserve_min_mib = $GpuReserveMinMiB; gpu_reserve_percent = $GpuReservePercent; partuuid = $PARTUUID; expected_swap_uuid = "generated-during-install"; existing_wsl_swap_vhdx = $ExistingSwapVhdx; host_mutation_requires_run = $true; host_mutation_requires_attended_action = $true; host_mutation_requires_exact_approval = $ApprovalToken } | ConvertTo-Json -Depth 4
}

function Get-OriginConfigurationSha256 {
    param(
        [Parameter(Mandatory = $true)][int]$ManifestLogicalCapacityMiB,
        [Parameter(Mandatory = $true)][int]$ManifestPhysicalCacheCapMiB,
        [Parameter(Mandatory = $true)][string]$ManifestPartUuid,
        [Parameter(Mandatory = $true)][string]$ManifestDiskGuid,
        [Parameter(Mandatory = $true)][string]$ManifestExpectedSwapUuid,
        [Parameter(Mandatory = $false)][uint64]$ManifestFixedSizeBytes = $OriginSize
    )
    $text = "schema=3`norigin_vhdx=$OriginVhdx`nfixed_size_bytes=$ManifestFixedSizeBytes`nlogical_capacity_mib=$ManifestLogicalCapacityMiB`nphysical_cache_cap_mib=$ManifestPhysicalCacheCapMiB`nchunk_mib=$ChunkMiB`ngpu_reserve_min_mib=$GpuReserveMinMiB`ngpu_reserve_percent=$GpuReservePercent`npartuuid=$ManifestPartUuid`ndisk_guid=$ManifestDiskGuid`nexpected_swap_uuid=$ManifestExpectedSwapUuid`nownership_proof_schema=$OwnershipProofSchema`nexisting_wsl_swap_vhdx=$ExistingSwapVhdx`n"
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
    $configurationHash = Get-OriginConfigurationSha256 -ManifestLogicalCapacityMiB $LogicalCapacityMiB -ManifestPhysicalCacheCapMiB $PhysicalCacheCapMiB -ManifestPartUuid $PARTUUID -ManifestDiskGuid $DiskGuid -ManifestExpectedSwapUuid $ExpectedSwapUuid -ManifestFixedSizeBytes $OriginSize
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
    if ($manifest.schema_version -ne 3 -or -not (Test-SameWindowsPath -Left ([string]$manifest.origin_vhdx) -Right $OriginVhdx) -or -not (Test-SameWindowsPath -Left ([string]$manifest.existing_wsl_swap_vhdx) -Right $ExistingSwapVhdx) -or $fixedSize -lt 5GB -or $fixedSize -gt 64GB -or ($fixedSize % 1GB) -ne 0 -or $fixedSize -lt [uint64](($logical + 1024) * 1MB) -or ($PSBoundParameters.ContainsKey("OriginSizeBytes") -and $fixedSize -ne [uint64]$OriginSize) -or $logical -lt 1024 -or $logical -gt 24576 -or ($logical % 1024) -ne 0 -or $physical -lt 1024 -or $physical -gt $logical -or ($physical % 1024) -ne 0 -or [int]$manifest.chunk_mib -ne $ChunkMiB -or [int]$manifest.gpu_reserve_min_mib -ne $GpuReserveMinMiB -or [int]$manifest.gpu_reserve_percent -ne $GpuReservePercent -or [int]$manifest.ownership_proof_schema -ne $OwnershipProofSchema -or -not (Test-CanonicalOriginGuid -Value $partUuid) -or -not (Test-CanonicalOriginGuid -Value $diskGuid) -or -not (Test-CanonicalOriginGuid -Value $expectedSwapUuid) -or ([string]$manifest.configuration_sha256) -notmatch '^[0-9a-f]{64}$') {
        throw "sealed origin manifest policy mismatch"
    }
    $actualHash = Get-OriginConfigurationSha256 -ManifestLogicalCapacityMiB $logical -ManifestPhysicalCacheCapMiB $physical -ManifestPartUuid $partUuid -ManifestDiskGuid $diskGuid -ManifestExpectedSwapUuid $expectedSwapUuid -ManifestFixedSizeBytes $fixedSize
    if ($actualHash -cne [string]$manifest.configuration_sha256) { throw "sealed origin manifest configuration hash mismatch" }
    return $manifest
}

function Get-OriginVhdxOwnershipProof {
    param([Parameter(Mandatory = $true)][string]$VhdxPath = $OriginVhdx)
    $vhd = Get-VHD -Path $VhdxPath -ErrorAction Stop
    $vhdSize = [uint64]$vhd.Size
    if ($null -eq $vhd -or $vhdSize -lt 5GB -or $vhdSize -gt 64GB -or ($vhdSize % 1GB) -ne 0 -or [string]$vhd.VhdType -cne "Fixed") {
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

function Test-OriginAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-OriginBoundedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FileName,
        [Parameter(Mandatory = $true)][string]$Arguments,
        [Parameter(Mandatory = $true)][ValidateRange(1, 30)][int]$TimeoutSeconds
    )
    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = $FileName
    $start.Arguments = $Arguments
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw "origin bounded process did not start" }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $completed = $process.WaitForExit($TimeoutSeconds * 1000)
        $terminated = $false
        if (-not $completed) {
            try {
                $process.Kill()
                $terminated = $process.WaitForExit(5000)
            } catch {
                $terminated = $false
            }
        }
        $streamsDrained = $false
        try {
            $streamsDrained = [Threading.Tasks.Task]::WaitAll(
                [Threading.Tasks.Task[]]@($stdoutTask, $stderrTask), 5000)
        } catch {
            $streamsDrained = $false
        }
        return [ordered]@{
            completed = [bool]($completed -and $streamsDrained)
            exit_code = if ($completed -and $streamsDrained) { [int]$process.ExitCode } else { $null }
            timed_out = [bool](-not $completed)
            process_terminated = [bool]$terminated
        }
    } finally {
        $process.Dispose()
    }
}

function Invoke-OriginGuestPartUuidProbe {
    param([Parameter(Mandatory = $true)][string]$PartUuid)
    if (-not (Test-CanonicalOriginGuid -Value $PartUuid)) {
        throw "origin guest PARTUUID probe received an invalid identity"
    }
    $arguments = "-d " + $Distro + " -u root -- test -b /dev/disk/by-partuuid/" + $PartUuid.ToLowerInvariant()
    $probe = Invoke-OriginBoundedProcess -FileName "wsl.exe" -Arguments $arguments -TimeoutSeconds 5
    if (-not $probe.completed) { throw "origin guest PARTUUID probe did not complete" }
    if ($probe.exit_code -eq 0) { return $true }
    if ($probe.exit_code -eq 1) { return $false }
    throw "origin guest PARTUUID probe failed"
}

function Get-OriginAttachmentDecision {
    param([Parameter(Mandatory = $true)][bool]$GuestPartuuidPresent)
    if ($GuestPartuuidPresent) {
        return [ordered]@{ state = "ALREADY_ATTACHED"; host_mutation = $false }
    }
    return [ordered]@{ state = "ATTACH_REQUIRED"; host_mutation = $true }
}

function Invoke-OriginAttachment {
    param([Parameter(Mandatory = $true)][object]$Manifest)
    if (-not (Test-OriginAdministrator)) { throw "origin attach requires an administrator token" }
    $partUuid = ([string]$Manifest.partuuid).ToLowerInvariant()
    $diskGuid = ([string]$Manifest.disk_guid).ToLowerInvariant()
    $decision = Get-OriginAttachmentDecision -GuestPartuuidPresent (Invoke-OriginGuestPartUuidProbe -PartUuid $partUuid)
    if ($decision.state -eq "ALREADY_ATTACHED") {
        [ordered]@{ state = $decision.state; action = "attach"; partuuid = $partUuid; disk_guid = $diskGuid; host_mutation = $false } | ConvertTo-Json -Depth 4
        return
    }
    if ($OriginVhdx -match '\s') { throw "origin attach requires a whitespace-free sealed VHDX path" }
    $proof = Get-OriginVhdxOwnershipProof -VhdxPath $OriginVhdx
    if (-not (Test-OriginProofMatchesManifest -Proof $proof -Manifest $Manifest)) {
        throw "origin attach ownership proof does not match the sealed VHDX"
    }
    $mount = Invoke-OriginBoundedProcess -FileName "wsl.exe" `
        -Arguments ("--mount --vhd " + $OriginVhdx + " --bare") -TimeoutSeconds 15
    if (-not $mount.completed) { throw "origin attach did not complete within the bounded deadline" }
    if ($mount.exit_code -ne 0) { throw "origin attach command failed" }
    for ($attempt = 1; $attempt -le 5; $attempt++) {
        if (Invoke-OriginGuestPartUuidProbe -PartUuid $partUuid) {
            [ordered]@{ state = "ATTACHED"; action = "attach"; partuuid = $partUuid; disk_guid = $diskGuid; host_mutation = $true } | ConvertTo-Json -Depth 4
            return
        }
        if ($attempt -lt 5) { Start-Sleep -Seconds 1 }
    }
    throw "origin attach did not expose the sealed PARTUUID"
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
    $alreadyAttached = Get-OriginAttachmentDecision -GuestPartuuidPresent $true
    $attachRequired = Get-OriginAttachmentDecision -GuestPartuuidPresent $false
    if ($alreadyAttached.state -cne "ALREADY_ATTACHED" -or $alreadyAttached.host_mutation -or
        $attachRequired.state -cne "ATTACH_REQUIRED" -or -not $attachRequired.host_mutation) {
        throw "manufactured origin attachment decision was not idempotent and fail closed"
    }
    $distroEnough = [pscustomobject]@{ Root = "I:\"; UniqueId = "volume-i"; DriveType = "Fixed"; FileSystemType = "NTFS"; FreeBytes = [uint64]20GB }
    $cEnough = [pscustomobject]@{ Root = "C:\"; UniqueId = "volume-c"; DriveType = "Fixed"; FileSystemType = "NTFS"; FreeBytes = [uint64]30GB }
    $selected = Select-OriginStorageVolume -DistroVolume $distroEnough -CVolume $cEnough -RequiredOriginBytes ([uint64]5GB) -FreeSpaceReserveBytes ([uint64]10GB)
    if ($selected.UniqueId -cne "volume-i") { throw "manufactured origin volume did not prefer the distro volume with reserve" }
    Write-Output "PASS origin_volume_prefers_distro_volume_with_reserve"

    $distroLow = [pscustomobject]@{ Root = "I:\"; UniqueId = "volume-i"; DriveType = "Fixed"; FileSystemType = "NTFS"; FreeBytes = [uint64]14GB }
    $selectedFallback = Select-OriginStorageVolume -DistroVolume $distroLow -CVolume $cEnough -RequiredOriginBytes ([uint64]5GB) -FreeSpaceReserveBytes ([uint64]10GB)
    if ($selectedFallback.UniqueId -cne "volume-c") { throw "manufactured origin volume did not fall back to C:" }
    Write-Output "PASS origin_volume_falls_back_to_c_when_preferred_lacks_reserve"

    $singleC = [pscustomobject]@{ Root = "C:\"; UniqueId = "volume-c"; DriveType = "Fixed"; FileSystemType = "NTFS"; FreeBytes = [uint64]15GB }
    $selectedSingle = Select-OriginStorageVolume -DistroVolume $singleC -CVolume $singleC -RequiredOriginBytes ([uint64]5GB) -FreeSpaceReserveBytes ([uint64]10GB)
    if ($selectedSingle.UniqueId -cne "volume-c") { throw "manufactured single-volume C: selection failed" }
    Write-Output "PASS origin_single_volume_c_satisfies_default"

    $cLow = [pscustomobject]@{ Root = "C:\"; UniqueId = "volume-c"; DriveType = "Fixed"; FileSystemType = "NTFS"; FreeBytes = [uint64]14GB }
    $lowSpaceError = ""
    try { $null = Select-OriginStorageVolume -DistroVolume $distroLow -CVolume $cLow -RequiredOriginBytes ([uint64]5GB) -FreeSpaceReserveBytes ([uint64]10GB) }
    catch { $lowSpaceError = $_.Exception.Message }
    if (-not $lowSpaceError.StartsWith("origin_volume_headroom_insufficient:") -or
        -not $lowSpaceError.Contains("required_free_bytes=16106127360") -or
        -not $lowSpaceError.Contains("I:\=15032385536,C:\=15032385536")) {
        throw "manufactured origin volume did not refuse with required and observed byte counts"
    }
    Write-Output "PASS origin_volume_refuses_when_all_candidates_below_reserve"

    $removableDistro = [pscustomobject]@{ Root = "I:\"; UniqueId = "removable-i"; DriveType = "Removable"; FileSystemType = "NTFS"; FreeBytes = [uint64]100GB }
    $unsupportedC = [pscustomobject]@{ Root = "C:\"; UniqueId = "unsupported-c"; DriveType = "Fixed"; FileSystemType = "FAT32"; FreeBytes = [uint64]100GB }
    $unsupportedRefused = $false
    try { $null = Select-OriginStorageVolume -DistroVolume $removableDistro -CVolume $unsupportedC -RequiredOriginBytes ([uint64]5GB) -FreeSpaceReserveBytes ([uint64]10GB) }
    catch { $unsupportedRefused = $_.Exception.Message -like "origin_volume_headroom_insufficient:*" }
    if (-not $unsupportedRefused) { throw "manufactured selector accepted removable or unsupported-file-system storage" }
    Write-Output "PASS origin_volume_rejects_removable_and_unsupported_filesystem"

    $movedDistroPath = Resolve-OriginVhdxPath -SealedPath "C:\RamShared\ramshared-origin.vhdx" -DistroVolume $distroEnough -CVolume $cEnough
    $movedDistroPathAgain = Resolve-OriginVhdxPath -SealedPath "C:\RamShared\ramshared-origin.vhdx" -DistroVolume $distroLow -CVolume $cEnough
    if ($movedDistroPath -cne "C:\RamShared\ramshared-origin.vhdx" -or $movedDistroPathAgain -cne $movedDistroPath) {
        throw "manufactured sealed origin path changed after distro volume movement or replay"
    }
    Write-Output "PASS origin_existing_manifest_path_survives_distro_volume_change"

    $overrideRefused = $false
    try { $null = Resolve-OriginVhdxPath -SealedPath "C:\RamShared\ramshared-origin.vhdx" -RequestedPath "I:\RamShared\ramshared-origin.vhdx" }
    catch { $overrideRefused = $_.Exception.Message -like "OriginVhdxPath conflicts with the sealed origin path*" }
    if (-not $overrideRefused) { throw "manufactured conflicting sealed-origin override was accepted" }
    Write-Output "PASS origin_existing_manifest_override_mismatch_is_refused"

    $postAllocationError = ""
    try { Assert-OriginFreeSpace -AvailableBytes ([uint64]10GB - 1) -RequiredFreeBytes $OriginHostFreeSpaceReserveBytes -Purpose "post-allocation" }
    catch { $postAllocationError = $_.Exception.Message }
    if (-not $postAllocationError.Contains("origin_free_space_insufficient:") -or
        -not $postAllocationError.Contains("required_free_bytes=10737418240") -or
        -not $postAllocationError.Contains("available_free_bytes=10737418239")) {
        throw "manufactured post-allocation reserve loss was accepted or reported without byte counts"
    }
    Assert-OriginFreeSpace -AvailableBytes $OriginHostFreeSpaceReserveBytes -RequiredFreeBytes $OriginHostFreeSpaceReserveBytes -Purpose "post-allocation boundary"
    Write-Output "PASS origin_install_rechecks_post_create_reserve_before_manifest"
    Write-Output "PASS origin_plan_is_separate_fixed_and_identity_bound"
    Write-Output "PASS foreign_or_unproven_partuuid_is_rejected"
    Write-Output "PASS origin_install_failure_rolls_back_current_run_only"
    Write-Output "PASS origin_preexisting_or_foreign_vhdx_is_never_removed"
    Write-Output "PASS origin_uninstall_requires_exact_sealed_ownership"
    Write-Output "PASS canonical_vhdx_guid_and_partuuid_are_accepted"
    Write-Output "PASS malformed_or_foreign_origin_identity_is_refused"
    Write-Output "PASS origin_uninstall_failure_restores_vhdx_and_manifest_authority"
    Write-Output "PASS origin_attach_decision_is_idempotent_and_fail_closed"
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
        $null = Assert-OriginPathFreeSpace -Path $OriginVhdx `
            -RequiredFreeBytes ($OriginSize + $OriginHostFreeSpaceReserveBytes) -Purpose "before fixed origin allocation"
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
            $null = Assert-OriginPathFreeSpace -Path $OriginVhdx `
                -RequiredFreeBytes $OriginHostFreeSpaceReserveBytes -Purpose "post-allocation origin reserve"
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
    "attach" {
        if ($PartUuidWasSupplied) { throw "origin attach does not accept a caller PARTUUID" }
        if ($LogicalCapacityWasSupplied -or $PhysicalCacheCapWasSupplied) { throw "origin attach does not accept caller capacity policy" }
        $manifest = Read-SealedOriginManifest
        Invoke-OriginAttachment -Manifest $manifest
    }
    "uninstall" {
        if ($PartUuidWasSupplied -or $LogicalCapacityWasSupplied -or $PhysicalCacheCapWasSupplied) { throw "origin uninstall does not accept caller identity or policy values" }
        $manifest = Read-SealedOriginManifest
        $proof = Get-OriginVhdxOwnershipProof -VhdxPath $OriginVhdx
        if (-not (Test-OriginProofMatchesManifest -Proof $proof -Manifest $manifest)) { throw "origin uninstall requires exact sealed ownership proof" }
        Invoke-OriginUninstallTransaction -Manifest $manifest
        Write-Output "origin uninstalled: owned VHDX and sealed manifest removed"
    }
}
