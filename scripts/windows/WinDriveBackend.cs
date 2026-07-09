// Minimal lab I/O backend for \\.\RamSharedCtl (SPEC RF-2 smoke).
// Compile: csc /out:WinDriveBackend.exe WinDriveBackend.cs
// Run (admin): WinDriveBackend.exe [sizeBytes] [seconds]
// C# 5 compatible (Framework csc)
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

static class WinDriveBackend
{
    const uint GENERIC_READ = 0x80000000;
    const uint GENERIC_WRITE = 0x40000000;
    const uint OPEN_EXISTING = 3;
    const uint MEM_COMMIT = 0x1000;
    const uint MEM_RESERVE = 0x2000;
    const uint MEM_RELEASE = 0x8000;
    const uint PAGE_READWRITE = 0x04;
    const uint FILE_FLAG_OVERLAPPED = 0x40000000;
    const uint WAIT_OBJECT_0 = 0;
    const uint WAIT_TIMEOUT = 0x102;
    const uint ERROR_IO_PENDING = 997;

    static uint Ioctl(uint n)
    {
        return (0x2du << 16) | (3u << 14) | ((0x800u + n) << 2);
    }

    static readonly uint IOCTL_REGISTER = Ioctl(0);
    static readonly uint IOCTL_UNREGISTER = Ioctl(1);
    static readonly uint IOCTL_COMMIT = Ioctl(2);
    static readonly uint IOCTL_CREATE = Ioctl(3);
    static readonly uint IOCTL_DESTROY = Ioctl(4);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern SafeFileHandle CreateFile(
        string name, uint access, uint share, IntPtr sec,
        uint disp, uint flags, IntPtr template);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool DeviceIoControl(
        SafeFileHandle h, uint code,
        byte[] inBuf, uint inLen, byte[] outBuf, uint outLen,
        out uint ret, IntPtr ov);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool DeviceIoControl(
        SafeFileHandle h, uint code,
        byte[] inBuf, uint inLen, byte[] outBuf, uint outLen,
        out uint ret, ref NativeOverlapped ov);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint WaitForSingleObject(IntPtr h, uint ms);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool CancelIo(SafeFileHandle h);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern IntPtr CreateEvent(IntPtr sa, bool manual, bool initial, string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool CloseHandle(IntPtr h);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool GetOverlappedResult(SafeFileHandle h, ref NativeOverlapped ov, out uint xfer, bool wait);

    [StructLayout(LayoutKind.Sequential)]
    struct NativeOverlapped
    {
        public IntPtr InternalLow;
        public IntPtr InternalHigh;
        public int OffsetLow;
        public int OffsetHigh;
        public IntPtr EventHandle;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern IntPtr VirtualAlloc(IntPtr addr, UIntPtr size, uint type, uint protect);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool VirtualFree(IntPtr addr, UIntPtr size, uint type);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool VirtualLock(IntPtr addr, UIntPtr size);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool VirtualUnlock(IntPtr addr, UIntPtr size);

    [StructLayout(LayoutKind.Sequential, Pack = 8)]
    struct DiskParams
    {
        public ulong size_bytes;
        public uint block_size;
        public uint reserved;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 16)]
        public byte[] serial;
    }

    [StructLayout(LayoutKind.Sequential, Pack = 8)]
    struct Register
    {
        public uint abi_version, disk_id, queue_depth, block_size, max_io_bytes, reserved;
        public ulong sq_ring_va, cq_ring_va, data_area_va, data_area_len;
        public ulong sq_event_handle, cq_event_handle;
    }

    static byte[] StructBytes(object s)
    {
        int n = Marshal.SizeOf(s);
        byte[] b = new byte[n];
        IntPtr p = Marshal.AllocHGlobal(n);
        try
        {
            Marshal.StructureToPtr(s, p, false);
            Marshal.Copy(p, b, 0, n);
        }
        finally { Marshal.FreeHGlobal(p); }
        return b;
    }

