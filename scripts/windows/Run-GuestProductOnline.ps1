#Requires -Version 5.1
<#
.SYNOPSIS
  Isolated win11-drill product Online: broker lease + CUDA LUN + 3-round SHA + graceful stop.

.DESCRIPTION
  Lab-only. Starts a minimal JSONL lease broker (Register/LeaseGrant/Release log) inside the
  guest, runs ramshared-winsvc console --storage-only at 64 MiB, formats only RAMSHARE VRAMDISK,
  three write/read SHA rounds, then stop.request teardown. Never targets the daily host miniport.

.EXAMPLE
  .\Run-GuestProductOnline.ps1 -VMName win11-drill
#>
[CmdletBinding()]
param(
    [string]$VMName = "win11-drill",
    [string]$User = "WIN11-DRILL\drilladmin",
    [string]$Password = $env:RAMSHARED_DRILL_PASSWORD,
    [UInt64]$SizeBytes = 67108864,
    [string]$Letter = "S",
    [int]$BrokerPort = 19876,
    # Lab-only: inject configured PagingFiles for product letter before stop;
    # expect Gate A refuse (code 7), Online retained, then restore registry and real stop.
    [switch]$ManufacturedPagefileRefuse
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrEmpty($Password) -and (Test-Path "C:\ramshared\bin\.drill-pw")) {
    $Password = (Get-Content "C:\ramshared\bin\.drill-pw" -Raw).Trim()
}
if ([string]::IsNullOrEmpty($Password)) { throw "RAMSHARED_DRILL_PASSWORD / .drill-pw required" }
$sec = ConvertTo-SecureString $Password -AsPlainText -Force
$cred = New-Object System.Management.Automation.PSCredential($User, $sec)

$art = "C:\ramshared\artifacts\guest-product-online-$(Get-Date -Format yyyyMMdd-HHmmss)"
New-Item -Force -ItemType Directory $art | Out-Null
function W($m) {
    $t = "[{0}] {1}" -f (Get-Date -Format HH:mm:ss), $m
    $t | Tee-Object -FilePath (Join-Path $art "host-side.log") -Append
}

function Invoke-GuestBounded {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$ScriptBlock,
        [object[]]$ArgumentList = @(),
        [ValidateRange(1, 900)][int]$TimeoutSec = 60
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
            throw ("guest command state {0}: {1}" -f $job.State, $job.ChildJobs[0].JobStateInfo.Reason)
        }
        Receive-Job -Job $job -ErrorAction Stop
    } finally {
        Remove-Job -Job $job -Force -ErrorAction SilentlyContinue
    }
}

trap {
    W ("ERROR " + $_.Exception.Message)
    try { Stop-VM -Name $VMName -Force -ErrorAction SilentlyContinue } catch {}
    W "VM stopped after harness error"
    exit 3
}

$vm = Get-VM -Name $VMName -ErrorAction Stop
if ($vm.State -ne "Running") {
    W ("Starting {0}" -f $VMName)
    Start-VM -Name $VMName
}
$ready = [Diagnostics.Stopwatch]::StartNew()
while ($ready.Elapsed.TotalSeconds -lt 240) {
    try {
        $null = Invoke-GuestBounded -TimeoutSec 20 -ScriptBlock { "PSD_OK" }
        break
    } catch { Start-Sleep 5 }
}
if ($ready.Elapsed.TotalSeconds -ge 240) { throw "PSD not ready" }
W ("PSD_OK elapsed={0:n0}s" -f $ready.Elapsed.TotalSeconds)

# Deploy package files if present on host
function Send-GuestFile([string]$Local, [string]$RemoteDir, [string]$Name) {
    if (-not (Test-Path $Local)) { throw ("missing " + $Local) }
    $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($Local))
    Invoke-GuestBounded -TimeoutSec 90 -ScriptBlock {
        param($d, $n, $b)
        New-Item -Force -ItemType Directory $d | Out-Null
        [IO.File]::WriteAllBytes((Join-Path $d $n), [Convert]::FromBase64String($b))
    } -ArgumentList @($RemoteDir, $Name, $b64)
}

