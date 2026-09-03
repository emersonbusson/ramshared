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
  .\Measure-RamSharedDiskIo.ps1 -Seconds 8 -DriveLetter S `
    -ExpectedSerial ABCDEF0123456789 -ExpectedSizeBytes 1GB -ChecksumRounds 3
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
    [uint64]$ExpectedSizeBytes = 0,
    [string]$JsonlOut = ""
)

$ErrorActionPreference = "Stop"

function Assert-PerfDiskCountersAvailable {
    try {
        $phys = @(Get-CimInstance -ClassName Win32_PerfFormattedData_PerfDisk_PhysicalDisk -ErrorAction Stop)
        $log = @(Get-CimInstance -ClassName Win32_PerfFormattedData_PerfDisk_LogicalDisk -ErrorAction Stop)
    } catch {
        try {
            $phys = @(Get-WmiObject -Class Win32_PerfFormattedData_PerfDisk_PhysicalDisk -ErrorAction Stop)
            $log = @(Get-WmiObject -Class Win32_PerfFormattedData_PerfDisk_LogicalDisk -ErrorAction Stop)
        } catch {
            Write-Error -ErrorId "MissingDiskCounters" -Message "Failed to query Disk performance counters: $($_.Exception.Message)" -ErrorAction Stop
        }
    }
    if ($phys.Count -eq 0) {
        Write-Error -ErrorId "MissingPhysicalDiskCounters" -Message "PhysicalDisk performance counters are missing or return zero instances." -ErrorAction Stop
    }
    if ($log.Count -eq 0) {
        Write-Error -ErrorId "MissingLogicalDiskCounters" -Message "LogicalDisk performance counters are missing or return zero instances." -ErrorAction Stop
    }
}

Assert-PerfDiskCountersAvailable

function L($m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m) }

function Get-Sha256Hex {
    param([byte[]]$Bytes)
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        return [BitConverter]::ToString($sha256.ComputeHash($Bytes)).Replace("-", "")
    } finally {
        $sha256.Dispose()
    }
}

function Test-RamSharedPayloadMatch {
    param([byte[]]$ExpectedBytes, [byte[]]$ActualBytes)
    if ($null -eq $ExpectedBytes -or $null -eq $ActualBytes -or
        $ExpectedBytes.Length -ne $ActualBytes.Length) {
        return $false
    }
    return (Get-Sha256Hex $ExpectedBytes) -eq (Get-Sha256Hex $ActualBytes)
}

function Resolve-RamSharedDiskBinding {
    param(
        [object[]]$Disks,
        [object[]]$TargetPartitions,
        [string]$Serial,
        [uint64]$SizeBytes,
        [string]$Letter,
        [string]$Path
    )
    $normalizedSerial = $Serial.Trim().ToUpperInvariant()
    if ($normalizedSerial -notmatch '^[0-9A-F]{16}$') {
        throw "ExpectedSerial must be exactly 16 hexadecimal characters"
    }
    if ($SizeBytes -eq 0) {
        throw "ExpectedSizeBytes must be greater than zero"
    }
    $candidates = @($Disks | Where-Object {
            ([string]$_.FriendlyName).Trim() -ieq "RAMSHARE VRAMDISK" -and
            ([string]$_.SerialNumber).Trim().ToUpperInvariant() -eq $normalizedSerial -and
            [uint64]$_.Size -eq $SizeBytes -and
            ([string]$_.BusType).Trim() -ieq "Virtual"
        })
    if ($candidates.Count -ne 1) {
        throw "exact RamShared disk cardinality must be one; observed=$($candidates.Count)"
    }

    if (-not [string]::IsNullOrWhiteSpace($Letter) -or
        -not [string]::IsNullOrWhiteSpace($Path)) {
        if ($TargetPartitions.Count -ne 1) {
            throw "target access path partition cardinality must be one; observed=$($TargetPartitions.Count)"
        }
        $partition = $TargetPartitions[0]
        if ([int]$partition.DiskNumber -ne [int]$candidates[0].Number) {
            throw "target access path maps to foreign disk"
        }
        if (-not [string]::IsNullOrWhiteSpace($Letter)) {
            $expectedLetter = $Letter.TrimEnd(':').Substring(0, 1).ToUpperInvariant()
            if (([string]$partition.DriveLetter).ToUpperInvariant() -ne $expectedLetter) {
                throw "target drive letter mapping changed"
            }
        }
        if (-not [string]::IsNullOrWhiteSpace($Path)) {
            $expectedPath = $Path.TrimEnd('\') + '\'
            $paths = @($partition.AccessPaths | ForEach-Object { ([string]$_).TrimEnd('\') + '\' })
            if ($paths -notcontains $expectedPath) {
                throw "target access path mapping changed"
            }
        }
    }
    return $candidates[0]
}

function Assert-RamSharedUncachedProbe {
    param([object[]]$Results)
    if ($Results.Count -ne 1) {
        throw "uncached probe result cardinality must be one; observed=$($Results.Count)"
    }
    $result = $Results[0]
    if (-not $result.probe_during_sampling -or
        [int64]$result.UNCACHED_WRITE_BYTES -le 0 -or
        [int64]$result.UNCACHED_READ_BYTES -le 0 -or
        -not [bool]$result.match) {
        throw "uncached probe failed integrity or positive-byte contract"
    }
    return $result
}

function Assert-RamSharedCounterActivity {
    param(
        [double]$ReadBytesPerSecMax,
        [double]$WriteBytesPerSecMax,
        [double]$PercentDiskTimeMax,
        [int]$PerfRowSamples
    )
    if ($PerfRowSamples -le 0) {
        throw "no exact PerfDisk row was sampled"
    }
    if ($ReadBytesPerSecMax -le 0 -and $WriteBytesPerSecMax -le 0) {
        throw "PerfDisk reported zero read/write activity"
    }
    if ($PercentDiskTimeMax -lt 0) {
        throw "PerfDisk reported invalid activity percentage"
    }
    return $true
}

function New-RamSharedCounterEvidence {
    param(
        $ReadStats,
        $WriteStats,
        $BusyStats,
        $ReadLatencyStats,
        $WriteLatencyStats,
        $QueueStats,
        [int]$PerfRowSamples,
        [int64]$UncachedWriteBytes,
        [int64]$UncachedReadBytes,
        [string]$ExpectedSerial,
        [uint64]$ExpectedSizeBytes
    )
    return [pscustomobject][ordered]@{
        serial = $ExpectedSerial.Trim().ToUpperInvariant()
        expected_size_bytes = $ExpectedSizeBytes
        perf_row_samples = $PerfRowSamples
        disk_read_bytes_per_sec_avg = $ReadStats.avg
        disk_read_bytes_per_sec_max = $ReadStats.max
        disk_write_bytes_per_sec_avg = $WriteStats.avg
        disk_write_bytes_per_sec_max = $WriteStats.max
        percent_disk_time_avg = $BusyStats.avg
        percent_disk_time_max = $BusyStats.max
        avg_disk_sec_per_read_ms_avg = $ReadLatencyStats.avg
        avg_disk_sec_per_write_ms_avg = $WriteLatencyStats.avg
        current_disk_queue_length_avg = $QueueStats.avg
        uncached_write_bytes = $UncachedWriteBytes
        uncached_read_bytes = $UncachedReadBytes
    }
}

function Write-Utf8JsonLine {
    param([string]$LiteralPath, $Value)
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    $json = $Value | ConvertTo-Json -Compress
    [IO.File]::AppendAllText($LiteralPath, $json + [Environment]::NewLine, $utf8NoBom)
}

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

$allDisks = @(Get-Disk -ErrorAction Stop)
$targetPartitions = @()
if ($ioRoot) {
    if ($DriveLetter) {
        $targetPartitions = @(Get-Partition -DriveLetter $DriveLetter.TrimEnd(':').Substring(0, 1) `
                -ErrorAction Stop)
    } else {
        $targetPartitions = @(Get-Partition -AccessPath ($AccessPath.TrimEnd('\') + '\') `
                -ErrorAction Stop)
    }
}
try {
    $selectedDisk = Resolve-RamSharedDiskBinding $allDisks $targetPartitions `
        $ExpectedSerial $ExpectedSizeBytes $DriveLetter $AccessPath
} catch {
    [Console]::Error.WriteLine("measurement_error_exits_7: " + $_.Exception.Message)
    exit 7
}
$disks = @($selectedDisk)

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
$perfRowSamples = 0
$uncachedWriteBytes = 0
$uncachedReadBytes = 0

$sampleLoadJob = $null
if ($ioRoot) {
    $sampleLoadPath = Join-Path $ioRoot "ramshared-io-sample-load.bin"
    L "Starting direct I/O load during PerfDisk sampling -> $sampleLoadPath"
    $sampleLoadJob = Start-Job -ArgumentList $sampleLoadPath, $ProbeMiB, $Seconds, $uncachedSource -ScriptBlock {
        param($Path, $MiB, $DurationSec, $Source)
        $ErrorActionPreference = "Stop"
        Add-Type -TypeDefinition $Source
        try {
            $result = [RamSharedUncachedIo]::Run($Path, $MiB, $DurationSec)
            [pscustomobject]@{
                probe_during_sampling = $true
                UNCACHED_WRITE_BYTES = [int64]$result[0]
                UNCACHED_READ_BYTES = [int64]$result[1]
                bytes_written = [int64]$result[0]
                bytes_read = [int64]$result[1]
                match = ([int64]$result[2] -eq 1)
            }
        } finally {
            Remove-Item $Path -Force -ErrorAction SilentlyContinue
        }
    }
}

$samples = [Math]::Max(1, [int]$Seconds)
L "Sampling PerfDisk for ${samples}s (interval ${SampleIntervalSec}s) via CIM (locale-safe)"
for ($i = 0; $i -lt $samples; $i++) {
    $rows = Get-RamSharedPerfRows -DiskNumbers $diskNums
    if ($rows.Count -gt 1) {
        throw "PerfDisk exact-row cardinality exceeded one; observed=$($rows.Count)"
    }
    foreach ($r in $rows) {
        $perfRowSamples++
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
    try {
        Wait-Job $sampleLoadJob -Timeout ([Math]::Max(5, $Seconds + 5)) | Out-Null
        if ($sampleLoadJob.State -ne "Completed") {
            # uncached_probe_uses_powershell51_job_cleanup: Stop-Job has no
            # -Force parameter on Windows PowerShell 5.1. The matrix owns the
            # independent process deadline for a job that cannot be stopped.
            Stop-Job $sampleLoadJob -ErrorAction SilentlyContinue
            throw "uncached direct I/O job did not complete; state=$($sampleLoadJob.State)"
        }
        $loadResult = @(Receive-Job $sampleLoadJob -ErrorAction Stop)
        $uncachedResult = Assert-RamSharedUncachedProbe $loadResult
        $uncachedWriteBytes = [int64]$uncachedResult.UNCACHED_WRITE_BYTES
        $uncachedReadBytes = [int64]$uncachedResult.UNCACHED_READ_BYTES
        L ("Direct load during sampling written={0} MiB read={1} MiB match={2}" -f
            [math]::Round(([double]$uncachedWriteBytes / 1MB), 2),
            [math]::Round(([double]$uncachedReadBytes / 1MB), 2),
            $uncachedResult.match)
    } finally {
        Remove-Job $sampleLoadJob -Force -ErrorAction SilentlyContinue
    }
}

function Stat($list) {
    if ($list.Count -eq 0) { return @{ avg = 0; max = 0 } }
    $a = ($list | Measure-Object -Average -Maximum)
    return @{ avg = [math]::Round($a.Average, 2); max = [math]::Round($a.Maximum, 2) }
}

$readStats = Stat $reads
$writeStats = Stat $writes
$busyStats = Stat $busy
$readLatencyStats = Stat $latR
$writeLatencyStats = Stat $latW
$queueStats = Stat $qDepth
if ($ioRoot) {
    Assert-RamSharedCounterActivity `
        $readStats.max $writeStats.max $busyStats.max $perfRowSamples | Out-Null
}

L "=== Summary ($Seconds s) ==="
L ("Busy pct DiskTime  avg={0} pct max={1} pct" -f $busyStats.avg, $busyStats.max)
L ("Read            avg={0} MB/s max={1} MB/s" -f [math]::Round($readStats.avg / 1MB, 2), [math]::Round($readStats.max / 1MB, 2))
L ("Write           avg={0} MB/s max={1} MB/s" -f [math]::Round($writeStats.avg / 1MB, 2), [math]::Round($writeStats.max / 1MB, 2))
L ("Latency read    avg={0} ms max={1} ms" -f $readLatencyStats.avg, $readLatencyStats.max)
L ("Latency write   avg={0} ms max={1} ms" -f $writeLatencyStats.avg, $writeLatencyStats.max)
L ("Queue depth     avg={0} max={1}" -f $queueStats.avg, $queueStats.max)
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
        $hashWrite = Get-Sha256Hex $bytes
        $hashRead = Get-Sha256Hex $got
        $match = Test-RamSharedPayloadMatch $bytes $got
        L ("Direct {0} MiB write={1} MB/s read={2} MB/s match={3} sha256={4}" -f $mib, $wMBs, $rMBs, $match, $hashWrite)
        Remove-Item $path -Force -EA SilentlyContinue
        $directOk = $match
    } catch {
        L ("Direct I/O failed: $($_.Exception.Message)")
    }
}

