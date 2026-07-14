#Requires -Version 5.1
<#
.SYNOPSIS
  Start lab backend + ensure RamShared LUN (VM only).

.DESCRIPTION
  Day-0 lab stand-in for ramshared-winsvc start: WinDriveBackend CREATE/REGISTER.
  Does not enable pagefile (caller may use NtPagefileHelper separately).

  Format path (#40): only Clear-Disk / Format on disks that pass RamShared
  identity checks. Never formats by drive letter alone.
#>
[CmdletBinding()]
param(
    [UInt64]$SizeBytes = 67108864,
    [int]$HoldSeconds = 600,
    [string]$Backend = "C:\ramshared\bin\WinDriveBackend.exe",
    [string]$PackageDir = "C:\ramshared\package",
    [string]$PreferredLetter = "D",
    [switch]$FormatIfNeeded,
    # Skip interactive confirmation (SCM / CI). Still enforces disk identity.
    [switch]$Force
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Backend)) { throw "missing $Backend" }

function Test-RamSharedDiskIdentity {
    param([Parameter(Mandatory)] $Disk, [UInt64]$ExpectedSizeBytes)
    $nameOk = $Disk.FriendlyName -match 'RAMSHARE|RamShared|VRAM'
    $sizeOk = ($ExpectedSizeBytes -gt 0) -and ($Disk.Size -eq $ExpectedSizeBytes)
    return [bool]($nameOk -or $sizeOk)
}

Get-Process WinDriveBackend -EA SilentlyContinue | Stop-Process -Force
Start-Sleep 1

if (Test-Path "$PackageDir\ramshared.inf") {
    if (Test-Path "$PackageDir\ramshared-test.cer") {
        certutil -f -addstore Root "$PackageDir\ramshared-test.cer" 2>&1 | Out-Null
        certutil -f -addstore TrustedPublisher "$PackageDir\ramshared-test.cer" 2>&1 | Out-Null
    }
    if (Test-Path "$PackageDir\ramshared.sys") {
        try {
            Copy-Item "$PackageDir\ramshared.sys" C:\Windows\System32\drivers\ramshared.sys -Force -EA Stop
        } catch {
            Write-Host "SYS_COPY_SKIP=$($_.Exception.Message)"
        }
    }
    pnputil /add-driver "$PackageDir\ramshared.inf" /install 2>&1 | Out-Null
    # Always ensure a single Root\RamShared node exists (post-reboot PnP often empty).
    $ex = @(Get-PnpDevice -EA SilentlyContinue | Where-Object {
            $_.FriendlyName -match "RamShared" -or $_.InstanceId -match "RamShared"
        })
    if ($ex.Count -eq 0) {
        if (Test-Path C:\ramshared\bin\devcon.exe) {
            & C:\ramshared\bin\devcon.exe install "$PackageDir\ramshared.inf" Root\RamShared 2>&1 | Out-String | Write-Host
        } else {
            pnputil /add-device "Root\RamShared" 2>&1 | Out-String | Write-Host
        }
        Start-Sleep 3
    } else {
        foreach ($d in $ex) {
            try { Enable-PnpDevice -InstanceId $d.InstanceId -Confirm:$false -EA SilentlyContinue } catch {}
        }
    }
}

Remove-Item C:\ramshared\bin\backend.out, C:\ramshared\bin\backend.err -Force -EA SilentlyContinue
$p = Start-Process $Backend -ArgumentList @("$SizeBytes", "$HoldSeconds") `
    -RedirectStandardOutput C:\ramshared\bin\backend.out `
    -RedirectStandardError C:\ramshared\bin\backend.err `
    -PassThru -WindowStyle Hidden
Start-Sleep 8
Write-Host "BACKEND pid=$($p.Id)"
Write-Host ((Get-Content C:\ramshared\bin\backend.out -Raw -EA SilentlyContinue) -replace '\s+', ' ')

try { Update-HostStorageCache -EA SilentlyContinue } catch {}
$d = Get-Disk | Where-Object {
        ($_.FriendlyName -match "RAMSHARE|RamShared") -and ($_.Size -ge 1MB)
    } | Select-Object -First 1
if (-not $d) {
    # Fallback: exact size match (not "any disk > 0").
    $d = Get-Disk | Where-Object { $_.Number -ne 0 -and $_.Size -eq $SizeBytes } |
        Select-Object -First 1
}
if (-not $d) { throw "no RAMSHARE disk after backend start" }
if (-not (Test-RamSharedDiskIdentity -Disk $d -ExpectedSizeBytes $SizeBytes)) {
    throw "REFUSE_FORMAT: disk N=$($d.Number) failed RamShared identity (Name=$($d.FriendlyName) Size=$($d.Size))"
}
Write-Host "DISK N=$($d.Number) Size=$($d.Size) Name=$($d.FriendlyName)"

if ($FormatIfNeeded) {
    $letter = $PreferredLetter.TrimEnd(':').Substring(0, 1).ToUpperInvariant()
    $vol = Get-Volume -DriveLetter $letter -EA SilentlyContinue
    $alreadyOk = $false
    if ($vol -and $vol.FileSystem -eq "NTFS") {
        $partOnLetter = Get-Partition -DriveLetter $letter -EA SilentlyContinue
        if ($partOnLetter -and $partOnLetter.DiskNumber -eq $d.Number) {
            $alreadyOk = $true
            Write-Host "FORMAT_SKIP already NTFS on ${letter}: (disk $($d.Number))"
        } elseif ($vol) {
            throw "REFUSE_FORMAT: letter ${letter}: is mounted on another volume (disk $($partOnLetter.DiskNumber)); not RamShared disk $($d.Number)"
        }
    }

    if (-not $alreadyOk) {
        if (-not $Force) {
            if (-not [Environment]::UserInteractive) {
                # SCM OnStart is non-interactive: require pre-formatted volume or -Force via winsvc args.
                Write-Warning "FormatIfNeeded skipped in non-interactive mode without -Force (disk left RAW/unformatted)."
            } else {
                $ans = Read-Host "Clear+format disk N=$($d.Number) ($($d.FriendlyName)) Type YES"
                if ($ans -ne 'YES') { throw "REFUSE_FORMAT: operator declined" }
                $Force = $true
            }
        }
        if ($Force) {
            Get-Partition -DiskNumber $d.Number -EA SilentlyContinue |
                Remove-Partition -Confirm:$false -EA SilentlyContinue
            Clear-Disk -Number $d.Number -RemoveData -Confirm:$false -ErrorAction Stop
            Initialize-Disk -Number $d.Number -PartitionStyle MBR -Confirm:$false
            # Prefer requested letter if free; else assign any free letter.
            $letterFree = -not (Get-Volume -DriveLetter $letter -EA SilentlyContinue)
            if ($letterFree) {
                $part = New-Partition -DiskNumber $d.Number -UseMaximumSize -DriveLetter $letter[0] -ErrorAction Stop
            } else {
                $part = New-Partition -DiskNumber $d.Number -UseMaximumSize -AssignDriveLetter -ErrorAction Stop
            }
            # Format the partition object only - never "format X:" by letter alone.
            $part | Format-Volume -FileSystem NTFS -NewFileSystemLabel "RAMSHARED" -Confirm:$false | Out-Null
            $assigned = ($part | Get-Partition).DriveLetter
            Write-Host "FORMAT ${assigned}:NTFS disk=$($d.Number)"
        }
    }
}
exit 0
