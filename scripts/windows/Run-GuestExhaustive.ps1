#Requires -Version 5.1
# Elevated: win11-drill deploy + IOCTL + optional Verifier reboot pass
param(
    [string]$VMName = "win11-drill",
    [string]$User = ".\drilladmin",
    [string]$Password = $env:RAMSHARED_DRILL_PASSWORD,
    [switch]$SkipVerifier,
    [switch]$LeaveVmOn
)
$ErrorActionPreference = "Stop"
if ([string]::IsNullOrEmpty($Password) -and (Test-Path "C:\ramshared\bin\.drill-pw")) {
    $Password = (Get-Content "C:\ramshared\bin\.drill-pw" -Raw).Trim()
}
if ([string]::IsNullOrEmpty($Password)) { throw "RAMSHARED_DRILL_PASSWORD / .drill-pw required" }
$sec = ConvertTo-SecureString $Password -AsPlainText -Force
$cred = New-Object System.Management.Automation.PSCredential($User, $sec)

$art = "C:\ramshared\artifacts\guest-exhaustive-$(Get-Date -Format yyyyMMdd-HHmmss)"
New-Item -Force -ItemType Directory $art | Out-Null
function W($m) {
    $t = "[{0}] {1}" -f (Get-Date -Format HH:mm:ss), $m
    $t | Tee-Object -FilePath (Join-Path $art "host-side.log") -Append
}