# --- SPEC checksum / percentile mode (optional) ---
if ($ChecksumRounds -gt 0) {
    $priorErrorAction = $ErrorActionPreference
    $ErrorActionPreference = "Stop"
    try {
        if (-not $ioRoot) { throw "ChecksumRounds requires -DriveLetter or -AccessPath" }
        $path = Join-Path $ioRoot "ramshared-probe.bin"
        $size = [int64]$ProbeMiB * 1MB
        $seed = [int]([DateTime]::UtcNow.Ticks % 251)
        $lat = New-Object System.Collections.Generic.List[double]
        $hashes = @()
        for ($r = 1; $r -le $ChecksumRounds; $r++) {
            $buf = New-Object byte[] $size
            for ($i = 0; $i -lt $buf.Length; $i++) {
                $buf[$i] = [byte](($i + $seed + $r) % 251)
            }
            $hWrite = Get-Sha256Hex $buf
            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            [System.IO.File]::WriteAllBytes($path, $buf)
            $fs = [System.IO.File]::Open($path, 'Open', 'ReadWrite', 'None')
            try {
                $fs.Flush($true)
            }
            finally {
                $fs.Dispose()
            }
            $sw.Stop()
            $lat.Add($sw.Elapsed.TotalMilliseconds)
            $sw2 = [System.Diagnostics.Stopwatch]::StartNew()
            $read = [System.IO.File]::ReadAllBytes($path)
            $sw2.Stop()
            $lat.Add($sw2.Elapsed.TotalMilliseconds)
            $hRead = Get-Sha256Hex $read
            if (-not (Test-RamSharedPayloadMatch $buf $read)) {
                Write-Host "checksum_mismatch_exits_6 write=$hWrite read=$hRead round=$r"
                exit 6
            }
            $hashes += $hWrite
            L ("ROUND $r SHA256=$hWrite write_ms={0:n1} read_ms={1:n1}" -f
                $lat[$lat.Count-2], $lat[$lat.Count-1])
        }
        if ($ChecksumRounds -ge 2) {
            $uniq = $hashes | Select-Object -Unique
            if ($uniq.Count -ne 1) {
                L "Rounds use distinct deterministic content as designed"
            }
        }
        $sorted = $lat | Sort-Object
        function Pct($arr, $p) {
            if ($arr.Count -eq 0) { return 0 }
            $rank = [math]::Ceiling(($p/100.0) * $arr.Count)
            $idx = [math]::Max(0, [math]::Min($arr.Count-1, $rank-1))
            return $arr[$idx]
        }
        $p50 = Pct $sorted 50
        $p95 = Pct $sorted 95
        $p99 = Pct $sorted 99
        Write-Host ("three_rounds_emit_p50_p95_p99 p50_ms={0:n2} p95_ms={1:n2} p99_ms={2:n2}" -f
            $p50, $p95, $p99)
        if ($JsonlOut) {
            $counterEvidence = New-RamSharedCounterEvidence `
                $readStats $writeStats $busyStats $readLatencyStats `
                $writeLatencyStats $queueStats $perfRowSamples `
                $uncachedWriteBytes $uncachedReadBytes `
                $ExpectedSerial $ExpectedSizeBytes
            $row = [ordered]@{
                schema=1; backend="cuda"; pid=$ProductPid; exe_sha256=$ProductSha256
                p50_ms=$p50; p95_ms=$p95; p99_ms=$p99
                rounds=$ChecksumRounds; last_sha256=$hashes[-1]
            }
            foreach ($property in $counterEvidence.PSObject.Properties) {
                $row[$property.Name] = $property.Value
            }
            Write-Utf8JsonLine $JsonlOut $row
        }
        Write-Host "matching_checksum_exits_0"
        exit 0
    }
    catch {
        Write-Error ("measurement_error_exits_7: " + $_.Exception.Message)
        exit 7
    }
    finally {
        if ($path) {
            Remove-Item $path -Force -ErrorAction SilentlyContinue
        }
        $ErrorActionPreference = $priorErrorAction
    }
}

# Exit 0 if we have disk + (any perf sample OR successful direct IO OR letter not requested)
if ($disks.Count -gt 0 -and ($reads.Count -gt 0 -or $writes.Count -gt 0 -or $directOk -or -not $DriveLetter)) {
    exit 0
}
if ($disks.Count -gt 0 -and $DriveLetter -and $directOk) { exit 0 }
if ($disks.Count -gt 0 -and -not $DriveLetter) { exit 0 }
exit 1