Send-GuestFile "C:\ramshared\bin\ramshared-winsvc.exe" "C:\ramshared\bin" "ramshared-winsvc.exe"
if ($ManufacturedPagefileRefuse) {
    $pfScript = "C:\ramshared\bin\Invoke-PagefileRefusalManufactured.ps1"
    if (-not (Test-Path $pfScript)) {
        # Prefer repo copy under src if bin not synced yet.
        $alt = "C:\ramshared\src\scripts\windows\Invoke-PagefileRefusalManufactured.ps1"
        if (Test-Path $alt) { $pfScript = $alt }
    }
    if (-not (Test-Path $pfScript)) {
        throw "Invoke-PagefileRefusalManufactured.ps1 missing (need for -ManufacturedPagefileRefuse)"
    }
    Send-GuestFile $pfScript "C:\ramshared\bin" "Invoke-PagefileRefusalManufactured.ps1"
}
foreach ($dll in @("VCRUNTIME140.dll", "VCRUNTIME140_1.dll", "MSVCP140.dll")) {
    $p = Join-Path $env:SystemRoot ("System32\" + $dll)
    if (Test-Path $p) {
        Send-GuestFile $p "C:\ramshared\bin" $dll
    }
}
if (Test-Path "C:\ramshared\package\ramshared.sys") {
    Send-GuestFile "C:\ramshared\package\ramshared.sys" "C:\ramshared\package" "ramshared.sys"
    Send-GuestFile "C:\ramshared\package\ramshared.inf" "C:\ramshared\package" "ramshared.inf"
    if (Test-Path "C:\ramshared\package\ramshared.cat") {
        Send-GuestFile "C:\ramshared\package\ramshared.cat" "C:\ramshared\package" "ramshared.cat"
    }
}
W "files deployed"

$cfgText = @"
[win_drive]
size_bytes = $SizeBytes
block_size = 4096
cuda_device = 0
reserve_bytes = 134217728
queue_depth = 4
max_io_bytes = 1048576
evidence_path = "C:\\ProgramData\\RamShared\\evidence"
volume_letter = "$Letter"
broker = "127.0.0.1:$BrokerPort"
tenant = "guest-online"
heartbeat_secs = 5
"@

# Deploy the miniport once, then reboot before any proof. Copying a SYS over a
# stopped service does not prove which image the SCM/kernel section mapped.
$deployResult = Invoke-GuestBounded -TimeoutSec 120 -ScriptBlock {
    $ErrorActionPreference = "Stop"
    if (-not (Test-Path C:\ramshared\package\ramshared.sys)) { throw "package SYS missing" }
    sc.exe stop ramshared 2>$null | Out-Null
    Get-PnpDevice -EA SilentlyContinue | Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' } |
        ForEach-Object { pnputil /remove-device $_.InstanceId 2>$null | Out-Null }
    Start-Sleep 3
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
    $install = pnputil /add-driver C:\ramshared\package\ramshared.inf /install 2>&1 | Out-String
    $installExit = $LASTEXITCODE
    $installOk = ($installExit -eq 0) -or
        ($install -match "Driver package added successfully" -and
         $install -match "Driver package is up-to-date on device")
    if (-not $installOk) { throw ("pnputil install failed exit={0}: {1}" -f $installExit, $install) }
    $haveRoot = @(Get-PnpDevice -EA SilentlyContinue |
        Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' }).Count -gt 0
    $adddev = "existing"
    if (-not $haveRoot) {
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
        $adddev = [RamSharedRootEnum]::Install("C:\ramshared\package\ramshared.inf")
        if ($adddev -notmatch '^OK') { throw ("root device create failed: " + $adddev) }
    }
    "ROOT_DEVICE=" + $adddev
    "PACKAGE_SHA=" + (Get-FileHash C:\ramshared\package\ramshared.sys -Algorithm SHA256).Hash
    shutdown.exe /r /t 0 /f
}
W ([string]($deployResult | Select-Object -Last 1))

$rebootReady = [Diagnostics.Stopwatch]::StartNew()
Start-Sleep 5
while ($rebootReady.Elapsed.TotalSeconds -lt 300) {
    try {
        $null = Invoke-GuestBounded -TimeoutSec 20 -ScriptBlock { "PSD_AFTER_DEPLOY_OK" }
        break
    } catch { Start-Sleep 5 }
}
if ($rebootReady.Elapsed.TotalSeconds -ge 300) { throw "PSD not ready after deploy reboot" }
W ("PSD_AFTER_DEPLOY_OK elapsed={0:n0}s" -f $rebootReady.Elapsed.TotalSeconds)

$loads = @()
$lifecycleRounds = 3
if ($ManufacturedPagefileRefuse) {
    # One Online lifecycle is enough for Gate A refuse proof.
    $lifecycleRounds = 1
    W "ManufacturedPagefileRefuse=1 (single lifecycle + Gate A inject)"
}
for ($campaignRound = 1; $campaignRound -le $lifecycleRounds; $campaignRound++) {
W ("lifecycle round {0}/{1} begin" -f $campaignRound, $lifecycleRounds)
$roundLoad = Invoke-GuestBounded -TimeoutSec 360 -ScriptBlock {
    param($cfg, $sizeBytes, $brokerPort, $letter, $manufacturedPagefileRefuse)
    $ErrorActionPreference = "Continue"
    $configuredLetter = $letter.ToUpperInvariant()
    $o = [ordered]@{}
    $o.cudaFreeBeforeMiB = [uint64](((& nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits 2>$null) | Select-Object -First 1).Trim())
    $dumpBefore = @(
        Get-ChildItem C:\Windows\Minidump\*.dmp -File -EA SilentlyContinue
        Get-Item C:\Windows\MEMORY.DMP -EA SilentlyContinue
    ) | ForEach-Object { $_.FullName + "|" + $_.LastWriteTimeUtc.Ticks }
    New-Item -Force -ItemType Directory C:\ProgramData\RamShared\evidence | Out-Null
    New-Item -Force -ItemType Directory C:\ramshared\artifacts | Out-Null
    Set-Content -Encoding ascii C:\ProgramData\RamShared\guest-product.toml -Value $cfg

    # The package was deployed followed by a mandatory reboot before round 1.
    $imagePath = [string](Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\ramshared" -Name ImagePath -EA Stop).ImagePath
    $sysDst = $imagePath.Trim('"') -replace '^\\SystemRoot', $env:SystemRoot -replace '^\\\?\?\\', ''
    $pnpLog = @()
    $ramPnp = @(Get-PnpDevice -EA SilentlyContinue | Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' })
    if ($ramPnp.Count -lt 1) { throw "ramshared PnP device missing before service start" }
    foreach ($dev in $ramPnp) {
        $enable = pnputil /enable-device $dev.InstanceId 2>&1 | Out-String
        $enableExit = $LASTEXITCODE
        $pnpLog += ("enable {0} exit={1} {2}" -f $dev.InstanceId, $enableExit, ($enable.Trim()))
        if ($enableExit -ne 0 -and $enable -notmatch "Device is already enabled") {
            throw ("pnputil enable failed: " + $enable)
        }
        $restart = pnputil /restart-device $dev.InstanceId 2>&1 | Out-String
        $restartExit = $LASTEXITCODE
        $pnpLog += ("restart {0} exit={1} {2}" -f $dev.InstanceId, $restartExit, ($restart.Trim()))
        if ($restartExit -ne 0 -and $restart -notmatch "not supported on this OS product") {
            throw ("pnputil restart failed: " + $restart)
        }
    }
    $pnpOk = $false
    $pnpState = @()
    for ($waitPnp = 0; $waitPnp -lt 30; $waitPnp++) {
        $pnpState = @(Get-PnpDevice -EA SilentlyContinue | Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' })
        $ok = @($pnpState | Where-Object { $_.Status -eq "OK" -and ([int]$_.Problem) -eq 0 })
        if ($ok.Count -ge 1) { $pnpOk = $true; break }
        Start-Sleep 1
    }
    $o.pnpBeforeServiceStart = ($pnpState | ForEach-Object {
        "{0}|problem={1}|{2}" -f $_.Status, ([int]$_.Problem), $_.InstanceId
    }) -join "; "
    $o.pnpEnableLog = $pnpLog -join "`n"
    if (-not $pnpOk) {
        throw ("ramshared PnP not OK before service start: " + $o.pnpBeforeServiceStart)
    }
    sc.exe start ramshared 2>&1 | Out-Null
    Start-Sleep 3
    $o.ram = ((sc.exe query ramshared 2>&1 | Out-String) -match "RUNNING")
    $o.sysSha = if (Test-Path $sysDst) { (Get-FileHash $sysDst -Algorithm SHA256).Hash } else { "missing" }
    $o.packageSha = if (Test-Path C:\ramshared\package\ramshared.sys) {
        (Get-FileHash C:\ramshared\package\ramshared.sys -Algorithm SHA256).Hash
    } else { "missing" }
    $o.driverImagePath = $sysDst
    $o.driverStoreImage = ($sysDst -match '\\DriverStore\\FileRepository\\')
    $o.binaryMatch = ($o.sysSha -eq $o.packageSha -and $o.sysSha -ne "missing" -and $o.driverStoreImage)
    if (-not $o.binaryMatch) {
        $o.driver_store_binary_mismatch = ("driver_store_binary_mismatch sys={0} package={1} image={2}" -f
            $o.sysSha, $o.packageSha, $o.driverImagePath)
        $o | ConvertTo-Json -Compress
        return
    }

    # Minimal lab lease broker (JSONL, no BOM). Hand-built JSON matches broker codec.
    $brokerScript = @'
$ErrorActionPreference = "Continue"
$port = [int]$env:RS_BROKER_PORT
$log = "C:\ProgramData\RamShared\broker-lab.log"
$utf8 = New-Object System.Text.UTF8Encoding $false
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $port)
$listener.Start()
[IO.File]::AppendAllText($log, ("listen 127.0.0.1:{0}`n" -f $port), $utf8)
$leaseSeq = 1
while ($true) {
  if (Test-Path "C:\ProgramData\RamShared\broker-lab.stop") { break }
  if (-not $listener.Pending()) { Start-Sleep -Milliseconds 50; continue }
  $client = $listener.AcceptTcpClient()
  $stream = $client.GetStream()
  $reader = New-Object System.IO.StreamReader($stream, $utf8, $false, 4096, $true)
  $writer = New-Object System.IO.StreamWriter($stream, $utf8, 4096, $true)
  $writer.NewLine = "`n"
  $writer.AutoFlush = $true
  try {
    while ($client.Connected) {
      $line = $reader.ReadLine()
      if ($null -eq $line) { break }
      [IO.File]::AppendAllText($log, ("in " + $line + "`n"), $utf8)
      try { $msg = $line | ConvertFrom-Json } catch {
        [IO.File]::AppendAllText($log, ("parse_err " + $_.Exception.Message + "`n"), $utf8)
        continue
      }
      $type = [string]$msg.type
      if ($type -eq "register") {
        $resp = '{"type":"registered","tenant_id":1}'
      } elseif ($type -eq "lease_request") {
        $bytes = [uint64]$msg.bytes
        $id = $leaseSeq; $leaseSeq++
        $resp = ('{{"type":"lease_granted","lease":{0},"bytes":{1}}}' -f $id, $bytes)
      } elseif ($type -eq "lease_release") {
        $id = [int]$msg.lease
        [IO.File]::AppendAllText($log, ("lease {0} liberado`n" -f $id), $utf8)
        $resp = '{"type":"ack"}'
      } elseif ($type -eq "psi" -or $type -eq "status") {
        $resp = '{"type":"ack"}'
      } else {
        $resp = ('{{"type":"error","reason":"unknown {0}"}}' -f $type)
      }
      $writer.WriteLine($resp)
      [IO.File]::AppendAllText($log, ("out " + $resp + "`n"), $utf8)
    }
  } catch {
    [IO.File]::AppendAllText($log, ("err " + $_.Exception.Message + "`n"), $utf8)
  } finally {
    try { $reader.Close() } catch {}
    try { $writer.Close() } catch {}
    try { $client.Close() } catch {}
  }
}
try { $listener.Stop() } catch {}
[IO.File]::AppendAllText($log, "stop`n", $utf8)
'@
    Set-Content -Encoding ascii C:\ramshared\bin\Lab-LeaseBroker.ps1 -Value $brokerScript
    Remove-Item C:\ProgramData\RamShared\broker-lab.stop -Force -EA SilentlyContinue
    Remove-Item C:\ProgramData\RamShared\broker-lab.log -Force -EA SilentlyContinue
    Remove-Item C:\ProgramData\RamShared\stop.request -Force -EA SilentlyContinue
    $env:RS_BROKER_PORT = "$brokerPort"
    $bp = Start-Process -FilePath powershell.exe -ArgumentList @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "C:\ramshared\bin\Lab-LeaseBroker.ps1"
    ) -WindowStyle Hidden -PassThru
    $o.brokerPid = $bp.Id
    Start-Sleep 1

    # Start product console
    $out = "C:\ramshared\artifacts\guest-online-console.out"
    $err = "C:\ramshared\artifacts\guest-online-console.err"
    Remove-Item $out, $err -Force -EA SilentlyContinue
    $p = Start-Process -FilePath "C:\ramshared\bin\ramshared-winsvc.exe" -ArgumentList @(
        "console", "--storage-only", "--config", "C:\ProgramData\RamShared\guest-product.toml"
    ) -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err
    $o.consolePid = $p.Id

    # Require the full Online line (not the "starting product Online" banner).
    $online = $false
    $serial = $null
    for ($i = 0; $i -lt 120; $i++) {
        Start-Sleep 1
        $txt = ""
        if (Test-Path $err) { $txt = Get-Content $err -Raw -EA SilentlyContinue }
        if ($txt -match "product Online:\s*run_id=") {
            $online = $true
            if ($txt -match "serial=([0-9A-Fa-f]{16})") { $serial = $Matches[1].ToUpperInvariant() }
            if ($txt -match "lease=([0-9]+)") { $o.leaseFromLog = [uint64]$Matches[1] }
            break
        }
        if ($p.HasExited) { break }
    }
    $o.online = $online
    $o.consoleExitedEarly = [bool]$p.HasExited
    if ($p.HasExited) { $o.consoleExit = $p.ExitCode }
    $o.consoleErrTail = if (Test-Path $err) { (Get-Content $err -Tail 40) -join "`n" } else { "" }
    $o.consoleOutTail = if (Test-Path $out) { (Get-Content $out -Tail 20) -join "`n" } else { "" }
    $o.serialFromLog = $serial
    $o.brokerLogEarly = if (Test-Path C:\ProgramData\RamShared\broker-lab.log) {
        # Plain text only — never pipe FileInfo into ConvertTo-Json (explodes PSDrive graph).
        [string](Get-Content C:\ProgramData\RamShared\broker-lab.log -Raw -EA SilentlyContinue)
    } else { "" }

    if (-not $online) {
        if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -EA SilentlyContinue }
        New-Item -ItemType File -Path C:\ProgramData\RamShared\broker-lab.stop -Force | Out-Null
        if (-not $bp.HasExited) { Stop-Process -Id $bp.Id -Force -EA SilentlyContinue }
        $o | ConvertTo-Json -Compress
        return
    }

    # Wait for disk with exact identity
    $disk = $null
    $expectedSerial = $serial
    for ($i = 0; $i -lt 40; $i++) {
        try { "rescan" | diskpart 2>$null | Out-Null } catch {}
        $cands = @(Get-Disk -EA SilentlyContinue | Where-Object {
            $_.FriendlyName -match "RAMSHARE" -and $_.FriendlyName -match "VRAMDISK" -and $_.Size -eq $sizeBytes
        })
        if ($expectedSerial) {
            $exact = @($cands | Where-Object { ([string]$_.SerialNumber).Trim() -ieq $expectedSerial })
            if ($exact.Count -eq 1) { $disk = $exact[0]; break }
        }
        if (-not $disk -and $cands.Count -eq 1) { $disk = $cands[0]; break }
        # Win32 path for serial
        $w = @(Get-CimInstance Win32_DiskDrive -EA SilentlyContinue | Where-Object {
            ([string]$_.Model) -match "RAMSHARE" -and ([string]$_.SerialNumber).Trim().Length -eq 16
        })
        if ($w.Count -ge 1 -and $expectedSerial) {
            $hit = $w | Where-Object { ([string]$_.SerialNumber).Trim() -ieq $expectedSerial } | Select-Object -First 1
            if ($hit) {
                $disk = Get-Disk -EA SilentlyContinue | Where-Object {
                    $_.FriendlyName -match "RAMSHARE" -and $_.Size -eq $sizeBytes
                } | Select-Object -First 1
                if ($disk) { break }
            }
        }
        Start-Sleep 1
    }
    if (-not $disk) {
        $o.disk = "none"
        $o | ConvertTo-Json -Compress
        return
    }
    $o.disk = ("N={0} Name={1} Size={2} Ser=[{3}]" -f $disk.Number, $disk.FriendlyName, $disk.Size, $disk.SerialNumber)

    # Online disk + format carefully (only this disk)
    try {
        if ($disk.IsOffline) { Set-Disk -Number $disk.Number -IsOffline $false -EA SilentlyContinue }
        if ($disk.IsReadOnly) { Set-Disk -Number $disk.Number -IsReadOnly $false -EA SilentlyContinue }
        if ($disk.PartitionStyle -eq "Raw") {
            Initialize-Disk -Number $disk.Number -PartitionStyle GPT -Confirm:$false -EA Stop
        }
        $part = Get-Partition -DiskNumber $disk.Number -EA SilentlyContinue | Where-Object { $_.DriveLetter } | Select-Object -First 1
        if (-not $part) {
            $used = @(); Get-PSDrive -PSProvider FileSystem | ForEach-Object { $used += $_.Name }
            $pick = $letter
            if ($used -contains $pick) {
                throw ("configured drive letter already occupied: " + $pick)
            }
            $part = New-Partition -DiskNumber $disk.Number -UseMaximumSize -DriveLetter $pick -EA Stop
            Format-Volume -DriveLetter $pick -FileSystem NTFS -NewFileSystemLabel "RAMSHARED" -Confirm:$false | Out-Null
            $letter = $pick
        } else {
            $letter = [string]$part.DriveLetter
            if ($letter.ToUpperInvariant() -ne $configuredLetter) {
                throw ("product partition mapped to unexpected letter: " + $letter)
            }
        }
        $o.letter = $letter
    } catch {
        $o.formatErr = $_.Exception.Message
        $o | ConvertTo-Json -Compress
        return
    }

    # One write/flush/read SHA per fresh lifecycle round (DT-13).
    $rounds = @()
    $probe = ($letter + ":\rs-probe.bin")
    for ($r = 1; $r -le 1; $r++) {
        $bytes = New-Object byte[] (4MB)
        (New-Object Random).NextBytes($bytes)
        [IO.File]::WriteAllBytes($probe, $bytes)
        $shaW = (Get-FileHash $probe -Algorithm SHA256).Hash
        $tmp = Join-Path $env:TEMP ("rs-guest-r" + $r + ".bin")
        [IO.File]::Copy($probe, $tmp, $true)
        $shaR = (Get-FileHash $tmp -Algorithm SHA256).Hash
        $rounds += [pscustomobject]@{ round = $r; match = ($shaW -eq $shaR); sha = $shaW }
    }
    Remove-Item $probe -Force -EA SilentlyContinue
    $o.rounds = $rounds
    $o.roundsPass = (($rounds | Where-Object { -not $_.match }).Count -eq 0) -and ($rounds.Count -eq 1)

    # Free handles on the product volume before teardown (lab).
    try {
        Get-ChildItem ($letter + ":\") -Force -EA SilentlyContinue |
            Where-Object { -not $_.PSIsContainer } |
            Remove-Item -Force -EA SilentlyContinue
    } catch {}
    Remove-Item C:\ProgramData\RamShared\teardown-diag.log -Force -EA SilentlyContinue
    Remove-Item C:\ProgramData\RamShared\stop.request -Force -EA SilentlyContinue

    $o.manufacturedPagefileRefuse = [bool]$manufacturedPagefileRefuse
    $o.pagefileRefuse = $null
    if ($manufacturedPagefileRefuse) {
        # Inject configured PagingFiles entry for product letter, pulse stop,
        # expect Gate A refuse (code 7) while process remains Online, then restore.
        $inj = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File `
            C:\ramshared\bin\Invoke-PagefileRefusalManufactured.ps1 `
            -Letter $letter `
            -StopRequestPath C:\ProgramData\RamShared\stop.request `
            -DiagPath C:\ProgramData\RamShared\teardown-diag.log `
            -ErrLogPath $err `
            -StopWaitSec 25 *>&1
        $o.pagefileInjectOut = ($inj | Out-String)
        $jsonLine = @($inj | Where-Object { $_ -is [string] -and $_.TrimStart().StartsWith("{") } | Select-Object -Last 1)
        if ($jsonLine) {
            try { $o.pagefileRefuse = ($jsonLine | ConvertFrom-Json) } catch {}
        }
        $o.pagefileRefusePass = $false
        if ($o.pagefileRefuse -and $o.pagefileRefuse.pass -and $o.pagefileRefuse.refuseObserved) {
            $o.pagefileRefusePass = $true
        }
        # Process should still be Online after refuse; now real stop without inject.
        Start-Sleep 1
    }

    # Graceful stop. On Gate A/B/lock refusal the runtime resumes Online and
    # clears the stop flag after the poller already deleted stop.request — so we
    # re-assert the file every 2s until exit, bounded by DT-13's 30s budget.
    $stopOk = $false
    $stopWatch = [Diagnostics.Stopwatch]::StartNew()
    for ($i = 0; $i -lt 30; $i++) {
        if (($i % 2) -eq 0) {
            New-Item -ItemType File -Path C:\ProgramData\RamShared\stop.request -Force | Out-Null
        }
        Start-Sleep 1
        if ($p.HasExited) { $stopOk = $true; break }
    }
    $stopWatch.Stop()
    if ($p.HasExited) {
        try {
            $p.Refresh()
            $p.WaitForExit()
        } catch {}
    }
    $o.teardownMs = [uint64]$stopWatch.ElapsedMilliseconds
    $o.stopErrSnap = if (Test-Path $err) { (Get-Content $err -Tail 60) -join "`n" } else { "" }
    if ($p.HasExited) {
        $rawExit = $null
        try { $rawExit = $p.ExitCode } catch {}
        if ($null -ne $rawExit) {
            $o.consoleExit = [int]$rawExit
            $o.consoleExitSource = "process"
        } elseif ($o.stopErrSnap -match 'console stopped: RuntimeSummary .*exit_code: 0') {
            $o.consoleExit = 0
            $o.consoleExitSource = "runtime_summary"
        } else {
            $o.consoleExit = "missing_exit_code"
            $o.consoleExitSource = "missing"
        }
    } else {
        $o.consoleExit = "still_running"
        $o.consoleExitSource = "process"
    }
    $o.teardownDiag = if (Test-Path C:\ProgramData\RamShared\teardown-diag.log) {
        [string](Get-Content C:\ProgramData\RamShared\teardown-diag.log -Raw -EA SilentlyContinue)
    } else { "" }
    $o.evidenceTail = ""
    try {
        $ev = Get-ChildItem C:\ProgramData\RamShared\evidence -File -EA SilentlyContinue |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($ev) {
            $o.evidenceTail = [string]((Get-Content $ev.FullName -Tail 5 -EA SilentlyContinue) -join "`n")
        }
    } catch {}
    # Never force-kill the product: timeout leaves owners intact until VM cleanup.
    $o.forceKilledConsole = $false

    # Stop lab broker
    New-Item -ItemType File -Path C:\ProgramData\RamShared\broker-lab.stop -Force | Out-Null
    Start-Sleep 1
    if (-not $bp.HasExited) { Stop-Process -Id $bp.Id -Force -EA SilentlyContinue }

    $o.brokerLog = if (Test-Path C:\ProgramData\RamShared\broker-lab.log) {
        # Keep last ~4KB only — full psi stream blew prior JSON artifacts.
        $bl = [string](Get-Content C:\ProgramData\RamShared\broker-lab.log -Raw -EA SilentlyContinue)
        if ($bl.Length -gt 4000) { $bl.Substring($bl.Length - 4000) } else { $bl }
    } else { "" }
    $o.consoleErrEnd = if (Test-Path $err) { (Get-Content $err -Tail 40) -join "`n" } else { "" }
    $o.leaseLiberado = $o.leaseFromLog -and ($o.brokerLog -match ("lease " + $o.leaseFromLog + " liberado"))
    $cudaRestoreWatch = [Diagnostics.Stopwatch]::StartNew()
    $cudaRestoreSamples = @()
    $cudaFreeAfter = [uint64]0
    $cudaRestored = $false
    for ($cudaPoll = 0; $cudaPoll -lt 31; $cudaPoll++) {
        $freeText = [string](((& nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits 2>$null) |
            Select-Object -First 1).Trim())
        $freeValue = [uint64]0
        if ([uint64]::TryParse($freeText, [ref]$freeValue)) {
            $cudaFreeAfter = $freeValue
            $cudaRestoreSamples += ("{0}ms={1}MiB" -f $cudaRestoreWatch.ElapsedMilliseconds, $freeValue)
            $cudaRestored = ($o.cudaFreeBeforeMiB -gt 0) -and ($freeValue -gt 0) -and `
                ($freeValue + 64 -ge $o.cudaFreeBeforeMiB)
            if ($cudaRestored) { break }
        } else {
            $cudaRestoreSamples += ("{0}ms=parse:{1}" -f $cudaRestoreWatch.ElapsedMilliseconds, $freeText)
        }
        Start-Sleep 1
    }
    $cudaRestoreWatch.Stop()
    $o.nvidiaAfter = [string]((& nvidia-smi --query-gpu=name,memory.used,memory.total --format=csv,noheader 2>$null) -join "; ")
    $o.cudaFreeAfterMiB = $cudaFreeAfter
    $o.cudaRestoreWaitMs = [uint64]$cudaRestoreWatch.ElapsedMilliseconds
    $o.cudaRestoreSamples = $cudaRestoreSamples
    $o.cudaRestored = $cudaRestored
    $dumpAfter = @(
        Get-ChildItem C:\Windows\Minidump\*.dmp -File -EA SilentlyContinue
        Get-Item C:\Windows\MEMORY.DMP -EA SilentlyContinue
    ) | ForEach-Object { $_.FullName + "|" + $_.LastWriteTimeUtc.Ticks }
    $dumpBeforeRows = @($dumpBefore)
    $dumpAfterRows = @($dumpAfter)
    $newDumps = @($dumpAfterRows | Where-Object { $dumpBeforeRows -notcontains $_ })
    $o.noNewDump = ($newDumps.Count -eq 0)
    $o.stopOk = $stopOk -and ($o.consoleExit -eq 0) -and ($o.teardownMs -le 30000) -and $o.leaseLiberado -and $o.cudaRestored -and $o.noNewDump
    if ($manufacturedPagefileRefuse) {
        # Campaign success requires Gate A refuse observed before the clean stop.
        $o.stopOk = $o.stopOk -and [bool]$o.pagefileRefusePass
    }
    # Compact JSON only — avoid serializing PSObject graphs.
    $o | ConvertTo-Json -Depth 4 -Compress
} -ArgumentList @($cfgText, [uint64]$SizeBytes, $BrokerPort, $Letter, [bool]$ManufacturedPagefileRefuse)
$load = [string]($roundLoad | Where-Object { $_ -is [string] -and $_.TrimStart().StartsWith("{") } | Select-Object -Last 1)
if ([string]::IsNullOrWhiteSpace($load)) { throw ("round {0} returned no JSON result" -f $campaignRound) }
$load | Set-Content (Join-Path $art ("guest-result-round-{0}.json" -f $campaignRound))
$loads += $load
W ("lifecycle round {0}/{1} result={2}" -f $campaignRound, $lifecycleRounds, $load)
$roundResult = $load | ConvertFrom-Json
$roundPass = $roundResult.online -and $roundResult.binaryMatch -and $roundResult.roundsPass -and `
    ($roundResult.consoleExit -eq 0) -and (-not $roundResult.forceKilledConsole) -and `
    $roundResult.leaseLiberado -and $roundResult.cudaRestored -and $roundResult.noNewDump -and `
    ($roundResult.teardownMs -le 30000)
if ($ManufacturedPagefileRefuse) {
    $roundPass = $roundPass -and [bool]$roundResult.pagefileRefusePass
}
if (-not $roundPass) { throw ("lifecycle round {0} failed; no retry" -f $campaignRound) }
}

$loads | Set-Content (Join-Path $art "guest-result.json")

Stop-VM -Name $VMName -Force -ErrorAction SilentlyContinue
Start-Sleep 2
$vmOff = ((Get-VM $VMName).State -eq "Off")
W ("VM=" + (Get-VM $VMName).State)
$n = Get-PnpDevice -Class Display | Where-Object FriendlyName -Match NVIDIA | Select-Object -First 1
W ("HOST_GPU=" + $n.Status)
$terminalSafe = $vmOff -and ($n.Status -eq "OK")

# DT-13 product Online requires 3 lifecycle rounds. Manufactured pagefile refuse
# is a single-round strengthening gate (Online + Gate A refuse + clean stop).
$results = @($loads | ForEach-Object { $_ | ConvertFrom-Json })
$expectedRounds = $lifecycleRounds
$allOnline = ($results.Count -eq $expectedRounds) -and (@($results | Where-Object { -not $_.online }).Count -eq 0)
$allBinaryMatch = (@($results | Where-Object { -not $_.binaryMatch }).Count -eq 0)
$allSha = (@($results | Where-Object { -not $_.roundsPass }).Count -eq 0)
$allExitZero = (@($results | Where-Object { $_.consoleExit -ne 0 }).Count -eq 0)
$noForceKill = (@($results | Where-Object { $_.forceKilledConsole }).Count -eq 0)
$allLeaseReleased = (@($results | Where-Object { -not $_.leaseLiberado }).Count -eq 0)
$allCudaRestored = (@($results | Where-Object { -not $_.cudaRestored }).Count -eq 0)
$allNoNewDump = (@($results | Where-Object { -not $_.noNewDump }).Count -eq 0)
$allWithinBudget = (@($results | Where-Object { $_.teardownMs -gt 30000 }).Count -eq 0)
$allPagefileRefuse = $true
if ($ManufacturedPagefileRefuse) {
    $allPagefileRefuse = (@($results | Where-Object { -not $_.pagefileRefusePass }).Count -eq 0)
}

$summary = [ordered]@{
    ARTIFACT = $art
    LIFECYCLE_ROUNDS = $results.Count
    ONLINE = $allOnline
    BINARY_MATCH = $allBinaryMatch
    ROUNDS_PASS = $allSha
    CONSOLE_EXIT_ZERO = $allExitZero
    NO_FORCE_KILL = $noForceKill
    LEASE_RELEASED = $allLeaseReleased
    CUDA_RESTORED = $allCudaRestored
    NO_NEW_DUMP = $allNoNewDump
    TEARDOWN_WITHIN_BUDGET = $allWithinBudget
    TERMINAL_SAFE = $terminalSafe
    MANUFACTURED_PAGEFILE_REFUSE = [bool]$ManufacturedPagefileRefuse
    PAGEFILE_REFUSE_PASS = $allPagefileRefuse
}
$summary.PASS = $summary.ONLINE -and $summary.BINARY_MATCH -and $summary.ROUNDS_PASS -and `
    $summary.CONSOLE_EXIT_ZERO -and $summary.NO_FORCE_KILL -and $summary.LEASE_RELEASED -and `
    $summary.CUDA_RESTORED -and $summary.NO_NEW_DUMP -and `
    $summary.TEARDOWN_WITHIN_BUDGET -and $summary.TERMINAL_SAFE -and $allPagefileRefuse
$summary | ConvertTo-Json | Set-Content (Join-Path $art "summary.json")
W ("SUMMARY " + ($summary | ConvertTo-Json -Compress))

if (-not $summary.PASS) {
    exit 2
}
exit 0
