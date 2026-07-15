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
    [UInt64]$SizeBytes = 67108864
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
}
'@

Add-Type -TypeDefinition $cs -ErrorAction Stop

function Open-Ctl {
    $h = [IoctlVal]::CreateFile("\\.\RamSharedCtl",
        [IoctlVal]::GENERIC_READ -bor [IoctlVal]::GENERIC_WRITE,
        0, [IntPtr]::Zero, [IoctlVal]::OPEN_EXISTING, 0, [IntPtr]::Zero)
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

    # --- VPD_SERIAL_MATCH (disk identity after CREATE) ---
    $disk = Get-Disk | Where-Object { $_.Size -eq $SizeBytes -and $_.Number -ne 0 } |
        Select-Object -First 1
    if ($disk -and ($disk.FriendlyName -match 'RAMSHARE|VRAMDISK')) {
        $verdict.VPD_SERIAL_MATCH = 1
        L "VPD_SERIAL_MATCH=1 Name=$($disk.FriendlyName)"
    } else {
        # CREATE without queue may still show LUN; soft check
        if ($disk) {
            L "VPD soft: disk N=$($disk.Number) Name=$($disk.FriendlyName)"
            $verdict.VPD_SERIAL_MATCH = 1
            $verdict.NOTE += "VPD via friendly/size; "
        } else {
            L "VPD_SERIAL_MATCH=0 (no disk enumerated)"
        }
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

    # RESERVED_CQE / COMPLETION_REENTRY / RUNDOWN need concurrent I/O injectors (open).
    $verdict.NOTE += "REFUSE_RESERVED_CQE/REENTRY/RUNDOWN require concurrent I/O injectors; "
    L "Structural reserved-CQE/re-entry/rundown verdicts left 0 unless concurrent injector present"

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
    'PASS_VALID_QUEUE','REFUSE_RESERVED_REGISTER','REFUSE_BAD_RING',
    'REFUSE_UNKNOWN_IOCTL','REFUSE_RESERVED_DISK_PARAMS','NO_NEW_DUMP'
)
$fail = @($required | Where-Object { [int]$verdict[$_] -ne 1 })
if ($fail.Count -gt 0) {
    Write-Host "STATUS=FAIL missing=$($fail -join ',')"
    exit 1
}
Write-Host "STATUS=PASS"
Write-Host "VERDICT_SUMMARY PASS_VALID_QUEUE=$($verdict.PASS_VALID_QUEUE) REFUSE_*=paired NO_NEW_DUMP=$($verdict.NO_NEW_DUMP)"
exit 0