    // Sync DeviceIoControl with event (handle is OVERLAPPED)
    static bool IoctlSync(SafeFileHandle h, uint code, byte[] inBuf, uint inLen, IntPtr ev, out uint ret)
    {
        ret = 0;
        NativeOverlapped ov = new NativeOverlapped();
        ov.EventHandle = ev;
        bool ok = DeviceIoControl(h, code, inBuf, inLen, null, 0, out ret, ref ov);
        int err = Marshal.GetLastWin32Error();
        if (!ok && err == (int)ERROR_IO_PENDING)
        {
            uint wr = WaitForSingleObject(ev, 10000);
            if (wr != WAIT_OBJECT_0)
            {
                CancelIo(h);
                return false;
            }
            return GetOverlappedResult(h, ref ov, out ret, false);
        }
        return ok;
    }

    static void Main(string[] args)
    {
        ulong sizeBytes = args.Length > 0 ? ulong.Parse(args[0]) : 64UL * 1024 * 1024;
        int seconds = args.Length > 1 ? int.Parse(args[1]) : 30;
        const uint qd = 8;
        const uint maxIo = 65536; // 64 KiB; data area 8*64K=512K < 4MiB MDL cap
        const uint bs = 4096;
        const uint magic = 0x52535244;
        const int hdr = 16, sqe = 32, cqe = 16;

        Console.WriteLine("WinDriveBackend size={0} seconds={1}", sizeBytes, seconds);

        SafeFileHandle h = CreateFile(
            @"\\.\RamSharedCtl",
            GENERIC_READ | GENERIC_WRITE,
            0, IntPtr.Zero, OPEN_EXISTING, FILE_FLAG_OVERLAPPED, IntPtr.Zero);
        if (h.IsInvalid)
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "CreateFile RamSharedCtl");

        IntPtr ev = CreateEvent(IntPtr.Zero, true, false, null);
        uint ret;

        // DESTROY first (ignore failure)
        IoctlSync(h, IOCTL_DESTROY, null, 0, ev, out ret);

