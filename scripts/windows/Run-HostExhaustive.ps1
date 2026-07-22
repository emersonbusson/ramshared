#Requires -Version 5.1
# Careful host campaign: Online + 3-round SHA + graceful stop.
# ONLY formats the RAMSHARE VRAMDISK LUN. Drive letter fixed to S (config).
[CmdletBinding()]
param(
    [UInt64]$SizeBytes = 67108864,
    [int]$ExternalWorkloadMiB = 0,
    [int]$ExternalWorkloadHoldSec = 20,
    [int]$MinFreeAfterPlanMiB = 256,
    [int]$WslPressureMiB = 0,
    [switch]$ApproveSharedDesktopWsl
)

$ErrorActionPreference = "Continue"
$PreferredLetter = "S"
$art = "C:\ramshared\artifacts\exhaustive-$(Get-Date -Format yyyyMMdd-HHmmss)"
New-Item -Force -ItemType Directory $art | Out-Null
$log = Join-Path $art "host-online.log"
function W($m) {
    $t = "[{0}] {1}" -f (Get-Date -Format HH:mm:ss), $m
    $t | Tee-Object -FilePath $log -Append
}
function Test-ControlPath([string]$Path) {
    if (-not ("RamSharedCtlOpen" -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class RamSharedCtlOpen {
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  static extern IntPtr CreateFile(string path, uint access, uint share, IntPtr sec, uint creation, uint flags, IntPtr template);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern bool CloseHandle(IntPtr h);

  public static int TryOpen(string path) {
    IntPtr h = CreateFile(path, 0x80000000u | 0x40000000u, 0, IntPtr.Zero, 3, 0, IntPtr.Zero);
    long v = h.ToInt64();
    if (v == -1 || v == 0) return Marshal.GetLastWin32Error();
    CloseHandle(h);
    return 0;
  }
}
'@
    }
    return [RamSharedCtlOpen]::TryOpen($Path)
}
function Read-GpuFreeMiB {
    try {
        $line = & nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits 2>$null |
            Select-Object -First 1
        if ($line) { return [int]$line.Trim() }
    } catch {}
    return 0
}

$stop = "C:\ProgramData\RamShared\stop.request"
$brokerStop = "C:\ProgramData\RamShared\broker-lab.stop"
$brokerLog = "C:\ProgramData\RamShared\broker-lab.log"
Remove-Item $stop -Force -ErrorAction SilentlyContinue
Remove-Item $brokerStop, $brokerLog -Force -ErrorAction SilentlyContinue
Get-Process ramshared-winsvc -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep 1

$rs = (sc.exe query ramshared | Out-String)
if ($rs -notmatch "RUNNING") {
    sc.exe start poolstress | Out-Null
    sc.exe start ramshared | Out-Null
    Start-Sleep 2
}
W ("ramshared running=" + ($rs -match "RUNNING"))
$rsNow = (sc.exe query ramshared | Out-String)
$ctlOk = $false
foreach ($ctl in @("\\.\RamSharedCtl", "\\.\GLOBALROOT\Device\RamSharedCtl")) {
    try {
        $ctlErr = Test-ControlPath $ctl
        if ($ctlErr -eq 0) {
            W ("control path OK " + $ctl)
            $ctlOk = $true
            break
        }
        W ("control path open failed " + $ctl + " err=" + $ctlErr)
    } catch {
        W ("control path query failed " + $ctl + ": " + $_.Exception.Message)
    }
}
if (($rsNow -match "RUNNING") -and -not $ctlOk) {
    W "FAIL ramshared service RUNNING but RamSharedCtl absent; reboot/unload/redeploy before physical Online"
    @{ HOST_ONLINE = $false; CONTROL_PATH = $false; ARTIFACT = $art } |
        ConvertTo-Json | Set-Content (Join-Path $art "summary.json")
    exit 1
}

$exe = "C:\ramshared\bin\ramshared-winsvc.exe"
$cfg = "C:\ProgramData\RamShared\winsvc-product.toml"
$mountRoot = "C:\ProgramData\RamShared\mounts"
$mountPath = Join-Path $mountRoot ("lun-{0}" -f $PID)
$cfgText = Get-Content $cfg -Raw -ErrorAction SilentlyContinue
$stagedPressureMiB = if ($WslPressureMiB -gt 0) { $WslPressureMiB } else { $ExternalWorkloadMiB }
$plannedMiB = [int][Math]::Ceiling(([double]$SizeBytes / 1MB)) + $stagedPressureMiB + $MinFreeAfterPlanMiB
$freeBeforeMiB = Read-GpuFreeMiB
W ("plan size_bytes={0} external_workload_mib={1} min_free_after_plan_mib={2} gpu_free_before_mib={3}" -f
    $SizeBytes, $ExternalWorkloadMiB, $MinFreeAfterPlanMiB, $freeBeforeMiB)
if ($freeBeforeMiB -gt 0 -and $plannedMiB -gt $freeBeforeMiB) {
    W ("FAIL insufficient VRAM headroom: need_mib={0} free_mib={1}" -f $plannedMiB, $freeBeforeMiB)
    @{ HOST_ONLINE = $false; HEADROOM = $false; ARTIFACT = $art; SIZE_BYTES = $SizeBytes } |
        ConvertTo-Json | Set-Content (Join-Path $art "summary.json")
    exit 1
}
if ($WslPressureMiB -gt 0 -and -not $ApproveSharedDesktopWsl) {
    W "REFUSE WSL pressure: -ApproveSharedDesktopWsl is required before LUN creation"
    @{ HOST_ONLINE = $false; WSL_PRESSURE_OK = $false; ARTIFACT = $art; SIZE_BYTES = $SizeBytes } |
        ConvertTo-Json | Set-Content (Join-Path $art "summary.json")
    exit 2
}
$cfgRun = Join-Path $art "winsvc-run.toml"
if ($cfgText -match '(?m)^size_bytes\s*=') {
    $cfgText = $cfgText -replace '(?m)^size_bytes\s*=.*$', ("size_bytes = " + $SizeBytes)
} else {
    $cfgText = "[win_drive]`nsize_bytes = $SizeBytes`n" + $cfgText
}
$mountToml = $mountPath.Replace('\', '\\')
if ($cfgText -match '(?m)^volume_mount_path\s*=') {
    $cfgText = $cfgText -replace '(?m)^volume_mount_path\s*=.*$', ('volume_mount_path = "' + $mountToml + '"')
} else {
    $cfgText = $cfgText -replace '(?m)^(volume_letter\s*=.*)$', ('$1' + "`n" + 'volume_mount_path = "' + $mountToml + '"')
}
Set-Content -Encoding ascii -Path $cfgRun -Value $cfgText
$cfg = $cfgRun
W ("run config=" + $cfg)
$brokerPort = 19876
if ($cfgText -match 'broker\s*=\s*"127\.0\.0\.1:(\d+)"') {
    $brokerPort = [int]$Matches[1]
}

# Minimal lab lease broker (JSONL, no BOM). This matches the guest product
# campaign broker and keeps the physical host runner self-contained.
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
$brokerPath = Join-Path $art "Lab-LeaseBroker.ps1"
Set-Content -Encoding ascii $brokerPath -Value $brokerScript
$env:RS_BROKER_PORT = "$brokerPort"
$bp = Start-Process -FilePath powershell.exe -ArgumentList @(
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $brokerPath
) -PassThru -WindowStyle Hidden
W ("started broker pid=" + $bp.Id + " port=" + $brokerPort)
Start-Sleep 1

$out = Join-Path $art "console.out"
$err = Join-Path $art "console.err"
$p = Start-Process -FilePath $exe -ArgumentList @("console", "--storage-only", "--config", $cfg) `
    -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err
W ("started winsvc pid=" + $p.Id)

$online = $false
for ($i = 0; $i -lt 90; $i++) {
    Start-Sleep 1
    $txt = ""
    if (Test-Path $err) { $txt = Get-Content $err -Raw -ErrorAction SilentlyContinue }
    if ($txt -match "product Online:") { $online = $true; W "ONLINE seen"; break }
    if ($p.HasExited) { W ("exited early code=" + $p.ExitCode); break }
}
if (-not $online) {
    W "FAIL no Online"
    if (Test-Path $err) { Get-Content $err | Select-Object -Last 40 | ForEach-Object { W $_ } }
    @{ HOST_ONLINE = $false; ARTIFACT = $art } | ConvertTo-Json | Set-Content (Join-Path $art "summary.json")
    New-Item -ItemType File -Path $brokerStop -Force | Out-Null
    Start-Sleep 1
    if ($bp -and -not $bp.HasExited) { Stop-Process -Id $bp.Id -Force -ErrorAction SilentlyContinue }
    if (Test-Path $brokerLog) { Copy-Item $brokerLog (Join-Path $art "broker-lab.log") -Force }
    exit 1
}

$disk = $null
for ($i = 0; $i -lt 40; $i++) {
    $disk = Get-Disk | Where-Object {
        $_.FriendlyName -match "RAMSHARE|VRAMDISK" -and $_.Size -eq $SizeBytes
    } | Select-Object -First 1
    if ($disk) { break }
    Start-Sleep 1
}
if (-not $disk) {
    W "FAIL no RAMSHARE disk"
    New-Item -ItemType File -Path $stop -Force | Out-Null
    New-Item -ItemType File -Path $brokerStop -Force | Out-Null
    Start-Sleep 1
    if ($bp -and -not $bp.HasExited) { Stop-Process -Id $bp.Id -Force -ErrorAction SilentlyContinue }
    if (Test-Path $brokerLog) { Copy-Item $brokerLog (Join-Path $art "broker-lab.log") -Force }
    exit 1
}
W ("disk=N=" + $disk.Number + " Name=" + $disk.FriendlyName + " (RAMSHARE only)")
$diskNameOk = ([string]$disk.FriendlyName) -match '^RAMSHARE\s+VRAMDISK$'
$diskSizeOk = ([uint64]$disk.Size -eq $SizeBytes)
$diskNumberOk = ([int]$disk.Number -ne 0)
$diskBootSystem = [bool]$disk.IsBoot -or [bool]$disk.IsSystem
if (-not ($diskNameOk -and $diskSizeOk -and $diskNumberOk) -or $diskBootSystem) {
    W ("FAIL disk identity refused before format/write: N={0} Name={1} Size={2} IsBoot={3} IsSystem={4}" -f
        $disk.Number, $disk.FriendlyName, $disk.Size, $disk.IsBoot, $disk.IsSystem)
    New-Item -ItemType File -Path $stop -Force | Out-Null
    New-Item -ItemType File -Path $brokerStop -Force | Out-Null
    Start-Sleep 1
    if ($bp -and -not $bp.HasExited) { Stop-Process -Id $bp.Id -Force -ErrorAction SilentlyContinue }
    if (Test-Path $brokerLog) { Copy-Item $brokerLog (Join-Path $art "broker-lab.log") -Force }
    exit 1
}

# Format carefully: only this disk number, mounted under a private directory so
# Explorer never observes a temporary drive letter.
$letter = $null
$part = Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue | Select-Object -First 1
if ($part) {
    W "FAIL existing partition refused before private mount/write"
    New-Item -ItemType File -Path $stop -Force | Out-Null
    New-Item -ItemType File -Path $brokerStop -Force | Out-Null
    exit 1
} else {
    try {
        if ($disk.PartitionStyle -ne "Raw") {
            throw "refuse non-raw RAMSHARE disk without mounted RAMSHARED volume"
        }
        Initialize-Disk -Number $disk.Number -PartitionStyle GPT -Confirm:$false -ErrorAction Stop
        New-Item -ItemType Directory -Force -Path $mountPath | Out-Null
        $np = New-Partition -DiskNumber $disk.Number -UseMaximumSize -ErrorAction Stop
        $np | Format-Volume -FileSystem NTFS -NewFileSystemLabel "RAMSHARED" -Confirm:$false | Out-Null
        Add-PartitionAccessPath -DiskNumber $disk.Number -PartitionNumber $np.PartitionNumber -AccessPath $mountPath -ErrorAction Stop
        W ("formatted private mount=" + $mountPath + " on disk " + $disk.Number + " only")
    } catch {
        W ("format err: " + $_.Exception.Message)
    }
}

$externalOk = $true
$wslPressureOk = ($WslPressureMiB -eq 0)
if ($WslPressureMiB -gt 0) {
    $sharedHarness = Join-Path $PSScriptRoot "Invoke-SharedWslPressureCampaign.ps1"
    if (-not (Test-Path $sharedHarness)) {
        $sharedHarness = "C:\ramshared\scripts\windows\Invoke-SharedWslPressureCampaign.ps1"
    }
    if (-not (Test-Path $sharedHarness)) {
        W "FAIL shared WSL pressure harness missing"
        $externalOk = $false
    } else {
        W ("shared WSL pressure start vram_mib={0} external_mib={1}" -f $WslPressureMiB, $ExternalWorkloadMiB)
        $sharedOut = & $sharedHarness -ApproveSharedDailyHost -VramMiB $WslPressureMiB `
            -ExternalWorkloadMiB $ExternalWorkloadMiB 2>&1 | ForEach-Object { $_.ToString() }
        $sharedExit = $LASTEXITCODE
        $sharedOut | Set-Content -Encoding utf8 (Join-Path $art "shared-wsl-pressure.out")
        $wslPressureOk = ($sharedExit -eq 0)
        $externalOk = $wslPressureOk
        W ("shared WSL pressure exit={0}" -f $sharedExit)
    }
} elseif ($ExternalWorkloadMiB -gt 0) {
    $workload = "C:\ramshared\src\scripts\p0\Start-CudaVramWorkload.ps1"
    if (-not (Test-Path $workload)) {
        $workload = "C:\ramshared\scripts\p0\Start-CudaVramWorkload.ps1"
    }
    if (-not (Test-Path $workload)) {
        W "FAIL external workload script missing"
        $externalOk = $false
    } else {
        $wo = Join-Path $art "external-workload.out"
        $we = Join-Path $art "external-workload.err"
        W ("external workload start mib=" + $ExternalWorkloadMiB)
        $wp = Start-Process -FilePath powershell.exe -ArgumentList @(
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $workload,
            "-MiB", "$ExternalWorkloadMiB", "-HoldSec", "$ExternalWorkloadHoldSec"
        ) -PassThru -WindowStyle Hidden -RedirectStandardOutput $wo -RedirectStandardError $we
        $completed = $wp.WaitForExit(($ExternalWorkloadHoldSec + 20) * 1000)
        $wp.Refresh()
        if (-not $completed -or -not $wp.HasExited) {
            W "FAIL external workload timeout"
            Stop-Process -Id $wp.Id -Force -ErrorAction SilentlyContinue
            $externalOk = $false
        } elseif ($null -eq $wp.ExitCode -and
            (Test-Path $wo) -and
            ((Get-Content $wo -Raw -ErrorAction SilentlyContinue) -match '\[cuda-vram-workload\] released')) {
            W "external workload exit_code recovered from success marker"
            W "external workload OK"
        } elseif ($wp.ExitCode -ne 0) {
            W ("FAIL external workload exit=" + $wp.ExitCode)
            if (Test-Path $we) { Get-Content $we -Tail 20 | ForEach-Object { W ("external err: " + $_) } }
            $externalOk = $false
        } else {
            W "external workload OK"
        }
    }
}

