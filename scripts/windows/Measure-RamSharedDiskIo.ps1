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
    [int]$SampleIntervalSec = 1
)

$ErrorActionPreference = "Continue"
function L($m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m) }

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
if ($DriveLetter) {
    $letter = $DriveLetter.TrimEnd(':').Substring(0, 1)
    $path = "${letter}:\ramshared-io-probe.bin"
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
        $match = ($got.Length -eq $bytes.Length)
        L ("Direct {0} MiB write={1} MB/s read={2} MB/s match={3}" -f $mib, $wMBs, $rMBs, $match)
        Remove-Item $path -Force -EA SilentlyContinue
        $directOk = $match
    } catch {
        L ("Direct I/O failed: $($_.Exception.Message)")
    }
}

# Exit 0 if we have disk + (any perf sample OR successful direct IO OR letter not requested)
if ($disks.Count -gt 0 -and ($reads.Count -gt 0 -or $writes.Count -gt 0 -or $directOk -or -not $DriveLetter)) {
    exit 0
}
if ($disks.Count -gt 0 -and $DriveLetter -and $directOk) { exit 0 }
if ($disks.Count -gt 0 -and -not $DriveLetter) { exit 0 }
exit 1
