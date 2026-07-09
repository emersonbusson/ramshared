using System;
using System.Runtime.InteropServices;
using System.Text;

public static class NtPf {
  [StructLayout(LayoutKind.Sequential)]
  public struct UNICODE_STRING {
    public ushort Length;
    public ushort MaximumLength;
    public IntPtr Buffer;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct LARGE_INTEGER {
    public long QuadPart;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct LUID {
    public uint LowPart;
    public int HighPart;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct TOKEN_PRIVILEGES {
    public uint PrivilegeCount;
    public LUID Luid;
    public uint Attributes;
  }

  const uint TOKEN_ADJUST_PRIVILEGES = 0x0020;
  const uint TOKEN_QUERY = 0x0008;
  const uint SE_PRIVILEGE_ENABLED = 0x00000002;

  [DllImport("ntdll.dll")]
  public static extern int NtCreatePagingFile(
    ref UNICODE_STRING PageFileName,
    ref LARGE_INTEGER MinimumSize,
    ref LARGE_INTEGER MaximumSize,
    uint Priority);

  [DllImport("advapi32.dll", SetLastError = true)]
  static extern bool OpenProcessToken(IntPtr ProcessHandle, uint DesiredAccess, out IntPtr TokenHandle);

  [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
  static extern bool LookupPrivilegeValue(string lpSystemName, string lpName, out LUID lpLuid);

  [DllImport("advapi32.dll", SetLastError = true)]
  static extern bool AdjustTokenPrivileges(IntPtr TokenHandle, bool DisableAllPrivileges,
    ref TOKEN_PRIVILEGES NewState, uint BufferLength, IntPtr PreviousState, IntPtr ReturnLength);

  [DllImport("kernel32.dll")]
  static extern IntPtr GetCurrentProcess();

  [DllImport("kernel32.dll", SetLastError = true)]
  static extern bool CloseHandle(IntPtr hObject);

  public static string EnableCreatePagefilePrivilege() {
    IntPtr token;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, out token))
      return "OpenProcessToken fail err=" + Marshal.GetLastWin32Error();
    try {
      LUID luid;
      if (!LookupPrivilegeValue(null, "SeCreatePagefilePrivilege", out luid))
        return "LookupPrivilegeValue fail err=" + Marshal.GetLastWin32Error();
      TOKEN_PRIVILEGES tp = new TOKEN_PRIVILEGES();
      tp.PrivilegeCount = 1;
      tp.Luid = luid;
      tp.Attributes = SE_PRIVILEGE_ENABLED;
      if (!AdjustTokenPrivileges(token, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero))
        return "AdjustTokenPrivileges fail err=" + Marshal.GetLastWin32Error();
      int err = Marshal.GetLastWin32Error();
      // ERROR_NOT_ALL_ASSIGNED = 1300
      if (err == 1300) return "PRIV_NOT_ASSIGNED (account lacks SeCreatePagefilePrivilege)";
      return "PRIV_OK lastErr=" + err;
    } finally {
      CloseHandle(token);
    }
  }

  public static string Create(string path, long minBytes, long maxBytes) {
    string priv = EnableCreatePagefilePrivilege();
    string nt = path;
    if (path.Length >= 2 && path[1] == ':')
      nt = @"\??\" + path;
    if (path.StartsWith(@"\??\"))
      nt = path;
    byte[] bytes = Encoding.Unicode.GetBytes(nt);
    IntPtr buf = Marshal.AllocHGlobal(bytes.Length + 2);
    try {
      Marshal.Copy(bytes, 0, buf, bytes.Length);
      Marshal.WriteInt16(buf, bytes.Length, 0);
      UNICODE_STRING us = new UNICODE_STRING();
      us.Length = (ushort)bytes.Length;
      us.MaximumLength = (ushort)(bytes.Length + 2);
      us.Buffer = buf;
      LARGE_INTEGER mn = new LARGE_INTEGER(); mn.QuadPart = minBytes;
      LARGE_INTEGER mx = new LARGE_INTEGER(); mx.QuadPart = maxBytes;
      int st = NtCreatePagingFile(ref us, ref mn, ref mx, 0);
      return priv + " | NTSTATUS=0x" + st.ToString("X8") + " path=" + nt;
    } finally {
      Marshal.FreeHGlobal(buf);
    }
  }
}
