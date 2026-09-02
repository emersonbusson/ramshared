#Requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet("plan", "install", "status", "uninstall")]
    [string]$Action = "plan",
    [switch]$Run,
    [ValidatePattern('^[A-Za-z0-9._-]+$')]
    [string]$Distro = "Ubuntu-24.04",
    [switch]$ValidateSignature
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$Root = Join-Path $env:LOCALAPPDATA "RamShared\launchers"
$Backup = Join-Path $env:LOCALAPPDATA "RamShared\launcher-backup"
$BackupManifest = Join-Path $Backup "launcher-backup-manifest.json"
$Files = @("ramshared-shell.cmd", "ramshared-terminal.cmd", "ramshared-vscode.cmd")

$executables = @("wsl.exe", "wt.exe", "code.cmd", "code")
foreach ($exe in $executables) {
    $cmd = Get-Command $exe -ErrorAction SilentlyContinue
    if (-not $cmd -or -not (Test-Path -LiteralPath $cmd.Path -PathType Leaf)) {
        if ($exe -eq "code.cmd" -or $exe -eq "code") {
            continue # VS Code is optional depending on environment
        }
        throw [System.IO.FileNotFoundException]::new("launcher_executable_not_found: $exe")
    }
    if ($ValidateSignature) {
        $sig = Get-AuthenticodeSignature -FilePath $cmd.Path -ErrorAction Stop
        if ($sig.Status -ne 'Valid') {
            throw [System.Security.SecurityException]::new("launcher_signature_invalid: $exe")
        }
    }
}

