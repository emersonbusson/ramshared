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
    "C:\Users\emedev\AppData\Local\Temp",
    "C:\Users\emedev\AppData\Local\Docker",
    "C:\Users\emedev\AppData\Local\Packages",
    "C:\Users\emedev\AppData\Local\pnpm-store",
    "C:\Users\emedev\AppData\Local\pnpm",
    "C:\Users\emedev\AppData\Local\npm-cache",
    "C:\Users\emedev\AppData\Local\Microsoft\Windows\INetCache",
    "C:\Users\emedev\AppData\Local\Microsoft\Windows\DeliveryOptimization",
    "C:\Users\emedev\AppData\Local\wsl",
    "C:\Users\emedev\Downloads",
    "C:\Users\emedev\.cargo",
    "C:\Users\emedev\.rustup",
    "C:\Users\emedev\ramshared-src",
    "C:\Users\emedev\ramshared-drill",
    "C:\Program Files\Microsoft Visual Studio",
    "C:\Program Files (x86)\Windows Kits",
    "C:\Program Files\NVIDIA GPU Computing Toolkit",
    "C:\Program Files\dotnet",
    "C:\ProgramData\Docker",
    "C:\ProgramData\Microsoft\VisualStudio"
)

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
