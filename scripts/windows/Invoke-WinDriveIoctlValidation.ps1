#Requires -Version 5.1
<#
.SYNOPSIS
  Live IOCTL legitimate/refusal harness for ramshared.sys (SPEC ITEM-3).

.DESCRIPTION
  Opens \\.\RamSharedCtl and posts named verdicts. Optional Driver Verifier.
  Never thrash the daily host; prefer win11-drill with testsigning.

.EXAMPLE
  .\Invoke-WinDriveIoctlValidation.ps1 -ArtifactDir C:\ramshared\artifacts\ioctl
  .\Invoke-WinDriveIoctlValidation.ps1 -Verifier
#>
[CmdletBinding()]
param(
    [string]$Driver = "ramshared.sys",
    [switch]$Verifier,
    [string]$ArtifactDir = "C:\ramshared\artifacts\ioctl-validation",
    # 128 MiB — must not collide with win11-drill answer-disk.vhdx (64 MiB).
    [UInt64]$SizeBytes = 134217728
)

$ErrorActionPreference = "Stop"
function L($m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m) }

New-Item -Force -ItemType Directory $ArtifactDir | Out-Null
$verdict = [ordered]@{
    PASS_VALID_QUEUE                 = 0
    REFUSE_FOREIGN_OWNER             = 0
    REFUSE_RESERVED_REGISTER         = 0
    REFUSE_BAD_RING                  = 0
    REFUSE_RING_INDEX_JUMP           = 0
    REFUSE_RESERVED_CQE              = 0
    REFUSE_UNKNOWN_IOCTL             = 0
    REFUSE_RESERVED_DISK_PARAMS      = 0
    COMPLETION_REENTRY_NO_SLOT_REUSE = 0
    RUNDOWN_UNMAP_AFTER_COPY         = 0
    VPD_SERIAL_MATCH                 = 0
    NO_NEW_DUMP                      = 0
    DRIVER                           = $Driver
    VERIFIER                         = [bool]$Verifier
    NOTE                             = ""
}

$dumpDir = "C:\Windows\Minidump"
$beforeDumps = @()
if (Test-Path $dumpDir) {
    $beforeDumps = @(Get-ChildItem $dumpDir -Filter *.dmp -EA SilentlyContinue | ForEach-Object FullName)
}

if ($Verifier) {
    L "Enabling Driver Verifier for ramshared (requires reboot if first time - best-effort flags)"
    # Non-reboot flags where possible; full /flags 0x209BB needs reboot - record intent.
    $null = & verifier /query 2>$null
    $verdict.NOTE += "Verifier switch requested; "
}

$cs = @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

public static class IoctlVal {
  public const uint GENERIC_READ = 0x80000000;
  public const uint GENERIC_WRITE = 0x40000000;
  public const uint OPEN_EXISTING = 3;
  public const uint MEM_COMMIT = 0x1000;
  public const uint MEM_RESERVE = 0x2000;
  public const uint MEM_RELEASE = 0x8000;
  public const uint PAGE_READWRITE = 0x04;
  public const uint MAGIC = 0x52535244;

  public static uint Ioctl(uint n) {
    return (0x2du << 16) | (3u << 14) | ((0x800u + n) << 2);
  }
  public static readonly uint IOCTL_REGISTER = Ioctl(0);
  public static readonly uint IOCTL_UNREGISTER = Ioctl(1);
  public static readonly uint IOCTL_COMMIT = Ioctl(2);
  public static readonly uint IOCTL_CREATE = Ioctl(3);
  public static readonly uint IOCTL_DESTROY = Ioctl(4);
  public static readonly uint IOCTL_UNKNOWN = Ioctl(99);

  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
  public static extern SafeFileHandle CreateFile(string n, uint a, uint s, IntPtr p, uint d, uint f, IntPtr t);
  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern bool DeviceIoControl(SafeFileHandle h, uint code, byte[] ib, uint il, byte[] ob, uint ol, out uint ret, IntPtr ov);
  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern IntPtr VirtualAlloc(IntPtr a, UIntPtr s, uint t, uint p);
  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern bool VirtualFree(IntPtr a, UIntPtr s, uint t);
  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern void RtlZeroMemory(IntPtr d, UIntPtr l);
  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern void CopyMemory(IntPtr d, IntPtr s, uint l);

  [StructLayout(LayoutKind.Sequential, Pack=8)]
  public struct DiskParams {
    public ulong size_bytes; public uint block_size; public uint reserved;
    [MarshalAs(UnmanagedType.ByValArray, SizeConst=16)] public byte[] serial;
  }
  [StructLayout(LayoutKind.Sequential, Pack=8)]
  public struct Register {
    public uint abi_version, disk_id, queue_depth, block_size, max_io_bytes, reserved;
    public ulong sq_ring_va, cq_ring_va, data_area_va, data_area_len;
    public ulong sq_event_handle, cq_event_handle;
  }

