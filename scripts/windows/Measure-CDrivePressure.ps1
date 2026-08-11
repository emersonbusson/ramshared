#Requires -RunAsAdministrator
# Quick C: free space + size of known heavy paths (no full C:\ walk).
$ErrorActionPreference = "SilentlyContinue"

function Get-SizeGB([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $item = Get-Item -LiteralPath $Path -Force
    if (-not $item.PSIsContainer) {
        return [math]::Round($item.Length / 1GB, 2)
    }
    $sum = 0L
    Get-ChildItem -LiteralPath $Path -Recurse -Force -File -ErrorAction SilentlyContinue |
        ForEach-Object { $sum += $_.Length }
    return [math]::Round($sum / 1GB, 2)
}

Write-Host "=== FREE SPACE ===" -ForegroundColor Cyan
Get-Volume -DriveLetter C, R, V, E, G | Select-Object DriveLetter, FileSystemLabel,
    @{n = "SizeGB"; e = { [math]::Round($_.Size / 1GB, 1) } },
    @{n = "FreeGB"; e = { [math]::Round($_.SizeRemaining / 1GB, 1) } } |
    Format-Table -AutoSize

$profilePaths = if ([string]::IsNullOrWhiteSpace($env:USERPROFILE)) { @() } else { @(
    "AppData\Local\Temp",
    "AppData\Local\Docker",
    "AppData\Local\Packages",
    "AppData\Local\pnpm-store",
    "AppData\Local\pnpm",
    "AppData\Local\npm-cache",
    "AppData\Local\Microsoft\Windows\INetCache",
    "AppData\Local\Microsoft\Windows\DeliveryOptimization",
    "AppData\Local\wsl",
    "Downloads",
    ".cargo",
    ".rustup"
) | ForEach-Object { Join-Path $env:USERPROFILE $_ } }

$paths = @(
    "C:\Hyper-V",
    "C:\ProgramData\Microsoft\Windows\Virtual Hard Disks",
    "C:\ProgramData\Microsoft\Windows\Hyper-V",
    "C:\ProgramData\Package Cache",
    "C:\Windows\SoftwareDistribution\Download",
    "C:\Windows\Temp",
    "C:\Windows\WinSxS",
    "C:\Windows\Installer",
    "C:\Windows\System32\DriverStore\FileRepository",
    "C:\pagefile.sys",
    "C:\hiberfil.sys",
    "C:\swapfile.sys",
    "C:\ramshared\src",
    "C:\ramshared\artifacts",
    "C:\Program Files\Microsoft Visual Studio",
    "C:\Program Files (x86)\Windows Kits",
    "C:\Program Files\NVIDIA GPU Computing Toolkit",
    "C:\Program Files\dotnet",
    "C:\ProgramData\Docker",
    "C:\ProgramData\Microsoft\VisualStudio"
) + $profilePaths

Write-Host "=== KNOWN HEAVY PATHS (GB) ===" -ForegroundColor Cyan
$rows = foreach ($p in $paths) {
    $g = Get-SizeGB $p
    if ($null -ne $g) {
        [pscustomobject]@{ GB = $g; Path = $p }
    }
}
$rows | Sort-Object GB -Descending | Format-Table -AutoSize

Write-Host "=== HYPER-V VMs ===" -ForegroundColor Cyan
Get-VM | Select-Object Name, State, Path | Format-Table -AutoSize
Get-VMHardDiskDrive -VMName * | Select-Object VMName, Path | Format-Table -AutoSize

Write-Host "=== LARGE FILES under C:\Hyper-V and VHD defaults ===" -ForegroundColor Cyan
@(
    "C:\Hyper-V",
    "C:\ProgramData\Microsoft\Windows\Virtual Hard Disks"
) | ForEach-Object {
    if (Test-Path $_) {
        Get-ChildItem $_ -Recurse -File -Force -ErrorAction SilentlyContinue |
            Sort-Object Length -Descending |
            Select-Object -First 15 @{n = "GB"; e = { [math]::Round($_.Length / 1GB, 2) } }, FullName
    }
} | Format-Table -AutoSize

Write-Host "DONE Measure-CDrivePressure" -ForegroundColor Green
