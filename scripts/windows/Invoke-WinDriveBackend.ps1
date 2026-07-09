#Requires -Version 5.1
<#
.SYNOPSIS
  Minimal userspace I/O backend for ramshared control device (RAM buffer).
  CREATE_DISK + REGISTER + COMMIT_AND_FETCH loop (SPEC RF-2 / ITEM-6 smoke).

.NOTES
  Lab only. Not the full ramshared-winsvc SCM path.
#>
[CmdletBinding()]
param(
    [UInt64]$SizeBytes = 268435456, # 256 MiB default
    [UInt32]$BlockSize = 4096,
    [UInt32]$QueueDepth = 32,
    [UInt32]$MaxIo = 1048576,
    [int]$Seconds = 120
)

$ErrorActionPreference = "Stop"

$cs = @'
using System;
using System.Runtime.InteropServices;
using System.Threading;
using Microsoft.Win32.SafeHandles;

public static class RamSharedNative {
  public const uint GENERIC_READ = 0x80000000;
  public const uint GENERIC_WRITE = 0x40000000;
  public const uint OPEN_EXISTING = 3;
  public const uint FILE_FLAG_OVERLAPPED = 0x40000000;

  // CTL_CODE(FILE_DEVICE_MASS_STORAGE=0x2d, 0x800|N, METHOD_BUFFERED, FILE_READ|FILE_WRITE)
  public static uint Ioctl(uint n) {
    return (0x2du << 16) | (3u << 14) | (((0x800u + n) << 2)) | 0u;
  }
  public static readonly uint IOCTL_REGISTER = Ioctl(0);
  public static readonly uint IOCTL_UNREGISTER = Ioctl(1);
  public static readonly uint IOCTL_COMMIT = Ioctl(2);
  public static readonly uint IOCTL_CREATE = Ioctl(3);
  public static readonly uint IOCTL_DESTROY = Ioctl(4);

  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
  public static extern SafeFileHandle CreateFile(string name, uint access, uint share,
    IntPtr sec, uint disp, uint flags, IntPtr template);

  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern bool DeviceIoControl(SafeFileHandle h, uint code,
    byte[] inBuf, uint inLen, byte[] outBuf, uint outLen, out uint ret, IntPtr ov);

  [StructLayout(LayoutKind.Sequential, Pack=8)]
  public struct DiskParams {
    public ulong size_bytes;
    public uint block_size;
    public uint reserved;
    [MarshalAs(UnmanagedType.ByValArray, SizeConst=16)]
    public byte[] serial;
  }

  [StructLayout(LayoutKind.Sequential, Pack=8)]
  public struct Register {
    public uint abi_version, disk_id, queue_depth, block_size, max_io_bytes, reserved;
    public ulong sq_ring_va, cq_ring_va, data_area_va, data_area_len;
    public ulong sq_event_handle, cq_event_handle;
  }

  [StructLayout(LayoutKind.Sequential, Pack=8)]
  public struct RingHdr {
    public uint magic, entries, head, tail;
  }

  [StructLayout(LayoutKind.Sequential, Pack=8)]
  public struct Sqe {
    public ulong tag;
    public uint op, flags;
    public ulong offset;
    public uint len, buf_slot;
  }

  [StructLayout(LayoutKind.Sequential, Pack=8)]
  public struct Cqe {
    public ulong tag;
    public int status;
    public uint reserved;
  }

  public static byte[] StructToBytes<T>(T s) where T : struct {
    int n = Marshal.SizeOf(typeof(T));
    byte[] b = new byte[n];
    IntPtr p = Marshal.AllocHGlobal(n);
    try {
      Marshal.StructureToPtr(s, p, false);
      Marshal.Copy(p, b, 0, n);
    } finally { Marshal.FreeHGlobal(p); }
    return b;
  }
}
'@

Add-Type -TypeDefinition $cs -ErrorAction Stop

$h = [RamSharedNative]::CreateFile(
    "\\.\RamSharedCtl",
    [RamSharedNative]::GENERIC_READ -bor [RamSharedNative]::GENERIC_WRITE,
    0, [IntPtr]::Zero, [RamSharedNative]::OPEN_EXISTING, 0, [IntPtr]::Zero)
