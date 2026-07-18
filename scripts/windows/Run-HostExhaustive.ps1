#Requires -Version 5.1
# Careful host campaign: Online + 3-round SHA + graceful stop.
# ONLY formats the RAMSHARE VRAMDISK LUN. Drive letter fixed to S (config).
$ErrorActionPreference = "Continue"
$PreferredLetter = "S"
$art = "C:\ramshared\artifacts\exhaustive-$(Get-Date -Format yyyyMMdd-HHmmss)"
New-Item -Force -ItemType Directory $art | Out-Null
$log = Join-Path $art "host-online.log"
function W($m) {
    $t = "[{0}] {1}" -f (Get-Date -Format HH:mm:ss), $m
    $t | Tee-Object -FilePath $log -Append
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
        if (Test-Path $ctl) { $ctlOk = $true; break }
    } catch {}
}
if (($rsNow -match "RUNNING") -and -not $ctlOk) {
    W "FAIL ramshared service RUNNING but RamSharedCtl absent; reboot/unload/redeploy before physical Online"
    @{ HOST_ONLINE = $false; CONTROL_PATH = $false; ARTIFACT = $art } |
        ConvertTo-Json | Set-Content (Join-Path $art "summary.json")
    exit 1
}

$exe = "C:\ramshared\bin\ramshared-winsvc.exe"
$cfg = "C:\ProgramData\RamShared\winsvc-product.toml"
$cfgText = Get-Content $cfg -Raw -ErrorAction SilentlyContinue
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
        $_.FriendlyName -match "RAMSHARE|VRAMDISK" -and $_.Size -eq 67108864
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
$diskSizeOk = ([uint64]$disk.Size -eq 67108864)
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

# Format carefully: only this disk number, prefer letter S, never steal C:
$letter = $null
$part = Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue |
    Where-Object { $_.DriveLetter } | Select-Object -First 1
if ($part) {
    $letter = [string]$part.DriveLetter
    $vol = Get-Volume -DriveLetter $letter -ErrorAction SilentlyContinue
    if (-not $vol -or $vol.FileSystemLabel -ne "RAMSHARED" -or $vol.FileSystem -ne "NTFS") {
        W ("FAIL existing letter refused before write: {0}: label={1} fs={2}" -f
            $letter, $vol.FileSystemLabel, $vol.FileSystem)
        New-Item -ItemType File -Path $stop -Force | Out-Null
        New-Item -ItemType File -Path $brokerStop -Force | Out-Null
        Start-Sleep 1
        if ($bp -and -not $bp.HasExited) { Stop-Process -Id $bp.Id -Force -ErrorAction SilentlyContinue }
        if (Test-Path $brokerLog) { Copy-Item $brokerLog (Join-Path $art "broker-lab.log") -Force }
        exit 1
    }
    W ("existing letter=" + $letter)
} else {
    try {
        if ($disk.PartitionStyle -ne "Raw") {
            throw "refuse non-raw RAMSHARE disk without mounted RAMSHARED volume"
        }
        Initialize-Disk -Number $disk.Number -PartitionStyle GPT -Confirm:$false -ErrorAction Stop
        # Prefer S; if taken by non-RAMSHARE, use first free from R,S,T,U,V (not D/E/G system-ish)
        $candidates = @("S","R","T","U","V","W")
        $used = @()
        Get-PSDrive -PSProvider FileSystem | ForEach-Object { $used += $_.Name }
        $pick = $null
        foreach ($c in $candidates) {
            if ($used -notcontains $c) { $pick = $c; break }
        }
        if (-not $pick) { throw "no free lab letter in S/R/T/U/V/W" }
        $np = New-Partition -DiskNumber $disk.Number -UseMaximumSize -DriveLetter $pick -ErrorAction Stop
        $np | Format-Volume -FileSystem NTFS -NewFileSystemLabel "RAMSHARED" -Confirm:$false | Out-Null
        $letter = $pick
        W ("formatted letter=" + $letter + " on disk " + $disk.Number + " only")
    } catch {
        W ("format err: " + $_.Exception.Message)
    }
}

$rounds = @()
if ($letter) {
    $probe = ($letter + ":\rs-probe.bin")
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

# Graceful stop - wait up to 45s (FSCTL dismount should be fast)
New-Item -ItemType File -Path $stop -Force | Out-Null
W "stop.request created"
$stopped = $false
$exitCode = -1
for ($i = 0; $i -lt 45; $i++) {
    Start-Sleep 1
    if ($p.HasExited) {
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
}

# Post-check: RAMSHARE disk should be gone after clean destroy
Start-Sleep 2
$left = @(Get-Disk | Where-Object { $_.FriendlyName -match "RAMSHARE|VRAMDISK" })
W ("ramshare_disks_left=" + $left.Count)

$sum = [ordered]@{
    HOST_ONLINE = [bool]$online
    ALL_MATCH   = [bool]$all
    GRACEFUL    = [bool]$stopped
    LETTER      = $letter
    ARTIFACT    = $art
    EXIT        = $exitCode
    ROUNDS      = $rounds.Count
    LUN_GONE    = ($left.Count -eq 0)
    LEASE_RELEASED = [bool]$leaseReleased
}
$sum | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $art "summary.json")
W ("SUMMARY " + ($sum | ConvertTo-Json -Compress))
Write-Host ("ARTIFACT=" + $art)
if ($online -and $all -and $stopped -and $exitCode -eq 0 -and $leaseReleased) { exit 0 } else { exit 2 }