function Invoke-GuestBounded {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$ScriptBlock,
        [object[]]$ArgumentList = @(),
        [ValidateRange(1, 900)][int]$TimeoutSec = 30
    )

    $job = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock $ScriptBlock `
        -ArgumentList $ArgumentList -AsJob -ErrorAction Stop
    try {
        $completed = Wait-Job -Job $job -Timeout $TimeoutSec
        if (-not $completed) {
            Stop-Job -Job $job -ErrorAction SilentlyContinue
            throw ("guest command timed out after {0}s" -f $TimeoutSec)
        }
        if ($job.State -ne "Completed") {
            $reason = $job.ChildJobs[0].JobStateInfo.Reason
            throw ("guest command ended in state {0}: {1}" -f $job.State, $reason)
        }
        Receive-Job -Job $job -ErrorAction Stop
    } finally {
        Remove-Job -Job $job -Force -ErrorAction SilentlyContinue
    }
}

trap {
    W ("ERROR " + $_.Exception.Message)
    if (-not $LeaveVmOn) {
        Stop-VM -Name $VMName -Force -ErrorAction SilentlyContinue
        W "VM stopped after harness error"
    }
    exit 3
}

$vm = Get-VM -Name $VMName -ErrorAction Stop
if ($vm.State -ne "Running") {
    W ("Starting " + $VMName + " StartupBytes=" + $vm.MemoryStartup)
    Start-VM -Name $VMName
    $t = 0
    while ((Get-VM $VMName).State -ne "Running" -and $t -lt 120) { Start-Sleep 2; $t += 2 }
}
W ("VM state=" + (Get-VM $VMName).State)

$readyWait = [Diagnostics.Stopwatch]::StartNew()
while ($readyWait.Elapsed.TotalSeconds -lt 300) {
    try {
        $r = Invoke-GuestBounded -TimeoutSec 20 -ScriptBlock {
            "PSD_OK " + $env:COMPUTERNAME + " " + [Environment]::OSVersion.VersionString
        } -ErrorAction Stop
        W $r
        break
    } catch {
        Start-Sleep 3
    }
}
if ($readyWait.Elapsed.TotalSeconds -ge 300) { throw "PSD not ready after 300s" }

# A prior aborted run may still have the package binaries mapped by SCM.
# Stop/delete + remove Root\RamShared so WriteAllBytes never races a loaded image.
Invoke-GuestBounded -TimeoutSec 45 -ScriptBlock {
    sc.exe stop ramshared 2>$null | Out-Null
    sc.exe stop poolstress 2>$null | Out-Null
    Start-Sleep 2
    # Remove root-enumerated PnP device from prior INF install (DT-25).
    $devs = @(Get-PnpDevice -ErrorAction SilentlyContinue |
        Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' -or $_.InstanceId -like '*Root\RamShared*' })
    foreach ($d in $devs) {
        pnputil /remove-device $d.InstanceId 2>$null | Out-Null
    }
    sc.exe delete ramshared 2>$null | Out-Null
    sc.exe delete poolstress 2>$null | Out-Null
    Start-Sleep 1
}
W "pre-deploy driver cleanup done"

# StorPort may retain the image mapping after SCM stop/delete. Rebooting the
# isolated guest is the only deterministic pre-deploy boundary; never overwrite
# a mapped .sys and never reset the physical host for this condition.
Restart-VM -Name $VMName -Force
Start-Sleep 10
$readyWait = [Diagnostics.Stopwatch]::StartNew()
while ($readyWait.Elapsed.TotalSeconds -lt 300) {
    try {
        $null = Invoke-GuestBounded -TimeoutSec 20 -ScriptBlock { "PSD_OK" }
        break
    } catch {
        Start-Sleep 3
    }
}
if ($readyWait.Elapsed.TotalSeconds -ge 300) { throw "PSD not ready after pre-deploy reboot (300s)" }
W "pre-deploy reboot complete"

function Send-GuestFile([string]$Local, [string]$RemoteDir, [string]$Name) {
    if (-not (Test-Path $Local)) { throw ("missing " + $Local) }
    $bytes = [IO.File]::ReadAllBytes($Local)
    $b64 = [Convert]::ToBase64String($bytes)
    Invoke-GuestBounded -TimeoutSec 60 -ScriptBlock {
        param($d, $n, $b)
        New-Item -Force -ItemType Directory $d | Out-Null
        [IO.File]::WriteAllBytes((Join-Path $d $n), [Convert]::FromBase64String($b))
    } -ArgumentList @($RemoteDir, $Name, $b64)
}

$pkg = "C:\ramshared\package"
Send-GuestFile (Join-Path $pkg "ramshared.sys") "C:\ramshared\package" "ramshared.sys"
Send-GuestFile (Join-Path $pkg "poolstress.sys") "C:\ramshared\package" "poolstress.sys"
Send-GuestFile (Join-Path $pkg "ramshared.inf") "C:\ramshared\package" "ramshared.inf"
if (Test-Path (Join-Path $pkg "ramshared.cat")) {
    Send-GuestFile (Join-Path $pkg "ramshared.cat") "C:\ramshared\package" "ramshared.cat"
}
Send-GuestFile "C:\ramshared\bin\Invoke-WinDriveIoctlValidation.ps1" "C:\ramshared\bin" "Invoke-WinDriveIoctlValidation.ps1"
W "files deployed"

$load = Invoke-GuestBounded -TimeoutSec 240 -ScriptBlock {
    $ErrorActionPreference = "Continue"
    $o = [ordered]@{}
    $o.testsigning = ((bcdedit /enum "{current}" | Out-String) -match "testsigning\s+Yes")

    # Stop + remove stale devices before purging only ramshared.inf DriverStore packages.
    # This avoids old DriverStore ghosts while still not deleting arbitrary services.
    sc.exe stop ramshared 2>$null | Out-Null
    sc.exe stop poolstress 2>$null | Out-Null
    Get-PnpDevice -EA SilentlyContinue |
        Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' } |
        ForEach-Object { pnputil /remove-device $_.InstanceId 2>$null | Out-Null }
    Start-Sleep 2
    function Get-RamSharedPublishedInf {
        $names = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
        try {
            Get-WindowsDriver -Online -All -EA Stop |
                Where-Object { $_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$' } |
                ForEach-Object {
                    if ($_.Driver -match '^oem\d+\.inf$') { [void]$names.Add($_.Driver) }
                }
        } catch {}
        $published = $null
        foreach ($line in (pnputil /enum-drivers 2>&1)) {
            $s = [string]$line
            if ($s -match 'Published Name\s*:\s*(oem\d+\.inf)') {
                $published = $Matches[1]
            } elseif ($published -and $s -match 'Original Name\s*:\s*ramshared\.inf') {
                [void]$names.Add($published)
                $published = $null
            }
        }
        $names | Sort-Object
    }
    foreach ($publishedInf in @(Get-RamSharedPublishedInf)) {
        $delete = pnputil /delete-driver $publishedInf /uninstall /force 2>&1 | Out-String
        $deleteExit = $LASTEXITCODE
        if ($deleteExit -ne 0 -and $delete -notmatch "Deleted driver package") {
            throw ("delete stale ramshared package failed {0}: {1}" -f $publishedInf, $delete)
        }
    }

    if (Test-Path C:\ramshared\package\poolstress.sys) {
        Copy-Item C:\ramshared\package\poolstress.sys C:\Windows\System32\drivers\poolstress.sys -Force -EA SilentlyContinue
    }

    $o.pnputil = (pnputil /add-driver C:\ramshared\package\ramshared.inf /install 2>&1 | Out-String)

    $haveRoot = @(Get-PnpDevice -EA SilentlyContinue |
        Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' }).Count -gt 0
    $o.adddev = "existing"
    if (-not $haveRoot) {
        try {
            if (-not ("RamSharedRootEnum" -as [type])) {
                Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class RamSharedRootEnum {
  static readonly Guid ScsiClass = new Guid("4d36e97b-e325-11ce-bfc1-08002be10318");
  [StructLayout(LayoutKind.Sequential)]
  struct SP_DEVINFO_DATA {
    public int cbSize; public Guid ClassGuid; public int DevInst; public IntPtr Reserved;
  }
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid g, IntPtr h);
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool SetupDiCreateDeviceInfo(IntPtr list, string name, ref Guid g, string desc, IntPtr hwnd, int flags, ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool SetupDiSetDeviceRegistryProperty(IntPtr list, ref SP_DEVINFO_DATA data, int prop, byte[] buf, int size);
  [DllImport("setupapi.dll", SetLastError=true)]
  static extern bool SetupDiCallClassInstaller(int dif, IntPtr list, ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll", SetLastError=true)]
  static extern bool SetupDiDestroyDeviceInfoList(IntPtr list);
  [DllImport("newdev.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool UpdateDriverForPlugAndPlayDevices(IntPtr hwnd, string hwid, string inf, uint flags, out bool reboot);
  public static string Install(string infPath) {
    Guid g = ScsiClass;
    IntPtr list = SetupDiCreateDeviceInfoList(ref g, IntPtr.Zero);
    if (list == IntPtr.Zero || list == new IntPtr(-1))
      return "CreateDeviceInfoList err=" + Marshal.GetLastWin32Error();
    try {
      SP_DEVINFO_DATA data = new SP_DEVINFO_DATA();
      data.cbSize = Marshal.SizeOf(typeof(SP_DEVINFO_DATA));
      if (!SetupDiCreateDeviceInfo(list, "RamShared", ref g, "RamShared VRAM Virtual Disk",
            IntPtr.Zero, 0x00000001, ref data))
        return "CreateDeviceInfo err=" + Marshal.GetLastWin32Error();
      byte[] buf = Encoding.Unicode.GetBytes("Root\\RamShared\0\0");
      if (!SetupDiSetDeviceRegistryProperty(list, ref data, 0x00000001, buf, buf.Length))
        return "SetHWID err=" + Marshal.GetLastWin32Error();
      if (!SetupDiCallClassInstaller(0x00000019, list, ref data))
        return "RegisterDevice err=" + Marshal.GetLastWin32Error();
      bool reboot;
      if (!UpdateDriverForPlugAndPlayDevices(IntPtr.Zero, "Root\\RamShared", infPath, 0x00000001, out reboot))
        return "UpdateDriver err=" + Marshal.GetLastWin32Error();
      return "OK reboot=" + reboot;
    } finally { SetupDiDestroyDeviceInfoList(list); }
  }
}
'@ -ErrorAction Stop
            }
            $o.adddev = [RamSharedRootEnum]::Install("C:\ramshared\package\ramshared.inf")
        } catch {
            $o.adddev = "setupapi-exception: $($_.Exception.Message)"
        }
    }

    $o.start_ram = (sc.exe start ramshared 2>&1 | Out-String)
    Get-PnpDevice -EA SilentlyContinue |
        Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' } |
        ForEach-Object {
            pnputil /enable-device $_.InstanceId 2>$null | Out-Null
            pnputil /restart-device $_.InstanceId 2>$null | Out-Null
        }
    Start-Sleep 4

    if (Test-Path C:\Windows\System32\drivers\poolstress.sys) {
        sc.exe create poolstress type= kernel binPath= \SystemRoot\System32\drivers\poolstress.sys 2>$null | Out-Null
        $o.start_pool = (sc.exe start poolstress 2>&1 | Out-String)
    } else { $o.start_pool = "skip" }

    $o.pool = (sc.exe query poolstress 2>&1 | Out-String)
    $o.ram = (sc.exe query ramshared 2>&1 | Out-String)
    $scsiRows = @()
    Get-PnpDevice -Class SCSIAdapter -EA SilentlyContinue | ForEach-Object {
        $prob = $null
        try {
            $prob = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_ProblemCode' -EA Stop).Data
        } catch {}
        $scsiRows += "$($_.Status)|prob=$prob|$($_.FriendlyName)|$($_.InstanceId)"
    }
    $o.scsi = $scsiRows -join "; "
    $o.disks = @(Get-Disk -EA SilentlyContinue |
        ForEach-Object { "N=$($_.Number) Name=$($_.FriendlyName) Size=$($_.Size) Ser=$($_.SerialNumber) Bus=$($_.BusType)" }) -join "; "
    $diskDrives = @(Get-PnpDevice -Class DiskDrive -EA SilentlyContinue |
        ForEach-Object { "$($_.Status)|$($_.FriendlyName)|$($_.InstanceId)" }) -join "; "
    $o.diskDrives = $diskDrives
    # Ghost RAMSHARE child PDOs from older always-present-LUN builds keep
    # placeholder identity in PnP and poison exact VPD uniqueness. Remove only
    # DiskDrive nodes for this vendor/product; never touch the daily host.
    $ghost = @(Get-PnpDevice -Class DiskDrive -EA SilentlyContinue |
        Where-Object {
            ($_.FriendlyName -match 'RAMSHARE' -and $_.FriendlyName -match 'VRAMDISK') -or
            ($_.InstanceId -match 'VEN_RAMSHARE' -and $_.InstanceId -match 'PROD_VRAMDISK')
        })
    $removed = @()
    foreach ($g in $ghost) {
        $rm = pnputil /remove-device $g.InstanceId 2>&1 | Out-String
        $removed += "$($g.InstanceId)=>$($rm.Trim())"
    }
    $o.ghostRemoved = $removed -join "; "
    Start-Sleep 2
    try { "rescan" | diskpart 2>$null | Out-Null } catch {}
    try { Update-HostStorageCache -ErrorAction SilentlyContinue } catch {}

    $o.running = ($o.ram -match "RUNNING")
    $rawImage = [string](Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\ramshared" -Name ImagePath -EA SilentlyContinue).ImagePath
    $sysImage = $rawImage.Trim('"') -replace '^\\SystemRoot', $env:SystemRoot -replace '^\\\?\?\\', ''
    $o.sysLen = (Get-Item $sysImage -EA SilentlyContinue).Length
    $o.sysSha = if (Test-Path $sysImage) {
        (Get-FileHash $sysImage -Algorithm SHA256).Hash
    } else { "missing" }
    $o.packageSha = if (Test-Path C:\ramshared\package\ramshared.sys) {
        (Get-FileHash C:\ramshared\package\ramshared.sys -Algorithm SHA256).Hash
    } else { "missing" }
    $o.driverImagePath = $sysImage
    $o.binaryMatch = ($o.sysSha -eq $o.packageSha -and $o.sysSha -ne "missing" -and $sysImage -match '\\DriverStore\\FileRepository\\')
    $o | ConvertTo-Json -Compress
}
$load | Set-Content (Join-Path $art "guest-load.json")
W ("load=" + $load)

# After replacing ramshared.sys on disk, the image already mapped by SCM may
# still be the previous build (start 1056 / pnputil "up-to-date"). Reboot the
# isolated guest once so pass1 always exercises the package SHA just deployed.
# Budget 300s: observed healthy boots land ~80–180s; 180s alone is flaky.
W "post-deploy reboot to map package image"
Restart-VM -Name $VMName -Force
Start-Sleep 10
$readyWait = [Diagnostics.Stopwatch]::StartNew()
while ($readyWait.Elapsed.TotalSeconds -lt 300) {
    try {
        $null = Invoke-GuestBounded -TimeoutSec 25 -ScriptBlock { "PSD_OK" }
        break
    } catch {
        Start-Sleep 3
    }
}
if ($readyWait.Elapsed.TotalSeconds -ge 300) { throw "PSD not ready after post-deploy reboot (300s)" }
W ("post-deploy reboot PSD_OK elapsed={0:n0}s" -f $readyWait.Elapsed.TotalSeconds)

$loadReady = Invoke-GuestBounded -TimeoutSec 180 -ScriptBlock {
    $ErrorActionPreference = "Continue"
    $o = [ordered]@{}
    function Wait-RamSharedRootOk {
        $log = @()
        $rootDevices = @(Get-PnpDevice -EA SilentlyContinue |
            Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' })
        if ($rootDevices.Count -lt 1) {
            return [pscustomobject]@{ Ok = $false; State = "missing ROOT\RAMSHARED"; Log = "" }
        }
        foreach ($dev in $rootDevices) {
            $enable = pnputil /enable-device $dev.InstanceId 2>&1 | Out-String
            $log += ("enable {0} exit={1} {2}" -f $dev.InstanceId, $LASTEXITCODE, ($enable.Trim()))
            $restart = pnputil /restart-device $dev.InstanceId 2>&1 | Out-String
            $log += ("restart {0} exit={1} {2}" -f $dev.InstanceId, $LASTEXITCODE, ($restart.Trim()))
        }
        $stateRows = @()
        for ($waitPnp = 0; $waitPnp -lt 30; $waitPnp++) {
            $state = @(Get-PnpDevice -EA SilentlyContinue |
                Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' })
            $stateRows = @($state | ForEach-Object {
                "{0}|problem={1}|{2}|{3}" -f $_.Status, ([int]$_.Problem), $_.FriendlyName, $_.InstanceId
            })
            $ok = @($state | Where-Object { $_.Status -eq "OK" -and ([int]$_.Problem) -eq 0 })
            $scsiState = @(Get-PnpDevice -Class SCSIAdapter -EA SilentlyContinue |
                Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' -or $_.FriendlyName -match 'RamShared' })
            $scsiRows = @($scsiState | ForEach-Object {
                "{0}|problem={1}|{2}|{3}" -f $_.Status, ([int]$_.Problem), $_.FriendlyName, $_.InstanceId
            })
            $scsiOk = @($scsiState | Where-Object { $_.Status -eq "OK" -and ([int]$_.Problem) -eq 0 })
            if ($ok.Count -ge 1 -and $scsiOk.Count -ge 1) {
                return [pscustomobject]@{
                    Ok = $true
                    State = ("root=[{0}] scsi=[{1}]" -f ($stateRows -join "; "), ($scsiRows -join "; "))
                    Log = ($log -join "`n")
                }
            }
            Start-Sleep 1
        }
        [pscustomobject]@{
            Ok = $false
            State = ("root=[{0}] scsi=[{1}]" -f ($stateRows -join "; "), ($scsiRows -join "; "))
            Log = ($log -join "`n")
        }
    }
    function Ensure-RamSharedRootDevice {
        try {
            if (-not ("RamSharedRootEnum" -as [type])) {
                Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class RamSharedRootEnum {
  static readonly Guid ScsiClass = new Guid("4d36e97b-e325-11ce-bfc1-08002be10318");
  [StructLayout(LayoutKind.Sequential)]
  struct SP_DEVINFO_DATA {
    public int cbSize; public Guid ClassGuid; public int DevInst; public IntPtr Reserved;
  }
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid g, IntPtr h);
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool SetupDiCreateDeviceInfo(IntPtr list, string name, ref Guid g, string desc, IntPtr hwnd, int flags, ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool SetupDiSetDeviceRegistryProperty(IntPtr list, ref SP_DEVINFO_DATA data, int prop, byte[] buf, int size);
  [DllImport("setupapi.dll", SetLastError=true)]
  static extern bool SetupDiCallClassInstaller(int dif, IntPtr list, ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll", SetLastError=true)]
  static extern bool SetupDiDestroyDeviceInfoList(IntPtr list);
  [DllImport("newdev.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool UpdateDriverForPlugAndPlayDevices(IntPtr hwnd, string hwid, string inf, uint flags, out bool reboot);
  public static string Install(string infPath) {
    Guid g = ScsiClass;
    IntPtr list = SetupDiCreateDeviceInfoList(ref g, IntPtr.Zero);
    if (list == IntPtr.Zero || list == new IntPtr(-1))
      return "CreateDeviceInfoList err=" + Marshal.GetLastWin32Error();
    try {
      SP_DEVINFO_DATA data = new SP_DEVINFO_DATA();
      data.cbSize = Marshal.SizeOf(typeof(SP_DEVINFO_DATA));
      if (!SetupDiCreateDeviceInfo(list, "RamShared", ref g, "RamShared VRAM Virtual Disk",
            IntPtr.Zero, 0x00000001, ref data))
        return "CreateDeviceInfo err=" + Marshal.GetLastWin32Error();
      byte[] buf = Encoding.Unicode.GetBytes("Root\\RamShared\0\0");
      if (!SetupDiSetDeviceRegistryProperty(list, ref data, 0x00000001, buf, buf.Length))
        return "SetHWID err=" + Marshal.GetLastWin32Error();
      if (!SetupDiCallClassInstaller(0x00000019, list, ref data))
        return "RegisterDevice err=" + Marshal.GetLastWin32Error();
      bool reboot;
      if (!UpdateDriverForPlugAndPlayDevices(IntPtr.Zero, "Root\\RamShared", infPath, 0x00000001, out reboot))
        return "UpdateDriver err=" + Marshal.GetLastWin32Error();
      return "OK reboot=" + reboot;
    } finally { SetupDiDestroyDeviceInfoList(list); }
  }
}
'@ -ErrorAction Stop
            }
            [RamSharedRootEnum]::Install("C:\ramshared\package\ramshared.inf")
        } catch {
            "setupapi-exception: $($_.Exception.Message)"
        }
    }
    $pnp = Wait-RamSharedRootOk
    $o.rootRecreateAfterReboot = "not_needed"
    if (-not $pnp.Ok -and $pnp.State -match "missing ROOT") {
        $o.rootRecreateAfterReboot = Ensure-RamSharedRootDevice
        Start-Sleep 2
        $pnp = Wait-RamSharedRootOk
    }
    $o.pnpBeforeIoctl = $pnp.State
    $o.pnpEnableLog = $pnp.Log
    $o.pnpReady = [bool]$pnp.Ok
    sc.exe start ramshared 2>$null | Out-Null
    sc.exe start poolstress 2>$null | Out-Null
    $pnpAfterStart = Wait-RamSharedRootOk
    $o.pnpBeforeIoctl = $pnpAfterStart.State
    $o.pnpEnableLog = (($o.pnpEnableLog, $pnpAfterStart.Log) -join "`n")
    $o.pnpReady = [bool]$pnpAfterStart.Ok
    Start-Sleep 3
    # Re-clean any ghost DiskDrive nodes that reappeared after reboot without CREATE.
    $ghost = @(Get-PnpDevice -Class DiskDrive -EA SilentlyContinue |
        Where-Object {
            ($_.FriendlyName -match 'RAMSHARE' -and $_.FriendlyName -match 'VRAMDISK') -or
            ($_.InstanceId -match 'VEN_RAMSHARE' -and $_.InstanceId -match 'PROD_VRAMDISK')
        })
    foreach ($g in $ghost) {
        pnputil /remove-device $g.InstanceId 2>$null | Out-Null
    }
    Start-Sleep 2
    try { "rescan" | diskpart 2>$null | Out-Null } catch {}
    $o.ram = (sc.exe query ramshared 2>&1 | Out-String)
    $o.running = ($o.ram -match "RUNNING")
    $rawImage = [string](Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\ramshared" -Name ImagePath -EA SilentlyContinue).ImagePath
    $sysImage = $rawImage.Trim('"') -replace '^\\SystemRoot', $env:SystemRoot -replace '^\\\?\?\\', ''
    $o.sysSha = if (Test-Path $sysImage) {
        (Get-FileHash $sysImage -Algorithm SHA256).Hash
    } else { "missing" }
    $o.packageSha = if (Test-Path C:\ramshared\package\ramshared.sys) {
        (Get-FileHash C:\ramshared\package\ramshared.sys -Algorithm SHA256).Hash
    } else { "missing" }
    $o.driverImagePath = $sysImage
    $o.binaryMatch = ($o.sysSha -eq $o.packageSha -and $o.sysSha -ne "missing" -and $sysImage -match '\\DriverStore\\FileRepository\\')
    $o.diskDrives = @(Get-PnpDevice -Class DiskDrive -EA SilentlyContinue |
        ForEach-Object { "$($_.Status)|$($_.FriendlyName)|$($_.InstanceId)" }) -join "; "
    $o.disks = @(Get-Disk -EA SilentlyContinue |
        ForEach-Object { "N=$($_.Number) Name=$($_.FriendlyName) Size=$($_.Size) Ser=[$($_.SerialNumber)] Bus=$($_.BusType)" }) -join "; "
    $o | ConvertTo-Json -Compress
}
$loadReady | Set-Content (Join-Path $art "guest-load-post-reboot.json")
W ("load-ready=" + $loadReady)
$loadReadyObject = $loadReady | ConvertFrom-Json
if (-not $loadReadyObject.running -or -not $loadReadyObject.binaryMatch -or -not $loadReadyObject.pnpReady) {
    throw ("RamShared PnP gate failed before IOCTL pass1: " + $loadReady)
}

# Budget 420s: VPD poll (~25s) + StartIo race (~15s) + concurrent injectors.
$ioctl1 = Invoke-GuestBounded -TimeoutSec 420 -ScriptBlock {
    $ErrorActionPreference = "Continue"
    New-Item -Force -ItemType Directory C:\ramshared\artifacts\ioctl-validation | Out-Null
    $log = "C:\ramshared\artifacts\ioctl-validation\live-console.txt"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File `
        C:\ramshared\bin\Invoke-WinDriveIoctlValidation.ps1 `
        -ArtifactDir C:\ramshared\artifacts\ioctl-validation *>&1 |
        ForEach-Object {
            $line = "$_"
            Add-Content -Path $log -Value $line
            $line
        }
    "EXIT=" + $LASTEXITCODE
}
$ioctl1 | Set-Content (Join-Path $art "ioctl-pass1.txt")
W "ioctl pass1 done"

$verdictJson = Invoke-GuestBounded -TimeoutSec 30 -ScriptBlock {
    $v = Get-ChildItem C:\ramshared\artifacts\ioctl-validation\verdict-*.json |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($v) { Get-Content $v.FullName -Raw } else { "{}" }
}
$verdictJson | Set-Content (Join-Path $art "verdict-pass1.json")

$verifierDone = $false
$ioctl2 = ""
if (-not $SkipVerifier) {
    W "Enabling Driver Verifier for ramshared.sys (guest reboot required)"
    # Virtual StorPort: omit DMA checking (0x80) — pure software path; full /standard
    # plus DMA has hung guest boot/PSD in this lab. Keep special pool, IRQL, pool,
    # I/O, deadlock, security, misc, DDI (0x2093B).
    Invoke-GuestBounded -TimeoutSec 60 -ScriptBlock {
        $ErrorActionPreference = "Continue"
        $out = New-Object System.Collections.Generic.List[string]
        # Do NOT /reset here if it races with flag set; set flags for next boot.
        $out.Add((verifier /flags 0x2093B /driver ramshared.sys 2>&1 | Out-String))
        $out.Add((verifier /bootmode persistent 2>&1 | Out-String))
        # Confirm registry schedule before reboot.
        $mm = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management'
        $vf = Get-ItemProperty -Path $mm -ErrorAction SilentlyContinue
        $out.Add(("VerifyDrivers={0}" -f $vf.VerifyDrivers))
        $out.Add(("VerifyDriverLevel={0}" -f $vf.VerifyDriverLevel))
        $out.Add((verifier /query 2>&1 | Out-String))
        $out.Add((verifier /querysettings 2>&1 | Out-String))
        # Guest OS reboot so Verifier applies (Restart-VM -Force can skip clean shutdown).
        Start-Process -FilePath shutdown.exe -ArgumentList '/r','/t','3','/f' -WindowStyle Hidden
        $out -join "`n"
    } | Set-Content (Join-Path $art "verifier-enable.txt")

    W "Waiting for guest OS reboot under Verifier (PSD wait up to 600s)..."
    Start-Sleep 15
    # If guest only rebooted in-place, VM stays Running; if it powered off, start it.
    $readyWait = [Diagnostics.Stopwatch]::StartNew()
    $psdOk = $false
    while ($readyWait.Elapsed.TotalSeconds -lt 600) {
        try {
            $st = (Get-VM $VMName).State
            if ($st -eq "Off" -or $st -eq "Saved") {
                Start-VM -Name $VMName -ErrorAction SilentlyContinue
                Start-Sleep 5
                continue
            }
            if ($st -ne "Running") { Start-Sleep 5; continue }
            # Require a boot time newer than enable, roughly: PSD answers.
            $null = Invoke-GuestBounded -TimeoutSec 25 -ScriptBlock { "PSD_OK" }
            $psdOk = $true
            break
        } catch { Start-Sleep 5 }
    }
    if (-not $psdOk) { throw "PSD not ready after Verifier reboot (600s)" }
    W ("PSD after Verifier reboot OK elapsed={0}s" -f [int]$readyWait.Elapsed.TotalSeconds)

    $load2 = Invoke-GuestBounded -TimeoutSec 180 -ScriptBlock {
        $ErrorActionPreference = "Continue"
        $o = [ordered]@{}
        function Wait-RamSharedRootOk {
            $log = @()
            $rootDevices = @(Get-PnpDevice -EA SilentlyContinue |
                Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' })
            if ($rootDevices.Count -lt 1) {
                return [pscustomobject]@{ Ok = $false; State = "missing ROOT\RAMSHARED"; Log = "" }
            }
            foreach ($dev in $rootDevices) {
                $enable = pnputil /enable-device $dev.InstanceId 2>&1 | Out-String
                $log += ("enable {0} exit={1} {2}" -f $dev.InstanceId, $LASTEXITCODE, ($enable.Trim()))
                $restart = pnputil /restart-device $dev.InstanceId 2>&1 | Out-String
                $log += ("restart {0} exit={1} {2}" -f $dev.InstanceId, $LASTEXITCODE, ($restart.Trim()))
            }
            $stateRows = @()
            for ($waitPnp = 0; $waitPnp -lt 30; $waitPnp++) {
                $state = @(Get-PnpDevice -EA SilentlyContinue |
                    Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' })
                $stateRows = @($state | ForEach-Object {
                    "{0}|problem={1}|{2}|{3}" -f $_.Status, ([int]$_.Problem), $_.FriendlyName, $_.InstanceId
                })
                $ok = @($state | Where-Object { $_.Status -eq "OK" -and ([int]$_.Problem) -eq 0 })
                $scsiState = @(Get-PnpDevice -Class SCSIAdapter -EA SilentlyContinue |
                    Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' -or $_.FriendlyName -match 'RamShared' })
                $scsiRows = @($scsiState | ForEach-Object {
                    "{0}|problem={1}|{2}|{3}" -f $_.Status, ([int]$_.Problem), $_.FriendlyName, $_.InstanceId
                })
                $scsiOk = @($scsiState | Where-Object { $_.Status -eq "OK" -and ([int]$_.Problem) -eq 0 })
                if ($ok.Count -ge 1 -and $scsiOk.Count -ge 1) {
                    return [pscustomobject]@{
                        Ok = $true
                        State = ("root=[{0}] scsi=[{1}]" -f ($stateRows -join "; "), ($scsiRows -join "; "))
                        Log = ($log -join "`n")
                    }
                }
                Start-Sleep 1
            }
            [pscustomobject]@{
                Ok = $false
                State = ("root=[{0}] scsi=[{1}]" -f ($stateRows -join "; "), ($scsiRows -join "; "))
                Log = ($log -join "`n")
            }
        }
        function Ensure-RamSharedRootDevice {
            try {
                if (-not ("RamSharedRootEnum" -as [type])) {
                    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class RamSharedRootEnum {
  static readonly Guid ScsiClass = new Guid("4d36e97b-e325-11ce-bfc1-08002be10318");
  [StructLayout(LayoutKind.Sequential)]
  struct SP_DEVINFO_DATA {
    public int cbSize; public Guid ClassGuid; public int DevInst; public IntPtr Reserved;
  }
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid g, IntPtr h);
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool SetupDiCreateDeviceInfo(IntPtr list, string name, ref Guid g, string desc, IntPtr hwnd, int flags, ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool SetupDiSetDeviceRegistryProperty(IntPtr list, ref SP_DEVINFO_DATA data, int prop, byte[] buf, int size);
  [DllImport("setupapi.dll", SetLastError=true)]
  static extern bool SetupDiCallClassInstaller(int dif, IntPtr list, ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll", SetLastError=true)]
  static extern bool SetupDiDestroyDeviceInfoList(IntPtr list);
  [DllImport("newdev.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool UpdateDriverForPlugAndPlayDevices(IntPtr hwnd, string hwid, string inf, uint flags, out bool reboot);
  public static string Install(string infPath) {
    Guid g = ScsiClass;
    IntPtr list = SetupDiCreateDeviceInfoList(ref g, IntPtr.Zero);
    if (list == IntPtr.Zero || list == new IntPtr(-1))
      return "CreateDeviceInfoList err=" + Marshal.GetLastWin32Error();
    try {
      SP_DEVINFO_DATA data = new SP_DEVINFO_DATA();
      data.cbSize = Marshal.SizeOf(typeof(SP_DEVINFO_DATA));
      if (!SetupDiCreateDeviceInfo(list, "RamShared", ref g, "RamShared VRAM Virtual Disk",
            IntPtr.Zero, 0x00000001, ref data))
        return "CreateDeviceInfo err=" + Marshal.GetLastWin32Error();
      byte[] buf = Encoding.Unicode.GetBytes("Root\\RamShared\0\0");
      if (!SetupDiSetDeviceRegistryProperty(list, ref data, 0x00000001, buf, buf.Length))
        return "SetHWID err=" + Marshal.GetLastWin32Error();
      if (!SetupDiCallClassInstaller(0x00000019, list, ref data))
        return "RegisterDevice err=" + Marshal.GetLastWin32Error();
      bool reboot;
      if (!UpdateDriverForPlugAndPlayDevices(IntPtr.Zero, "Root\\RamShared", infPath, 0x00000001, out reboot))
        return "UpdateDriver err=" + Marshal.GetLastWin32Error();
      return "OK reboot=" + reboot;
    } finally { SetupDiDestroyDeviceInfoList(list); }
  }
}
'@ -ErrorAction Stop
                }
                [RamSharedRootEnum]::Install("C:\ramshared\package\ramshared.inf")
            } catch {
                "setupapi-exception: $($_.Exception.Message)"
            }
        }
        $pnp = Wait-RamSharedRootOk
        $o.rootRecreateAfterReboot = "not_needed"
        if (-not $pnp.Ok -and $pnp.State -match "missing ROOT") {
            $o.rootRecreateAfterReboot = Ensure-RamSharedRootDevice
            Start-Sleep 2
            $pnp = Wait-RamSharedRootOk
        }
        $o.pnpBeforeIoctl = $pnp.State
        $o.pnpEnableLog = $pnp.Log
        $o.pnpReady = [bool]$pnp.Ok
        sc.exe start ramshared 2>$null | Out-Null
        sc.exe start poolstress 2>$null | Out-Null
        $pnpAfterStart = Wait-RamSharedRootOk
        $o.pnpBeforeIoctl = $pnpAfterStart.State
        $o.pnpEnableLog = (($o.pnpEnableLog, $pnpAfterStart.Log) -join "`n")
        $o.pnpReady = [bool]$pnpAfterStart.Ok
        Start-Sleep 4
        $o.verifier = (verifier /query 2>&1 | Out-String)
        $o.ram = (sc.exe query ramshared 2>&1 | Out-String)
        $o.scsi = @(Get-PnpDevice -Class SCSIAdapter -ErrorAction SilentlyContinue |
            ForEach-Object {
                $p = $null
                try { $p = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_ProblemCode -EA Stop).Data } catch {}
                "$($_.Status)|prob=$p|$($_.FriendlyName)"
            }) -join "; "
        $o.running = (($o.ram -match "RUNNING") -or ($o.scsi -match "RamShared"))
        $o.verifierActive = ($o.verifier -match "ramshared\.sys") -and ($o.verifier -notmatch "No drivers are currently verified")
        $o.dumps = @(Get-ChildItem C:\Windows\Minidump -Filter *.dmp -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
        $o | ConvertTo-Json -Depth 4 -Compress
    }
    $load2 | Set-Content (Join-Path $art "guest-load-verifier.json")
    W ("load2=" + $load2)
    $load2Object = $load2 | ConvertFrom-Json
    if (-not $load2Object.running -or -not $load2Object.verifierActive -or -not $load2Object.pnpReady) {
        throw ("RamShared PnP gate failed before IOCTL verifier pass: " + $load2)
    }

    $ioctl2 = Invoke-GuestBounded -TimeoutSec 420 -ScriptBlock {
        $ErrorActionPreference = "Continue"
        & powershell.exe -NoProfile -ExecutionPolicy Bypass -File `
            C:\ramshared\bin\Invoke-WinDriveIoctlValidation.ps1 `
            -ArtifactDir C:\ramshared\artifacts\ioctl-validation -Verifier 2>&1 | Out-String
        "EXIT=" + $LASTEXITCODE
    }
    $ioctl2 | Set-Content (Join-Path $art "ioctl-pass2-verifier.txt")
    $verdict2 = Invoke-GuestBounded -TimeoutSec 30 -ScriptBlock {
        $v = Get-ChildItem C:\ramshared\artifacts\ioctl-validation\verdict-*.json |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($v) { Get-Content $v.FullName -Raw } else { "{}" }
    }
    $verdict2 | Set-Content (Join-Path $art "verdict-pass2-verifier.json")
    $verifierDone = $true
    W "ioctl pass2 under Verifier done"
}

function Parse-Status([string]$text) {
    if ($text -match "STATUS=PASS") { return "PASS" }
    if ($text -match "STATUS=FAIL") { return "FAIL" }
    return "UNKNOWN"
}
$s1 = Parse-Status $ioctl1
$s2 = if ($verifierDone) { Parse-Status $ioctl2 } else { "SKIPPED" }

$sum = [ordered]@{
    ARTIFACT       = $art
    IOCTL_PASS1    = $s1
    IOCTL_VERIFIER = $s2
    VERIFIER_RAN   = $verifierDone
    LEAVE_VM_ON    = [bool]$LeaveVmOn
}
$sum | ConvertTo-Json | Set-Content (Join-Path $art "summary.json")
W ("SUMMARY " + ($sum | ConvertTo-Json -Compress))
Write-Host ("ARTIFACT=" + $art)

if (-not $LeaveVmOn) {
    try {
        Invoke-GuestBounded -TimeoutSec 20 -ScriptBlock {
            verifier /reset 2>&1 | Out-Null
        } -ErrorAction SilentlyContinue
    } catch {}
    Stop-VM -Name $VMName -Force -ErrorAction SilentlyContinue
    W "VM stopped; verifier reset best-effort"
}

if ($s1 -eq "PASS" -and ($s2 -eq "PASS" -or $s2 -eq "SKIPPED")) { exit 0 }
exit 2
