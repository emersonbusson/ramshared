#Requires -Version 5.1
[CmdletBinding()]
param()
$target = Join-Path $PSScriptRoot "Manage-RamSharedLaunchers.ps1"
$source = Get-Content -Raw -LiteralPath $target
foreach ($required in @(
    'ValidateSet("plan", "install", "status", "uninstall")',
    'ramshared session --class interactive',
    'ramshared run --class interactive -- code .',
    'wt.exe wsl.exe',
    'launcher-backup',
    'launcher-backup-manifest.json',
    'launcher_backup_already_exists',
    'launcher_target_modified_after_install',
    'launcher_backup_digest_mismatch',
    'without changing terminal defaults'
    'Invoke-LauncherUninstallTransaction'
    'Rollback-LauncherUninstallTransaction'
    'launcher_uninstall_rollback_failed'
    'launcher_manifest_distro_mismatch'
    'schema = 2'
    'distro = $Distro'
    'created_backup_paths'
    'launcher_backup_staging_cleanup_failed'
)) { if (-not $source.Contains($required)) { throw "launcher contract missing: $required" } }

function Invoke-ManufacturedLauncher(
    [string]$PowerShellPath,
    [string]$ScriptPath,
    [string]$LocalAppData,
    [string]$Action
) {
    $previousLocalAppData = $env:LOCALAPPDATA
    $process = $null
    try {
        $env:LOCALAPPDATA = $LocalAppData
        $startInfo = [Diagnostics.ProcessStartInfo]::new()
        $startInfo.FileName = $PowerShellPath
        $startInfo.Arguments = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $ScriptPath +
            '" -Action "' + $Action + '" -Run'
        $startInfo.UseShellExecute = $false
        $startInfo.RedirectStandardOutput = $true
        $startInfo.RedirectStandardError = $true
        $startInfo.CreateNoWindow = $true
        $process = [Diagnostics.Process]::new()
        $process.StartInfo = $startInfo
        [void]$process.Start()
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(10000)) {
            $process.Kill()
            $process.WaitForExit()
            throw "launcher_child_timeout"
        }
        if (-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdoutTask, $stderrTask), 10000)) {
            throw "launcher_child_stream_timeout"
        }
        [pscustomobject]@{
            exit_code = [int]$process.ExitCode
            output = [string]($stdoutTask.Result + $stderrTask.Result)
            child_exited = [bool]$process.HasExited
        }
    }
    finally {
        if ($null -ne $process) {
            $process.Dispose()
        }
        $env:LOCALAPPDATA = $previousLocalAppData
    }
}

function New-PostWriteFaultLauncherScript([string]$Destination) {
    $needle = '[IO.File]::WriteAllText($Path, $Content, [Text.ASCIIEncoding]::new())'
    $replacement = @'
[IO.File]::WriteAllText($Path, $Content, [Text.ASCIIEncoding]::new())
    if ([string]$env:RAMSHARED_TEST_FAIL_AFTER_LAUNCHER_WRITE -ceq [IO.Path]::GetFileName($Path)) {
        throw "launcher_test_post_write_failure"
    }
'@
    $script = Get-Content -Raw -LiteralPath $target
    if (-not $script.Contains($needle)) {
        throw "launcher fault fixture cannot find write seam"
    }
    [IO.File]::WriteAllText($Destination, $script.Replace($needle, $replacement), [Text.UTF8Encoding]::new($false))
}

function New-UninstallManifestFaultLauncherScript([string]$Destination) {
    $needle = '$Manifest.state = "uninstalled"'
    $replacement = 'throw "launcher_test_uninstall_pre_manifest_failure"'
    $script = Get-Content -Raw -LiteralPath $target
    if (-not $script.Contains($needle)) {
        throw "launcher uninstall fault fixture cannot find manifest seam"
    }
    [IO.File]::WriteAllText($Destination, $script.Replace($needle, $replacement), [Text.UTF8Encoding]::new($false))
}