if ($h.IsInvalid) { throw "open RamSharedCtl failed err=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }

$dp = New-Object RamSharedNative+DiskParams
$dp.size_bytes = [UInt64]$SizeBytes
$dp.block_size = [UInt32]$BlockSize
$dp.reserved = 0
$dp.serial = New-Object byte[] 16
[Text.Encoding]::ASCII.GetBytes("RAMSHARED-DISK01").CopyTo($dp.serial, 0)

$in = [RamSharedNative]::StructToBytes($dp)
$ret = [uint32]0
if (-not [RamSharedNative]::DeviceIoControl($h, [RamSharedNative]::IOCTL_CREATE, $in, [uint32]$in.Length, $null, 0, [ref]$ret, [IntPtr]::Zero)) {
    throw "CREATE_DISK failed err=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
}
Write-Host "CREATE_DISK ok size=$SizeBytes bs=$BlockSize"

# Allocate SQ/CQ rings + data area (pinned)
$sqeSize = 32
$cqeSize = 16
$hdrSize = 16
$sqBytes = $hdrSize + ($QueueDepth * $sqeSize)
$cqBytes = $hdrSize + ($QueueDepth * $cqeSize)
$dataBytes = [int]($QueueDepth * $MaxIo)

$sq = New-Object byte[] $sqBytes
$cq = New-Object byte[] $cqBytes
$data = New-Object byte[] $dataBytes
# pin
$hSq = [Runtime.InteropServices.GCHandle]::Alloc($sq, 'Pinned')
$hCq = [Runtime.InteropServices.GCHandle]::Alloc($cq, 'Pinned')
$hData = [Runtime.InteropServices.GCHandle]::Alloc($data, 'Pinned')
$sqPtr = $hSq.AddrOfPinnedObject()
$cqPtr = $hCq.AddrOfPinnedObject()
$dataPtr = $hData.AddrOfPinnedObject()

# init ring headers: magic RSRD, entries=qd, head=tail=0
[BitConverter]::GetBytes([uint32]0x52535244).CopyTo($sq, 0)
[BitConverter]::GetBytes([uint32]$QueueDepth).CopyTo($sq, 4)
[BitConverter]::GetBytes([uint32]0x52535244).CopyTo($cq, 0)
[BitConverter]::GetBytes([uint32]$QueueDepth).CopyTo($cq, 4)

$reg = New-Object RamSharedNative+Register
$reg.abi_version = 1
$reg.disk_id = 0
$reg.queue_depth = $QueueDepth
$reg.block_size = $BlockSize
$reg.max_io_bytes = $MaxIo
$reg.reserved = 0
$reg.sq_ring_va = [uint64]$sqPtr.ToInt64()
$reg.cq_ring_va = [uint64]$cqPtr.ToInt64()
$reg.data_area_va = [uint64]$dataPtr.ToInt64()
$reg.data_area_len = [uint64]$dataBytes
$reg.sq_event_handle = 0
$reg.cq_event_handle = 0

$rin = [RamSharedNative]::StructToBytes($reg)
if (-not [RamSharedNative]::DeviceIoControl($h, [RamSharedNative]::IOCTL_REGISTER, $rin, [uint32]$rin.Length, $null, 0, [ref]$ret, [IntPtr]::Zero)) {
    throw "REGISTER_QUEUE failed err=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
}
Write-Host "REGISTER_QUEUE ok qd=$QueueDepth"

# RAM backend
$backend = New-Object byte[] ([int]$SizeBytes)
$ops = 0
$deadline = (Get-Date).AddSeconds($Seconds)
Write-Host "I/O loop for ${Seconds}s (COMMIT_AND_FETCH)..."

while ((Get-Date) -lt $deadline) {
    $ok = [RamSharedNative]::DeviceIoControl($h, [RamSharedNative]::IOCTL_COMMIT, $null, 0, $null, 0, [ref]$ret, [IntPtr]::Zero)
    $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    # Process SQ: head/tail in sq[8]/sq[12]
    $sqHead = [BitConverter]::ToUInt32($sq, 8)
    $sqTail = [BitConverter]::ToUInt32($sq, 12)
    while ($sqHead -ne $sqTail) {
        $idx = $sqHead -band ($QueueDepth - 1)
        $off = $hdrSize + ($idx * $sqeSize)
        $tag = [BitConverter]::ToUInt64($sq, $off)
        $op = [BitConverter]::ToUInt32($sq, $off + 8)
        $boff = [BitConverter]::ToUInt64($sq, $off + 16)
        $len = [BitConverter]::ToUInt32($sq, $off + 24)
        $slot = [BitConverter]::ToUInt32($sq, $off + 28)
        $status = 0
        try {
            $slotOff = [int]($slot * $MaxIo)
            if ($op -eq 0) { # READ
                [Array]::Copy($backend, [int]$boff, $data, $slotOff, [int]$len)
            } elseif ($op -eq 1) { # WRITE
                [Array]::Copy($data, $slotOff, $backend, [int]$boff, [int]$len)
            } elseif ($op -eq 2) {
                # FLUSH no-op
            } else {
                $status = 22
            }
        } catch {
            $status = 5
        }
        # push CQE
        $cqHead = [BitConverter]::ToUInt32($cq, 8)
        $cqTail = [BitConverter]::ToUInt32($cq, 12)
        $cidx = $cqTail -band ($QueueDepth - 1)
        $coff = $hdrSize + ($cidx * $cqeSize)
        [BitConverter]::GetBytes([uint64]$tag).CopyTo($cq, $coff)
        [BitConverter]::GetBytes([int32]$status).CopyTo($cq, $coff + 8)
        [BitConverter]::GetBytes([uint32]0).CopyTo($cq, $coff + 12)
        $cqTail = $cqTail + 1
        [BitConverter]::GetBytes([uint32]$cqTail).CopyTo($cq, 12)
        $sqHead = $sqHead + 1
        [BitConverter]::GetBytes([uint32]$sqHead).CopyTo($sq, 8)
        $ops++
    }
    if (-not $ok -and $err -ne 0 -and $err -ne 997) {
        # 997 ERROR_IO_PENDING if overlapped - we use sync
        Start-Sleep -Milliseconds 5
    } else {
        Start-Sleep -Milliseconds 1
    }
}

Write-Host "loop done ops=$ops"
[void][RamSharedNative]::DeviceIoControl($h, [RamSharedNative]::IOCTL_UNREGISTER, $null, 0, $null, 0, [ref]$ret, [IntPtr]::Zero)
[void][RamSharedNative]::DeviceIoControl($h, [RamSharedNative]::IOCTL_DESTROY, $null, 0, $null, 0, [ref]$ret, [IntPtr]::Zero)
$h.Dispose()
$hSq.Free(); $hCq.Free(); $hData.Free()
Write-Host "TEARDOWN_OK ops=$ops"
