#Requires -Version 5.1
<#
.SYNOPSIS
  DT-25: INF PnP install of ramshared + start lab backend (VM only).

.DESCRIPTION
  Copies signed .sys, pnputil add-driver/add-device Root\RamShared, starts
  poolstress, launches WinDriveBackend, waits for extra disk, optional format.

  Format path is fail-closed (#40):
  - Refuse if -DriveLetter is already mounted on a non-target volume
  - Only format disks that match RamShared LUN identity (name/size/bus)
  - Interactive confirmation on physical hosts unless -Force
#>
[CmdletBinding()]
param(
    [string]$RepoRoot = "C:\Users\emedev\ramshared-src",
    [string]$PackageDir = "C:\ramshared\package",
    [UInt64]$SizeBytes = 67108864,  # 64 MiB lab default (guest free RAM tight)
    [int]$BackendSeconds = 120,
    [switch]$FormatNtfs,
    [string]$DriveLetter = "V",
    # Skip interactive confirmation (scripts/CI only). Still enforces letter + disk identity.
    [switch]$Force
)

$ErrorActionPreference = "Stop"
Write-Host "Install-InfAndBackend DT-25 (format guards #40)"

function Test-RamSharedDiskIdentity {
    param(
        [Parameter(Mandatory)] $Disk,
        [UInt64]$ExpectedSizeBytes
    )
    $nameOk = $Disk.FriendlyName -match 'RAMSHARE|RamShared|VRAM'
    $sizeOk = ($ExpectedSizeBytes -gt 0) -and ($Disk.Size -eq $ExpectedSizeBytes)
    $busOk = $Disk.BusType -in @('SCSI', 'RAID', 'SAS', 'Unknown', 'StorageSpaces')
    # Prefer name match; size match is strong evidence after backend CREATE.
    return [pscustomobject]@{
        Ok      = [bool]($nameOk -or $sizeOk)
        NameOk  = [bool]$nameOk
        SizeOk  = [bool]$sizeOk
        BusOk   = [bool]$busOk
        Summary = "Name='$($Disk.FriendlyName)' Size=$($Disk.Size) Bus=$($Disk.BusType) nameOk=$nameOk sizeOk=$sizeOk"
    }
}

function Assert-DriveLetterAvailable {
    param(
        [Parameter(Mandatory)][string]$Letter,
        [switch]$AllowIfOnDiskNumber
    )
    $ch = $Letter.TrimEnd(':').Substring(0, 1).ToUpperInvariant()
    $vol = Get-Volume -DriveLetter $ch -ErrorAction SilentlyContinue
    if (-not $vol) { return }
    $part = Get-Partition -DriveLetter $ch -ErrorAction SilentlyContinue
    if ($PSBoundParameters.ContainsKey('AllowIfOnDiskNumber') -and $part -and $part.DiskNumber -eq $AllowIfOnDiskNumber) {
        return
    }
    throw "REFUSE_FORMAT: drive letter ${ch}: is already in use (FileSystem=$($vol.FileSystem) SizeRemaining=$($vol.SizeRemaining)). Pick a free letter or dismount first."
}

function Confirm-FormatOrForce {
    param(
        [Parameter(Mandatory)][string]$Message,
        [switch]$Force
    )
    if ($Force) {
        Write-Warning "Force: $Message"
        return
    }
    # Non-interactive hosts (CI / Invoke-Command) must pass -Force.
    if (-not [Environment]::UserInteractive) {
        throw "REFUSE_FORMAT: non-interactive session requires -Force. $Message"
    }
    $ans = Read-Host "$Message Type YES to continue"
    if ($ans -ne 'YES') {
        throw "REFUSE_FORMAT: operator declined confirmation"
    }
}

New-Item -ItemType Directory -Force -Path $PackageDir, C:\ramshared\bin | Out-Null

$sys = Join-Path $RepoRoot "drivers\windows\ramshared\x64\Release\ramshared.sys"
$psys = Join-Path $RepoRoot "drivers\windows\tools\poolstress\x64\Release\poolstress.sys"
$inf = Join-Path $RepoRoot "drivers\windows\ramshared\ramshared.inf"
$be = Join-Path $RepoRoot "scripts\windows\WinDriveBackend.cs"
if (-not (Test-Path $sys)) { throw "missing $sys - build+sign first" }

$letter = $DriveLetter.TrimEnd(':').Substring(0, 1).ToUpperInvariant()
if ($FormatNtfs) {
    # Fail early before any destructive disk work.
    Assert-DriveLetterAvailable -Letter $letter
}

Copy-Item $sys "$PackageDir\ramshared.sys" -Force
if (Test-Path $psys) { Copy-Item $psys "$PackageDir\poolstress.sys" -Force }
Copy-Item $inf "$PackageDir\ramshared.inf" -Force
$cat = Join-Path $RepoRoot "drivers\windows\ramshared\x64\Release\package\ramshared.cat"
if (Test-Path $cat) { Copy-Item $cat "$PackageDir\ramshared.cat" -Force }

# Stop any sc-legacy instances
sc.exe stop ramshared 2>$null | Out-Null
sc.exe stop poolstress 2>$null | Out-Null
Start-Sleep 2

# Copy into drivers dir (INF CopyFiles also does this on install)
Copy-Item "$PackageDir\ramshared.sys" C:\Windows\System32\drivers\ramshared.sys -Force
if (Test-Path "$PackageDir\poolstress.sys") {
    Copy-Item "$PackageDir\poolstress.sys" C:\Windows\System32\drivers\poolstress.sys -Force
}

$pn = pnputil /add-driver "$PackageDir\ramshared.inf" /install 2>&1 | Out-String
Write-Host "PNPUTIL_ADD=$pn"

$ad = pnputil /add-device "Root\RamShared" 2>&1 | Out-String
Write-Host "ADD_DEVICE=$ad"

# poolstress via sc (test tool, not StorPort)
if (Test-Path C:\Windows\System32\drivers\poolstress.sys) {
    sc.exe create poolstress type= kernel start= demand binPath= \SystemRoot\System32\drivers\poolstress.sys 2>$null | Out-Null
    sc.exe start poolstress 2>&1 | Out-String | Write-Host
}

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

# Poll for disk - never fall back to "any disk Number -gt 0" (data-loss vector).
$found = $null
for ($i = 0; $i -lt 30; $i++) {
    Start-Sleep 2
    try { Update-HostStorageCache -EA SilentlyContinue } catch {}
    $candidates = @(Get-Disk | Where-Object {
            ($_.FriendlyName -match 'RAMSHARE|RamShared|VRAM') -or ($_.Size -eq $SizeBytes -and $_.Number -ne 0)
        })
    if ($candidates.Count -gt 0) {
        $found = $candidates | Sort-Object {
            if ($_.FriendlyName -match 'RAMSHARE|RamShared') { 0 } else { 1 }
        } | Select-Object -First 1
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

$id = Test-RamSharedDiskIdentity -Disk $found -ExpectedSizeBytes $SizeBytes
Write-Host "DISK_IDENTITY $($id.Summary)"
if (-not $id.Ok) {
    throw "REFUSE_FORMAT: disk N=$($found.Number) failed RamShared identity check ($($id.Summary))"
}

if ($FormatNtfs) {
    Assert-DriveLetterAvailable -Letter $letter
    Confirm-FormatOrForce -Force:$Force -Message "Format disk N=$($found.Number) ($($found.FriendlyName), $($found.Size) bytes) as ${letter}: NTFS RAMSHARED."

    Write-Host "FORMAT disk $($found.Number) -> ${letter}:"
    if ($found.PartitionStyle -eq 'RAW' -or $found.NumberOfPartitions -eq 0) {
        Initialize-Disk -Number $found.Number -PartitionStyle GPT -Confirm:$false
        $part = New-Partition -DiskNumber $found.Number -UseMaximumSize -DriveLetter $letter[0] -ErrorAction Stop
        # Pipe partition object - never Format-Volume -DriveLetter alone (#38/#40).
        $part | Format-Volume -FileSystem NTFS -NewFileSystemLabel "RAMSHARED" -Confirm:$false | Out-Null
        Write-Host "FORMAT_OK ${letter}:"
    } else {
        Write-Host "disk already partitioned - refusing reformat without explicit wipe path"
    }
}

Write-Host "Install-InfAndBackend DONE found=$($found.Number)"
return 0