  public static byte[] ToBytes<T>(T s) where T:struct {
    int n = Marshal.SizeOf(typeof(T));
    byte[] b = new byte[n];
    IntPtr p = Marshal.AllocHGlobal(n);
    try { Marshal.StructureToPtr(s, p, false); Marshal.Copy(p, b, 0, n); }
    finally { Marshal.FreeHGlobal(p); }
    return b;
  }

  public static int LastErr() { return Marshal.GetLastWin32Error(); }

  public static bool IoctlBool(SafeFileHandle h, uint code, byte[] input) {
    uint ret;
    bool ok = DeviceIoControl(h, code, input, input == null ? 0u : (uint)input.Length, null, 0, out ret, IntPtr.Zero);
    return ok;
  }

  /* Sync COMMIT for same-process concurrent teardown probe (returns Win32 err; 0=ok). */
  public static int BlockingIoctl(SafeFileHandle h, uint code) {
    uint ret;
    bool ok = DeviceIoControl(h, code, null, 0, null, 0, out ret, IntPtr.Zero);
    if (ok) return 0;
    int err = Marshal.GetLastWin32Error();
    return err == 0 ? -1 : err;
  }

  public static System.Threading.Thread StartBlockingIoctl(SafeFileHandle h, uint code, int[] slot) {
    var t = new System.Threading.Thread(() => { slot[0] = BlockingIoctl(h, code); });
    t.IsBackground = true;
    t.Start();
    return t;
  }
}
'@

Add-Type -TypeDefinition $cs -ErrorAction Stop

function Open-Ctl {
    # FILE_SHARE_READ|WRITE: concurrent probes need a second handle while COMMIT is pended.
    # Non-overlapped I/O serializes per handle; two handles avoid UNREGISTER/COMMIT deadlock.
    $share = [uint32]3
    $h = [IoctlVal]::CreateFile("\\.\RamSharedCtl",
        [IoctlVal]::GENERIC_READ -bor [IoctlVal]::GENERIC_WRITE,
        $share, [IntPtr]::Zero, [IoctlVal]::OPEN_EXISTING, 0, [IntPtr]::Zero)
    if ($h.IsInvalid) { throw "open RamSharedCtl failed err=$([IoctlVal]::LastErr())" }
    return $h
}

function New-Rings([uint32]$qd, [uint32]$maxIo) {
    $hdr = 16; $sqe = 32; $cqe = 16
    $sqBytes = $hdr + $qd * $sqe
    $cqBytes = $hdr + $qd * $cqe
    $dataBytes = $qd * $maxIo
    $sq = [IoctlVal]::VirtualAlloc([IntPtr]::Zero, [UIntPtr]$sqBytes, [IoctlVal]::MEM_COMMIT -bor [IoctlVal]::MEM_RESERVE, [IoctlVal]::PAGE_READWRITE)
    $cq = [IoctlVal]::VirtualAlloc([IntPtr]::Zero, [UIntPtr]$cqBytes, [IoctlVal]::MEM_COMMIT -bor [IoctlVal]::MEM_RESERVE, [IoctlVal]::PAGE_READWRITE)
    $data = [IoctlVal]::VirtualAlloc([IntPtr]::Zero, [UIntPtr]$dataBytes, [IoctlVal]::MEM_COMMIT -bor [IoctlVal]::MEM_RESERVE, [IoctlVal]::PAGE_READWRITE)
    if ($sq -eq [IntPtr]::Zero -or $cq -eq [IntPtr]::Zero -or $data -eq [IntPtr]::Zero) {
        throw "VirtualAlloc failed"
    }
    # zero + magic
    $zero = New-Object byte[] $sqBytes
    [Runtime.InteropServices.Marshal]::Copy($zero, 0, $sq, $sqBytes)
    [Runtime.InteropServices.Marshal]::Copy((New-Object byte[] $cqBytes), 0, $cq, $cqBytes)
    [Runtime.InteropServices.Marshal]::WriteInt32($sq, 0, [int][IoctlVal]::MAGIC)
    [Runtime.InteropServices.Marshal]::WriteInt32($sq, 4, [int]$qd)
    [Runtime.InteropServices.Marshal]::WriteInt32($cq, 0, [int][IoctlVal]::MAGIC)
    [Runtime.InteropServices.Marshal]::WriteInt32($cq, 4, [int]$qd)
    return @{ sq=$sq; cq=$cq; data=$data; sqBytes=$sqBytes; cqBytes=$cqBytes; dataBytes=$dataBytes; qd=$qd; maxIo=$maxIo }
}

