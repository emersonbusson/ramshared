#Requires -Version 5.1
<#
.SYNOPSIS
  Start lab backend + ensure RamShared LUN (VM only).

.DESCRIPTION
  Day-0 lab stand-in for ramshared-winsvc start: WinDriveBackend CREATE/REGISTER.
  Does not enable pagefile (caller may use NtPagefileHelper separately).
#>
[CmdletBinding()]
param(
    [UInt64]$SizeBytes = 67108864,
    [int]$HoldSeconds = 600,
    [string]$Backend = "C:\ramshared\bin\WinDriveBackend.exe",
    [string]$PackageDir = "C:\ramshared\package",
    [switch]$FormatIfNeeded
)

$ErrorActionPreference = "Continue"
if (-not (Test-Path $Backend)) { throw "missing $Backend" }

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
$d = Get-Disk | Where-Object { $_.FriendlyName -match "RAMSHARE" -and $_.Size -ge 1MB } |
    Select-Object -First 1
if (-not $d) { throw "no RAMSHARE disk after backend start" }
Write-Host "DISK N=$($d.Number) Size=$($d.Size)"

if ($FormatIfNeeded) {
    $vol = Get-Volume -DriveLetter D -EA SilentlyContinue
    if (-not $vol -or $vol.FileSystem -ne "NTFS") {
        Get-Partition -DiskNumber $d.Number -EA SilentlyContinue |
            Remove-Partition -Confirm:$false -EA SilentlyContinue
        Clear-Disk -Number $d.Number -RemoveData -Confirm:$false -EA SilentlyContinue
        Initialize-Disk -Number $d.Number -PartitionStyle MBR -Confirm:$false
        $part = New-Partition -DiskNumber $d.Number -UseMaximumSize -AssignDriveLetter
        cmd /c "format $($part.DriveLetter): /fs:NTFS /q /y /v:RAMSHARED" | Out-Null
        Write-Host "FORMAT $($part.DriveLetter):NTFS"
    }
}
exit 0