        DiskParams dp = new DiskParams();
        dp.size_bytes = sizeBytes;
        dp.block_size = bs;
        dp.reserved = 0;
        dp.serial = new byte[16];
        System.Text.Encoding.ASCII.GetBytes("RAMSHARED-DISK01").CopyTo(dp.serial, 0);
        byte[] din = StructBytes(dp);
        if (!IoctlSync(h, IOCTL_CREATE, din, (uint)din.Length, ev, out ret))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "CREATE_DISK");
        Console.WriteLine("CREATE_DISK ok");

        int sqBytes = hdr + (int)qd * sqe;
        int cqBytes = hdr + (int)qd * cqe;
        int dataBytes = (int)(qd * maxIo);

        IntPtr sq = VirtualAlloc(IntPtr.Zero, (UIntPtr)(uint)sqBytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        IntPtr cq = VirtualAlloc(IntPtr.Zero, (UIntPtr)(uint)cqBytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        IntPtr data = VirtualAlloc(IntPtr.Zero, (UIntPtr)(uint)dataBytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if (sq == IntPtr.Zero || cq == IntPtr.Zero || data == IntPtr.Zero)
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "VirtualAlloc");

        VirtualLock(sq, (UIntPtr)(uint)sqBytes);
        VirtualLock(cq, (UIntPtr)(uint)cqBytes);
        VirtualLock(data, (UIntPtr)(uint)dataBytes);

        for (int i = 0; i < sqBytes; i++) Marshal.WriteByte(sq, i, 0);
        for (int i = 0; i < cqBytes; i++) Marshal.WriteByte(cq, i, 0);
        Marshal.WriteInt32(sq, 0, unchecked((int)magic));
        Marshal.WriteInt32(sq, 4, (int)qd);
        Marshal.WriteInt32(cq, 0, unchecked((int)magic));
        Marshal.WriteInt32(cq, 4, (int)qd);

        Register reg = new Register();
        reg.abi_version = 1;
        reg.disk_id = 0;
        reg.queue_depth = qd;
        reg.block_size = bs;
        reg.max_io_bytes = maxIo;
        reg.reserved = 0;
        reg.sq_ring_va = (ulong)sq.ToInt64();
        reg.cq_ring_va = (ulong)cq.ToInt64();
        reg.data_area_va = (ulong)data.ToInt64();
        reg.data_area_len = (ulong)dataBytes;
        reg.sq_event_handle = 0;
        reg.cq_event_handle = 0;
        byte[] rin = StructBytes(reg);
        if (!IoctlSync(h, IOCTL_REGISTER, rin, (uint)rin.Length, ev, out ret))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "REGISTER_QUEUE");
        Console.WriteLine("REGISTER_QUEUE ok");

        // sizeBytes may be large; use chunked array if needed - 256MiB is OK
        if (sizeBytes > (ulong)int.MaxValue)
            throw new ArgumentOutOfRangeException("sizeBytes too large for lab backend");
        byte[] backend = new byte[(int)sizeBytes];
        int ops = 0;
        DateTime deadline = DateTime.UtcNow.AddSeconds(seconds);
        while (DateTime.UtcNow < deadline)
        {
            NativeOverlapped ov = new NativeOverlapped();
            ov.EventHandle = ev;
            bool ok = DeviceIoControl(h, IOCTL_COMMIT, null, 0, null, 0, out ret, ref ov);
            int err = Marshal.GetLastWin32Error();
            if (!ok && err == (int)ERROR_IO_PENDING)
            {
                uint wr = WaitForSingleObject(ev, 50);
                if (wr == WAIT_TIMEOUT)
                {
                    CancelIo(h);
                    // still drain any completed rings
                }
                else
                {
                    GetOverlappedResult(h, ref ov, out ret, false);
                }
            }

            uint sqHead = (uint)Marshal.ReadInt32(sq, 8);
            uint sqTail = (uint)Marshal.ReadInt32(sq, 12);
            while (sqHead != sqTail)
            {
                int idx = (int)(sqHead & (qd - 1));
                int off = hdr + idx * sqe;
                ulong tag = (ulong)Marshal.ReadInt64(sq, off);
                uint op = (uint)Marshal.ReadInt32(sq, off + 8);
                ulong boff = (ulong)Marshal.ReadInt64(sq, off + 16);
                uint len = (uint)Marshal.ReadInt32(sq, off + 24);
                uint slot = (uint)Marshal.ReadInt32(sq, off + 28);
                int status = 0;
                try
                {
                    int slotOff = (int)(slot * maxIo);
                    if (op == 0) // READ
                    {
                        for (int i = 0; i < (int)len; i++)
                            Marshal.WriteByte(data, slotOff + i, backend[(int)boff + i]);
                    }
                    else if (op == 1) // WRITE
                    {
                        for (int i = 0; i < (int)len; i++)
                            backend[(int)boff + i] = Marshal.ReadByte(data, slotOff + i);
                    }
                    else if (op != 2)
                        status = 22;
                }
                catch { status = 5; }

                uint cqTail = (uint)Marshal.ReadInt32(cq, 12);
                int cidx = (int)(cqTail & (qd - 1));
                int coff = hdr + cidx * cqe;
                Marshal.WriteInt64(cq, coff, unchecked((long)tag));
                Marshal.WriteInt32(cq, coff + 8, status);
                Marshal.WriteInt32(cq, coff + 12, 0);
                cqTail++;
                Marshal.WriteInt32(cq, 12, unchecked((int)cqTail));
                sqHead++;
                Marshal.WriteInt32(sq, 8, unchecked((int)sqHead));
                ops++;
            }
        }

        Console.WriteLine("loop done ops={0}", ops);
        IoctlSync(h, IOCTL_UNREGISTER, null, 0, ev, out ret);
        IoctlSync(h, IOCTL_DESTROY, null, 0, ev, out ret);
        if (ev != IntPtr.Zero) CloseHandle(ev);
        VirtualUnlock(sq, (UIntPtr)(uint)sqBytes);
        VirtualUnlock(cq, (UIntPtr)(uint)cqBytes);
        VirtualUnlock(data, (UIntPtr)(uint)dataBytes);
        VirtualFree(sq, UIntPtr.Zero, MEM_RELEASE);
        VirtualFree(cq, UIntPtr.Zero, MEM_RELEASE);
        VirtualFree(data, UIntPtr.Zero, MEM_RELEASE);
        h.Dispose();
        Console.WriteLine("TEARDOWN_OK ops={0}", ops);
    }
}