$rounds = @()
if ($mountPath -and (Test-Path -LiteralPath $mountPath)) {
    $probe = Join-Path $mountPath "rs-probe.bin"
    for ($r = 1; $r -le 3; $r++) {
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $bytes = New-Object byte[] (4MB)
        (New-Object Random).NextBytes($bytes)
        [IO.File]::WriteAllBytes($probe, $bytes)
        $shaW = (Get-FileHash $probe -Algorithm SHA256).Hash
        $buf = [IO.File]::ReadAllBytes($probe)
        $tmp = Join-Path $env:TEMP ("rs-r" + $r + ".bin")
        [IO.File]::WriteAllBytes($tmp, $buf)
        $shaR = (Get-FileHash $tmp -Algorithm SHA256).Hash
        $sw.Stop()
        $match = ($shaW -eq $shaR)
        $rounds += [pscustomobject]@{ round = $r; match = $match; ms = $sw.ElapsedMilliseconds; sha = $shaW }
        W ("R" + $r + " match=" + $match + " ms=" + $sw.ElapsedMilliseconds + " sha=" + $shaW.Substring(0, 16))
    }
    Remove-Item $probe -Force -ErrorAction SilentlyContinue
}
$all = ($rounds.Count -eq 3) -and (@($rounds | Where-Object { -not $_.match }).Count -eq 0)
$rounds | ConvertTo-Json | Set-Content (Join-Path $art "rounds.json")