function New-BackupStagingFaultLauncherScript([string]$Destination, [string]$Mode) {
    $script = Get-Content -Raw -LiteralPath $target
    if ($Mode -ceq 'copy') {
        $needle = '$created_backup_paths += $backupPath'
        $replacement = @'
$created_backup_paths += $backupPath
                throw "launcher_test_backup_copy_postwrite_failure"
'@
    } else {
        $needle = 'original_sha256 = if ($originalExists) { Get-LauncherSha256 -Path $backupPath } else { "" }'
        $replacement = @'
original_sha256 = if ($originalExists) { $hash = Get-LauncherSha256 -Path $backupPath; throw "launcher_test_backup_hash_failure"; $hash } else { "" }
'@
    }
    if (-not $script.Contains($needle)) { throw "launcher backup staging fault fixture cannot find $Mode seam" }
    [IO.File]::WriteAllText($Destination, $script.Replace($needle, $replacement), [Text.UTF8Encoding]::new($false))
}

function Assert-BackupStagingFailureLeavesNoForeignState(
    [string]$PowerShellPath,
    [string]$FaultScript,
    [string]$LocalAppData,
    [string]$ExpectedFailure
) {
    $launcherRoot = Join-Path $LocalAppData "RamShared\launchers"
    $backupRoot = Join-Path $LocalAppData "RamShared\launcher-backup"
    New-Item -ItemType Directory -Path $launcherRoot -Force | Out-Null
    foreach ($name in @("ramshared-shell.cmd", "ramshared-terminal.cmd", "ramshared-vscode.cmd")) {
        [IO.File]::WriteAllText((Join-Path $launcherRoot $name), "@echo BACKUP-ORIGINAL-$name", [Text.UTF8Encoding]::new($false))
    }
    $failed = Invoke-ManufacturedLauncher $PowerShellPath $FaultScript $LocalAppData "install"
    if ($failed.exit_code -eq 0 -or $failed.output -notmatch $ExpectedFailure) {
        throw "launcher_backup_staging_failure did not reach $ExpectedFailure"
    }
    if ((Test-Path -LiteralPath $backupRoot -PathType Container) -and
        $null -ne (Get-ChildItem -LiteralPath $backupRoot -Force -ErrorAction SilentlyContinue | Select-Object -First 1)) {
        throw "launcher_backup_staging_failure left partial or foreign staging"
    }
    $green = Invoke-ManufacturedLauncher $PowerShellPath $target $LocalAppData "install"
    if ($green.exit_code -ne 0) { throw "launcher_backup_staging_failure blocked future clean install: $($green.output)" }
}

function Assert-UninstallFailureRestoresTargetsAndManifest(
    [string]$PowerShellPath,
    [string]$FaultScript,
    [string]$LocalAppData
) {
    $install = Invoke-ManufacturedLauncher $PowerShellPath $FaultScript $LocalAppData "install"
    if ($install.exit_code -ne 0) { throw "launcher_uninstall_transaction fixture install failed: $($install.output)" }
    $launcherRoot = Join-Path $LocalAppData "RamShared\launchers"
    $backupManifest = Join-Path $LocalAppData "RamShared\launcher-backup\launcher-backup-manifest.json"
    $uninstall = Invoke-ManufacturedLauncher $PowerShellPath $FaultScript $LocalAppData "uninstall"
    if ($uninstall.exit_code -eq 0 -or $uninstall.output -notmatch "launcher_test_uninstall_pre_manifest_failure") {
        throw "launcher_uninstall_failure did not reach the transaction rollback seam"
    }
    $manifest = Get-Content -LiteralPath $backupManifest -Raw | ConvertFrom-Json
    if ([string]$manifest.state -cne "installed") {
        throw "launcher_uninstall_failure did not restore the installed manifest state"
    }
    foreach ($entry in @($manifest.entries)) {
        $targetPath = Join-Path $launcherRoot ([string]$entry.name)
        if (-not (Test-Path -LiteralPath $targetPath -PathType Leaf) -or
            (Get-FileHash -LiteralPath $targetPath -Algorithm SHA256).Hash.ToUpperInvariant() -cne [string]$entry.installed_sha256) {
            throw "launcher_uninstall_failure did not restore installed target $($entry.name)"
        }
    }
}