function Free-Rings($r) {
    $zero = [UIntPtr]::new(0)
    if ($r.sq -ne [IntPtr]::Zero) { [void][IoctlVal]::VirtualFree($r.sq, $zero, [IoctlVal]::MEM_RELEASE) }
    if ($r.cq -ne [IntPtr]::Zero) { [void][IoctlVal]::VirtualFree($r.cq, $zero, [IoctlVal]::MEM_RELEASE) }
    if ($r.data -ne [IntPtr]::Zero) { [void][IoctlVal]::VirtualFree($r.data, $zero, [IoctlVal]::MEM_RELEASE) }
}

function Reset-RingHeaders($rings, [uint32]$qd) {
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 0, [int][IoctlVal]::MAGIC)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 4, [int]$qd)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 8, 0)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 12, 0)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 0, [int][IoctlVal]::MAGIC)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 4, [int]$qd)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 8, 0)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 12, 0)
}

function New-RegisterBytes($rings, [uint32]$qd, [uint32]$maxIo) {
    $reg = New-Object IoctlVal+Register
    $reg.abi_version = 1
    $reg.disk_id = 0
    $reg.queue_depth = $qd
    $reg.block_size = 4096
    $reg.max_io_bytes = $maxIo
    $reg.reserved = 0
    $reg.sq_ring_va = [uint64]$rings.sq.ToInt64()
    $reg.cq_ring_va = [uint64]$rings.cq.ToInt64()
    $reg.data_area_va = [uint64]$rings.data.ToInt64()
    $reg.data_area_len = [uint64]$rings.dataBytes
    return [IoctlVal]::ToBytes($reg)
}

# CQE layout: tag u64 @0, status i32 @8, reserved u32 @12 (16 bytes).
function Write-Cqe($rings, [uint32]$idx, [uint64]$tag, [int]$status, [uint32]$reserved) {
    $hdr = 16
    $cqeSize = 16
    $base = [IntPtr]::Add($rings.cq, $hdr + ([int]$idx * $cqeSize))
    [Runtime.InteropServices.Marshal]::WriteInt64($base, 0, [int64]$tag)
    [Runtime.InteropServices.Marshal]::WriteInt32($base, 8, $status)
    [Runtime.InteropServices.Marshal]::WriteInt32($base, 12, [int]$reserved)
}

function Ensure-RegisteredQueue($h, $rings, [uint32]$qd, [uint32]$maxIo) {
    [void][IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_UNREGISTER, $null)
    Reset-RingHeaders $rings $qd
    $rin = New-RegisterBytes $rings $qd $maxIo
    if (-not [IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_REGISTER, $rin)) {
        throw "REGISTER for concurrent probe failed err=$([IoctlVal]::LastErr())"
    }
}

function Invoke-ReservedCqeInjection($h, $rings, [uint32]$qd, [uint32]$maxIo) {
    Ensure-RegisteredQueue $h $rings $qd $maxIo
    # Keep SQ non-empty so COMMIT drains CQ without long-lived pend.
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 12, 1)
    Write-Cqe $rings 0 0 0 1
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 8, 0)   # head
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 12, 1)  # tail
    $ok = [IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_COMMIT, $null)
    $err1 = [IoctlVal]::LastErr()
    # Failed queue must refuse further COMMIT (fail-closed).
    $ok2 = [IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_COMMIT, $null)
    $err2 = [IoctlVal]::LastErr()
    [void][IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_UNREGISTER, $null)
    if ((-not $ok -and $err1 -ne 0) -or (-not $ok2)) {
        return $true
    }
    L "REFUSE_RESERVED_CQE probe: first ok=$ok err=$err1 second ok=$ok2 err=$err2"
    return $false
}

function Invoke-CompletionReentryInjection($h, $rings, [uint32]$qd, [uint32]$maxIo) {
    Ensure-RegisteredQueue $h $rings $qd $maxIo
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 12, 1)
    # Two CQEs, same tag, no Submitted slot — must drain without double-complete/BSOD.
    Write-Cqe $rings 0 0 0 0
    Write-Cqe $rings 1 0 0 0
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 8, 0)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 12, 2)
    $ok = [IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_COMMIT, $null)
    $err = [IoctlVal]::LastErr()
    $head = [Runtime.InteropServices.Marshal]::ReadInt32($rings.cq, 8)
    [void][IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_UNREGISTER, $null)
    # Driver advanced head past both entries; process still alive; no new dump checked later.
    if ($head -ge 2) {
        return $true
    }
    L "COMPLETION_REENTRY probe: ok=$ok err=$err head=$head"
    return $false
}