$diskIoOk = $false
if ($mountPath -and (Test-Path -LiteralPath $mountPath)) {
    $measure = Join-Path $PSScriptRoot "Measure-RamSharedDiskIo.ps1"
    if (-not (Test-Path $measure)) {
        $measure = "C:\ramshared\scripts\windows\Measure-RamSharedDiskIo.ps1"
    }
    if (Test-Path $measure) {
        $mo = Join-Path $art "disk-io.out"
        $me = Join-Path $art "disk-io.err"
        W ("disk I/O measure start access_path=" + $mountPath)
        $mp = Start-Process -FilePath powershell.exe -ArgumentList @(
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $measure,
            "-Seconds", "3", "-AccessPath", $mountPath, "-ProbeMiB", "8"
        ) -PassThru -WindowStyle Hidden -RedirectStandardOutput $mo -RedirectStandardError $me
        $measureCompleted = $mp.WaitForExit(20000)
        $mp.Refresh()
        if (-not $measureCompleted -or -not $mp.HasExited) {
            W "FAIL disk I/O measure timeout"
            Stop-Process -Id $mp.Id -Force -ErrorAction SilentlyContinue
        } elseif ($null -eq $mp.ExitCode -and
            (Test-Path $mo) -and
            ((Get-Content $mo -Raw -ErrorAction SilentlyContinue) -match 'Direct .* match=True')) {
            $diskIoOk = $true
            W "disk I/O measure exit_code recovered from direct checksum"
        } elseif ($mp.ExitCode -eq 0) {
            $diskIoOk = $true
            W "disk I/O measure OK"
        } else {
            W ("FAIL disk I/O measure exit=" + $mp.ExitCode)
            if (Test-Path $me) { Get-Content $me -Tail 20 | ForEach-Object { W ("disk I/O err: " + $_) } }
        }
    } else {
        W "FAIL disk I/O measure script missing"
    }
}