function Get-LauncherSha256 {
    param([string]$Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
}

function Get-LauncherContent {
    param([string]$Name)
    switch ($Name) {
        "ramshared-shell.cmd" { return "@wsl.exe -d `"$Distro`" -- ramshared session --class interactive" }
        "ramshared-terminal.cmd" { return "@wt.exe wsl.exe -d `"$Distro`" -- ramshared session --class interactive" }
        "ramshared-vscode.cmd" { return "@wsl.exe -d `"$Distro`" -- ramshared run --class interactive -- code ." }
        default { throw "launcher_name_invalid" }
    }
}

function Write-LauncherContent {
    param([string]$Path, [string]$Content)
    [IO.File]::WriteAllText($Path, $Content, [Text.ASCIIEncoding]::new())
}

function Get-LauncherContentSha256 {
    param([string]$Content)
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        ([BitConverter]::ToString($sha256.ComputeHash([Text.ASCIIEncoding]::new().GetBytes($Content))) -replace "-", "")
    }
    finally {
        $sha256.Dispose()
    }
}

function Get-LauncherManifest {
    if (-not (Test-Path -LiteralPath $BackupManifest -PathType Leaf)) { return $null }
    try {
        $manifest = Get-Content -LiteralPath $BackupManifest -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
    }
    catch {
        throw "launcher_backup_manifest_invalid"
    }
    if ([int]$manifest.schema -ne 2 -or [string]$manifest.state -notin @("installed", "uninstalled") -or
        [string]$manifest.distro -notmatch '^[A-Za-z0-9._-]+$') {
        throw "launcher_backup_manifest_invalid"
    }
    if ([string]$manifest.distro -cne $Distro) { throw "launcher_manifest_distro_mismatch" }
    $entries = @($manifest.entries)
    if ($entries.Count -ne $Files.Count) { throw "launcher_backup_manifest_invalid" }
    foreach ($name in $Files) {
        $matches = @($entries | Where-Object { [string]$_.name -ceq $name })
        if ($matches.Count -ne 1) { throw "launcher_backup_manifest_invalid" }
        $entry = $matches[0]
        if ($entry.original_exists -isnot [bool] -or
            ([bool]$entry.original_exists -and [string]$entry.original_sha256 -notmatch '^[0-9A-F]{64}$') -or
            ([string]$entry.installed_sha256 -notmatch '^[0-9A-F]{64}$')) {
            throw "launcher_backup_manifest_invalid"
        }
    }
    return $manifest
}

function Assert-LauncherManifestDistro {
    param([Parameter(Mandatory = $true)][object]$Manifest)
    if ([string]$Manifest.distro -cne $Distro) { throw "launcher_manifest_distro_mismatch" }
}

function Get-LauncherManifestEntry {
    param([object]$Manifest, [string]$Name)
    @($Manifest.entries | Where-Object { [string]$_.name -ceq $Name })[0]
}

function Write-LauncherManifest {
    param([object]$Manifest)
    $temporary = Join-Path $Backup (".launcher-backup-manifest-" + [guid]::NewGuid().ToString("N") + ".json")
    try {
        [IO.File]::WriteAllText($temporary, ($Manifest | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
        Move-Item -LiteralPath $temporary -Destination $BackupManifest -Force -ErrorAction Stop
    }
    finally {
        Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue
    }
}

function Assert-LauncherBackupIntegrity {
    param([object]$Manifest)
    foreach ($name in $Files) {
        $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
        $backupPath = Join-Path $Backup $name
        if ([bool]$entry.original_exists) {
            if (-not (Test-Path -LiteralPath $backupPath -PathType Leaf) -or
                (Get-LauncherSha256 -Path $backupPath) -cne [string]$entry.original_sha256) {
                throw "launcher_backup_digest_mismatch"
            }
        }
        elseif (Test-Path -LiteralPath $backupPath -PathType Leaf) {
            throw "launcher_backup_unexpected"
        }
    }
}

function Assert-InstalledLauncherTargets {
    param([object]$Manifest)
    foreach ($name in $Files) {
        $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
        $target = Join-Path $Root $name
        if (-not (Test-Path -LiteralPath $target -PathType Leaf) -or
            (Get-LauncherSha256 -Path $target) -cne [string]$entry.installed_sha256) {
            throw "launcher_target_modified_after_install"
        }
    }
}

function Assert-UninstalledLauncherTargets {
    param([object]$Manifest)
    foreach ($name in $Files) {
        $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
        $target = Join-Path $Root $name
        if ([bool]$entry.original_exists) {
            if (-not (Test-Path -LiteralPath $target -PathType Leaf) -or
                (Get-LauncherSha256 -Path $target) -cne [string]$entry.original_sha256) {
                throw "launcher_target_changed_after_restore"
            }
        }
        elseif (Test-Path -LiteralPath $target -PathType Leaf) {
            throw "launcher_target_changed_after_restore"
        }
    }
}

function Get-LauncherInstallCandidates {
    param([object]$Manifest)
    $candidates = @()
    foreach ($name in $Files) {
        $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
        $content = Get-LauncherContent -Name $name
        $expectedSha256 = Get-LauncherContentSha256 -Content $content
        if ($expectedSha256 -notmatch '^[0-9A-F]{64}$' -or
            $expectedSha256 -cne [string]$entry.installed_sha256) {
            throw "launcher_install_candidate_hash_mismatch"
        }
        $target = Join-Path $Root $name
        if ((Test-Path -LiteralPath $target) -and -not (Test-Path -LiteralPath $target -PathType Leaf)) {
            throw "launcher_target_invalid"
        }
        $candidates += [pscustomobject]@{ name = $name; target = $target; content = $content }
    }
    return @($candidates)
}

function New-LauncherManifest {
    foreach ($name in $Files) {
        $target = Join-Path $Root $name
        $backupPath = Join-Path $Backup $name
        if (Test-Path -LiteralPath $backupPath) {
            throw "launcher_backup_already_exists"
        }
        if ((Test-Path -LiteralPath $target) -and -not (Test-Path -LiteralPath $target -PathType Leaf)) {
            throw "launcher_target_invalid"
        }
    }
    $entries = @()
    $created_backup_paths = @()
    try {
        foreach ($name in $Files) {
            $target = Join-Path $Root $name
            $backupPath = Join-Path $Backup $name
            $originalExists = Test-Path -LiteralPath $target -PathType Leaf
            if ($originalExists) {
                Copy-Item -LiteralPath $target -Destination $backupPath -ErrorAction Stop
                # Track immediately: hashing or manifest append can fail after
                # a successful copy and must not strand foreign-looking staging.
                $created_backup_paths += $backupPath
            }
            $content = Get-LauncherContent -Name $name
            $entries += [pscustomobject]@{
                name = $name
                original_exists = [bool]$originalExists
                original_sha256 = if ($originalExists) { Get-LauncherSha256 -Path $backupPath } else { "" }
                installed_sha256 = Get-LauncherContentSha256 -Content $content
            }
        }
    }
    catch {
        $failure = $_
        $cleanupErrors = @()
        foreach ($backupPath in @($created_backup_paths | Select-Object -Unique)) {
            try {
                if (Test-Path -LiteralPath $backupPath -PathType Leaf) {
                    Remove-Item -LiteralPath $backupPath -Force -ErrorAction Stop
                }
            } catch { $cleanupErrors += $_.Exception.Message }
        }
        if ($cleanupErrors.Count -ne 0) { throw ("launcher_backup_staging_cleanup_failed: " + ($cleanupErrors -join "; ")) }
        throw $failure
    }
    [pscustomobject]@{
        schema = 2
        state = "installed"
        distro = $Distro
        entries = @($entries)
    }
}

function Restore-LauncherInstallTargets {
    param([object]$Manifest)
    foreach ($name in $Files) {
        $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
        $target = Join-Path $Root $name
        $targetIsLeaf = Test-Path -LiteralPath $target -PathType Leaf
        if ($targetIsLeaf -and (Get-LauncherSha256 -Path $target) -ceq [string]$entry.installed_sha256) {
            continue
        }
        if ([bool]$entry.original_exists) {
            if ($targetIsLeaf -and
                (Get-LauncherSha256 -Path $target) -ceq [string]$entry.original_sha256) {
                continue
            }
        }
        elseif (-not (Test-Path -LiteralPath $target)) {
            continue
        }
        throw "launcher_install_rollback_refused"
    }
    foreach ($name in $Files) {
        $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
        $target = Join-Path $Root $name
        if (-not (Test-Path -LiteralPath $target -PathType Leaf) -or
            (Get-LauncherSha256 -Path $target) -cne [string]$entry.installed_sha256) {
            continue
        }
        if ([bool]$entry.original_exists) {
            Copy-Item -LiteralPath (Join-Path $Backup $name) -Destination $target -Force -ErrorAction Stop
        }
        else {
            Remove-Item -LiteralPath $target -Force -ErrorAction Stop
        }
    }
    Assert-UninstalledLauncherTargets -Manifest $Manifest
}

function Remove-NewLauncherBackupStaging {
    param([object]$Manifest)
    foreach ($name in $Files) {
        $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
        $backupPath = Join-Path $Backup $name
        if ([bool]$entry.original_exists) {
            if (-not (Test-Path -LiteralPath $backupPath -PathType Leaf) -or
                (Get-LauncherSha256 -Path $backupPath) -cne [string]$entry.original_sha256) {
                throw "launcher_install_rollback_refused"
            }
            Remove-Item -LiteralPath $backupPath -Force -ErrorAction Stop
        }
        elseif (Test-Path -LiteralPath $backupPath) {
            throw "launcher_install_rollback_refused"
        }
    }
}

function Install-LauncherTargetsTransaction {
    param([object]$Manifest, [bool]$NewManifest)
    try {
        Assert-LauncherManifestDistro -Manifest $Manifest
        $candidates = Get-LauncherInstallCandidates -Manifest $Manifest
        Assert-LauncherBackupIntegrity -Manifest $Manifest
        foreach ($candidate in $candidates) {
            Write-LauncherContent -Path $candidate.target -Content $candidate.content
        }
        Assert-InstalledLauncherTargets -Manifest $Manifest
        $Manifest.state = "installed"
        foreach ($name in $Files) {
            $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
            $entry.installed_sha256 = Get-LauncherContentSha256 -Content (Get-LauncherContent -Name $name)
        }
        Write-LauncherManifest -Manifest $Manifest
    }
    catch {
        $installFailure = $_
        try {
            Restore-LauncherInstallTargets -Manifest $Manifest
            if ($NewManifest) {
                Remove-NewLauncherBackupStaging -Manifest $Manifest
            }
        }
        catch {
            throw ("launcher_install_rollback_failed: " + $_.Exception.Message)
        }
        throw $installFailure
    }
}

function New-LauncherUninstallTransaction {
    param([object]$Manifest)
    Assert-LauncherManifestDistro -Manifest $Manifest
    if (-not (Test-Path -LiteralPath $BackupManifest -PathType Leaf)) {
        throw "launcher_uninstall_state_invalid"
    }
    $manifestSnapshot = [IO.File]::ReadAllBytes($BackupManifest)
    if ($manifestSnapshot.Length -eq 0) { throw "launcher_backup_manifest_invalid" }
    return [pscustomobject]@{
        manifest = $Manifest
        manifest_snapshot = $manifestSnapshot
        restored_targets = $false
        manifest_may_be_modified = $false
    }
}

function Restore-LauncherUninstallTargets {
    param([object]$Manifest)
    foreach ($name in $Files) {
        $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
        $target = Join-Path $Root $name
        if ([bool]$entry.original_exists) {
            if ((Test-Path -LiteralPath $target -PathType Leaf) -and
                (Get-LauncherSha256 -Path $target) -ceq [string]$entry.installed_sha256) {
                continue
            }
            if ((Test-Path -LiteralPath $target -PathType Leaf) -and
                (Get-LauncherSha256 -Path $target) -ceq [string]$entry.original_sha256) {
                Copy-Item -LiteralPath (Join-Path $Backup $name) -Destination $target -Force -ErrorAction Stop
                continue
            }
        } elseif (-not (Test-Path -LiteralPath $target)) {
            continue
        }
        throw "launcher_uninstall_rollback_refused"
    }
    foreach ($name in $Files) {
        $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
        $target = Join-Path $Root $name
        Write-LauncherContent -Path $target -Content (Get-LauncherContent -Name $name)
        if ((Get-LauncherSha256 -Path $target) -cne [string]$entry.installed_sha256) {
            throw "launcher_uninstall_rollback_failed"
        }
    }
    Assert-InstalledLauncherTargets -Manifest $Manifest
}

function Restore-LauncherManifestSnapshot {
    param([byte[]]$Snapshot)
    $temporary = Join-Path $Backup (".launcher-uninstall-rollback-" + [guid]::NewGuid().ToString("N") + ".json")
    try {
        [IO.File]::WriteAllBytes($temporary, $Snapshot)
        Move-Item -LiteralPath $temporary -Destination $BackupManifest -Force -ErrorAction Stop
        Get-LauncherManifest | Out-Null
    } finally {
        Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue
    }
}

function Rollback-LauncherUninstallTransaction {
    param([object]$Transaction)
    $errors = @()
    if ($Transaction.restored_targets) {
        try { Restore-LauncherUninstallTargets -Manifest $Transaction.manifest } catch { $errors += $_.Exception.Message }
    }
    if ($Transaction.manifest_may_be_modified) {
        try { Restore-LauncherManifestSnapshot -Snapshot $Transaction.manifest_snapshot } catch { $errors += $_.Exception.Message }
    }
    if ($errors.Count -ne 0) { throw ("launcher_uninstall_rollback_failed: " + ($errors -join "; ")) }
}

function Invoke-LauncherUninstallTransaction {
    param([object]$Manifest)
    Assert-LauncherManifestDistro -Manifest $Manifest
    $transaction = New-LauncherUninstallTransaction -Manifest $Manifest
    try {
        $transaction.restored_targets = $true
        foreach ($name in $Files) {
            $entry = Get-LauncherManifestEntry -Manifest $Manifest -Name $name
            $target = Join-Path $Root $name
            if ([bool]$entry.original_exists) {
                Copy-Item -LiteralPath (Join-Path $Backup $name) -Destination $target -Force -ErrorAction Stop
            } else {
                Remove-Item -LiteralPath $target -Force -ErrorAction Stop
            }
        }
        Assert-UninstalledLauncherTargets -Manifest $Manifest
        $transaction.manifest_may_be_modified = $true
        $Manifest.state = "uninstalled"
        Write-LauncherManifest -Manifest $Manifest
    } catch {
        $failure = $_
        try { Rollback-LauncherUninstallTransaction -Transaction $transaction } catch { throw $_ }
        throw $failure
    }
}

$plan = [ordered]@{ state = "PLAN"; action = $Action; distro = $Distro; directory = $Root; reversible = $true }
if ($Action -eq "plan" -or (-not $Run -and $Action -ne "status")) { $plan | ConvertTo-Json; exit 0 }
if ($Action -eq "status") {
    Get-Item -LiteralPath ($Files | ForEach-Object { Join-Path $Root $_ }) -ErrorAction SilentlyContinue |
        Select-Object FullName, Length
    exit 0
}

if ($Action -eq "install") {
    New-Item -ItemType Directory -Force -Path $Root, $Backup | Out-Null
    $manifest = Get-LauncherManifest
    if ($null -eq $manifest) {
        $manifest = New-LauncherManifest
        Install-LauncherTargetsTransaction -Manifest $manifest -NewManifest $true
    }
    else {
        Assert-LauncherBackupIntegrity -Manifest $manifest
        if ([string]$manifest.state -ceq "installed") {
            Assert-InstalledLauncherTargets -Manifest $manifest
            Write-Output "launchers already installed without changing terminal defaults: $Root"
            exit 0
        }
        Assert-UninstalledLauncherTargets -Manifest $manifest
        Install-LauncherTargetsTransaction -Manifest $manifest -NewManifest $false
    }
    Write-Output "launchers installed without changing terminal defaults: $Root"
    exit 0
}

$manifest = Get-LauncherManifest
if ($null -eq $manifest -or [string]$manifest.state -cne "installed") {
    throw "launcher_uninstall_state_invalid"
}
Assert-LauncherBackupIntegrity -Manifest $manifest
Assert-InstalledLauncherTargets -Manifest $manifest
Invoke-LauncherUninstallTransaction -Manifest $manifest
Write-Output "launchers rolled back"
