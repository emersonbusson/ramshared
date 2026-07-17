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
Remove-Item $stop -Force -ErrorAction SilentlyContinue
Get-Process ramshared-winsvc -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep 1

$rs = (sc.exe query ramshared | Out-String)
if ($rs -notmatch "RUNNING") {
    sc.exe start poolstress | Out-Null
    sc.exe start ramshared | Out-Null
    Start-Sleep 2
}
W ("ramshared running=" + ($rs -match "RUNNING"))

$exe = "C:\ramshared\bin\ramshared-winsvc.exe"
$cfg = "C:\ProgramData\RamShared\winsvc-product.toml"
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
    if ($txt -match "product Online") { $online = $true; W "ONLINE seen"; break }
    if ($p.HasExited) { W ("exited early code=" + $p.ExitCode); break }
}
if (-not $online) {
    W "FAIL no Online"
    if (Test-Path $err) { Get-Content $err | Select-Object -Last 40 | ForEach-Object { W $_ } }
    @{ HOST_ONLINE = $false; ARTIFACT = $art } | ConvertTo-Json | Set-Content (Join-Path $art "summary.json")
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
    exit 1
}
W ("disk=N=" + $disk.Number + " Name=" + $disk.FriendlyName + " (RAMSHARE only)")

# Format carefully: only this disk number, prefer letter S, never steal C:
$letter = $null
$part = Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue |
    Where-Object { $_.DriveLetter } | Select-Object -First 1
if ($part) {
    $letter = [string]$part.DriveLetter
    W ("existing letter=" + $letter)
} else {
    try {
        if ($disk.PartitionStyle -eq "Raw") {
            Initialize-Disk -Number $disk.Number -PartitionStyle GPT -Confirm:$false -ErrorAction SilentlyContinue
        }
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
        Format-Volume -DriveLetter $pick -FileSystem NTFS -NewFileSystemLabel "RAMSHARED" -Confirm:$false | Out-Null
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
}
$sum | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $art "summary.json")
W ("SUMMARY " + ($sum | ConvertTo-Json -Compress))
Write-Host ("ARTIFACT=" + $art)
if ($online -and $all -and $stopped -and $exitCode -eq 0) { exit 0 } else { exit 2 }