# Graceful stop - wait up to 45s (FSCTL dismount should be fast)
New-Item -ItemType File -Path $stop -Force | Out-Null
W "stop.request created"
$stopped = $false
$exitCode = -1
for ($i = 0; $i -lt 45; $i++) {
    Start-Sleep 1
    if ($p.HasExited) {
        $p.Refresh()
        $stopped = $true
        $exitCode = $p.ExitCode
        W ("process exited code=" + $exitCode + " after " + ($i+1) + "s")
        break
    }
    if (($i % 5) -eq 4 -and (Test-Path $err)) {
        $tail = Get-Content $err -Tail 3 -ErrorAction SilentlyContinue
        if ($tail) { W ("stderr: " + ($tail -join " | ")) }
    }
}
if (-not $stopped) {
    W "WARN still running after 45s - force kill (gap remains)"
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
}

$ev = Get-ChildItem C:\ProgramData\RamShared\evidence\run-*.jsonl -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($ev) { Copy-Item $ev.FullName (Join-Path $art $ev.Name); W ("evidence=" + $ev.FullName) }
if (Test-Path $err) { Copy-Item $err (Join-Path $art "console.err.copy") -Force }
New-Item -ItemType File -Path $brokerStop -Force | Out-Null
Start-Sleep 1
if ($bp -and -not $bp.HasExited) { Stop-Process -Id $bp.Id -Force -ErrorAction SilentlyContinue }
if (Test-Path $brokerLog) { Copy-Item $brokerLog (Join-Path $art "broker-lab.log") -Force }
$brokerTail = ""
if (Test-Path (Join-Path $art "broker-lab.log")) {
    $brokerTail = [string](Get-Content (Join-Path $art "broker-lab.log") -Raw -ErrorAction SilentlyContinue)
    if ($brokerTail.Length -gt 4000) { $brokerTail = $brokerTail.Substring($brokerTail.Length - 4000) }
}
$leaseReleased = $false
if (Test-Path $err) {
    $errText = [string](Get-Content $err -Raw -ErrorAction SilentlyContinue)
    if ($errText -match "lease=(\d+)") {
        $leaseReleased = $brokerTail -match ("lease " + $Matches[1] + " liberado")
    }
    if ($stopped -and $null -eq $exitCode -and $errText -match "exit_code:\s*0") {
        $exitCode = 0
        W "exit_code recovered from RuntimeSummary"
    }
}

