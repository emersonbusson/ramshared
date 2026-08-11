#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Destination,
    [ValidateRange(1, 4096)][int]$MaxFiles = 4096,
    [UInt64]$MaxBytes = 268435456
)

$ErrorActionPreference = "Stop"
$sourceRoot = "C:\ProgramData\RamShared\evidence"
$artifactRoot = "C:\ramshared\artifacts"

function Assert-PathHasNoReparsePoint([string]$Path, [string]$Label) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Label is a reparse point: $Path"
    }
    return $item
}

function Assert-ExistingPathTreeHasNoReparsePoint(
    [string]$CanonicalRoot,
    [string]$CanonicalPath
) {
    $rootItem = Assert-PathHasNoReparsePoint $CanonicalRoot "artifact root"
    if (-not $rootItem.PSIsContainer) {
        throw "artifact root is not a directory"
    }
    $relative = $CanonicalPath.Substring($CanonicalRoot.Length).TrimStart('\', '/')
    if ([string]::IsNullOrWhiteSpace($relative)) { return }
    $cursor = $CanonicalRoot
    $segments = @([regex]::Split($relative, '[\\/]+') | Where-Object {
            -not [string]::IsNullOrWhiteSpace($_)
        })
    for ($index = 0; $index -lt $segments.Count; $index++) {
        $cursor = Join-Path $cursor $segments[$index]
        try {
            $existing = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        }
        catch [System.Management.Automation.ItemNotFoundException] {
            break
        }
        catch {
            throw "destination ancestor discovery failed: $($_.Exception.Message)"
        }
        $item = Assert-PathHasNoReparsePoint $cursor "destination ancestor"
        if ($index -lt ($segments.Count - 1) -and -not $item.PSIsContainer) {
            throw "destination ancestor is not a directory: $cursor"
        }
    }
}

function Resolve-CanonicalArtifactDestination([string]$RawDestination) {
    if ([string]::IsNullOrWhiteSpace($RawDestination) -or
        -not [IO.Path]::IsPathRooted($RawDestination)) {
        throw "Destination must be an absolute path below C:\ramshared\artifacts"
    }
    if ([regex]::IsMatch($RawDestination, '(^|[\\/])\.\.([\\/]|$)')) {
        throw "raw destination contains .. path segment"
    }
    $canonicalRoot = [IO.Path]::GetFullPath($artifactRoot).TrimEnd('\', '/')
    $canonicalDestination = [IO.Path]::GetFullPath($RawDestination)
    $rootPrefix = $canonicalRoot + [IO.Path]::DirectorySeparatorChar
    if (-not $canonicalDestination.StartsWith(
            $rootPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Destination must be a child of C:\ramshared\artifacts"
    }
    Assert-ExistingPathTreeHasNoReparsePoint $canonicalRoot $canonicalDestination
    try {
        $existingDestination = Get-Item -LiteralPath $canonicalDestination `
            -Force -ErrorAction Stop
    }
    catch [System.Management.Automation.ItemNotFoundException] {
        $existingDestination = $null
    }
    catch {
        throw "destination discovery failed: $($_.Exception.Message)"
    }
    if ($existingDestination) {
        throw "Destination already exists: $canonicalDestination"
    }
    return [pscustomobject]@{
        root = $canonicalRoot
        path = $canonicalDestination
    }
}

function Get-BoundedEvidenceFiles(
    [string]$Source,
    [int]$FileLimit,
    [UInt64]$ByteLimit
) {
    if ($ByteLimit -eq 0) { throw "MaxBytes must be greater than zero" }
    $sourceItem = Assert-PathHasNoReparsePoint $Source "source evidence path"
    if (-not $sourceItem.PSIsContainer) {
        throw "source evidence path is not a directory"
    }
    $pending = New-Object 'System.Collections.Generic.Stack[string]'
    $pending.Push($sourceItem.FullName)
    $files = @()
    [UInt64]$totalBytes = 0
    while ($pending.Count -ne 0) {
        $directory = $pending.Pop()
        Assert-PathHasNoReparsePoint $directory "source evidence path" | Out-Null
        foreach ($entry in @(Get-ChildItem -LiteralPath $directory -Force -ErrorAction Stop)) {
            if (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "source evidence path is a reparse point: $($entry.FullName)"
            }
            if ($entry.PSIsContainer) {
                $pending.Push($entry.FullName)
                continue
            }
            if ($files.Count -ge $FileLimit) {
                throw "evidence file limit exceeded: $FileLimit"
            }
            [UInt64]$length = [UInt64]$entry.Length
            if ($length -gt ($ByteLimit - $totalBytes)) {
                throw "evidence byte limit exceeded: $ByteLimit"
            }
            $totalBytes += $length
            $files += [pscustomobject]@{
                source_path = $entry.FullName
                relative_path = $entry.FullName.Substring($sourceItem.FullName.Length).
                    TrimStart('\', '/')
                bytes = $length
                kind = "runtime"
            }
        }
    }
    return [pscustomobject]@{ files = $files; total_bytes = $totalBytes }
}

function Ensure-SafeDirectory([string]$Path, [string]$Root) {
    Assert-ExistingPathTreeHasNoReparsePoint $Root $Path
    try {
        $existing = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    }
    catch [System.Management.Automation.ItemNotFoundException] {
        $existing = $null
    }
    catch {
        throw "destination directory discovery failed: $($_.Exception.Message)"
    }
    if ($existing) {
        $item = Assert-PathHasNoReparsePoint $Path "destination directory"
        if (-not $item.PSIsContainer) { throw "destination path is not a directory: $Path" }
        return
    }
    $parent = [IO.Path]::GetDirectoryName($Path)
    if ([string]::IsNullOrWhiteSpace($parent)) {
        throw "destination directory has no parent: $Path"
    }
    Ensure-SafeDirectory $parent $Root
    New-Item -ItemType Directory -Path $Path -ErrorAction Stop | Out-Null
    Assert-PathHasNoReparsePoint $Path "created destination directory" | Out-Null
}

function Remove-SafeStaging([string]$Path, [string]$Root) {
    $canonicalRoot = [IO.Path]::GetFullPath($Root).TrimEnd('\', '/')
    $canonicalPath = [IO.Path]::GetFullPath($Path)
    $rootPrefix = $canonicalRoot + [IO.Path]::DirectorySeparatorChar
    if (-not $canonicalPath.StartsWith(
            $rootPrefix, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($canonicalPath) -notmatch '^\.staging-[0-9a-f]{32}$') {
        throw "refuse unsafe staging cleanup path"
    }
    $stagingItem = Assert-PathHasNoReparsePoint $canonicalPath `
        "evidence staging directory"
    if (-not $stagingItem.PSIsContainer) {
        throw "evidence staging path is not a directory"
    }
    foreach ($entry in @(Get-ChildItem -LiteralPath $canonicalPath -Recurse `
            -Force -ErrorAction Stop)) {
        if (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "refuse staging cleanup containing a reparse point"
        }
    }
    [IO.Directory]::Delete($canonicalPath, $true)
}

$destinationInfo = Resolve-CanonicalArtifactDestination $Destination
$bounded = Get-BoundedEvidenceFiles $sourceRoot $MaxFiles $MaxBytes
$filesToCopy = @($bounded.files)
[UInt64]$totalBytes = [UInt64]$bounded.total_bytes

foreach ($diagnosticPath in @(
        "C:\ProgramData\RamShared\teardown-diag.log",
        "C:\ProgramData\RamShared\active-manifest.json")) {
    try {
        $diagnostic = Get-Item -LiteralPath $diagnosticPath -Force -ErrorAction Stop
    }
    catch [System.Management.Automation.ItemNotFoundException] {
        continue
    }
    catch {
        throw "diagnostic evidence discovery failed: $($_.Exception.Message)"
    }
    if ($diagnostic.PSIsContainer) {
        throw "diagnostic evidence is not a file: $diagnosticPath"
    }
    if (($diagnostic.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "diagnostic evidence is a reparse point: $diagnosticPath"
    }
    if ($filesToCopy.Count -ge $MaxFiles) {
        throw "evidence file limit exceeded: $MaxFiles"
    }
    [UInt64]$length = [UInt64]$diagnostic.Length
    if ($length -gt ($MaxBytes - $totalBytes)) {
        throw "evidence byte limit exceeded: $MaxBytes"
    }
    $totalBytes += $length
    $filesToCopy += [pscustomobject]@{
        source_path = $diagnostic.FullName
        relative_path = $diagnostic.Name
        bytes = $length
        kind = "diagnostic"
    }
}

$staging = Join-Path $destinationInfo.root ".staging-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $staging -ErrorAction Stop | Out-Null
Assert-PathHasNoReparsePoint $staging "evidence staging directory" | Out-Null
$stagingPrefix = $staging + [IO.Path]::DirectorySeparatorChar
$inventoryRows = @()
$published = $false

try {
    foreach ($entry in $filesToCopy) {
        $sourceItem = Assert-PathHasNoReparsePoint $entry.source_path "source evidence file"
        if ($sourceItem.PSIsContainer -or [UInt64]$sourceItem.Length -ne [UInt64]$entry.bytes) {
            throw "source evidence file changed during copy: $($entry.source_path); expected_bytes=$([UInt64]$entry.bytes); actual_bytes=$([UInt64]$sourceItem.Length)"
        }
        $target = [IO.Path]::GetFullPath((Join-Path $staging $entry.relative_path))
        if (-not $target.StartsWith($stagingPrefix, [StringComparison]::OrdinalIgnoreCase)) {
            throw "evidence target escaped staging directory"
        }
        Ensure-SafeDirectory ([IO.Path]::GetDirectoryName($target)) $staging
        [IO.File]::Copy($sourceItem.FullName, $target, $false)
        $targetItem = Assert-PathHasNoReparsePoint $target "copied evidence file"
        $sourceHash = (Get-FileHash -LiteralPath $sourceItem.FullName -Algorithm SHA256 `
                -ErrorAction Stop).Hash.ToUpperInvariant()
        $targetHash = (Get-FileHash -LiteralPath $targetItem.FullName -Algorithm SHA256 `
                -ErrorAction Stop).Hash.ToUpperInvariant()
        if ([UInt64]$targetItem.Length -ne [UInt64]$sourceItem.Length -or
            $sourceHash -ne $targetHash) {
            throw "source/destination SHA256 mismatch: $($entry.relative_path)"
        }
        $inventoryRows += [ordered]@{
            relative_path = ($entry.relative_path -replace '\\', '/')
            kind = $entry.kind
            bytes = [UInt64]$targetItem.Length
            source_sha256 = $sourceHash
            destination_sha256 = $targetHash
        }
    }

    $inventoryPath = Join-Path $staging "evidence-copy-inventory.json"
    $inventory = [ordered]@{
        schema = 1
        source_root = $sourceRoot
        destination = $destinationInfo.path
        files = $inventoryRows
        file_count = $inventoryRows.Count
        total_bytes = $totalBytes
    }
    [IO.File]::WriteAllText($inventoryPath, ($inventory | ConvertTo-Json -Depth 8),
        [Text.UTF8Encoding]::new($false))
    $inventoryHash = (Get-FileHash -LiteralPath $inventoryPath -Algorithm SHA256 `
            -ErrorAction Stop).Hash.ToUpperInvariant()
    Move-Item -LiteralPath $staging -Destination $destinationInfo.path -ErrorAction Stop
    $published = $true
    Write-Host "EVIDENCE_COPY_OK=$($destinationInfo.path); files=$($inventoryRows.Count); bytes=$totalBytes; inventory_sha256=$inventoryHash"
}
finally {
    if (-not $published -and (Test-Path -LiteralPath $staging)) {
        Remove-SafeStaging $staging $destinationInfo.root
    }
}
