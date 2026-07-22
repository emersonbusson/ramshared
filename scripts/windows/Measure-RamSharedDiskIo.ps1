#Requires -Version 5.1
<#
.SYNOPSIS
  Live read/write metrics for the RamShared virtual LUN (Task Manager alternative).

.DESCRIPTION
  Task Manager often shows 100% active time and 0 KB/s on StorPort virtual
  miniports when the LUN is RAW, when TUR was SRB_STATUS_BUSY (fixed in
  virtdisk.c), or when only polling I/O runs. This script:
    1) Identifies the RamShared disk
    2) Samples Win32_PerfFormattedData_PerfDisk_PhysicalDisk (locale-safe)
    3) Optionally runs a 16 MiB sequential write/read probe on a mounted letter

  Prefer this over Task Manager for lab numbers.

.EXAMPLE
  .\Measure-RamSharedDiskIo.ps1 -Seconds 10
  .\Measure-RamSharedDiskIo.ps1 -Seconds 8 -DriveLetter S
#>
[CmdletBinding()]
param(
    [int]$Seconds = 10,
    [string]$DriveLetter = "",
    [string]$AccessPath = "",
    [int]$SampleIntervalSec = 1,
    # SPEC DT-13 / RF-4: optional exact checksum mode (three rounds)
    [int]$ChecksumRounds = 0,
    [int]$ProbeMiB = 8,
    [string]$ProductPid = "",
    [string]$ProductSha256 = "",
    [string]$ExpectedSerial = "",
    [string]$JsonlOut = ""
)

$ErrorActionPreference = "Continue"
function L($m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m) }