# Post-check: RAMSHARE disk and stale class-stack nodes should be gone after clean destroy.
Start-Sleep 2
$left = @(Get-Disk | Where-Object { $_.FriendlyName -match "RAMSHARE|VRAMDISK" })
W ("ramshare_disks_left=" + $left.Count)
$win32Left = @(Get-CimInstance Win32_DiskDrive -ErrorAction SilentlyContinue | Where-Object {
        $_.Model -match "RAMSHARE|VRAMDISK|RamShared"
    })
W ("ramshare_win32_disks_left=" + $win32Left.Count)
$pnpLeft = @(Get-PnpDevice -PresentOnly:$false -ErrorAction SilentlyContinue | Where-Object {
        $_.InstanceId -like "SCSI\DISK&VEN_RAMSHARE&PROD_VRAMDISK*" -or
        $_.FriendlyName -match "RAMSHARE|VRAMDISK|RamShared"
    })
if ($left.Count -eq 0 -and $win32Left.Count -eq 0 -and $pnpLeft.Count -gt 0) {
    foreach ($node in $pnpLeft) {
        W ("remove stale pnp=" + $node.InstanceId)
        pnputil.exe /remove-device $node.InstanceId | Out-File -FilePath $log -Append -Encoding utf8
    }
    Start-Sleep 2
    $pnpLeft = @(Get-PnpDevice -PresentOnly:$false -ErrorAction SilentlyContinue | Where-Object {
            $_.InstanceId -like "SCSI\DISK&VEN_RAMSHARE&PROD_VRAMDISK*" -or
            $_.FriendlyName -match "RAMSHARE|VRAMDISK|RamShared"
        })
}
W ("ramshare_pnp_nodes_left=" + $pnpLeft.Count)
if ($mountPath -and (Test-Path -LiteralPath $mountPath)) {
    Remove-Item -LiteralPath $mountPath -Force -ErrorAction SilentlyContinue
}

