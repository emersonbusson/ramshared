# Manage-WslKernelLab.ps1 - backup / restore / status for RamShared-Kernel WSL lab
#
# Live:   R:\WSL\RamShared-Kernel\ext4.vhdx   (not C:)
# Backup: E:\WSL-backup\RamShared-Kernel\
# Default product distro must remain Ubuntu-24.04
#
# Usage (elevated Windows PowerShell recommended for import/unregister):
#   .\Manage-WslKernelLab.ps1 -Status
#   .\Manage-WslKernelLab.ps1 -Export
#   .\Manage-WslKernelLab.ps1 -Smoke
#   .\Manage-WslKernelLab.ps1 -Restore   # DESTRUCTIVE: re-import from base tar
#
#Requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$Status,
    [switch]$Export,
    [switch]$Smoke,
    [switch]$Restore,
    [string]$LiveDir = 'R:\WSL\RamShared-Kernel',
    [string]$BackupDir = 'E:\WSL-backup\RamShared-Kernel',
    [string]$BackupTar = 'RamShared-Kernel-base.tar',
    [string]$Distro = 'RamShared-Kernel',
    [string]$ProductDefault = 'Ubuntu-24.04',
    [int]$VhdSizeGb = 40
)

$ErrorActionPreference = 'Stop'
$tarPath = Join-Path $BackupDir $BackupTar

function Ensure-Dir([string]$p) {
    if (-not (Test-Path -LiteralPath $p)) {
        New-Item -ItemType Directory -Path $p -Force | Out-Null
    }
}

function Set-ProductDefault {
    & wsl.exe --set-default $ProductDefault | Out-Null
}

if (-not ($Status -or $Export -or $Smoke -or $Restore)) {
    $Status = $true
}

if ($Status) {
    Write-Host '=== wsl -l -v ==='
    & wsl.exe -l -v
    Write-Host "`n=== live ==="
    if (Test-Path -LiteralPath $LiveDir) {
        Get-ChildItem -LiteralPath $LiveDir | Format-Table Name, Length, LastWriteTime -AutoSize
    } else {
        Write-Host "missing: $LiveDir"
    }
    Write-Host '=== backup ==='
    if (Test-Path -LiteralPath $BackupDir) {
        Get-ChildItem -LiteralPath $BackupDir | Format-Table Name, Length, LastWriteTime -AutoSize
    } else {
        Write-Host "missing: $BackupDir"
    }
}

if ($Export) {
    Ensure-Dir $BackupDir
    Write-Host "Stopping $Distro ..."
    & wsl.exe -t $Distro 2>$null | Out-Null
    Start-Sleep -Seconds 1
    Write-Host "Exporting -> $tarPath"
    & wsl.exe --export $Distro $tarPath
    if ($LASTEXITCODE -ne 0) { throw "wsl --export failed: $LASTEXITCODE" }
    Set-ProductDefault
    Get-Item -LiteralPath $tarPath | Format-List FullName, Length, LastWriteTime
}

if ($Smoke) {
    Write-Host "Smoke $Distro ..."
    & wsl.exe -d $Distro -- bash -lc 'whoami; sudo -n id; test -f /etc/ramshared/lab-profile && cat /etc/ramshared/lab-profile; uname -r; gcc --version | head -1'
    if ($LASTEXITCODE -ne 0) { throw "smoke failed: $LASTEXITCODE" }
    Set-ProductDefault
}

if ($Restore) {
    if (-not (Test-Path -LiteralPath $tarPath)) {
        throw "Backup missing: $tarPath - run -Export first or copy base tar"
    }
    Write-Host "DESTRUCTIVE: unregister + re-import $Distro from $tarPath"
    Write-Host "Live dir: $LiveDir"
    $confirm = Read-Host 'Type RESTORE to continue'
    if ($confirm -ne 'RESTORE') {
        Write-Host 'Aborted.'
        exit 1
    }
    & wsl.exe -t $Distro 2>$null | Out-Null
    & wsl.exe --unregister $Distro 2>$null | Out-Null
    Ensure-Dir $LiveDir
    # --vhd keeps modern layout; size hint only on some wsl versions
    & wsl.exe --import $Distro $LiveDir $tarPath --version 2
    if ($LASTEXITCODE -ne 0) { throw "wsl --import failed: $LASTEXITCODE" }
    Set-ProductDefault
    Write-Host 'Restore done. Run -Smoke next.'
    & wsl.exe -l -v
}
