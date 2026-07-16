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
    [string]$User = ".\drilladmin",
    [string]$Password = $env:RAMSHARED_DRILL_PASSWORD,
    [UInt64]$SizeBytes = 67108864,
    [string]$Letter = "S",
    [int]$BrokerPort = 19876
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

$load = Invoke-GuestBounded -TimeoutSec 600 -ScriptBlock {
    param($cfg, $sizeBytes, $brokerPort, $letter)
    $ErrorActionPreference = "Continue"
    $o = [ordered]@{}
    New-Item -Force -ItemType Directory C:\ProgramData\RamShared\evidence | Out-Null
    New-Item -Force -ItemType Directory C:\ramshared\artifacts | Out-Null
    Set-Content -Encoding ascii C:\ProgramData\RamShared\guest-product.toml -Value $cfg

    # Ensure guest-proven miniport image is loaded (package SHA).
    sc.exe stop ramshared 2>$null | Out-Null
    Get-PnpDevice -EA SilentlyContinue | Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' } |
        ForEach-Object { pnputil /disable-device $_.InstanceId 2>$null | Out-Null }
    Start-Sleep 3
    $sysDst = "C:\Windows\System32\drivers\ramshared.sys"
    if (Test-Path C:\ramshared\package\ramshared.sys) {
        $copied = $false
        foreach ($i in 1..8) {
            try {
                if (Test-Path $sysDst) {
                    takeown.exe /F $sysDst 2>$null | Out-Null
                    icacls.exe $sysDst /grant Administrators:F 2>$null | Out-Null
                    Move-Item -LiteralPath $sysDst -Destination ("$sysDst.bak-$PID-$i") -Force -EA Stop
                }
                Copy-Item C:\ramshared\package\ramshared.sys $sysDst -Force -EA Stop
                $copied = $true
                break
            } catch { Start-Sleep 1 }
        }
        $o.sysCopy = if ($copied) { "ok" } else { "locked" }
        pnputil /add-driver C:\ramshared\package\ramshared.inf /install 2>&1 | Out-Null
    } else { $o.sysCopy = "no_package" }

    sc.exe create ramshared type= kernel start= demand binPath= \SystemRoot\System32\drivers\ramshared.sys group= "SCSI Miniport" 2>$null | Out-Null
    $svc = "HKLM:\SYSTEM\CurrentControlSet\Services\ramshared"
    if (Test-Path $svc) {
        New-Item -Path "$svc\Parameters\PnpInterface" -Force | Out-Null
        New-ItemProperty -Path "$svc\Parameters\PnpInterface" -Name "5" -PropertyType DWord -Value 1 -Force | Out-Null
        New-ItemProperty -Path "$svc\Parameters" -Name "BusType" -PropertyType DWord -Value 0xA -Force | Out-Null
    }
    sc.exe start ramshared 2>&1 | Out-Null
    Get-PnpDevice -EA SilentlyContinue | Where-Object { $_.InstanceId -like '*ROOT\RAMSHARED*' } |
        ForEach-Object { pnputil /enable-device $_.InstanceId 2>$null | Out-Null; pnputil /restart-device $_.InstanceId 2>$null | Out-Null }
    Start-Sleep 3
    $o.ram = ((sc.exe query ramshared 2>&1 | Out-String) -match "RUNNING")
    $o.sysSha = if (Test-Path $sysDst) { (Get-FileHash $sysDst -Algorithm SHA256).Hash } else { "missing" }
    $o.packageSha = if (Test-Path C:\ramshared\package\ramshared.sys) {
        (Get-FileHash C:\ramshared\package\ramshared.sys -Algorithm SHA256).Hash
    } else { "missing" }
    $o.binaryMatch = ($o.sysSha -eq $o.packageSha -and $o.sysSha -ne "missing")

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
                foreach ($c in @("S", "R", "T", "U", "V", "W")) {
                    if ($used -notcontains $c) { $pick = $c; break }
                }
            }
            $part = New-Partition -DiskNumber $disk.Number -UseMaximumSize -DriveLetter $pick -EA Stop
            Format-Volume -DriveLetter $pick -FileSystem NTFS -NewFileSystemLabel "RAMSHARED" -Confirm:$false | Out-Null
            $letter = $pick
        } else {
            $letter = [string]$part.DriveLetter
        }
        $o.letter = $letter
    } catch {
        $o.formatErr = $_.Exception.Message
        $o | ConvertTo-Json -Compress
        return
    }

    # 3-round SHA
    $rounds = @()
    $probe = ($letter + ":\rs-probe.bin")
    for ($r = 1; $r -le 3; $r++) {
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
    $o.roundsPass = (($rounds | Where-Object { -not $_.match }).Count -eq 0) -and ($rounds.Count -eq 3)

    # Free handles on the product volume before teardown (lab).
    try {
        Get-ChildItem ($letter + ":\") -Force -EA SilentlyContinue |
            Where-Object { -not $_.PSIsContainer } |
            Remove-Item -Force -EA SilentlyContinue
    } catch {}
    Start-Sleep 2

    # Graceful stop. On Gate A/B/lock refusal the runtime resumes Online and
    # clears the stop flag after the poller already deleted stop.request — so we
    # re-assert the file every 2s until exit (up to 180s).
    $stopOk = $false
    for ($i = 0; $i -lt 180; $i++) {
        if (($i % 2) -eq 0) {
            New-Item -ItemType File -Path C:\ProgramData\RamShared\stop.request -Force | Out-Null
        }
        Start-Sleep 1
        if ($p.HasExited) { $stopOk = $true; break }
    }
    $o.stopOk = $stopOk
    $o.consoleExit = if ($p.HasExited) { $p.ExitCode } else { "still_running" }
    $o.stopErrSnap = if (Test-Path $err) { (Get-Content $err -Tail 60) -join "`n" } else { "" }
    if (-not $p.HasExited) {
        Stop-Process -Id $p.Id -Force -EA SilentlyContinue
        $o.forceKilledConsole = $true
    } else {
        $o.forceKilledConsole = $false
    }

    # Stop lab broker
    New-Item -ItemType File -Path C:\ProgramData\RamShared\broker-lab.stop -Force | Out-Null
    Start-Sleep 1
    if (-not $bp.HasExited) { Stop-Process -Id $bp.Id -Force -EA SilentlyContinue }

    $o.brokerLog = if (Test-Path C:\ProgramData\RamShared\broker-lab.log) {
        [string](Get-Content C:\ProgramData\RamShared\broker-lab.log -Raw -EA SilentlyContinue)
    } else { "" }
    $o.consoleErrEnd = if (Test-Path $err) { (Get-Content $err -Tail 40) -join "`n" } else { "" }
    $o.nvidiaAfter = [string]((& nvidia-smi --query-gpu=name,memory.used,memory.total --format=csv,noheader 2>$null) -join "; ")
    # Compact JSON only — avoid serializing PSObject graphs.
    $o | ConvertTo-Json -Depth 4 -Compress
} -ArgumentList @($cfgText, [uint64]$SizeBytes, $BrokerPort, $Letter)

$load | Set-Content (Join-Path $art "guest-result.json")
W ("result=" + $load)

# Parse summary
$sum = $null
try { $sum = $load | ConvertFrom-Json } catch {}
$summary = [ordered]@{
    ARTIFACT   = $art
    ONLINE     = [bool]$sum.online
    BINARY_MATCH = [bool]$sum.binaryMatch
    SYS_SHA    = [string]$sum.sysSha
    ROUNDS_PASS = [bool]$sum.roundsPass
    STOP_OK    = [bool]$sum.stopOk
    LETTER     = [string]$sum.letter
    DISK       = [string]$sum.disk
}
$summary | ConvertTo-Json | Set-Content (Join-Path $art "summary.json")
W ("SUMMARY " + ($summary | ConvertTo-Json -Compress))

Stop-VM -Name $VMName -Force -ErrorAction SilentlyContinue
Start-Sleep 2
W ("VM=" + (Get-VM $VMName).State)
$n = Get-PnpDevice -Class Display | Where-Object FriendlyName -Match NVIDIA | Select-Object -First 1
W ("HOST_GPU=" + $n.Status)

if (-not $summary.ONLINE -or -not $summary.ROUNDS_PASS -or -not $summary.STOP_OK) {
    exit 2
}
exit 0