function Assert-PostWriteFailureRestoresLaunchers(
    [string]$PowerShellPath,
    [string]$FaultScript,
    [string]$LocalAppData,
    [string]$FailName
) {
    $launcherRoot = Join-Path $LocalAppData "RamShared\launchers"
    $backupRoot = Join-Path $LocalAppData "RamShared\launcher-backup"
    New-Item -ItemType Directory -Path $launcherRoot -Force | Out-Null
    foreach ($name in @("ramshared-shell.cmd", "ramshared-terminal.cmd", "ramshared-vscode.cmd")) {
        [IO.File]::WriteAllText((Join-Path $launcherRoot $name), "@echo ORIGINAL-$name", [Text.UTF8Encoding]::new($false))
    }

    $previousFailure = $env:RAMSHARED_TEST_FAIL_AFTER_LAUNCHER_WRITE
    try {
        $env:RAMSHARED_TEST_FAIL_AFTER_LAUNCHER_WRITE = $FailName
        $result = Invoke-ManufacturedLauncher $PowerShellPath $FaultScript $LocalAppData "install"
    }
    finally {
        $env:RAMSHARED_TEST_FAIL_AFTER_LAUNCHER_WRITE = $previousFailure
    }
    if ($result.exit_code -eq 0 -or $result.output -notmatch "launcher_test_post_write_failure") {
        throw "launcher_post_write_failure_$FailName did not fail at the injected write"
    }
    foreach ($name in @("ramshared-shell.cmd", "ramshared-terminal.cmd", "ramshared-vscode.cmd")) {
        $targetPath = Join-Path $launcherRoot $name
        if ([IO.File]::ReadAllText($targetPath) -cne "@echo ORIGINAL-$name") {
            throw "launcher_post_write_failure_$FailName did not restore $name"
        }
        if (Test-Path -LiteralPath (Join-Path $backupRoot $name) -PathType Leaf) {
            throw "launcher_post_write_failure_$FailName left backup staging for $name"
        }
    }
    if (Test-Path -LiteralPath (Join-Path $backupRoot "launcher-backup-manifest.json") -PathType Leaf) {
        throw "launcher_post_write_failure_$FailName committed the manifest before verified targets"
    }
}

