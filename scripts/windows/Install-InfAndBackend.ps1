#Requires -Version 5.1
<#
.SYNOPSIS
  DT-25: INF PnP install of ramshared + start lab backend (VM only).

.DESCRIPTION
  Copies signed .sys, pnputil add-driver/add-device Root\RamShared, starts
  poolstress, launches WinDriveBackend, waits for extra disk, optional format.
#>
[CmdletBinding()]
param(
    [string]$RepoRoot = "C:\Users\emedev\ramshared-src",
    [string]$PackageDir = "C:\ramshared\package",
    [UInt64]$SizeBytes = 67108864,  # 64 MiB lab default (guest free RAM tight)
    [int]$BackendSeconds = 120,
    [switch]$FormatNtfs,
    [string]$DriveLetter = "V"
)

$ErrorActionPreference = "Continue"
Write-Host "Install-InfAndBackend DT-25"

New-Item -ItemType Directory -Force -Path $PackageDir, C:\ramshared\bin | Out-Null

$sys = Join-Path $RepoRoot "drivers\windows\ramshared\x64\Release\ramshared.sys"
$psys = Join-Path $RepoRoot "drivers\windows\tools\poolstress\x64\Release\poolstress.sys"
$inf = Join-Path $RepoRoot "drivers\windows\ramshared\ramshared.inf"
$be = Join-Path $RepoRoot "scripts\windows\WinDriveBackend.cs"
if (-not (Test-Path $sys)) { throw "missing $sys - build+sign first" }

Copy-Item $sys "$PackageDir\ramshared.sys" -Force
Copy-Item $psys "$PackageDir\poolstress.sys" -Force
Copy-Item $inf "$PackageDir\ramshared.inf" -Force
Copy-Item (Join-Path $RepoRoot "drivers\windows\ramshared\x64\Release\package\ramshared.cat") "$PackageDir\ramshared.cat" -Force

# Stop any sc-legacy instances
sc.exe stop ramshared 2>$null | Out-Null
sc.exe stop poolstress 2>$null | Out-Null
Start-Sleep 2

# Copy into drivers dir (INF CopyFiles also does this on install)
Copy-Item "$PackageDir\ramshared.sys" C:\Windows\System32\drivers\ramshared.sys -Force
Copy-Item "$PackageDir\poolstress.sys" C:\Windows\System32\drivers\poolstress.sys -Force

$pn = pnputil /add-driver "$PackageDir\ramshared.inf" /install 2>&1 | Out-String
Write-Host "PNPUTIL_ADD=$pn"

$ad = pnputil /add-device "Root\RamShared" 2>&1 | Out-String
Write-Host "ADD_DEVICE=$ad"

# poolstress via sc (test tool, not StorPort)
sc.exe create poolstress type= kernel start= demand binPath= \SystemRoot\System32\drivers\poolstress.sys 2>$null | Out-Null
sc.exe start poolstress 2>&1 | Out-String | Write-Host

Start-Sleep 3
Write-Host "SCSIAdapters:"
Get-PnpDevice -Class SCSIAdapter -EA SilentlyContinue |
  ForEach-Object { Write-Host "  $($_.Status) | $($_.FriendlyName) | $($_.InstanceId)" }

Write-Host "Disks before backend:"
Get-Disk | ForEach-Object { Write-Host "  N=$($_.Number) $($_.FriendlyName) $($_.Size)" }

# Build backend if needed
if (-not (Test-Path C:\ramshared\bin\WinDriveBackend.exe) -or (Test-Path $be)) {
    $csc = (Get-ChildItem "C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe" -EA SilentlyContinue).FullName
    if ($csc -and (Test-Path $be)) {
        & $csc /nologo /target:exe /platform:x64 /out:C:\ramshared\bin\WinDriveBackend.exe $be
    }
}
if (-not (Test-Path C:\ramshared\bin\WinDriveBackend.exe)) {
    throw "WinDriveBackend.exe missing"
}

Remove-Item C:\ramshared\bin\backend.out, C:\ramshared\bin\backend.err -Force -EA SilentlyContinue
$p = Start-Process C:\ramshared\bin\WinDriveBackend.exe `
    -ArgumentList @("$SizeBytes", "$BackendSeconds") `
    -RedirectStandardOutput C:\ramshared\bin\backend.out `
    -RedirectStandardError C:\ramshared\bin\backend.err `
    -PassThru -WindowStyle Hidden
Write-Host "BACKEND pid=$($p.Id)"

# Poll for disk
$found = $null
for ($i = 0; $i -lt 30; $i++) {
    Start-Sleep 2
    try { Update-HostStorageCache -EA SilentlyContinue } catch {}
    $disks = @(Get-Disk | Where-Object { $_.Number -ne 0 -and $_.Size -eq $SizeBytes })
    if ($disks.Count -eq 0) {
        $disks = @(Get-Disk | Where-Object { $_.FriendlyName -match 'RAMSHARE|VRAM|RamShared' })
    }
    if ($disks.Count -eq 0) {
        $disks = @(Get-Disk | Where-Object { $_.Number -gt 0 })
    }
    if ($disks.Count -gt 0) {
        $found = $disks[0]
        Write-Host "DISK_FOUND N=$($found.Number) Size=$($found.Size) Name=$($found.FriendlyName)"
        break
    }
    $bout = if (Test-Path C:\ramshared\bin\backend.out) { Get-Content C:\ramshared\bin\backend.out -Raw } else { "" }
    Write-Host "poll $i out=$($bout -replace '\r?\n',' | ')"
}

if (-not $found) {
    Write-Host "NO_EXTRA_DISK"
    Write-Host "backend.out=$(Get-Content C:\ramshared\bin\backend.out -Raw -EA SilentlyContinue)"
    Write-Host "backend.err=$(Get-Content C:\ramshared\bin\backend.err -Raw -EA SilentlyContinue)"
    Write-Host "All disks:"
    Get-Disk | Format-Table Number, FriendlyName, Size, BusType, OperationalStatus | Out-String | Write-Host
    Get-PnpDevice -Class DiskDrive -EA SilentlyContinue | Format-Table Status, FriendlyName, InstanceId | Out-String | Write-Host
    return 2
}

if ($FormatNtfs) {
    Write-Host "FORMAT disk $($found.Number) -> ${DriveLetter}:"
    if ($found.PartitionStyle -eq 'RAW' -or $found.NumberOfPartitions -eq 0) {
        Initialize-Disk -Number $found.Number -PartitionStyle GPT -Confirm:$false -EA SilentlyContinue
        $part = New-Partition -DiskNumber $found.Number -UseMaximumSize -DriveLetter $DriveLetter[0]
        Format-Volume -DriveLetter $DriveLetter[0] -FileSystem NTFS -NewFileSystemLabel "RAMSHARED" -Confirm:$false
        Write-Host "FORMAT_OK ${DriveLetter}:"
    } else {
        Write-Host "disk already partitioned"
    }
}

Write-Host "Install-InfAndBackend DONE found=$($found.Number)"
return 0