$sum = [ordered]@{
    HOST_ONLINE = [bool]$online
    ALL_MATCH   = [bool]$all
    GRACEFUL    = [bool]$stopped
    LETTER      = $letter
    MOUNT_PATH  = $mountPath
    ARTIFACT    = $art
    EXIT        = $exitCode
    ROUNDS      = $rounds.Count
    SIZE_BYTES  = $SizeBytes
    EXTERNAL_WORKLOAD_OK = [bool]$externalOk
    WSL_PRESSURE_OK = [bool]$wslPressureOk
    LUN_GONE    = ($left.Count -eq 0)
    WIN32_GONE  = ($win32Left.Count -eq 0)
    PNP_GONE    = ($pnpLeft.Count -eq 0)
    LEASE_RELEASED = [bool]$leaseReleased
    DISK_IO_MEASURE_OK = [bool]$diskIoOk
}
$sum | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $art "summary.json")
W ("SUMMARY " + ($sum | ConvertTo-Json -Compress))
Write-Host ("ARTIFACT=" + $art)
if ($online -and $all -and $externalOk -and $diskIoOk -and $stopped -and $exitCode -eq 0 -and $leaseReleased -and
    $left.Count -eq 0 -and $win32Left.Count -eq 0 -and $pnpLeft.Count -eq 0) {
    exit 0
} else {
    exit 2
}