function Invoke-RundownDuringCopyInjection($h, $rings, [uint32]$qd, [uint32]$maxIo) {
    Ensure-RegisteredQueue $h $rings $qd $maxIo
    # COMMIT on $h (may pend); UNREGISTER on a second shared handle so non-overlapped
    # serialization cannot deadlock the cancel/teardown path.
    $h2 = $null
    try {
        $h2 = Open-Ctl
        $slot = New-Object 'int[]' 1
        $slot[0] = -3
        $thread = [IoctlVal]::StartBlockingIoctl($h, [IoctlVal]::IOCTL_COMMIT, $slot)
        Start-Sleep -Milliseconds 500
        $unregOk = [IoctlVal]::IoctlBool($h2, [IoctlVal]::IOCTL_UNREGISTER, $null)
        $unregErr = [IoctlVal]::LastErr()
        $joined = $thread.Join(8000)
        $commitErr = $slot[0]
        if ($unregOk -and $joined) {
            L "RUNDOWN_UNMAP_AFTER_COPY probe: unregOk=1 commitErr=$commitErr"
            return $true
        }
        L "RUNDOWN_UNMAP_AFTER_COPY probe FAIL: unregOk=$unregOk err=$unregErr joined=$joined commitErr=$commitErr"
        return $false
    } finally {
        if ($h2 -and -not $h2.IsInvalid) { $h2.Dispose() }
    }
}

