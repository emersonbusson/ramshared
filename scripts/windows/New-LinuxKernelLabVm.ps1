#Requires -RunAsAdministrator
# Create Hyper-V Gen2 Ubuntu lab VM with disks on R:\ (RUSSIA).
# Path 1: generic kernel lab (no GPU). See Prepare-DdaGpu.ps1 for GPU experiments.
param(
    [string]$VmName = "linux-kernel-lab",
    [int]$MemoryGB = 8,
    [int]$VhdSizeGB = 80,
    [int]$CpuCount = 4,
    [string]$Root = "R:\Hyper-V",
    [string]$SwitchName = "Default Switch",
    [switch]$SkipDownload,
    [switch]$Start
)

$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host ("==> " + $Message) -ForegroundColor Cyan
}

if (-not (Test-Path "R:\")) {
    throw "Drive R: not found. Lab disks must live on RUSSIA."
}

$VmDir = Join-Path $Root $VmName
$IsoDir = Join-Path $Root "iso"
$VhdPath = Join-Path $VmDir ($VmName + ".vhdx")
$IsoName = "ubuntu-24.04.2-live-server-amd64.iso"
$IsoPath = Join-Path $IsoDir $IsoName
$IsoUrl = "https://releases.ubuntu.com/24.04.2/ubuntu-24.04.2-live-server-amd64.iso"
$IsoUrlFallback = "https://cdimage.ubuntu.com/releases/24.04.2/release/ubuntu-24.04.2-live-server-amd64.iso"

$vol = Get-Volume -DriveLetter R
$freeGB = [math]::Round($vol.SizeRemaining / 1GB, 1)
Write-Step ("R: " + $vol.FileSystemLabel + " free=" + $freeGB + " GB")
if ($freeGB -lt ($VhdSizeGB + 5)) {
    throw ("Not enough free space on R: need about " + ($VhdSizeGB + 5) + " GB free, have " + $freeGB + " GB")
}

New-Item -ItemType Directory -Force -Path $VmDir, $IsoDir | Out-Null

$needDownload = $true
if ($SkipDownload -and (Test-Path $IsoPath)) {
    $needDownload = $false
}
if ((Test-Path $IsoPath) -and ((Get-Item $IsoPath).Length -gt 1GB)) {
    Write-Step ("ISO present: " + $IsoPath)
    $needDownload = $false
}

if ($needDownload) {
    Write-Step ("Downloading Ubuntu 24.04.2 live-server to " + $IsoPath)
    if (Test-Path $IsoPath) {
        Remove-Item $IsoPath -Force
    }
    try {
        Start-BitsTransfer -Source $IsoUrl -Destination $IsoPath -DisplayName "Ubuntu-ISO"
    } catch {
        Write-Warning ("Primary URL failed: " + $_.Exception.Message)
        if (Test-Path $IsoPath) {
            Remove-Item $IsoPath -Force
        }
        Start-BitsTransfer -Source $IsoUrlFallback -Destination $IsoPath -DisplayName "Ubuntu-ISO-fallback"
    }
    if (-not (Test-Path $IsoPath) -or ((Get-Item $IsoPath).Length -lt 1GB)) {
        throw ("ISO download failed or file too small: " + $IsoPath)
    }
    Write-Step ("ISO OK sizeGB=" + [math]::Round((Get-Item $IsoPath).Length / 1GB, 2))
}

$existing = Get-VM -Name $VmName -ErrorAction SilentlyContinue
if ($null -ne $existing) {
    Write-Step ("VM already exists: " + $VmName)
    $existing | Format-List Name, State, Generation, Path
    if ($Start) {
        if ($existing.State -eq "Off") {
            Start-VM -Name $VmName
        }
        Start-Process -FilePath "vmconnect.exe" -ArgumentList @("localhost", $VmName) -ErrorAction SilentlyContinue
    }
    return
}

$sw = Get-VMSwitch -Name $SwitchName -ErrorAction SilentlyContinue
if ($null -eq $sw) {
    $sw = Get-VMSwitch | Select-Object -First 1
    if ($null -eq $sw) {
        throw "No Hyper-V virtual switch found"
    }
    $SwitchName = $sw.Name
    Write-Step ("Using switch: " + $SwitchName)
}

Write-Step ("Creating dynamic VHDX " + $VhdSizeGB + " GB at " + $VhdPath)
New-VHD -Path $VhdPath -SizeBytes ($VhdSizeGB * 1GB) -Dynamic | Out-Null

Write-Step ("Creating Gen2 VM " + $VmName)
New-VM -Name $VmName -Generation 2 -MemoryStartupBytes ($MemoryGB * 1GB) `
    -VHDPath $VhdPath -SwitchName $SwitchName -Path $VmDir | Out-Null

Set-VM -Name $VmName -ProcessorCount $CpuCount -AutomaticCheckpointsEnabled $false `
    -CheckpointType Production -Notes "RamShared kernel lab on R: RUSSIA. No GPU until DDA."

try {
    Set-VMProcessor -VMName $VmName -ExposeVirtualizationExtensions $true
} catch {
    Write-Warning ("Nested virt: " + $_.Exception.Message)
}

Set-VMFirmware -VMName $VmName -EnableSecureBoot On -SecureBootTemplate "MicrosoftUEFICertificateAuthority"
Set-VMDvdDrive -VMName $VmName -Path $IsoPath
$dvd = Get-VMDvdDrive -VMName $VmName
Set-VMFirmware -VMName $VmName -FirstBootDevice $dvd
Set-VM -Name $VmName -AutomaticStartAction Nothing -AutomaticStopAction ShutDown

Write-Step "VM created"
Get-VM -Name $VmName | Format-List Name, State, Generation, Path, ProcessorCount

Write-Host "Next: vmconnect.exe localhost linux-kernel-lab and install Ubuntu" -ForegroundColor Green
Write-Host "After install: Set-VMDvdDrive -VMName linux-kernel-lab -Path `$null" -ForegroundColor Green
Write-Host "DDA later: Prepare-DdaGpu.ps1 -Inventory" -ForegroundColor Green

if ($Start) {
    Write-Step "Starting VM"
    Start-VM -Name $VmName
    Start-Process -FilePath "vmconnect.exe" -ArgumentList @("localhost", $VmName) -ErrorAction SilentlyContinue
}

Write-Host "DONE New-LinuxKernelLabVm" -ForegroundColor Green