$ioRoot = ""
if ($AccessPath) {
    $ioRoot = $AccessPath.TrimEnd('\')
} elseif ($DriveLetter) {
    $letter = $DriveLetter.TrimEnd(':').Substring(0, 1)
    $ioRoot = "${letter}:"
}

$uncachedSource = @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

public static class RamSharedUncachedIo {
  const uint GENERIC_READ = 0x80000000, GENERIC_WRITE = 0x40000000;
  const uint CREATE_ALWAYS = 2;
  public const uint FILE_FLAG_NO_BUFFERING = 0x20000000;
  public const uint FILE_FLAG_WRITE_THROUGH = 0x80000000;
  const uint MEM_COMMIT = 0x1000, MEM_RESERVE = 0x2000, MEM_RELEASE = 0x8000, PAGE_READWRITE = 4;
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  static extern SafeFileHandle CreateFile(string p, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool WriteFile(SafeFileHandle h, IntPtr b, uint n, out uint done, IntPtr ov);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool ReadFile(SafeFileHandle h, IntPtr b, uint n, out uint done, IntPtr ov);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetFilePointerEx(SafeFileHandle h, long d, out long pos, uint method);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr VirtualAlloc(IntPtr a, UIntPtr n, uint type, uint protect);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool VirtualFree(IntPtr a, UIntPtr n, uint type);

  public static long[] Run(string path, int mib, int seconds) {
    int size = checked(mib * 1024 * 1024);
    byte[] expected = new byte[size], actual = new byte[size];
    new Random(0x5253).NextBytes(expected);
    IntPtr write = VirtualAlloc(IntPtr.Zero, (UIntPtr)size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    IntPtr read = VirtualAlloc(IntPtr.Zero, (UIntPtr)size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (write == IntPtr.Zero || read == IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
    long written = 0, readBytes = 0;
    try {
      Marshal.Copy(expected, 0, write, size);
      using (var h = CreateFile(path, GENERIC_READ | GENERIC_WRITE, 0, IntPtr.Zero, CREATE_ALWAYS,
                                FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH, IntPtr.Zero)) {
        if (h.IsInvalid) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        DateTime deadline = DateTime.UtcNow.AddSeconds(Math.Max(1, seconds));
        do {
          uint done; long pos;
          if (!SetFilePointerEx(h, 0, out pos, 0) || !WriteFile(h, write, (uint)size, out done, IntPtr.Zero) || done != size)
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
          written += done;
          if (!SetFilePointerEx(h, 0, out pos, 0) || !ReadFile(h, read, (uint)size, out done, IntPtr.Zero) || done != size)
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
          readBytes += done;
        } while (DateTime.UtcNow < deadline);
      }
      Marshal.Copy(read, actual, 0, size);
      bool match = expected.Length == actual.Length;
      for (int i = 0; match && i < expected.Length; i++) match = expected[i] == actual[i];
      return new long[] { written, readBytes, match ? 1 : 0 };
    } finally {
      if (write != IntPtr.Zero) VirtualFree(write, UIntPtr.Zero, MEM_RELEASE);
      if (read != IntPtr.Zero) VirtualFree(read, UIntPtr.Zero, MEM_RELEASE);
    }
  }
}
'@

$disks = @(Get-Disk | Where-Object {
        $_.FriendlyName -match 'RAMSHARE|RamShared|VRAMDISK' -or
        ($_.BusType -eq 'Fibre Channel' -and $_.FriendlyName -match 'RAM')
    })
if ($disks.Count -eq 0) {
    Write-Warning "No RamShared disk found. Is WinDriveBackend / CREATE_DISK running?"
    Get-Disk | Format-Table Number, FriendlyName, Size, BusType, PartitionStyle -AutoSize
    exit 2
}

foreach ($d in $disks) {
    L ("DISK N=$($d.Number) Name=$($d.FriendlyName) Size=$($d.Size) Style=$($d.PartitionStyle) Bus=$($d.BusType)")
    $parts = @(Get-Partition -DiskNumber $d.Number -EA SilentlyContinue)
    if ($parts.Count -eq 0) {
        L "  (RAW - no partition. Task Manager Formatado 0 MB is expected. Format with Format-RamSharedLun.ps1)"
    } else {
        $parts | Format-Table PartitionNumber, DriveLetter, Size, Type -AutoSize | Out-String | Write-Host
    }
}

# Locale-safe: WMI/CIM class names stay English on PT-BR Windows.
# Counter paths like \PhysicalDisk(*)\% Disk Time are often translated and fail.
function Get-RamSharedPerfRows {
    param([int[]]$DiskNumbers)
    $rows = @()
    try {
        $all = @(Get-CimInstance -ClassName Win32_PerfFormattedData_PerfDisk_PhysicalDisk -EA Stop)
    } catch {
        try {
            $all = @(Get-WmiObject -Class Win32_PerfFormattedData_PerfDisk_PhysicalDisk -EA Stop)
        } catch {
            return @()
        }
    }
    foreach ($r in $all) {
        if ($r.Name -eq '_Total') { continue }
        $name = [string]$r.Name
        $hit = $false
        if ($name -match 'RAMSHARE|VRAMDISK|RamShared') { $hit = $true }
        foreach ($n in $DiskNumbers) {
            if ($name -match ("^\s*{0}\b" -f $n) -or $name -match ("^{0}\s" -f $n)) {
                $hit = $true
            }
        }
        if ($hit) { $rows += $r }
    }
    return $rows
}

$diskNums = @($disks | ForEach-Object { [int]$_.Number })
$probe = Get-RamSharedPerfRows -DiskNumbers $diskNums
if ($probe.Count -eq 0) {
    L "PerfDisk instances (all non-total):"
    try {
        Get-CimInstance Win32_PerfFormattedData_PerfDisk_PhysicalDisk -EA SilentlyContinue |
            Where-Object { $_.Name -ne '_Total' } |
            ForEach-Object { L ("  Name='{0}'" -f $_.Name) }
    } catch {}
    L "No RamShared PerfDisk row yet; will still try direct I/O if -DriveLetter set."
} else {
    L ("PerfDisk match: " + (($probe | ForEach-Object { $_.Name }) -join ', '))
}

$reads = New-Object System.Collections.Generic.List[double]
$writes = New-Object System.Collections.Generic.List[double]
$busy = New-Object System.Collections.Generic.List[double]
$latR = New-Object System.Collections.Generic.List[double]
$latW = New-Object System.Collections.Generic.List[double]
$qDepth = New-Object System.Collections.Generic.List[double]

$sampleLoadJob = $null
if ($ioRoot) {
    $sampleLoadPath = Join-Path $ioRoot "ramshared-io-sample-load.bin"
    L "Starting direct I/O load during PerfDisk sampling -> $sampleLoadPath"
    $sampleLoadJob = Start-Job -ArgumentList $sampleLoadPath, $ProbeMiB, $Seconds, $uncachedSource -ScriptBlock {
        param($Path, $MiB, $DurationSec, $Source)
        $ErrorActionPreference = "Stop"
        Add-Type -TypeDefinition $Source
        $result = [RamSharedUncachedIo]::Run($Path, $MiB, $DurationSec)
        Remove-Item $Path -Force -EA SilentlyContinue
        [pscustomobject]@{
            probe_during_sampling = $true
            UNCACHED_WRITE_BYTES = [int64]$result[0]
            UNCACHED_READ_BYTES = [int64]$result[1]
            bytes_written = [int64]$result[0]
            bytes_read = [int64]$result[1]
            match = ([int64]$result[2] -eq 1)
        }
    }
}

$samples = [Math]::Max(1, [int]$Seconds)
L "Sampling PerfDisk for ${samples}s (interval ${SampleIntervalSec}s) via CIM (locale-safe)"
for ($i = 0; $i -lt $samples; $i++) {
    $rows = Get-RamSharedPerfRows -DiskNumbers $diskNums
    foreach ($r in $rows) {
        # Properties are bytes/sec and percent already cooked in FormattedData.
        if ($null -ne $r.DiskReadBytesPersec) { $reads.Add([double]$r.DiskReadBytesPersec) }
        if ($null -ne $r.DiskWriteBytesPersec) { $writes.Add([double]$r.DiskWriteBytesPersec) }
        if ($null -ne $r.PercentDiskTime) { $busy.Add([double]$r.PercentDiskTime) }
        if ($null -ne $r.AvgDiskSecPerRead) { $latR.Add([double]$r.AvgDiskSecPerRead * 1000.0) }
        if ($null -ne $r.AvgDiskSecPerWrite) { $latW.Add([double]$r.AvgDiskSecPerWrite * 1000.0) }
        if ($null -ne $r.CurrentDiskQueueLength) { $qDepth.Add([double]$r.CurrentDiskQueueLength) }
    }
    if ($i -lt ($samples - 1)) { Start-Sleep -Seconds $SampleIntervalSec }
}

if ($sampleLoadJob) {
    Wait-Job $sampleLoadJob -Timeout ([Math]::Max(5, $Seconds + 5)) | Out-Null
    if ($sampleLoadJob.State -eq "Running") {
        Stop-Job $sampleLoadJob -Force -EA SilentlyContinue
        L "Direct I/O load during sampling timed out"
    } else {
        $loadResult = Receive-Job $sampleLoadJob -EA SilentlyContinue
        foreach ($lr in @($loadResult)) {
            if ($lr.probe_during_sampling) {
                L ("Direct load during sampling iterations={0} written={1} MiB read={2} MiB match={3}" -f
                    $lr.iterations,
                    [math]::Round(([double]$lr.bytes_written / 1MB), 2),
                    [math]::Round(([double]$lr.bytes_read / 1MB), 2),
                    $lr.match)
            }
        }
    }
    Remove-Job $sampleLoadJob -Force -EA SilentlyContinue
}

function Stat($list) {
    if ($list.Count -eq 0) { return @{ avg = 0; max = 0 } }
    $a = ($list | Measure-Object -Average -Maximum)
    return @{ avg = [math]::Round($a.Average, 2); max = [math]::Round($a.Maximum, 2) }
}

$sr = Stat $reads
$sw = Stat $writes
$sb = Stat $busy
$slr = Stat $latR
$slw = Stat $latW
$sq = Stat $qDepth

L "=== Summary ($Seconds s) ==="
L ("Busy pct DiskTime  avg={0} pct max={1} pct" -f $sb.avg, $sb.max)
L ("Read            avg={0} MB/s max={1} MB/s" -f [math]::Round($sr.avg / 1MB, 2), [math]::Round($sr.max / 1MB, 2))
L ("Write           avg={0} MB/s max={1} MB/s" -f [math]::Round($sw.avg / 1MB, 2), [math]::Round($sw.max / 1MB, 2))
L ("Latency read    avg={0} ms max={1} ms" -f $slr.avg, $slr.max)
L ("Latency write   avg={0} ms max={1} ms" -f $slw.avg, $slw.max)
L ("Queue depth     avg={0} max={1}" -f $sq.avg, $sq.max)
L "Note: Task Manager may still mis-report StorPort virtual disks; trust this sample + direct I/O."

$directOk = $false
if ($ioRoot) {
    $path = Join-Path $ioRoot "ramshared-io-probe.bin"
    L "Optional direct I/O probe -> $path"
    try {
        # 16 MiB may exceed free space on 64 MiB LUN after NTFS overhead; use 8 MiB.
        $sz = 8 * 1MB
        $bytes = New-Object byte[] $sz
        (New-Object Random).NextBytes($bytes)
        $swWrite = [Diagnostics.Stopwatch]::StartNew()
        [IO.File]::WriteAllBytes($path, $bytes)
        $swWrite.Stop()
        $swRead = [Diagnostics.Stopwatch]::StartNew()
        $got = [IO.File]::ReadAllBytes($path)
        $swRead.Stop()
        $mib = $sz / 1MB
        $wMBs = [math]::Round($mib / [Math]::Max(0.001, $swWrite.Elapsed.TotalSeconds), 1)
        $rMBs = [math]::Round($mib / [Math]::Max(0.001, $swRead.Elapsed.TotalSeconds), 1)
        $tmp = Join-Path $env:TEMP "ramshared-io-probe-read.bin"
        [IO.File]::WriteAllBytes($tmp, $got)
        $hashWrite = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash
        $hashRead = (Get-FileHash -Algorithm SHA256 -LiteralPath $tmp).Hash
        Remove-Item $tmp -Force -EA SilentlyContinue
        $match = ($got.Length -eq $bytes.Length -and $hashWrite -eq $hashRead)
        L ("Direct {0} MiB write={1} MB/s read={2} MB/s match={3} sha256={4}" -f $mib, $wMBs, $rMBs, $match, $hashWrite)
        Remove-Item $path -Force -EA SilentlyContinue
        $directOk = $match
    } catch {
        L ("Direct I/O failed: $($_.Exception.Message)")
    }
}

# --- SPEC checksum / percentile mode (optional) ---
if ($ChecksumRounds -gt 0) {
    if (-not $ioRoot) { throw "ChecksumRounds requires -DriveLetter or -AccessPath" }
    $path = Join-Path $ioRoot "ramshared-probe.bin"
    $size = [int64]$ProbeMiB * 1MB
    $seed = [int](Get-Date -UFormat %s) % 251
    $lat = New-Object System.Collections.Generic.List[double]
    $hashes = @()
    for ($r = 1; $r -le $ChecksumRounds; $r++) {
        $buf = New-Object byte[] $size
        for ($i = 0; $i -lt $buf.Length; $i++) { $buf[$i] = [byte](($i + $seed + $r) % 251) }
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        [System.IO.File]::WriteAllBytes($path, $buf)
        $fs = [System.IO.File]::Open($path, 'Open', 'ReadWrite', 'None')
        $fs.Flush($true)
        $fs.Close()
        $sw.Stop()
        $lat.Add($sw.Elapsed.TotalMilliseconds)
        $hWrite = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash
        $sw2 = [System.Diagnostics.Stopwatch]::StartNew()
        $read = [System.IO.File]::ReadAllBytes($path)
        $sw2.Stop()
        $lat.Add($sw2.Elapsed.TotalMilliseconds)
        $tmp = Join-Path $env:TEMP ("rs-read-{0}.bin" -f $r)
        [System.IO.File]::WriteAllBytes($tmp, $read)
        $hRead = (Get-FileHash -Algorithm SHA256 -LiteralPath $tmp).Hash
        Remove-Item $tmp -Force -EA SilentlyContinue
        if ($hWrite -ne $hRead) {
            Write-Host "checksum_mismatch_exits_6 write=$hWrite read=$hRead round=$r"
            exit 6
        }
        $hashes += $hWrite
        L ("ROUND $r SHA256=$hWrite write_ms={0:n1} read_ms={1:n1}" -f $lat[$lat.Count-2], $lat[$lat.Count-1])
    }
    if ($ChecksumRounds -ge 2) {
        $uniq = $hashes | Select-Object -Unique
        if ($uniq.Count -ne 1) {
            Write-Warning "rounds produced different hashes (seeded content differs by design per round)"
        }
    }
    $sorted = $lat | Sort-Object
    function Pct($arr, $p) {
        if ($arr.Count -eq 0) { return 0 }
        $rank = [math]::Ceiling(($p/100.0) * $arr.Count)
        $idx = [math]::Max(0, [math]::Min($arr.Count-1, $rank-1))
        return $arr[$idx]
    }
    $p50 = Pct $sorted 50; $p95 = Pct $sorted 95; $p99 = Pct $sorted 99
    Write-Host ("three_rounds_emit_p50_p95_p99 p50_ms={0:n2} p95_ms={1:n2} p99_ms={2:n2}" -f $p50,$p95,$p99)
    if ($JsonlOut) {
        $row = [ordered]@{
            schema=1; backend="cuda"; pid=$ProductPid; exe_sha256=$ProductSha256
            serial=$ExpectedSerial; p50_ms=$p50; p95_ms=$p95; p99_ms=$p99
            rounds=$ChecksumRounds; last_sha256=$hashes[-1]
        }
        ($row | ConvertTo-Json -Compress) | Add-Content -Path $JsonlOut -Encoding utf8
    }
    Write-Host "matching_checksum_exits_0"
    exit 0
}

# Exit 0 if we have disk + (any perf sample OR successful direct IO OR letter not requested)
if ($disks.Count -gt 0 -and ($reads.Count -gt 0 -or $writes.Count -gt 0 -or $directOk -or -not $DriveLetter)) {
    exit 0
}
if ($disks.Count -gt 0 -and $DriveLetter -and $directOk) { exit 0 }
if ($disks.Count -gt 0 -and -not $DriveLetter) { exit 0 }
exit 1