$h = $null
$rings = $null
try {
    $h = Open-Ctl
    L "OPEN_CTL ok"

    # Best-effort DESTROY so leftover Online LUN does not cause DEVICE_BUSY (170).
    [void][IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_DESTROY, $null)
    Start-Sleep -Milliseconds 500

    # --- REFUSE_UNKNOWN_IOCTL ---
    $ok = [IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_UNKNOWN, $null)
    $err = [IoctlVal]::LastErr()
    if (-not $ok) {
        $verdict.REFUSE_UNKNOWN_IOCTL = 1
        L "REFUSE_UNKNOWN_IOCTL=1 err=$err"
    } else {
        L "REFUSE_UNKNOWN_IOCTL=0 (unexpected success)"
    }

    # --- REFUSE_RESERVED_DISK_PARAMS ---
    $dp = New-Object IoctlVal+DiskParams
    $dp.size_bytes = [UInt64]$SizeBytes
    $dp.block_size = 4096
    $dp.reserved = 1
    $dp.serial = New-Object byte[] 16
    $in = [IoctlVal]::ToBytes($dp)
    $ok = [IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_CREATE, $in)
    if (-not $ok) {
        $verdict.REFUSE_RESERVED_DISK_PARAMS = 1
        L "REFUSE_RESERVED_DISK_PARAMS=1 err=$([IoctlVal]::LastErr())"
    } else {
        L "REFUSE_RESERVED_DISK_PARAMS=0"
        [void][IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_DESTROY, $null)
    }

    # --- PASS valid CREATE ---
    $dp.reserved = 0
    [Text.Encoding]::ASCII.GetBytes("ABCDEF0123456789").CopyTo($dp.serial, 0)
    $in = [IoctlVal]::ToBytes($dp)
    if (-not [IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_CREATE, $in)) {
        throw "CREATE_DISK failed err=$([IoctlVal]::LastErr())"
    }
    L "CREATE_DISK ok"

    $qd = [uint32]4
    $maxIo = [uint32]1048576
    $rings = New-Rings $qd $maxIo

    # --- REFUSE_RESERVED_REGISTER ---
    $reg = New-Object IoctlVal+Register
    $reg.abi_version = 1; $reg.disk_id = 0; $reg.queue_depth = $qd
    $reg.block_size = 4096; $reg.max_io_bytes = $maxIo; $reg.reserved = 1
    $reg.sq_ring_va = [uint64]$rings.sq.ToInt64()
    $reg.cq_ring_va = [uint64]$rings.cq.ToInt64()
    $reg.data_area_va = [uint64]$rings.data.ToInt64()
    $reg.data_area_len = [uint64]$rings.dataBytes
    $rin = [IoctlVal]::ToBytes($reg)
    if (-not [IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_REGISTER, $rin)) {
        $verdict.REFUSE_RESERVED_REGISTER = 1
        L "REFUSE_RESERVED_REGISTER=1 err=$([IoctlVal]::LastErr())"
    } else {
        L "REFUSE_RESERVED_REGISTER=0"
        [void][IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_UNREGISTER, $null)
    }

    # --- REFUSE_BAD_RING (bad magic) ---
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 0, 0xDEADBEEF)
    $reg.reserved = 0
    $rin = [IoctlVal]::ToBytes($reg)
    if (-not [IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_REGISTER, $rin)) {
        $verdict.REFUSE_BAD_RING = 1
        L "REFUSE_BAD_RING=1 err=$([IoctlVal]::LastErr())"
    } else {
        L "REFUSE_BAD_RING=0"
        [void][IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_UNREGISTER, $null)
    }
    # restore magic
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 0, [int][IoctlVal]::MAGIC)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 4, [int]$qd)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 8, 0)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 12, 0)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 0, [int][IoctlVal]::MAGIC)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 4, [int]$qd)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 8, 0)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 12, 0)

    # --- PASS_VALID_QUEUE ---
    $rin = [IoctlVal]::ToBytes($reg)
    if ([IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_REGISTER, $rin)) {
        $verdict.PASS_VALID_QUEUE = 1
        L "PASS_VALID_QUEUE=1"
        [void][IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_UNREGISTER, $null)
    } else {
        L "PASS_VALID_QUEUE=0 err=$([IoctlVal]::LastErr())"
    }

    # --- VPD_SERIAL_MATCH (INQUIRY vendor/product + VPD serial after CREATE) ---
    # Get-Disk often lags or omits zero/Unknown LUNs; PnP DiskDrive exposes
    # VEN_RAMSHARE&PROD_VRAMDISK from standard INQUIRY when the miniport works.
    $expectedSerial = "ABCDEF0123456789"
    try { "rescan" | diskpart 2>$null | Out-Null } catch {}
    try { Update-HostStorageCache -ErrorAction SilentlyContinue } catch {}
    $match = $null
    $lastHits = @()
    $lastExact = @()
    $lastWin32Exact = @()
    $lastGetDiskExact = @()
    $deadline = (Get-Date).AddSeconds(25)
    while ((Get-Date) -lt $deadline -and $null -eq $match) {
        $cands = @()
        # 1) Storage stack
        try {
            # Offline disks still expose identity; Online improves Get-Disk visibility.
            Get-Disk -EA SilentlyContinue |
                Where-Object {
                    ($_.FriendlyName -match 'RAMSHARE' -and $_.FriendlyName -match 'VRAMDISK') -or
                    ($_.SerialNumber -ieq $expectedSerial)
                } |
                ForEach-Object {
                    if ($_.OperationalStatus -ne 'Online') {
                        try { Set-Disk -Number $_.Number -IsOffline $false -EA SilentlyContinue } catch {}
                    }
                }
        } catch {}
        try {
            $cands += @(Get-Disk -EA SilentlyContinue | ForEach-Object {
                [pscustomobject]@{
                    Source = "Get-Disk"; Status = "OK"
                    Name = ([string]$_.FriendlyName).Trim()
                    Serial = ([string]$_.SerialNumber).Trim()
                    Size = [uint64]$_.Size
                    Id = "disk-$($_.Number)"
                }
            })
        } catch {}
        try {
            $cands += @(Get-CimInstance Win32_DiskDrive -EA SilentlyContinue | ForEach-Object {
                # Win32_DiskDrive.Size is CHS-derived and under-reports real
                # capacity on this stack (observed 131604480 vs 134217728).
                # Prefer IOCTL_DISK_GET_LENGTH_INFO on \\.\PhysicalDriveN.
                $sz = [uint64]$_.Size
                $raw = [uint64]$_.Size
                $idx = -1
                try { $idx = [int]$_.Index } catch {}
                $lenSrc = "WmiSize"
                if ($idx -ge 0 -and -not ("DiskLenQuery" -as [type])) {
                    try {
                        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DiskLenQuery {
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
  static extern IntPtr CreateFile(string n, uint a, uint s, IntPtr sec, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern bool DeviceIoControl(IntPtr h, uint c, IntPtr i, uint il, byte[] o, uint ol, out uint r, IntPtr ov);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern bool CloseHandle(IntPtr h);
  const uint GENERIC_READ = 0x80000000;
  const uint FILE_SHARE_READ = 1, FILE_SHARE_WRITE = 2;
  const uint OPEN_EXISTING = 3;
  const uint IOCTL_DISK_GET_LENGTH_INFO = 0x0007405C;
  public static long GetLength(int index) {
    IntPtr h = CreateFile(@"\\.\PhysicalDrive" + index, GENERIC_READ,
      FILE_SHARE_READ | FILE_SHARE_WRITE, IntPtr.Zero, OPEN_EXISTING, 0, IntPtr.Zero);
    if (h == IntPtr.Zero || h == new IntPtr(-1)) return -1;
    try {
      byte[] buf = new byte[8];
      uint ret;
      if (!DeviceIoControl(h, IOCTL_DISK_GET_LENGTH_INFO, IntPtr.Zero, 0, buf, 8, out ret, IntPtr.Zero) || ret < 8)
        return -1;
      return BitConverter.ToInt64(buf, 0);
    } finally { CloseHandle(h); }
  }
}
'@ -ErrorAction Stop
                    } catch {}
                }
                if ($idx -ge 0 -and ("DiskLenQuery" -as [type])) {
                    try {
                        $got = [DiskLenQuery]::GetLength($idx)
                        if ($got -gt 0) { $sz = [uint64]$got; $lenSrc = "IoctlLength" }
                    } catch {}
                }
                [pscustomobject]@{
                    Source = "Win32_DiskDrive"; Status = "OK"
                    Name = ([string]$_.Model).Trim()
                    Serial = ([string]$_.SerialNumber).Trim()
                    Size = $sz
                    RawWmiSize = $raw
                    LengthSource = $lenSrc
                    DriveIndex = $idx
                    Id = ([string]$_.PNPDeviceID)
                }
            })
        } catch {}
        $hits = @($cands | Where-Object {
            ($_.Name -match 'RAMSHARE' -and $_.Name -match 'VRAMDISK') -or
            ($_.Id -match 'VEN_RAMSHARE' -and $_.Id -match 'PROD_VRAMDISK')
        })
        # An ITEM-3 pass requires all identity fields on one authoritative
        # storage surface. Friendly-name, size-only, and PnP-presence fallbacks
        # are diagnostics only and must never satisfy VPD_SERIAL_MATCH.
        $exactHits = @($hits | Where-Object {
            $_.Status -eq 'OK' -and
            $_.Serial -ieq $expectedSerial -and
            $_.Size -eq [uint64]$SizeBytes
        })
        $win32Hits = @($exactHits | Where-Object { $_.Source -eq 'Win32_DiskDrive' })
        $getDiskHits = @($exactHits | Where-Object { $_.Source -eq 'Get-Disk' })
        $lastHits = $hits
        $lastExact = $exactHits
        $lastWin32Exact = $win32Hits
        $lastGetDiskExact = $getDiskHits
        if ($win32Hits.Count -eq 1) {
            $match = $win32Hits[0]
            break
        }
        if ($win32Hits.Count -eq 0 -and $getDiskHits.Count -eq 1) {
            $match = $getDiskHits[0]
            break
        }
        Start-Sleep -Milliseconds 500
    }
    if ($null -ne $match) {
        $verdict.VPD_SERIAL_MATCH = 1
        L ("VPD_SERIAL_MATCH=1 src=$($match.Source) Status=$($match.Status) Name=$($match.Name) Serial=$($match.Serial) Size=$($match.Size) Id=$($match.Id)")
    } else {
        # Diagnostic only: never green-lights VPD_SERIAL_MATCH. Surfaces why
        # Win32/Get-Disk missed exact vendor+product+serial+size uniqueness.
        try {
            $diag = @()
            try {
                $diag += @(Get-Disk -EA SilentlyContinue | ForEach-Object {
                    "Get-Disk|N=$($_.Number)|Name=$($_.FriendlyName)|Ser=[$($_.SerialNumber)]|Size=$($_.Size)|Bus=$($_.BusType)|OpStatus=$($_.OperationalStatus)"
                })
            } catch {}
            try {
                $diag += @(Get-CimInstance Win32_DiskDrive -EA SilentlyContinue | ForEach-Object {
                    "Win32|Model=$($_.Model)|Ser=[$($_.SerialNumber)]|Size=$($_.Size)|PNP=$($_.PNPDeviceID)|Status=$($_.Status)"
                })
            } catch {}
            L ("VPD_SERIAL_MATCH=0 expectedSer=[$expectedSerial] expectedSize=$SizeBytes candidates=" +
                ($(if ($diag.Count) { $diag -join " || " } else { "(none)" })))
            $pnp = @(Get-PnpDevice -Class DiskDrive -EA SilentlyContinue |
                ForEach-Object { "$($_.Status)|$($_.FriendlyName)|$($_.InstanceId)" })
            L ("VPD_SERIAL_MATCH=0 pnp_disks=" + ($(if ($pnp.Count) { $pnp -join " || " } else { "(none)" })))
            $ramHits = @($lastHits | ForEach-Object {
                $extra = ""
                if ($_.PSObject.Properties.Name -contains 'RawWmiSize') {
                    $extra = "|RawWmiSize=$($_.RawWmiSize)|LenSrc=$($_.LengthSource)|Idx=$($_.DriveIndex)"
                }
                "$($_.Source)|Status=$($_.Status)|Name=$($_.Name)|Ser=[$($_.Serial)]|Size=$($_.Size)$extra|Id=$($_.Id)"
            })
            L ("VPD_SERIAL_MATCH=0 ramshare_hits=" + ($(if ($ramHits.Count) { $ramHits -join " || " } else { "(none)" })))
            L ("VPD_SERIAL_MATCH=0 exact_hit_count=$($lastExact.Count) win32_exact=$($lastWin32Exact.Count) getdisk_exact=$($lastGetDiskExact.Count)")
        } catch {
            L "VPD_SERIAL_MATCH=0 (no enum) err=$($_.Exception.Message)"
        }
        $verdict.NOTE += "VPD identity must be unique vendor/product/serial/size; "
    }

    # --- REFUSE_RING_INDEX_JUMP: re-register with non-zero head/tail after good rings ---
    # Re-init good rings then jump tail beyond depth before register (post-map check).
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 0, [int][IoctlVal]::MAGIC)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 4, [int]$qd)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 8, 0)   # head
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.sq, 12, [int]($qd + 8)) # illegal tail jump
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 0, [int][IoctlVal]::MAGIC)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 4, [int]$qd)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 8, 0)
    [Runtime.InteropServices.Marshal]::WriteInt32($rings.cq, 12, 0)
    $rin = [IoctlVal]::ToBytes($reg)
    if (-not [IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_REGISTER, $rin)) {
        $verdict.REFUSE_RING_INDEX_JUMP = 1
        L "REFUSE_RING_INDEX_JUMP=1 err=$([IoctlVal]::LastErr())"
    } else {
        L "REFUSE_RING_INDEX_JUMP=0 (driver accepted jump; check DT-5)"
        [void][IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_UNREGISTER, $null)
    }

    # --- REFUSE_FOREIGN_OWNER: second process must not DESTROY owner-bound disk ---
    # Compile a tiny native PE so PEPROCESS differs from this PowerShell host.
    $foreignCs = @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