$manufacturedRoot = Join-Path ([IO.Path]::GetTempPath()) `
    ("ramshared-launcher-static-" + [guid]::NewGuid().ToString("N"))
try {
    $localAppData = Join-Path $manufacturedRoot "localappdata"
    $launcherRoot = Join-Path $localAppData "RamShared\launchers"
    New-Item -ItemType Directory -Path $launcherRoot -Force | Out-Null
    $shellTarget = Join-Path $launcherRoot "ramshared-shell.cmd"
    [IO.File]::WriteAllText($shellTarget, "@echo ORIGINAL-LAUNCHER", [Text.UTF8Encoding]::new($false))
    $currentPowerShell = (Get-Process -Id $PID -ErrorAction Stop).Path
    if ([string]::IsNullOrWhiteSpace($currentPowerShell) -or
        -not (Test-Path -LiteralPath $currentPowerShell -PathType Leaf)) {
        throw "launcher backup manufactured test cannot resolve current PowerShell"
    }

    $firstInstall = Invoke-ManufacturedLauncher $currentPowerShell $target $localAppData "install"
    if ($firstInstall.exit_code -ne 0) {
        throw "launcher first install failed: $($firstInstall.output)"
    }
    if (-not $firstInstall.child_exited) {
        throw "launcher first install left a child process running"
    }
    $backupPath = Join-Path $localAppData "RamShared\launcher-backup\ramshared-shell.cmd"
    $firstBackupHash = (Get-FileHash -LiteralPath $backupPath -Algorithm SHA256).Hash
    $secondInstall = Invoke-ManufacturedLauncher $currentPowerShell $target $localAppData "install"
    if ($secondInstall.exit_code -ne 0) {
        throw "launcher second install failed: $($secondInstall.output)"
    }
    if (-not $secondInstall.child_exited) {
        throw "launcher second install left a child process running"
    }
    $secondBackupHash = (Get-FileHash -LiteralPath $backupPath -Algorithm SHA256).Hash
    if ($firstBackupHash -cne $secondBackupHash -or
        [IO.File]::ReadAllText($backupPath) -cne "@echo ORIGINAL-LAUNCHER") {
        throw "launcher_second_install_preserves_original_backup failed: original backup changed"
    }

    # Mutate only the child command line for the cross-distro transaction
    # regression; it must leave the sealed default-distro install untouched.
    $previousLocalAppData = $env:LOCALAPPDATA
    try {
        $env:LOCALAPPDATA = $localAppData
        $foreign = & $currentPowerShell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $target -Action uninstall -Run -Distro 'Foreign-Ubuntu' 2>&1
        if ($LASTEXITCODE -eq 0 -or ($foreign -join "`n") -notmatch 'launcher_manifest_distro_mismatch') {
            throw 'launcher_cross_distro_uninstall_was_not_refused'
        }
    } finally { $env:LOCALAPPDATA = $previousLocalAppData }
    $sealedManifest = Get-Content -LiteralPath (Join-Path $localAppData 'RamShared\launcher-backup\launcher-backup-manifest.json') -Raw | ConvertFrom-Json
    if ([string]$sealedManifest.distro -cne 'Ubuntu-24.04') { throw 'launcher_manifest_distro_not_sealed' }

    [IO.File]::WriteAllText($shellTarget, "@echo OPERATOR-EDIT", [Text.UTF8Encoding]::new($false))
    $uninstall = Invoke-ManufacturedLauncher $currentPowerShell $target $localAppData "uninstall"
    if ($uninstall.exit_code -eq 0 -or
        $uninstall.output -notmatch "launcher_target_modified_after_install") {
        throw "launcher_restore_refuses_operator_modified_target failed: modified target was overwritten"
    }
    if (-not $uninstall.child_exited) {
        throw "launcher_expected_refusal_left_child_running"
    }
    if ([IO.File]::ReadAllText($shellTarget) -cne "@echo OPERATOR-EDIT") {
        throw "launcher_restore_refuses_operator_modified_target failed: operator edit was lost"
    }

    $faultScript = Join-Path $manufacturedRoot "Manage-RamSharedLaunchers-fault.ps1"
    New-PostWriteFaultLauncherScript $faultScript
    foreach ($failureName in @("ramshared-shell.cmd", "ramshared-terminal.cmd", "ramshared-vscode.cmd")) {
        $failureLocalAppData = Join-Path $manufacturedRoot ("post-write-" + $failureName)
        Assert-PostWriteFailureRestoresLaunchers $currentPowerShell $faultScript $failureLocalAppData $failureName
    }
    $uninstallFaultScript = Join-Path $manufacturedRoot "Manage-RamSharedLaunchers-uninstall-fault.ps1"
    New-UninstallManifestFaultLauncherScript $uninstallFaultScript
    Assert-UninstallFailureRestoresTargetsAndManifest $currentPowerShell $uninstallFaultScript `
        (Join-Path $manufacturedRoot "uninstall-transaction")

    foreach ($case in @(
            [pscustomobject]@{ name = 'copy'; failure = 'launcher_test_backup_copy_postwrite_failure' },
            [pscustomobject]@{ name = 'hash'; failure = 'launcher_test_backup_hash_failure' })) {
        $backupFaultScript = Join-Path $manufacturedRoot ("Manage-RamSharedLaunchers-backup-" + $case.name + ".ps1")
        New-BackupStagingFaultLauncherScript $backupFaultScript $case.name
        Assert-BackupStagingFailureLeavesNoForeignState $currentPowerShell $backupFaultScript `
            (Join-Path $manufacturedRoot ("backup-" + $case.name)) $case.failure
    }

}
finally {
    Remove-Item -LiteralPath $manufacturedRoot -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Output "PASS launcher_second_install_preserves_original_backup"
Write-Output "PASS launcher_restore_refuses_operator_modified_target"
Write-Output "PASS launcher_post_write_failures_restore_targets_before_manifest_commit"
Write-Output "PASS launcher_expected_refusal_is_captured_and_child_exits"
Write-Output "PASS workload_launchers_are_explicit_and_reversible"
Write-Output "PASS launcher_uninstall_failures_restore_all_targets_and_manifest"
Write-Output "PASS launcher_uninstall_refuses_cross_distro_manifest_mismatch"
Write-Output "PASS launcher_partial_backup_copy_or_hash_failure_leaves_no_staging"