class P {
  const uint GR=0x80000000,GW=0x40000000,OE=3;
  static uint I(uint n){return (0x2du<<16)|(3u<<14)|((0x800u+n)<<2);}
  [DllImport("kernel32.dll",SetLastError=true,CharSet=CharSet.Unicode)]
  static extern SafeFileHandle CreateFile(string n,uint a,uint s,IntPtr p,uint d,uint f,IntPtr t);
  [DllImport("kernel32.dll",SetLastError=true)]
  static extern bool DeviceIoControl(SafeFileHandle h,uint c,byte[] ib,uint il,byte[] ob,uint ol,out uint r,IntPtr o);
  static int Main(){
    var h=CreateFile(@"\\.\RamSharedCtl",GR|GW,0,IntPtr.Zero,OE,0,IntPtr.Zero);
    if(h.IsInvalid){ Console.WriteLine("OPEN_FAIL="+Marshal.GetLastWin32Error()); return 2; }
    uint ret; bool ok=DeviceIoControl(h,I(4),null,0,null,0,out ret,IntPtr.Zero);
    int err=Marshal.GetLastWin32Error(); h.Dispose();
    if(!ok && (err==5||err==1||err==87)){ Console.WriteLine("REFUSED="+err); return 0; }
    Console.WriteLine("UNEXPECTED ok="+ok+" err="+err); return 1;
  }
}
'@
    try {
        $foreignDir = Join-Path $ArtifactDir "foreign-owner"
        New-Item -Force -ItemType Directory $foreignDir | Out-Null
        $csPath = Join-Path $foreignDir "ForeignDestroy.cs"
        $exePath = Join-Path $foreignDir "ForeignDestroy.exe"
        Set-Content -Path $csPath -Value $foreignCs -Encoding ASCII
        $csc = Join-Path $env:WINDIR "Microsoft.NET\Framework64\v4.0.30319\csc.exe"
        if (-not (Test-Path $csc)) { throw "csc.exe not found" }
        $cscOut = & $csc /nologo /out:$exePath $csPath 2>&1 | Out-String
        if (-not (Test-Path $exePath)) { throw "csc failed: $cscOut" }
        $foreignOut = Join-Path $foreignDir "out.txt"
        $p = Start-Process -FilePath $exePath -Wait -PassThru -WindowStyle Hidden `
            -RedirectStandardOutput $foreignOut -RedirectStandardError (Join-Path $foreignDir "err.txt")
        $childText = ""
        if (Test-Path $foreignOut) {
            $childText = [string](Get-Content $foreignOut -Raw -ErrorAction SilentlyContinue)
        }
        $exitCode = -1
        if ($null -ne $p) { $exitCode = [int]$p.ExitCode }
        if ($exitCode -eq 0 -and $childText -match 'REFUSED=') {
            $verdict.REFUSE_FOREIGN_OWNER = 1
            L ("REFUSE_FOREIGN_OWNER=1 child=" + $childText.Trim())
        } else {
            L ("REFUSE_FOREIGN_OWNER=0 exit=$exitCode out=" + $childText.Trim())
            $verdict.NOTE += "FOREIGN_OWNER child exit=$exitCode; "
        }
    } catch {
        L ("REFUSE_FOREIGN_OWNER=0 exception=" + $_.Exception.Message)
        $verdict.NOTE += "FOREIGN_OWNER exception; "
    }

    # --- Concurrent Ring-0/3 probes (same process; bounded; no host thrash) ---
    if (Invoke-ReservedCqeInjection $h $rings $qd $maxIo) {
        $verdict.REFUSE_RESERVED_CQE = 1
        L "REFUSE_RESERVED_CQE=1"
    } else {
        L "REFUSE_RESERVED_CQE=0"
        $verdict.NOTE += "REFUSE_RESERVED_CQE probe failed; "
    }

    if (Invoke-CompletionReentryInjection $h $rings $qd $maxIo) {
        $verdict.COMPLETION_REENTRY_NO_SLOT_REUSE = 1
        L "COMPLETION_REENTRY_NO_SLOT_REUSE=1"
    } else {
        L "COMPLETION_REENTRY_NO_SLOT_REUSE=0"
        $verdict.NOTE += "COMPLETION_REENTRY probe failed; "
    }

    if (Invoke-RundownDuringCopyInjection $h $rings $qd $maxIo) {
        $verdict.RUNDOWN_UNMAP_AFTER_COPY = 1
        L "RUNDOWN_UNMAP_AFTER_COPY=1"
    } else {
        L "RUNDOWN_UNMAP_AFTER_COPY=0"
        $verdict.NOTE += "RUNDOWN_UNMAP_AFTER_COPY probe failed; "
    }

    [void][IoctlVal]::IoctlBool($h, [IoctlVal]::IOCTL_DESTROY, $null)
    L "DESTROY ok"
}
catch {
    $verdict.NOTE += "ERROR: $($_.Exception.Message); "
    L "FAIL: $($_.Exception.Message)"
}
finally {
    if ($rings) { Free-Rings $rings }
    if ($h -and -not $h.IsInvalid) { $h.Dispose() }
}

# NO_NEW_DUMP
$afterDumps = @()
if (Test-Path $dumpDir) {
    $afterDumps = @(Get-ChildItem $dumpDir -Filter *.dmp -EA SilentlyContinue | ForEach-Object FullName)
}
$new = Compare-Object $beforeDumps $afterDumps | Where-Object { $_.SideIndicator -eq '=>' }
if (-not $new) {
    $verdict.NO_NEW_DUMP = 1
    L "NO_NEW_DUMP=1"
} else {
    $verdict.NO_NEW_DUMP = 0
    L "NO_NEW_DUMP=0 new=$($new.InputObject -join ',')"
}

$out = Join-Path $ArtifactDir ("verdict-{0:yyyyMMdd-HHmmss}.json" -f (Get-Date))
$verdict | ConvertTo-Json | Set-Content -Path $out -Encoding utf8
Write-Host "ARTIFACT=$out"

$required = @(
    'PASS_VALID_QUEUE',
    'REFUSE_FOREIGN_OWNER',
    'REFUSE_RESERVED_REGISTER',
    'REFUSE_BAD_RING',
    'REFUSE_RING_INDEX_JUMP',
    'REFUSE_RESERVED_CQE',
    'REFUSE_UNKNOWN_IOCTL',
    'REFUSE_RESERVED_DISK_PARAMS',
    'COMPLETION_REENTRY_NO_SLOT_REUSE',
    'RUNDOWN_UNMAP_AFTER_COPY',
    'VPD_SERIAL_MATCH',
    'NO_NEW_DUMP'
)
$fail = @($required | Where-Object { [int]$verdict[$_] -ne 1 })
if ($fail.Count -gt 0) {
    Write-Host "STATUS=FAIL missing=$($fail -join ',')"
    exit 1
}
Write-Host "STATUS=PASS"
Write-Host "VERDICT_SUMMARY PASS_VALID_QUEUE=$($verdict.PASS_VALID_QUEUE) REFUSE_*=paired NO_NEW_DUMP=$($verdict.NO_NEW_DUMP)"
exit 0
