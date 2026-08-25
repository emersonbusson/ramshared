[CmdletBinding()]
param(
  [switch]$FocusedConfinementOnly,
  [switch]$FocusedTimeoutOnly,
  [switch]$FocusedDeadlineOnly,
  [switch]$R6StaticOnly,
  [ValidateSet('', 'direct_entrypoints_refuse_before_mutation', 'legitimate_gate_and_fixture_paths_pass_without_live_effects', 'fixture_parameters_cannot_carry_live_authority')]
  [string]$SpecTest = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$powerShell51 = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
if (-not (Test-Path -LiteralPath $powerShell51 -PathType Leaf)) {
  throw 'Windows PowerShell 5.1 is unavailable'
}
if ($PSVersionTable.PSVersion.Major -ne 5) {
  throw 'this test must itself run under Windows PowerShell 5.1'
}

$launcher = Join-Path $PSScriptRoot 'boot-kernel-safe.ps1'
$wrapper = Join-Path $PSScriptRoot 'boot-kernel-logged.ps1'
$installer = Join-Path $PSScriptRoot 'Install-BootKernelLaunchers.ps1'
$control = Join-Path $PSScriptRoot 'wsl-kernel.sh'

function Assert-True([bool]$Condition, [string]$Message) {
  if (-not $Condition) { throw $Message }
}

function Get-Sha256([string]$Path) {
  $stream = [System.IO.File]::OpenRead($Path)
  try {
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
      return ([System.BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
    } finally {
      $algorithm.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

function Get-PathState([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path)) {
    return 'ABSENT'
  }
  $fullPath = [System.IO.Path]::GetFullPath($Path)
  $rootItem = Get-Item -LiteralPath $fullPath -Force
  if (-not $rootItem.PSIsContainer) {
    return ('FILE|' + $rootItem.Length + '|' + (Get-Sha256 $rootItem.FullName))
  }
  $records = New-Object 'System.Collections.Generic.List[string]'
  $records.Add('DIRECTORY')
  foreach ($item in @(Get-ChildItem -LiteralPath $fullPath -Force -Recurse | Sort-Object FullName)) {
    $relative = $item.FullName.Substring($fullPath.Length).TrimStart('\')
    if ($item.PSIsContainer) {
      $records.Add('D|' + $relative)
    } else {
      $records.Add('F|' + $relative + '|' + $item.Length + '|' + (Get-Sha256 $item.FullName))
    }
  }
  return ($records.ToArray() -join "`n")
}

function Assert-PathState([string]$Path, [string]$Expected, [string]$Message) {
  $actual = Get-PathState $Path
  Assert-True ($actual -ceq $Expected) $Message
}

function Write-Ascii([string]$Path, [string]$Text) {
  [System.IO.File]::WriteAllText($Path, $Text, [System.Text.Encoding]::ASCII)
}

function ConvertTo-TestProcessArgument([string]$Value) {
  if ($Value.Length -gt 0 -and $Value -cnotmatch '[\s"]') { return $Value }
  $escaped = $Value -replace '(\\*)"', '$1$1\"'
  $escaped = $escaped -replace '(\\+)$', '$1$1'
  return '"' + $escaped + '"'
}

function Test-HasArgument([string[]]$Arguments, [string]$Name) {
  return @($Arguments | Where-Object { $_ -ceq $Name }).Count -gt 0
}

function Initialize-SuspendedJobProcessType {
  if ($null -ne ('RamSharedSuspendedJobProcess' -as [type])) { return }
  Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public sealed class RamSharedSuspendedJobResult {
    public int ExitCode;
    public string Stdout;
    public string Stderr;
    public int ProcessId;
    public long CreationTimeUtcFileTime;
    public bool JobEmpty;
}

public sealed class RamSharedSuspendedJobProcess : IDisposable {
    const uint CREATE_SUSPENDED = 0x00000004;
    const uint CREATE_NO_WINDOW = 0x08000000;
    const uint STARTF_USESTDHANDLES = 0x00000100;
    const uint HANDLE_FLAG_INHERIT = 0x00000001;
    const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
    const int JobObjectBasicAccountingInformation = 1;
    const int JobObjectExtendedLimitInformation = 9;
    const uint WAIT_OBJECT_0 = 0;
    const uint WAIT_TIMEOUT = 258;
    const uint WAIT_FAILED = 0xffffffff;
    const uint STILL_ACTIVE = 259;
    const int ERROR_BROKEN_PIPE = 109;
    const int ERROR_NO_DATA = 232;

    [StructLayout(LayoutKind.Sequential)]
    struct SECURITY_ATTRIBUTES {
        public int Length;
        public IntPtr SecurityDescriptor;
        [MarshalAs(UnmanagedType.Bool)] public bool InheritHandle;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct STARTUPINFO {
        public int cb;
        public string lpReserved;
        public string lpDesktop;
        public string lpTitle;
        public uint dwX;
        public uint dwY;
        public uint dwXSize;
        public uint dwYSize;
        public uint dwXCountChars;
        public uint dwYCountChars;
        public uint dwFillAttribute;
        public uint dwFlags;
        public short wShowWindow;
        public short cbReserved2;
        public IntPtr lpReserved2;
        public IntPtr hStdInput;
        public IntPtr hStdOutput;
        public IntPtr hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct PROCESS_INFORMATION {
        public IntPtr hProcess;
        public IntPtr hThread;
        public uint dwProcessId;
        public uint dwThreadId;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct FILETIME {
        public uint Low;
        public uint High;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize;
        public UIntPtr MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct IO_COUNTERS {
        public ulong ReadOperationCount;
        public ulong WriteOperationCount;
        public ulong OtherOperationCount;
        public ulong ReadTransferCount;
        public ulong WriteTransferCount;
        public ulong OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
        public IO_COUNTERS IoInfo;
        public UIntPtr ProcessMemoryLimit;
        public UIntPtr JobMemoryLimit;
        public UIntPtr PeakProcessMemoryUsed;
        public UIntPtr PeakJobMemoryUsed;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION {
        public long TotalUserTime;
        public long TotalKernelTime;
        public long ThisPeriodTotalUserTime;
        public long ThisPeriodTotalKernelTime;
        public uint TotalPageFaultCount;
        public uint TotalProcesses;
        public uint ActiveProcesses;
        public uint TotalTerminatedProcesses;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool CreatePipe(out IntPtr readPipe, out IntPtr writePipe,
        ref SECURITY_ATTRIBUTES attributes, uint size);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateFileW(string fileName, uint access, uint share,
        ref SECURITY_ATTRIBUTES attributes, uint creation, uint flags, IntPtr template);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true,
        EntryPoint = "CreateJobObjectW")]
    static extern IntPtr CreateJobObjectWInheritable(ref SECURITY_ATTRIBUTES attributes,
        string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool SetInformationJobObject(IntPtr job, int infoClass,
        IntPtr information, uint informationLength);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool CreateProcessW(string applicationName, StringBuilder commandLine,
        IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles,
        uint creationFlags, IntPtr environment, string currentDirectory,
        ref STARTUPINFO startupInfo, out PROCESS_INFORMATION processInformation);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint ResumeThread(IntPtr thread);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool TerminateJobObject(IntPtr job, uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool TerminateProcess(IntPtr process, uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool GetProcessTimes(IntPtr process, out FILETIME creation,
        out FILETIME exit, out FILETIME kernel, out FILETIME user);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool QueryInformationJobObject(IntPtr job, int infoClass,
        out JOBOBJECT_BASIC_ACCOUNTING_INFORMATION information,
        uint informationLength, IntPtr returnLength);

    [DllImport("kernel32.dll", SetLastError = true, EntryPoint = "QueryInformationJobObject")]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool QueryInformationJobObjectBuffer(IntPtr job, int infoClass,
        IntPtr information, uint informationLength, IntPtr returnLength);

    [DllImport("kernel32.dll")]
    static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool IsProcessInJob(IntPtr process, IntPtr job,
        [MarshalAs(UnmanagedType.Bool)] out bool result);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool ReadFile(IntPtr handle, byte[] buffer, uint bytesToRead,
        out uint bytesRead, IntPtr overlapped);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool CloseHandle(IntPtr handle);

    readonly object outputLock = new object();
    readonly List<byte> stdout = new List<byte>();
    readonly List<byte> stderr = new List<byte>();
    readonly Stopwatch clock;
    readonly int deadlineMilliseconds;
    readonly int maximumOutputBytes;
    readonly bool injectTerminateFailure;
    readonly string label;
    IntPtr processHandle;
    IntPtr threadHandle;
    IntPtr jobHandle;
    IntPtr stdoutRead;
    IntPtr stderrRead;
    Thread stdoutThread;
    Thread stderrThread;
    volatile bool stdoutDone;
    volatile bool stderrDone;
    volatile bool outputOverflow;
    Exception readerFailure;
    int processId;
    long creationTime;
    bool completed;

    RamSharedSuspendedJobProcess(Stopwatch startedClock, int deadlineMs,
        int maxOutputBytes, bool terminateFailure, string processLabel) {
        clock = startedClock;
        deadlineMilliseconds = deadlineMs;
        maximumOutputBytes = maxOutputBytes;
        injectTerminateFailure = terminateFailure;
        label = processLabel;
    }

    static Exception NativeFailure(string operation) {
        return new Win32Exception(Marshal.GetLastWin32Error(), operation + " failed");
    }

    static void CheckedClose(ref IntPtr handle, string operation) {
        if (handle == IntPtr.Zero || handle == new IntPtr(-1)) return;
        IntPtr value = handle;
        handle = IntPtr.Zero;
        if (!CloseHandle(value)) throw NativeFailure(operation);
    }

    static void CheckedCloseNoThrow(ref IntPtr handle) {
        if (handle == IntPtr.Zero || handle == new IntPtr(-1)) return;
        IntPtr value = handle;
        handle = IntPtr.Zero;
        if (!CloseHandle(value)) throw NativeFailure("CloseHandle(cleanup)");
    }

    static void ConfigureKillOnClose(IntPtr job) {
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION information =
            new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        int size = Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION));
        IntPtr buffer = Marshal.AllocHGlobal(size);
        try {
            Marshal.StructureToPtr(information, buffer, false);
            if (!SetInformationJobObject(job, JobObjectExtendedLimitInformation,
                    buffer, (uint)size)) {
                throw NativeFailure("SetInformationJobObject");
            }
        } finally {
            Marshal.FreeHGlobal(buffer);
        }
    }

    public static RamSharedSuspendedJobProcess Start(string applicationName,
        string argumentLine, int deadlineMs, int maxOutputBytes,
        bool injectAssignFailure, bool injectResumeFailure,
        bool injectTerminateFailure, bool injectRootCreationMismatch,
        string label) {
        Stopwatch startedClock = Stopwatch.StartNew();
        if (deadlineMs < 1) throw new ArgumentOutOfRangeException("deadlineMs");
        if (maxOutputBytes < 1) throw new ArgumentOutOfRangeException("maxOutputBytes");
        RamSharedSuspendedJobProcess owned = new RamSharedSuspendedJobProcess(
            startedClock, deadlineMs, maxOutputBytes, injectTerminateFailure, label);
        SECURITY_ATTRIBUTES attributes = new SECURITY_ATTRIBUTES();
        attributes.Length = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES));
        attributes.InheritHandle = true;
        IntPtr stdoutWrite = IntPtr.Zero;
        IntPtr stderrWrite = IntPtr.Zero;
        IntPtr nullInput = IntPtr.Zero;
        bool created = false;
        bool assigned = false;
        try {
            if (!CreatePipe(out owned.stdoutRead, out stdoutWrite, ref attributes, 0))
                throw NativeFailure("CreatePipe(stdout)");
            if (!SetHandleInformation(owned.stdoutRead, HANDLE_FLAG_INHERIT, 0))
                throw NativeFailure("SetHandleInformation(stdout)");
            if (!CreatePipe(out owned.stderrRead, out stderrWrite, ref attributes, 0))
                throw NativeFailure("CreatePipe(stderr)");
            if (!SetHandleInformation(owned.stderrRead, HANDLE_FLAG_INHERIT, 0))
                throw NativeFailure("SetHandleInformation(stderr)");
            nullInput = CreateFileW("NUL", 0x80000000, 3, ref attributes, 3, 0, IntPtr.Zero);
            if (nullInput == new IntPtr(-1)) throw NativeFailure("CreateFileW(NUL)");
            bool exposeCustodyHandle = argumentLine.IndexOf(
                "{RAMSHARED_JOB_HANDLE}", StringComparison.Ordinal) >= 0;
            owned.jobHandle = exposeCustodyHandle
                ? CreateJobObjectWInheritable(ref attributes, null)
                : CreateJobObjectW(IntPtr.Zero, null);
            if (owned.jobHandle == IntPtr.Zero) throw NativeFailure("CreateJobObjectW");
            ConfigureKillOnClose(owned.jobHandle);
            if (exposeCustodyHandle) {
                argumentLine = argumentLine.Replace("{RAMSHARED_JOB_HANDLE}",
                    owned.jobHandle.ToInt64().ToString(
                        System.Globalization.CultureInfo.InvariantCulture));
            }

            STARTUPINFO startup = new STARTUPINFO();
            startup.cb = Marshal.SizeOf(typeof(STARTUPINFO));
            startup.dwFlags = STARTF_USESTDHANDLES;
            startup.hStdInput = nullInput;
            startup.hStdOutput = stdoutWrite;
            startup.hStdError = stderrWrite;
            PROCESS_INFORMATION process;
            StringBuilder command = new StringBuilder(
                QuoteArgument(applicationName) +
                (String.IsNullOrEmpty(argumentLine) ? "" : " " + argumentLine));
            if (!CreateProcessW(applicationName, command, IntPtr.Zero, IntPtr.Zero, true,
                    CREATE_SUSPENDED | CREATE_NO_WINDOW, IntPtr.Zero, null,
                    ref startup, out process)) {
                throw NativeFailure("CreateProcessW(CREATE_SUSPENDED)");
            }
            created = true;
            owned.processHandle = process.hProcess;
            owned.threadHandle = process.hThread;
            owned.processId = checked((int)process.dwProcessId);
            FILETIME creation;
            FILETIME exit;
            FILETIME kernel;
            FILETIME user;
            if (!GetProcessTimes(owned.processHandle, out creation, out exit, out kernel, out user))
                throw NativeFailure("GetProcessTimes");
            owned.creationTime = ((long)creation.High << 32) | creation.Low;
            if (injectRootCreationMismatch) owned.creationTime ^= 1L;

            if (injectAssignFailure) {
                if (!TerminateProcess(owned.processHandle, 225))
                    throw NativeFailure("TerminateProcess(injected assignment failure)");
                uint reaped = WaitForSingleObject(owned.processHandle, 5000);
                if (reaped != WAIT_OBJECT_0)
                    throw new InvalidOperationException("injected assignment failure root was not reaped by handle");
                throw new InvalidOperationException(
                    "INJECTED_ASSIGN_FAILURE root-terminated-and-reaped-by-handle-before-resume");
            }
            if (!AssignProcessToJobObject(owned.jobHandle, owned.processHandle)) {
                int assignmentError = Marshal.GetLastWin32Error();
                if (!TerminateProcess(owned.processHandle, 226))
                    throw NativeFailure("TerminateProcess(assignment failure)");
                uint reaped = WaitForSingleObject(owned.processHandle, 5000);
                if (reaped != WAIT_OBJECT_0)
                    throw new InvalidOperationException("assignment failure root was not reaped by handle");
                throw new Win32Exception(assignmentError,
                    "AssignProcessToJobObject failed; root terminated and reaped by handle");
            }
            assigned = true;
            CheckedClose(ref stdoutWrite, "CloseHandle(child stdout)");
            CheckedClose(ref stderrWrite, "CloseHandle(child stderr)");
            CheckedClose(ref nullInput, "CloseHandle(child stdin)");
            owned.StartReaders();
            if (injectResumeFailure)
                throw new InvalidOperationException(
                    "INJECTED_RESUME_FAILURE before-resume");
            uint resumeCount = ResumeThread(owned.threadHandle);
            if (resumeCount == 0xffffffff) throw NativeFailure("ResumeThread");
            return owned;
        } catch (Exception startFailure) {
            bool startCleanupJobEmpty = false;
            try {
                if (created && owned.processHandle != IntPtr.Zero) {
                    uint state = WaitForSingleObject(owned.processHandle, 0);
                    if (state == WAIT_FAILED) throw NativeFailure("WaitForSingleObject(start cleanup)");
                    if (state == WAIT_TIMEOUT) {
                        bool terminated = assigned
                            ? TerminateJobObject(owned.jobHandle, 227)
                            : TerminateProcess(owned.processHandle, 227);
                        if (!terminated) throw NativeFailure("handle-bound start cleanup termination");
                        state = WaitForSingleObject(owned.processHandle, 5000);
                        if (state != WAIT_OBJECT_0)
                            throw new InvalidOperationException("handle-bound start cleanup did not reap root");
                    }
                    if (assigned) {
                        if (owned.QueryActiveProcesses() != 0)
                            throw new InvalidOperationException(
                                "handle-bound start cleanup did not empty the assigned job");
                        startCleanupJobEmpty = true;
                    }
                }
                if (owned.stdoutThread != null && !owned.stdoutThread.Join(5000))
                    throw new InvalidOperationException("stdout cleanup reader did not terminate");
                if (owned.stderrThread != null && !owned.stderrThread.Join(5000))
                    throw new InvalidOperationException("stderr cleanup reader did not terminate");
            } catch (Exception cleanupFailure) {
                throw new InvalidOperationException(startFailure.Message +
                    "; START_CLEANUP_FAILED: " + cleanupFailure.Message, startFailure);
            } finally {
                CheckedCloseNoThrow(ref stdoutWrite);
                CheckedCloseNoThrow(ref stderrWrite);
                CheckedCloseNoThrow(ref nullInput);
                CheckedCloseNoThrow(ref owned.stdoutRead);
                CheckedCloseNoThrow(ref owned.stderrRead);
                CheckedCloseNoThrow(ref owned.threadHandle);
                CheckedCloseNoThrow(ref owned.processHandle);
                CheckedCloseNoThrow(ref owned.jobHandle);
                owned.completed = true;
            }
            if (injectResumeFailure && assigned && startCleanupJobEmpty)
                throw new InvalidOperationException(
                    "INJECTED_RESUME_FAILURE job-empty-root-reaped-before-resume",
                    startFailure);
            throw;
        }
    }

    static string QuoteArgument(string value) {
        if (value.Length > 0 && value.IndexOfAny(new char[] { ' ', '\t', '"' }) < 0)
            return value;
        StringBuilder result = new StringBuilder();
        result.Append('"');
        int slashes = 0;
        foreach (char character in value) {
            if (character == '\\') { slashes++; continue; }
            if (character == '"') {
                result.Append('\\', slashes * 2 + 1);
                result.Append('"');
                slashes = 0;
                continue;
            }
            result.Append('\\', slashes);
            slashes = 0;
            result.Append(character);
        }
        result.Append('\\', slashes * 2);
        result.Append('"');
        return result.ToString();
    }

    public static void ValidateAndCloseInheritedJobHandle(long inheritedHandle) {
        if (inheritedHandle <= 0)
            throw new InvalidOperationException("inherited custody handle is invalid");
        IntPtr job = new IntPtr(inheritedHandle);
        bool inJob;
        if (!IsProcessInJob(GetCurrentProcess(), job, out inJob))
            throw NativeFailure("IsProcessInJob(inherited custody)");
        if (!inJob)
            throw new InvalidOperationException("worker is not assigned to its inherited custody job");
        int size = Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION));
        IntPtr buffer = Marshal.AllocHGlobal(size);
        try {
            if (!QueryInformationJobObjectBuffer(job,
                    JobObjectExtendedLimitInformation, buffer, (uint)size, IntPtr.Zero))
                throw NativeFailure("QueryInformationJobObject(inherited custody)");
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION information =
                (JOBOBJECT_EXTENDED_LIMIT_INFORMATION)Marshal.PtrToStructure(buffer,
                    typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION));
            if ((information.BasicLimitInformation.LimitFlags &
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE) == 0)
                throw new InvalidOperationException(
                    "inherited custody job lacks KILL_ON_JOB_CLOSE");
        } finally {
            Marshal.FreeHGlobal(buffer);
        }
        IntPtr close = job;
        CheckedClose(ref close, "CloseHandle(inherited custody)");
    }

    void StartReaders() {
        stdoutThread = new Thread(delegate() { Drain(stdoutRead, stdout, true); });
        stderrThread = new Thread(delegate() { Drain(stderrRead, stderr, false); });
        stdoutThread.IsBackground = true;
        stderrThread.IsBackground = true;
        stdoutThread.Start();
        stderrThread.Start();
    }

    void Drain(IntPtr pipe, List<byte> destination, bool isStdout) {
        try {
            byte[] buffer = new byte[4096];
            while (true) {
                uint count;
                if (!ReadFile(pipe, buffer, (uint)buffer.Length, out count, IntPtr.Zero)) {
                    int error = Marshal.GetLastWin32Error();
                    if (error == ERROR_BROKEN_PIPE || error == ERROR_NO_DATA) break;
                    throw new Win32Exception(error, "ReadFile redirected output failed");
                }
                if (count == 0) break;
                lock (outputLock) {
                    int total = stdout.Count + stderr.Count;
                    int permitted = Math.Min((int)count, Math.Max(0, maximumOutputBytes - total));
                    for (int index = 0; index < permitted; index++) destination.Add(buffer[index]);
                    if (permitted != (int)count) outputOverflow = true;
                }
            }
        } catch (Exception failure) {
            lock (outputLock) { if (readerFailure == null) readerFailure = failure; }
        } finally {
            if (isStdout) stdoutDone = true; else stderrDone = true;
        }
    }

    uint QueryActiveProcesses() {
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION information;
        if (!QueryInformationJobObject(jobHandle, JobObjectBasicAccountingInformation,
                out information,
                (uint)Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)),
                IntPtr.Zero)) {
            throw NativeFailure("QueryInformationJobObject");
        }
        return information.ActiveProcesses;
    }

    public bool HasExited {
        get {
            if (completed) return true;
            uint state = WaitForSingleObject(processHandle, 0);
            if (state == WAIT_FAILED) throw NativeFailure("WaitForSingleObject(HasExited)");
            return state == WAIT_OBJECT_0;
        }
    }

    public int ProcessId { get { return processId; } }
    public long CreationTimeUtcFileTime { get { return creationTime; } }

    long ReadRootCreationTime() {
        FILETIME creation;
        FILETIME exit;
        FILETIME kernel;
        FILETIME user;
        if (!GetProcessTimes(processHandle, out creation, out exit, out kernel, out user))
            throw NativeFailure("GetProcessTimes(identity revalidation)");
        return ((long)creation.High << 32) | creation.Low;
    }

    public RamSharedSuspendedJobResult Wait() {
        if (completed) throw new InvalidOperationException("process custody is already closed");
        string terminationReason = null;
        while (true) {
            if (ReadRootCreationTime() != creationTime) {
                terminationReason = "root creation identity mismatch";
                break;
            }
            uint rootState = WaitForSingleObject(processHandle, 0);
            if (rootState == WAIT_FAILED) throw NativeFailure("WaitForSingleObject(process)");
            uint active = QueryActiveProcesses();
            Exception drainFailure;
            lock (outputLock) { drainFailure = readerFailure; }
            if (drainFailure != null) {
                terminationReason = "redirected-output drain failed: " + drainFailure.Message;
                break;
            }
            if (outputOverflow) {
                terminationReason = "output exceeded the bounded capture limit";
                break;
            }
            if (rootState == WAIT_OBJECT_0 && active == 0 && stdoutDone && stderrDone) break;
            if (clock.ElapsedMilliseconds >= deadlineMilliseconds) {
                terminationReason = "timed out at the monotonic deadline";
                break;
            }
            uint remaining = (uint)Math.Max(1,
                Math.Min(20, deadlineMilliseconds - (int)clock.ElapsedMilliseconds));
            uint waited = WaitForSingleObject(processHandle, remaining);
            if (waited == WAIT_FAILED) throw NativeFailure("WaitForSingleObject(deadline)");
        }

        if (terminationReason != null) {
            bool injected = injectTerminateFailure;
            if (!injected && !TerminateJobObject(jobHandle, 228u))
                throw NativeFailure("TerminateJobObject");
            if (injected) {
                bool injectedPrimaryResult = false;
                if (injectedPrimaryResult)
                    throw new InvalidOperationException("injected termination seam unexpectedly succeeded");
                if (!TerminateJobObject(jobHandle, 229u))
                    throw NativeFailure("TerminateJobObject(injected-failure cleanup)");
            }
            long cleanupDeadline = clock.ElapsedMilliseconds + 5000;
            while (QueryActiveProcesses() != 0) {
                if (clock.ElapsedMilliseconds >= cleanupDeadline)
                    throw new InvalidOperationException(label +
                        " process job did not become empty after checked termination");
                Thread.Sleep(10);
            }
            uint reaped = WaitForSingleObject(processHandle,
                (uint)Math.Max(1, cleanupDeadline - clock.ElapsedMilliseconds));
            if (reaped != WAIT_OBJECT_0)
                throw new InvalidOperationException(label +
                    " root was not reaped by retained handle after job termination");
            if (!JoinReaders(cleanupDeadline))
                throw new InvalidOperationException(label +
                    " redirected pipes did not close after empty-job proof");
            string suffix = injected
                ? " INJECTED_TERMINATE_FAILURE=OBSERVED cleanup-terminate-checked"
                : "";
            string mismatch = terminationReason == "root creation identity mismatch"
                ? " ROOT_CREATION_MISMATCH=REFUSED"
                : "";
            bool slowHashSeamEntered;
            lock (outputLock) {
                slowHashSeamEntered = Encoding.UTF8.GetString(stderr.ToArray()).IndexOf(
                    "SLOW_HASH_SEAM=ENTERED", StringComparison.Ordinal) >= 0;
            }
            string slowHashEvidence = slowHashSeamEntered
                ? " SLOW_HASH_SEAM=ENTERED"
                : "";
            string identity = mismatch + " JOB_EMPTY=1 ROOT_CREATION=" +
                creationTime.ToString(System.Globalization.CultureInfo.InvariantCulture);
            CloseCustody();
            throw new InvalidOperationException(label + " " + terminationReason + suffix +
                slowHashEvidence + identity);
        }

        uint exitCode;
        if (!GetExitCodeProcess(processHandle, out exitCode))
            throw NativeFailure("GetExitCodeProcess");
        if (exitCode == STILL_ACTIVE)
            throw new InvalidOperationException(label + " reported STILL_ACTIVE after empty-job proof");
        if (!JoinReaders(clock.ElapsedMilliseconds + 1000))
            throw new InvalidOperationException(label + " redirected readers did not reach EOF");
        byte[] stdoutBytes;
        byte[] stderrBytes;
        lock (outputLock) {
            stdoutBytes = stdout.ToArray();
            stderrBytes = stderr.ToArray();
        }
        RamSharedSuspendedJobResult result = new RamSharedSuspendedJobResult();
        result.ExitCode = unchecked((int)exitCode);
        result.Stdout = Encoding.UTF8.GetString(stdoutBytes);
        result.Stderr = Encoding.UTF8.GetString(stderrBytes);
        result.ProcessId = processId;
        result.CreationTimeUtcFileTime = creationTime;
        result.JobEmpty = true;
        CloseCustody();
        return result;
    }

    bool JoinReaders(long absoluteDeadline) {
        while ((!stdoutDone || !stderrDone) && clock.ElapsedMilliseconds < absoluteDeadline)
            Thread.Sleep(5);
        if (!stdoutDone || !stderrDone) return false;
        int remaining = (int)Math.Max(1, absoluteDeadline - clock.ElapsedMilliseconds);
        return stdoutThread.Join(remaining) && stderrThread.Join(remaining);
    }

    void CloseCustody() {
        if (QueryActiveProcesses() != 0)
            throw new InvalidOperationException(label + " refused to close a non-empty process job");
        CheckedClose(ref stdoutRead, "CloseHandle(stdout read)");
        CheckedClose(ref stderrRead, "CloseHandle(stderr read)");
        CheckedClose(ref threadHandle, "CloseHandle(thread)");
        CheckedClose(ref processHandle, "CloseHandle(process)");
        CheckedClose(ref jobHandle, "CloseHandle(job)");
        completed = true;
        clock.Stop();
    }

    public void Dispose() {
        if (completed) return;
        if (jobHandle != IntPtr.Zero) {
            if (!TerminateJobObject(jobHandle, 230))
                throw NativeFailure("TerminateJobObject(Dispose)");
            long cleanupDeadline = clock.ElapsedMilliseconds + 5000;
            while (QueryActiveProcesses() != 0) {
                if (clock.ElapsedMilliseconds >= cleanupDeadline)
                    throw new InvalidOperationException(label + " dispose could not empty the process job");
                Thread.Sleep(10);
            }
            uint reaped = WaitForSingleObject(processHandle,
                (uint)Math.Max(1, cleanupDeadline - clock.ElapsedMilliseconds));
            if (reaped != WAIT_OBJECT_0)
                throw new InvalidOperationException(label + " dispose could not reap root by handle");
            if (!JoinReaders(cleanupDeadline))
                throw new InvalidOperationException(label + " dispose could not close redirected pipes");
        }
        CloseCustody();
    }
}
'@
}

function Initialize-OwnedTreeDeletionType {
  if ($null -ne ('RamSharedOwnedTreeDeletion' -as [type])) { return }
  $installerSourceForType = [System.IO.File]::ReadAllText($installer)
  $beginMarker = '// RAMSHARED_OWNED_TREE_CSHARP_BEGIN'
  $endMarker = '// RAMSHARED_OWNED_TREE_CSHARP_END'
  $begin = $installerSourceForType.IndexOf($beginMarker, [System.StringComparison]::Ordinal)
  $end = $installerSourceForType.IndexOf($endMarker, [System.StringComparison]::Ordinal)
  if ($begin -lt 0 -or $end -le $begin) {
    throw 'installer is missing its handle-bound owned-tree deletion helper'
  }
  $begin += $beginMarker.Length
  Add-Type -TypeDefinition $installerSourceForType.Substring($begin, $end - $begin).Trim()
}

function Get-OwnedTreeIdentity([string]$Path) {
  Initialize-OwnedTreeDeletionType
  return [RamSharedOwnedTreeDeletion]::Capture($Path)
}

function Remove-OwnedTree([string]$Path, [object]$Identity) {
  Initialize-OwnedTreeDeletionType
  [RamSharedOwnedTreeDeletion]::DeleteTree($Path, $Identity)
}

function Invoke-Ps51([string]$Script, [string[]]$Arguments) {
  $effectiveArguments = @($Arguments)
  $fixtureNames = @(
    '-EvaluateCanaryFixture', '-BaselineCanaryFixture', '-EvaluateRollbackFixture',
    '-EvaluateRuntimeFixture', '-DryRunConfig', '-EvaluateExternalFailureFixture',
    '-EvaluateExternalTimeoutFixture', '-EvaluateTransactionFixture', '-InjectFailureBoundary',
    '-InjectAssignFailureFixture', '-InjectResumeFailureFixture',
    '-InjectTerminateFailureFixture', '-InjectRootCreationMismatchFixture',
    '-EvaluateDeadlineFixtureSec', '-EvaluateSlowStartupFixture',
    '-EvaluateSlowHashFixture',
    '-TransactionLockTimeoutSec', '-HoldTransactionLockMilliseconds', '-TransactionLockEvidenceFixture', '-PreflightOnly',
    '-EvaluateLiveGateFixture'
  )
  $hermetic = $false
  foreach ($fixtureName in $fixtureNames) {
    if (Test-HasArgument $effectiveArguments $fixtureName) { $hermetic = $true; break }
  }
  if ($hermetic -and ($Script -ceq $script:launcher -or $Script -ceq $script:wrapper -or
      (Split-Path -Leaf $Script) -ceq 'boot-kernel-logged.ps1')) {
    if (-not (Test-HasArgument $effectiveArguments '-FixtureRoot')) {
      $effectiveArguments += @('-FixtureRoot', $script:testRoot, '-FixtureNonce', $script:fixtureNonce)
    }
    if (-not (Test-HasArgument $effectiveArguments '-WslConfig')) {
      $effectiveArguments += @('-WslConfig', (Join-Path $script:testRoot 'default-fixture.wslconfig'))
    }
    if (-not (Test-HasArgument $effectiveArguments '-ReceiptDirectory')) {
      $effectiveArguments += @('-ReceiptDirectory', (Join-Path $script:testRoot 'default-fixture-receipts'))
    }
  }

  Initialize-SuspendedJobProcessType
  $ownedProcess = $null
  try {
    $childArguments = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $Script) + $effectiveArguments
    $argumentLine = (@($childArguments | ForEach-Object { ConvertTo-TestProcessArgument $_ }) -join ' ')
    $ownedProcess = [RamSharedSuspendedJobProcess]::Start(
      $powerShell51, $argumentLine, 45000, 2097152,
      $false, $false, $false, $false,
      'child PowerShell invocation'
    )
    $result = $ownedProcess.Wait()
    Assert-True $result.JobEmpty 'child PowerShell result lacked empty-job custody proof'
    $exitCode = $result.ExitCode
    $combined = $result.Stdout + $result.Stderr
    $output = @($combined -split "`r?`n" | Where-Object { $_.Length -gt 0 })
  } finally {
    if ($null -ne $ownedProcess) { $ownedProcess.Dispose() }
  }
  return [pscustomobject]@{ ExitCode = $exitCode; Output = @($output) }
}

function Start-WatchedPs51([string]$Script, [string[]]$Arguments) {
  Initialize-SuspendedJobProcessType
  $childArguments = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $Script) + $Arguments
  $argumentLine = (@($childArguments | ForEach-Object { ConvertTo-TestProcessArgument $_ }) -join ' ')
  $ownedProcess = [RamSharedSuspendedJobProcess]::Start(
    $powerShell51, $argumentLine, 20000, 2097152,
    $false, $false, $false, $false,
    'concurrent child PowerShell invocation'
  )
  return [pscustomobject]@{ Process = $ownedProcess }
}

function Wait-WatchedPs51([object]$Invocation, [int]$DeadlineSeconds) {
  if ($DeadlineSeconds -gt 20) {
    throw 'concurrent child deadline cannot exceed its suspended-Job deadline'
  }
  try {
    $result = $Invocation.Process.Wait()
    Assert-True $result.JobEmpty 'concurrent child lacked empty-job custody proof'
    return $result.ExitCode
  } finally {
    $Invocation.Process.Dispose()
  }
}

function New-FixtureAuthorityCases([object[]]$Descriptors) {
  $cases = New-Object 'System.Collections.Generic.List[object]'
  foreach ($descriptor in $Descriptors) {
    $cases.Add([pscustomobject]@{
      Name = 'standalone-' + $descriptor.Name
      Arguments = @($descriptor.Arguments)
    })
  }
  for ($left = 0; $left -lt $Descriptors.Count; $left++) {
    for ($right = $left + 1; $right -lt $Descriptors.Count; $right++) {
      $cases.Add([pscustomobject]@{
        Name = 'pair-' + $Descriptors[$left].Name + '-' + $Descriptors[$right].Name
        Arguments = @($Descriptors[$left].Arguments) + @($Descriptors[$right].Arguments)
      })
    }
  }
  $allArguments = @()
  foreach ($descriptor in $Descriptors) {
    $allArguments += @($descriptor.Arguments)
  }
  $cases.Add([pscustomobject]@{ Name = 'combined-all'; Arguments = $allArguments })
  return @($cases.ToArray())
}

foreach ($script in @($launcher, $wrapper, $installer)) {
  $tokens = $null
  $errors = $null
  [System.Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$errors) | Out-Null
  Assert-True ($errors.Count -eq 0) ("PowerShell 5.1 parser rejected {0}: {1}" -f $script, (($errors | ForEach-Object { $_.Message }) -join '; '))
  $bytes = [System.IO.File]::ReadAllBytes($script)
  Assert-True (-not ($bytes | Where-Object { $_ -gt 127 })) "$script contains non-ASCII source bytes"
}

$launcherSource = Get-Content -LiteralPath $launcher -Raw
$wrapperSource = Get-Content -LiteralPath $wrapper -Raw
$installerSource = Get-Content -LiteralPath $installer -Raw
$controlSource = Get-Content -LiteralPath $control -Raw
# R6_STATIC_ASSERTIONS_BEGIN
if ($R6StaticOnly.IsPresent) {
  Initialize-SuspendedJobProcessType
  $testSource = Get-Content -LiteralPath $PSCommandPath -Raw
  $beginToken = '# R6_STATIC_' + 'ASSERTIONS_BEGIN'
  $endToken = '# R6_STATIC_' + 'ASSERTIONS_END'
  $beginIndex = $testSource.IndexOf($beginToken, [System.StringComparison]::Ordinal)
  $endIndex = $testSource.IndexOf($endToken, $beginIndex + $beginToken.Length, [System.StringComparison]::Ordinal)
  Assert-True ($beginIndex -ge 0 -and $endIndex -gt $beginIndex) 'R6 static assertion markers are malformed'
  $testExecutableSource = $testSource.Remove($beginIndex, ($endIndex + $endToken.Length) - $beginIndex)
  $nativeSources = $launcherSource + $wrapperSource + $testExecutableSource
  foreach ($requiredNativePrimitive in @(
    'CreateProcessW', 'CREATE_SUSPENDED', 'SetInformationJobObject',
    'JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE', 'AssignProcessToJobObject',
    'ResumeThread', 'TerminateJobObject', 'WaitForSingleObject',
    'GetProcessTimes', 'QueryInformationJobObject'
  )) {
    Assert-True ($nativeSources.Contains($requiredNativePrimitive)) "R6 suspended-Job primitive missing: $requiredNativePrimitive"
  }
  $numericPidTerminationPattern = '(?i)' + 'task' + 'kill(?:\.exe)?\b|' + '/' + 'PID\b'
  Assert-True (-not ($nativeSources -match $numericPidTerminationPattern)) 'R6 forbids numeric-PID process termination'
  Assert-True (-not (($installerSource + $nativeSources) -match 'Remove-Item[^\r\n]*-Recurse')) 'R6 forbids recursive PowerShell path cleanup'
  foreach ($cleanupPrimitive in @('FILE_FLAG_OPEN_REPARSE_POINT', 'FILE_FLAG_BACKUP_SEMANTICS', 'SetFileInformationByHandle')) {
    Assert-True (($installerSource + $testExecutableSource).Contains($cleanupPrimitive)) "R6 handle-bound cleanup primitive missing: $cleanupPrimitive"
  }
  Assert-True ($launcherSource.Contains('$captureComplete')) 'R6 rollback capture-complete guard is missing'
  Assert-True ($launcherSource.Contains('$mutationStarted')) 'R6 rollback mutation guard is missing'
  Assert-True (($launcherSource + $installerSource).Contains('Global\RamShared')) 'R6 canonical mutex is not Global'
  Assert-True (($launcherSource + $wrapperSource + $installerSource).Contains('DOS_DEVICE_SEGMENT')) 'R6 DOS-device segment refusal is missing'
  Assert-True ($installerSource.Contains('STAGING_CUSTODY=NO_GO')) 'R6 installer staging must remain NO-GO without proven direct supervision'
  Assert-True ($testExecutableSource.Contains('[string]$SpecTest')) 'R6 independently selectable PowerShell SPEC mode parameter is missing'
  Write-Host 'R6_STATIC_PROCESS_CLEANUP_ROLLBACK_PATHS=PASS'
  exit 0
}
# R6_STATIC_ASSERTIONS_END
Assert-True ($launcherSource.Contains('root-terminated-and-reaped-by-handle-before-resume')) 'assignment failure lacks retained-handle reap evidence'
Assert-True ($launcherSource.Contains('kernelModules=')) 'launcher does not arm the modules side of the pair'
Assert-True ($launcherSource.Contains("'CANARY_WSLG_TRANSACTION'")) 'WSLg transaction is not a hard canary field'
Assert-True ($launcherSource.Contains('xdpyinfo')) 'WSLg canary does not exercise an actual display transaction'
Assert-True ($launcherSource.Contains('wsl.exe running-distro probe')) 'stopped-state gate is missing'
Assert-True ($launcherSource.Contains('unified modules artifacts are not admitted')) 'unreleased unified layout is not fail-closed'
Assert-True ($launcherSource.Contains('modules-layout.manifest')) 'launcher does not verify the sealed modules layout inventory'
Assert-True ($wrapperSource.Contains('WindowsPowerShell\v1.0\powershell.exe')) 'wrapper does not bind Windows PowerShell 5.1'
Assert-True (-not $wrapperSource.Contains('C:\wsl\boot-kernel-safe.ps1')) 'wrapper retains the stale global launcher default'
Assert-True ($controlSource.Contains('Install-BootKernelLaunchers.ps1')) 'normal apply does not install a bound launcher bundle'
Assert-True (-not $controlSource.Contains('bzImage-ramshared-latest')) 'normal control still references a mutable latest kernel'
Assert-True ($installerSource.Contains('INSTALL_IMMUTABLE_KERNEL_LAUNCHER_BUNDLE')) 'installer lacks its exact attended effect token'
Assert-True ($launcherSource.Contains('PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL')) 'launcher lacks its exact attended effect token'
Assert-True ($wrapperSource.Contains('PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL')) 'wrapper lacks its exact attended effect token'
Assert-True ($installerSource.Contains('[switch]$Run')) 'installer lacks an explicit Run switch'
Assert-True ($launcherSource.Contains('[switch]$Run')) 'launcher lacks an explicit Run switch'
Assert-True ($wrapperSource.Contains('[switch]$Run')) 'wrapper lacks an explicit Run switch'
Assert-True ($controlSource.Contains("'-ConfirmationToken' 'INSTALL_IMMUTABLE_KERNEL_LAUNCHER_BUNDLE'")) 'apply does not forward the installer gate'
Assert-True ($controlSource.Contains("'-ConfirmationToken' 'PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL'")) 'apply does not forward the promotion gate'
Assert-True (-not $launcherSource.Contains('WaitForExit()')) 'launcher contains an unbounded process wait'
Assert-True (-not $wrapperSource.Contains('WaitForExit()')) 'wrapper contains an unbounded process wait'
Assert-True (-not $launcherSource.Contains('ReadToEnd')) 'launcher contains an unbounded redirected-output drain'
Assert-True (-not $wrapperSource.Contains('ReadToEnd')) 'wrapper contains an unbounded redirected-output drain'
Assert-True ($launcherSource.Contains('Test-TransientSharingException')) 'atomic writes do not classify transient sharing failures'
Assert-True ($launcherSource.Contains('TRANSACTION_LOCK=REFUSED')) 'launcher lacks the exclusive canonical transaction-lock refusal'
Assert-True ($installerSource.Contains('Copy-SealedSource')) 'installer does not copy from a locked source handle into owned staging'
Assert-True ($installerSource.Contains('Enter-InstallLock')) 'installer lacks its exclusive canonical publication lock'
Assert-True ($controlSource.Contains('ENABLE_LIVE_MUTATION=NO_GO')) 'module enable lacks unconditional NO-GO'
Assert-True (-not ($controlSource -match '(?m)(^|\s)(sudo\s+-n\s+--\s+modprobe|modprobe)(\s|$)')) 'module enable retains a module-loader path'
Assert-True ($controlSource.IndexOf('MODULE_VHDX_PROVENANCE=REFUSED') -lt $controlSource.IndexOf('Install-BootKernelLaunchers.ps1')) 'apply does not refuse unverifiable provenance before installer use'

$fixtureNonce = [guid]::NewGuid().ToString('N')
$testRoot = Join-Path $env:TEMP ('ramshared-kernel-static-' + $fixtureNonce)
$script:fixtureNonce = $fixtureNonce
$script:testRoot = $testRoot
$script:launcher = $launcher
$script:wrapper = $wrapper
$pairRoot = Join-Path $testRoot 'pair'
$installRoot = Join-Path $testRoot 'installed'
$receiptRoot = Join-Path $testRoot 'receipts'
New-Item -ItemType Directory -Path $pairRoot, $installRoot, $receiptRoot | Out-Null
$testRootIdentity = Get-OwnedTreeIdentity $testRoot
Write-Ascii (Join-Path $testRoot '.ramshared-kernel-fixture-root') ('ramshared.kernel-fixture.v1:' + $fixtureNonce)

$cleanupProbe = Join-Path $testRoot 'owned-cleanup-probe'
$cleanupMoved = Join-Path $testRoot 'owned-cleanup-moved'
New-Item -ItemType Directory -Path $cleanupProbe | Out-Null
Write-Ascii (Join-Path $cleanupProbe 'owned.txt') 'owned-original'
$cleanupIdentity = Get-OwnedTreeIdentity $cleanupProbe
[System.IO.Directory]::Move($cleanupProbe, $cleanupMoved)
New-Item -ItemType Directory -Path $cleanupProbe | Out-Null
$replacementSentinel = Join-Path $cleanupProbe 'replacement.sentinel'
Write-Ascii $replacementSentinel 'replacement-must-survive'
$replacementSha = Get-Sha256 $replacementSentinel
$cleanupRefused = $false
try {
  Remove-OwnedTree $cleanupProbe $cleanupIdentity
} catch {
  $cleanupRefused = $_.Exception.Message -match 'identity changed|replacement was not touched'
}
Assert-True $cleanupRefused 'handle-bound cleanup accepted a renamed replacement root'
Assert-True ((Get-Sha256 $replacementSentinel) -ceq $replacementSha) 'handle-bound cleanup changed the replacement sentinel'
$cleanupIdentity.Dispose()
$cleanupMovedIdentity = Get-OwnedTreeIdentity $cleanupMoved
Remove-OwnedTree $cleanupMoved $cleanupMovedIdentity
$replacementIdentity = Get-OwnedTreeIdentity $cleanupProbe
Remove-OwnedTree $cleanupProbe $replacementIdentity

$cleanupReplacementStages = @(
  'before-root-validation',
  'before-node-validation',
  'before-enumeration',
  'before-child-open',
  'before-child-delete',
  'before-root-delete'
)
foreach ($cleanupStage in $cleanupReplacementStages) {
  $cleanupStageSlug = $cleanupStage.Replace('-', '_')
  $stageRoot = Join-Path $testRoot ('owned-stage-' + $cleanupStageSlug)
  $stageMoved = Join-Path $testRoot ('owned-stage-moved-' + $cleanupStageSlug)
  $stageSentinel = Join-Path $stageRoot 'replacement.sentinel'
  New-Item -ItemType Directory -Path $stageRoot | Out-Null
  Write-Ascii (Join-Path $stageRoot 'owned.txt') ('owned:' + $cleanupStage)
  $stageIdentity = Get-OwnedTreeIdentity $stageRoot
  $stageRefused = $false
  try {
    [RamSharedOwnedTreeDeletion]::DeleteTreeWithReplacementFixture(
      $stageRoot,
      $stageIdentity,
      $cleanupStage,
      $stageMoved,
      $stageSentinel)
  } catch {
    $stageRefused = $_.Exception.Message.Contains(
      'owned cleanup identity changed at stage=' + $cleanupStage + '; replacement was not touched')
  }
  Assert-True $stageRefused ('handle-bound cleanup did not fail closed at ' + $cleanupStage)
  Assert-True (Test-Path -LiteralPath $stageSentinel -PathType Leaf) ('cleanup replacement seam omitted sentinel at ' + $cleanupStage)
  Assert-True ((Get-Content -LiteralPath $stageSentinel -Raw) -ceq ('replacement-must-survive:' + $cleanupStage)) ('cleanup replacement sentinel changed at ' + $cleanupStage)
  $stageIdentity.Dispose()
  $stageMovedIdentity = Get-OwnedTreeIdentity $stageMoved
  Remove-OwnedTree $stageMoved $stageMovedIdentity
  $stageReplacementIdentity = Get-OwnedTreeIdentity $stageRoot
  Remove-OwnedTree $stageRoot $stageReplacementIdentity
}
Write-Host ("HANDLE_CLEANUP_REPLACEMENT_MATRIX=PASS stages=$($cleanupReplacementStages.Count)")

$release = '6.18.test-microsoft-standard-WSL2+'
$kernelPath = Join-Path $pairRoot 'kernel.bzImage'
$modulesPath = Join-Path $pairRoot 'modules.vhdx'
$layoutPath = Join-Path $pairRoot 'modules-layout.manifest'
$qemuPath = Join-Path $pairRoot 'qemu-pass.stamp'
$manifestPath = Join-Path $pairRoot 'kernel-pair.manifest'
$kernelStream = New-Object System.IO.FileStream($kernelPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
try {
  $refusalCaseCount = 0
  $kernelStream.SetLength(1048577)
} finally {
  $kernelStream.Dispose()
}
[System.IO.File]::WriteAllBytes($modulesPath, ([byte[]](255..1)))
$kernelSha = Get-Sha256 $kernelPath
$modulesSha = Get-Sha256 $modulesPath
$vermagic = "$release SMP preempt mod_unload"

function Write-PairManifest(
  [string]$Path,
  [string]$ManifestRelease,
  [string]$Layout,
  [int]$ReleaseCount,
  [int]$NestedCount,
  [string]$MinimumVersion = '2.7.12.0'
) {
  $directory = Split-Path -Parent $Path
  $caseLayoutPath = Join-Path $directory 'modules-layout.manifest'
  $caseQemuPath = Join-Path $directory 'qemu-pass.stamp'
  Write-Ascii $caseLayoutPath @"
schema=ramshared.modules-layout.v1
layout=$Layout
release=$ManifestRelease
release_directory_count=$ReleaseCount
nested_release_directory_count=$NestedCount
modules_sha256=$modulesSha
modules_size_bytes=$((Get-Item -LiteralPath $modulesPath).Length)
"@
  Write-Ascii $caseQemuPath "REL=$ManifestRelease`r`nKERNEL_SHA256=$kernelSha`r`nHEAD=abcdef1`r`nDATE=2026-08-23T00:00:00Z`r`nVALIDATE=qemu-validate.sh`r`n"
  $caseLayoutSha = Get-Sha256 $caseLayoutPath
  $caseQemuSha = Get-Sha256 $caseQemuPath
  $text = @"
schema=ramshared.kernel-pair.v1
pair_id=v1-$($kernelSha.Substring(0,16))-$($modulesSha.Substring(0,16))
release=$ManifestRelease
kernel_file=kernel.bzImage
kernel_sha256=$kernelSha
kernel_size_bytes=$((Get-Item -LiteralPath $kernelPath).Length)
modules_file=modules.vhdx
modules_sha256=$modulesSha
modules_size_bytes=$((Get-Item -LiteralPath $modulesPath).Length)
modules_layout=$Layout
layout_release_directory_count=$ReleaseCount
layout_nested_release_directory_count=$NestedCount
layout_inventory_sha256=$caseLayoutSha
module_name=ublk_drv
module_vermagic=$ManifestRelease SMP preempt mod_unload
minimum_wsl_version=$MinimumVersion
qemu_stamp_sha256=$caseQemuSha
qemu_kernel_sha256=$kernelSha
qemu_release=$ManifestRelease
"@
  Write-Ascii $Path ($text.TrimStart() -replace "`n", "`r`n")
}

Write-PairManifest $manifestPath $release 'legacy_flat_v1' 0 0
$manifestSha = Get-Sha256 $manifestPath
$layoutSha = Get-Sha256 $layoutPath
$qemuSha = Get-Sha256 $qemuPath

$runtimePath = Join-Path $testRoot 'wsl-version.txt'
Write-Ascii $runtimePath @'
WSL version: 2.7.12.0
Kernel version: 6.6.87.2-1
WSLg version: 1.0.71
Windows version: 10.0.26100.4946
'@
$localizedRuntimePath = Join-Path $testRoot 'wsl-version-localized.txt'
Write-Ascii $localizedRuntimePath @'
Versao do WSL: 2.7.12.0
Versao do kernel: 6.6.87.2-1
Versao do WSLg: 1.0.71
Versao do Windows: 10.0.26100.4946
'@

function New-CanaryPayload(
  [string]$Phase,
  [string]$BootId,
  [string]$Uname,
  [string]$ModulesState,
  [string]$ModuleVermagic,
  [string]$ModuleTree,
  [int]$DxgErrors
) {
  return @"
CANARY_SCHEMA=1
CANARY_PHASE=$Phase
CANARY_WSL_EXIT=0
CANARY_BOOT_ID=$BootId
CANARY_UNAME=$Uname
CANARY_SYSTEMD=running
CANARY_FAILED_UNITS=none
CANARY_DXG_NODE=char
CANARY_DXG_DEV_T=e7:0
CANARY_DXG_COUNT=1
CANARY_XWAYLAND_COUNT_BEFORE=0
CANARY_XWAYLAND_COUNT_AFTER=1
CANARY_WSLG_TRANSACTION=ok
CANARY_GPU_DRIVER=560.94
CANARY_DXG_PROBE=ok
CANARY_MODULES=$ModulesState
CANARY_MODULE_VERMAGIC=$ModuleVermagic
CANARY_MODULE_TREE=$ModuleTree
CANARY_DISTRO_ID=ubuntu
CANARY_DISTRO_VERSION_ID=24.04
CANARY_DMESG_READABLE=1
CANARY_DMESG_SHA256=$('d' * 64)
CANARY_DXG_FORTIFY_WARNINGS=0
CANARY_WAIT_FOR_BOOT_FAILURES=0
CANARY_JOURNAL_UNCLEAN=0
CANARY_P9_CANCELLED=0
CANARY_KERNEL_FATALS=0
CANARY_DXG_QUERY_ERRORS=$DxgErrors
"@
}

$baselinePath = Join-Path $testRoot 'baseline.txt'
$candidatePath = Join-Path $testRoot 'candidate.txt'
$rollbackPath = Join-Path $testRoot 'rollback.txt'
$baseline = New-CanaryPayload 'bundled' '11111111-2222-4333-8444-555555555555' '6.6.87.2-microsoft-standard-WSL2' 'missing' 'unavailable' 'failed' 3
$candidate = New-CanaryPayload 'candidate' 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee' $release 'ok' $vermagic 'ok' 2
$rollback = New-CanaryPayload 'bundled' '99999999-8888-4777-8666-555555555555' '6.6.87.2-microsoft-standard-WSL2' 'missing' 'unavailable' 'failed' 2
Write-Ascii $baselinePath $baseline
Write-Ascii $candidatePath $candidate
Write-Ascii $rollbackPath $rollback

$commonArgs = @(
  '-KernelPairManifest', $manifestPath,
  '-ExpectedKernelManifestSha256', $manifestSha,
  '-EvaluateRuntimeFixture', $runtimePath,
  '-BaselineCanaryFixture', $baselinePath,
  '-EvaluateCanaryFixture', $candidatePath,
  '-WslConfig', (Join-Path $testRoot 'gate-safe.wslconfig'),
  '-ReceiptDirectory', (Join-Path $testRoot 'gate-safe-receipts')
)

try {
  $safeSentinel = Join-Path $testRoot 'safe-gate-sentinel.txt'
  Write-Ascii $safeSentinel 'safe-gate-sentinel'
  $safeSentinelSha = Get-Sha256 $safeSentinel
  $safeConfig = Join-Path $testRoot 'gate-safe.wslconfig'
  Write-Ascii $safeConfig "[wsl2]`r`nmemory=8GB`r`n"
  $safeReceiptRoot = Join-Path $testRoot 'gate-safe-receipts'
  $safeConfigState = Get-PathState $safeConfig
  $safeReceiptState = Get-PathState $safeReceiptRoot
  $safeGateBase = @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-WslConfig', $safeConfig,
    '-ReceiptDirectory', $safeReceiptRoot
  )
  $promotionToken = 'PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL'
  if ($FocusedConfinementOnly.IsPresent) {
    $focusedOutside = Join-Path ([System.IO.Path]::GetTempPath()) ('ramshared-outside-' + $fixtureNonce + '.focused')
    $focusedCases = @(
      [pscustomobject]@{ Name = 'WslConfig'; Arguments = @('-EvaluateRuntimeFixture', $runtimePath, '-PreflightOnly', '-WslConfig', $focusedOutside, '-ReceiptDirectory', $safeReceiptRoot) },
      [pscustomobject]@{ Name = 'ReceiptDirectory'; Arguments = @('-EvaluateRuntimeFixture', $runtimePath, '-PreflightOnly', '-WslConfig', $safeConfig, '-ReceiptDirectory', $focusedOutside) },
      [pscustomobject]@{ Name = 'EvaluateRuntimeFixture'; Arguments = @('-EvaluateRuntimeFixture', $focusedOutside, '-PreflightOnly', '-WslConfig', $safeConfig, '-ReceiptDirectory', $safeReceiptRoot) }
    )
    foreach ($focusedCase in $focusedCases) {
      $before = Get-PathState $testRoot
      $focusedResult = Invoke-Ps51 $launcher (@(
        '-KernelPairManifest', $manifestPath,
        '-ExpectedKernelManifestSha256', $manifestSha
      ) + $focusedCase.Arguments)
      Assert-True ($focusedResult.ExitCode -eq 2) 'focused confinement probe did not exit 2'
      Assert-True (($focusedResult.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) ('focused confinement probe missed path boundary: ' + ($focusedResult.Output -join ' | '))
      Assert-True (($focusedResult.Output -join "`n").Contains($focusedCase.Name)) ('focused confinement probe did not name ' + $focusedCase.Name + ': ' + ($focusedResult.Output -join ' | '))
      Assert-True (-not (Test-Path -LiteralPath $focusedOutside)) 'focused confinement probe wrote outside FixtureRoot'
      Assert-PathState $testRoot $before 'focused confinement probe changed FixtureRoot'
    }
    $focusedJunctionTarget = Join-Path $testRoot 'focused-junction-target'
    $focusedJunction = Join-Path $testRoot 'focused-junction'
    New-Item -ItemType Directory -Path $focusedJunctionTarget | Out-Null
    New-Item -ItemType Junction -Path $focusedJunction -Target $focusedJunctionTarget | Out-Null
    try {
      $focusedJunctionState = Get-PathState $focusedJunctionTarget
      $focusedReparseResult = Invoke-Ps51 $launcher @(
        '-KernelPairManifest', $manifestPath,
        '-ExpectedKernelManifestSha256', $manifestSha,
        '-EvaluateRuntimeFixture', $runtimePath,
        '-PreflightOnly',
        '-WslConfig', (Join-Path $focusedJunction 'target.wslconfig'),
        '-ReceiptDirectory', $safeReceiptRoot
      )
      Assert-True ($focusedReparseResult.ExitCode -eq 2) 'focused reparse-ancestor probe did not exit 2'
      Assert-True (($focusedReparseResult.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) ('focused reparse-ancestor probe missed path boundary: ' + ($focusedReparseResult.Output -join ' | '))
      Assert-True (($focusedReparseResult.Output -join "`n").Contains('reparse-point ancestor')) ('focused reparse-ancestor probe did not identify the reparse point: ' + ($focusedReparseResult.Output -join ' | '))
      Assert-PathState $focusedJunctionTarget $focusedJunctionState 'focused reparse-ancestor probe changed its target'
    } finally {
      Remove-Item -LiteralPath $focusedJunction -Force
      Remove-Item -LiteralPath $focusedJunctionTarget -Force
    }
    $before = Get-PathState $testRoot
    $combinationResult = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateRuntimeFixture', $runtimePath
    )
    Assert-True ($combinationResult.ExitCode -eq 2) 'focused companion-only probe did not exit 2'
    Assert-True (($combinationResult.Output -join "`n").Contains('FIXTURE_COMBINATION=REFUSED')) 'focused path change weakened companion-only refusal'
    Assert-PathState $testRoot $before 'focused companion-only refusal changed FixtureRoot'
    Write-Host 'FOCUSED_CONFINEMENT=PASS probes=3 reparse=1 combination=preserved'
    exit 0
  }
  if ($FocusedTimeoutOnly.IsPresent) {
    $focusedTimeoutPid = Join-Path $testRoot 'focused-timeout-child.pid'
    $focusedTimeout = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateExternalTimeoutFixture', $focusedTimeoutPid
    )
    Assert-True ($focusedTimeout.ExitCode -eq 0) ('focused timeout-tree fixture failed: ' + ($focusedTimeout.Output -join ' | '))
    Assert-True (($focusedTimeout.Output -join "`n").Contains('EXTERNAL_TIMEOUT_FIXTURE=PASS')) 'focused timeout-tree fixture lacked its exact pass marker'

    $focusedAssignPid = Join-Path $testRoot 'focused-assign-must-remain-absent.pid'
    $focusedAssign = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateExternalTimeoutFixture', $focusedAssignPid,
      '-InjectAssignFailureFixture'
    )
    Assert-True ($focusedAssign.ExitCode -eq 0) ('focused assignment-failure fixture failed: ' + ($focusedAssign.Output -join ' | '))
    Assert-True (($focusedAssign.Output -join "`n").Contains('PROCESS_ASSIGN_FAILURE_FIXTURE=PASS root-reaped-by-handle-before-resume')) 'focused assignment seam lacked handle-reap proof'
    Assert-True (-not (Test-Path -LiteralPath $focusedAssignPid)) 'focused assignment seam resumed a root or descendant'

    $focusedResumePid = Join-Path $testRoot 'focused-resume-must-remain-absent.pid'
    $focusedResume = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateExternalTimeoutFixture', $focusedResumePid,
      '-InjectResumeFailureFixture'
    )
    Assert-True ($focusedResume.ExitCode -eq 0) ('focused resume-failure fixture failed: ' + ($focusedResume.Output -join ' | '))
    Assert-True (($focusedResume.Output -join "`n").Contains('PROCESS_RESUME_FAILURE_FIXTURE=PASS job-empty-root-reaped-before-resume')) 'focused resume seam lacked empty-job handle-reap proof'
    Assert-True (-not (Test-Path -LiteralPath $focusedResumePid)) 'focused resume seam ran child code'

    $focusedTerminatePid = Join-Path $testRoot 'focused-terminate-child.pid'
    $focusedTerminate = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateExternalTimeoutFixture', $focusedTerminatePid,
      '-InjectTerminateFailureFixture'
    )
    Assert-True ($focusedTerminate.ExitCode -eq 0) ('focused termination-failure fixture failed: ' + ($focusedTerminate.Output -join ' | '))
    Assert-True (($focusedTerminate.Output -join "`n").Contains('EXTERNAL_TIMEOUT_FIXTURE=PASS JOB_EMPTY=1 HANDLE_IDENTITY=PROVEN')) 'focused termination seam lacked empty-job proof'
    Assert-True (Test-Path -LiteralPath $focusedTerminatePid -PathType Leaf) 'focused termination seam did not resume its descendant probe'

    $focusedCreationPid = Join-Path $testRoot 'focused-creation-child.pid'
    $focusedCreation = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateExternalTimeoutFixture', $focusedCreationPid,
      '-InjectRootCreationMismatchFixture'
    )
    Assert-True ($focusedCreation.ExitCode -eq 0) ('focused creation-mismatch fixture failed: ' + ($focusedCreation.Output -join ' | '))
    Assert-True (($focusedCreation.Output -join "`n").Contains('PROCESS_CREATION_IDENTITY_FIXTURE=PASS mismatch-refused-job-empty')) 'focused creation mismatch lacked exact empty-job refusal'

    $focusedHarnessSource = [System.IO.File]::ReadAllText($PSCommandPath)
    $focusedStaticBegin = $focusedHarnessSource.IndexOf('# R6_STATIC_' + 'ASSERTIONS_BEGIN', [System.StringComparison]::Ordinal)
    $focusedStaticEndToken = '# R6_STATIC_' + 'ASSERTIONS_END'
    $focusedStaticEnd = $focusedHarnessSource.IndexOf($focusedStaticEndToken, $focusedStaticBegin + 1, [System.StringComparison]::Ordinal)
    Assert-True ($focusedStaticBegin -ge 0 -and $focusedStaticEnd -gt $focusedStaticBegin) 'focused process-custody source markers are malformed'
    $focusedHarnessExecutable = $focusedHarnessSource.Remove($focusedStaticBegin, ($focusedStaticEnd + $focusedStaticEndToken.Length) - $focusedStaticBegin)
    $numericPidTerminationPattern = '(?i)' + 'task' + 'kill(?:\.exe)?\b|' + '/' + 'PID\b'
    Assert-True (-not (($launcherSource + $wrapperSource + $installerSource + $focusedHarnessExecutable) -match $numericPidTerminationPattern)) 'focused process custody retains a numeric-PID kill path'
    Write-Host 'FOCUSED_TIMEOUT_TREE=PASS descendant_pipe=bounded assignment=handle-reaped resume=job-empty terminate=fallback-checked creation=mismatch-refused'
    exit 0
  }
  if ($FocusedDeadlineOnly.IsPresent) {
    $startupBefore = Get-PathState $testRoot
    $startupDeadline = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateRuntimeFixture', $runtimePath,
      '-PreflightOnly',
      '-EvaluateDeadlineFixtureSec', '1',
      '-EvaluateSlowStartupFixture'
    )
    $startupText = $startupDeadline.Output -join "`n"
    Assert-True ($startupDeadline.ExitCode -eq 2) ('pre-child deadline fixture did not refuse exactly: ' + $startupText)
    Assert-True ($startupText.Contains('PRE_CHILD_DEADLINE=REFUSED NO_CHILD_CREATED=1')) ('pre-child deadline fixture lacked no-child proof: ' + $startupText)
    Assert-PathState $testRoot $startupBefore 'pre-child deadline fixture changed FixtureRoot'

    $slowHashBefore = Get-PathState $testRoot
    $slowHashClock = [System.Diagnostics.Stopwatch]::StartNew()
    $slowHash = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateRuntimeFixture', $runtimePath,
      '-PreflightOnly',
      '-EvaluateDeadlineFixtureSec', '5',
      '-EvaluateSlowHashFixture'
    )
    $slowHashClock.Stop()
    $slowHashText = $slowHash.Output -join "`n"
    Assert-True ($slowHash.ExitCode -ne 0) 'slow-hash fixture escaped its outer deadline'
    Assert-True ($slowHashText.Contains('SLOW_HASH_SEAM=ENTERED')) ('slow-hash fixture never entered its blocked-I/O seam: ' + $slowHashText)
    Assert-True ($slowHashText -match '(?s)timed out.*JOB_EMPTY=1.*ROOT_CREATION=') ('slow-hash fixture lacked checked empty-job timeout proof: ' + $slowHashText)
    Assert-True ($slowHashClock.Elapsed.TotalSeconds -lt 15) 'slow-hash fixture exceeded its bounded outer cleanup window'
    Assert-PathState $testRoot $slowHashBefore 'slow-hash cancellation changed FixtureRoot'
    Write-Host 'FOCUSED_DEADLINE=PASS pre_child=no-child slow_hash=job-empty'
    exit 0
  }
  $safeRefusals = @(
    [pscustomobject]@{ Name = 'default'; Extra = @() },
    [pscustomobject]@{ Name = 'run-missing-token'; Extra = @('-Run') },
    [pscustomobject]@{ Name = 'run-blank-token'; Extra = @('-Run', '-ConfirmationToken', '   ') },
    [pscustomobject]@{ Name = 'run-malformed-token'; Extra = @('-Run', '-ConfirmationToken', 'PROMOTE WSL') },
    [pscustomobject]@{ Name = 'run-wrong-token'; Extra = @('-Run', '-ConfirmationToken', 'PROMOTE_WSL_KERNEL') },
    [pscustomobject]@{ Name = 'run-wrong-case-token'; Extra = @('-Run', '-ConfirmationToken', $promotionToken.ToLowerInvariant()) },
    [pscustomobject]@{ Name = 'token-without-run'; Extra = @('-ConfirmationToken', $promotionToken) }
  )
  foreach ($case in $safeRefusals) {
    $result = Invoke-Ps51 $launcher ($safeGateBase + $case.Extra)
    Assert-True ($result.ExitCode -ne 0) "launcher gate case $($case.Name) was accepted"
    Assert-True (($result.Output -join "`n").Contains('LIVE_GATE=REFUSED')) "launcher gate case $($case.Name) did not report gate refusal"
    Assert-PathState $safeConfig $safeConfigState "launcher gate case $($case.Name) changed the config sentinel"
    Assert-PathState $safeReceiptRoot $safeReceiptState "launcher gate case $($case.Name) changed the receipt root"
    Assert-True ((Get-Sha256 $safeSentinel) -ceq $safeSentinelSha) "launcher gate case $($case.Name) changed the sentinel hash"
    $refusalCaseCount += 1
  }
  $safeGatePass = Invoke-Ps51 $launcher ($safeGateBase + @('-Run', '-ConfirmationToken', $promotionToken))
  Assert-True ($safeGatePass.ExitCode -ne 0) 'launcher exact live gate unexpectedly promoted a kernel'
  Assert-True (-not (($safeGatePass.Output -join "`n").Contains('LIVE_GATE=REFUSED'))) 'launcher exact live gate was not recognized'
  Assert-PathState $safeConfig $safeConfigState 'launcher exact gate path refusal changed the config sentinel'
  Assert-PathState $safeReceiptRoot $safeReceiptState 'launcher exact gate path refusal changed the receipt root'

  $authorityTimeoutPid = Join-Path $testRoot 'authority-timeout-child.pid'
  $fixtureDescriptors = @(
    [pscustomobject]@{ Name = 'candidate'; Arguments = @('-EvaluateCanaryFixture', $candidatePath) },
    [pscustomobject]@{ Name = 'baseline'; Arguments = @('-BaselineCanaryFixture', $baselinePath) },
    [pscustomobject]@{ Name = 'rollback'; Arguments = @('-EvaluateRollbackFixture', $rollbackPath) },
    [pscustomobject]@{ Name = 'runtime'; Arguments = @('-EvaluateRuntimeFixture', $runtimePath) },
    [pscustomobject]@{ Name = 'dry-config'; Arguments = @('-DryRunConfig', $safeConfig) },
    [pscustomobject]@{ Name = 'external-failure'; Arguments = @('-EvaluateExternalFailureFixture') },
    [pscustomobject]@{ Name = 'external-timeout'; Arguments = @('-EvaluateExternalTimeoutFixture', $authorityTimeoutPid) },
    [pscustomobject]@{ Name = 'transaction'; Arguments = @('-EvaluateTransactionFixture', $safeConfig) },
    [pscustomobject]@{ Name = 'failure-boundary'; Arguments = @('-InjectFailureBoundary', 'after_snapshot') },
    [pscustomobject]@{ Name = 'lock-timeout'; Arguments = @('-TransactionLockTimeoutSec', '1') },
    [pscustomobject]@{ Name = 'lock-hold'; Arguments = @('-HoldTransactionLockMilliseconds', '1') },
    [pscustomobject]@{ Name = 'lock-evidence'; Arguments = @('-TransactionLockEvidenceFixture', (Join-Path $testRoot 'lock-evidence.txt')) },
    [pscustomobject]@{ Name = 'preflight'; Arguments = @('-PreflightOnly') }
  )
  $safeFixtureAuthorityBase = @(
    '-KernelPairManifest', (Join-Path $testRoot 'fixture-authority-must-not-read.manifest'),
    '-ExpectedKernelManifestSha256', ('0' * 64),
    '-WslConfig', $safeConfig,
    '-ReceiptDirectory', $safeReceiptRoot
  )
  $fixtureAuthorityCases = @(New-FixtureAuthorityCases $fixtureDescriptors)
  $fixtureAuthorityCases += [pscustomobject]@{
    Name = 'gate-fixture-conflict-runtime'
    Arguments = @('-EvaluateLiveGateFixture', '-EvaluateRuntimeFixture', $runtimePath)
  }
  foreach ($case in $fixtureAuthorityCases) {
    $before = Get-PathState $testRoot
    $result = Invoke-Ps51 $launcher ($safeFixtureAuthorityBase + $case.Arguments + @('-Run', '-ConfirmationToken', $promotionToken))
    Assert-True ($result.ExitCode -eq 2) "launcher fixture authority case $($case.Name) did not exit 2"
    Assert-True (($result.Output -join "`n").Contains('FIXTURE_LIVE_AUTHORITY=REFUSED')) "launcher fixture authority case $($case.Name) did not refuse before validation"
    Assert-PathState $testRoot $before "launcher fixture authority case $($case.Name) changed the temporary root"
    Assert-True ((Get-Sha256 $safeSentinel) -ceq $safeSentinelSha) "launcher fixture authority case $($case.Name) changed the sentinel hash"
    $refusalCaseCount += 1
  }

  $additionalCustodyAuthorityCases = @(
    [pscustomobject]@{ Name = 'assign-seam'; Arguments = @('-EvaluateExternalTimeoutFixture', $authorityTimeoutPid, '-InjectAssignFailureFixture') },
    [pscustomobject]@{ Name = 'resume-seam'; Arguments = @('-EvaluateExternalTimeoutFixture', $authorityTimeoutPid, '-InjectResumeFailureFixture') },
    [pscustomobject]@{ Name = 'terminate-seam'; Arguments = @('-EvaluateExternalTimeoutFixture', $authorityTimeoutPid, '-InjectTerminateFailureFixture') },
    [pscustomobject]@{ Name = 'creation-seam'; Arguments = @('-EvaluateExternalTimeoutFixture', $authorityTimeoutPid, '-InjectRootCreationMismatchFixture') },
    [pscustomobject]@{ Name = 'startup-deadline'; Arguments = @('-EvaluateRuntimeFixture', $runtimePath, '-PreflightOnly', '-EvaluateDeadlineFixtureSec', '1', '-EvaluateSlowStartupFixture') },
    [pscustomobject]@{ Name = 'hash-deadline'; Arguments = @('-EvaluateRuntimeFixture', $runtimePath, '-PreflightOnly', '-EvaluateDeadlineFixtureSec', '5', '-EvaluateSlowHashFixture') }
  )
  foreach ($case in $additionalCustodyAuthorityCases) {
    $before = Get-PathState $testRoot
    $result = Invoke-Ps51 $launcher ($safeFixtureAuthorityBase + $case.Arguments + @('-Run', '-ConfirmationToken', $promotionToken))
    Assert-True ($result.ExitCode -eq 2) "launcher additional custody authority case $($case.Name) did not exit 2"
    Assert-True (($result.Output -join "`n").Contains('FIXTURE_LIVE_AUTHORITY=REFUSED')) "launcher additional custody authority case $($case.Name) did not refuse before work"
    Assert-PathState $testRoot $before "launcher additional custody authority case $($case.Name) changed FixtureRoot"
    $refusalCaseCount++
  }

  $fixtureCredentialCases = @(
    [pscustomobject]@{ Name = 'run-missing-token'; Extra = @('-Run'); Marker = 'FIXTURE_LIVE_AUTHORITY=REFUSED' },
    [pscustomobject]@{ Name = 'run-blank-token'; Extra = @('-Run', '-ConfirmationToken', '   '); Marker = 'FIXTURE_LIVE_AUTHORITY=REFUSED' },
    [pscustomobject]@{ Name = 'run-wrong-token'; Extra = @('-Run', '-ConfirmationToken', 'PROMOTE_WSL_KERNEL'); Marker = 'FIXTURE_LIVE_AUTHORITY=REFUSED' },
    [pscustomobject]@{ Name = 'token-without-run'; Extra = @('-ConfirmationToken', $promotionToken); Marker = 'FIXTURE_LIVE_AUTHORITY=REFUSED' },
    [pscustomobject]@{ Name = 'whitespace-token-without-run'; Extra = @('-ConfirmationToken', '   '); Marker = 'FIXTURE_LIVE_AUTHORITY=REFUSED' },
    [pscustomobject]@{ Name = 'duplicate-token'; Extra = @('-Run', '-ConfirmationToken', $promotionToken, '-ConfirmationToken', $promotionToken); Marker = '' }
  )
  foreach ($case in $fixtureCredentialCases) {
    $before = Get-PathState $testRoot
    $result = Invoke-Ps51 $launcher ($safeFixtureAuthorityBase + @('-EvaluateRuntimeFixture', $runtimePath) + $case.Extra)
    Assert-True ($result.ExitCode -ne 0) "launcher fixture credential case $($case.Name) was accepted"
    if (-not [string]::IsNullOrWhiteSpace($case.Marker)) {
      Assert-True (($result.Output -join "`n").Contains($case.Marker)) "launcher fixture credential case $($case.Name) lacked fixture refusal"
    }
    Assert-PathState $testRoot $before "launcher fixture credential case $($case.Name) changed the temporary root"
    $refusalCaseCount += 1
  }

  foreach ($descriptor in @($fixtureDescriptors | Where-Object { $_.Name -in @('baseline', 'rollback', 'runtime') })) {
    $before = Get-PathState $testRoot
    $result = Invoke-Ps51 $launcher ($safeFixtureAuthorityBase + $descriptor.Arguments)
    Assert-True ($result.ExitCode -eq 2) "launcher companion-only fixture $($descriptor.Name) did not exit 2"
    Assert-True (($result.Output -join "`n").Contains('FIXTURE_COMBINATION=REFUSED')) "launcher companion-only fixture $($descriptor.Name) did not refuse before validation"
    Assert-PathState $testRoot $before "launcher companion-only fixture $($descriptor.Name) changed the temporary root"
    $refusalCaseCount += 1
  }

  $safeFixtureAuthority = Invoke-Ps51 $launcher ($commonArgs + @('-Run', '-ConfirmationToken', $promotionToken))
  Assert-True ($safeFixtureAuthority.ExitCode -ne 0) 'launcher fixture accepted live authority'
  Assert-True (($safeFixtureAuthority.Output -join "`n").Contains('FIXTURE_LIVE_AUTHORITY=REFUSED')) 'launcher fixture did not report live-authority refusal'
  Assert-PathState $safeConfig $safeConfigState 'launcher fixture live-authority refusal changed the config sentinel'
  Assert-PathState $safeReceiptRoot $safeReceiptState 'launcher fixture live-authority refusal changed the receipt root'
  Assert-True ((Get-Sha256 $safeSentinel) -ceq $safeSentinelSha) 'launcher fixture live-authority refusal changed the sentinel hash'

  foreach ($preflightArgs in @(
    @('-PreflightOnly'),
    @('-PreflightOnly', '-EvaluateRuntimeFixture', '   ')
  )) {
    $before = Get-PathState $testRoot
    $preflightRefusal = Invoke-Ps51 $launcher (@(
      '-KernelPairManifest', (Join-Path $testRoot 'preflight-must-not-read.manifest'),
      '-ExpectedKernelManifestSha256', ('0' * 64)
    ) + $preflightArgs)
    Assert-True ($preflightRefusal.ExitCode -eq 2) 'preflight without a usable runtime fixture did not exit 2'
    Assert-True (($preflightRefusal.Output -join "`n").Contains('PREFLIGHT_RUNTIME_FIXTURE=REFUSED')) 'preflight missing-runtime refusal was not early and exact'
    Assert-PathState $testRoot $before 'preflight missing-runtime refusal changed the fixture root'
    $refusalCaseCount += 1
  }

  $outsidePath = Join-Path ([System.IO.Path]::GetTempPath()) ('ramshared-outside-' + $fixtureNonce + '.sentinel')
  Assert-True (-not (Test-Path -LiteralPath $outsidePath)) 'outside-path fixture unexpectedly exists'
  $confinedPathCases = @(
    [pscustomobject]@{ Name = 'config-outside'; Extra = @('-EvaluateRuntimeFixture', $runtimePath, '-PreflightOnly', '-WslConfig', $outsidePath, '-ReceiptDirectory', $safeReceiptRoot) },
    [pscustomobject]@{ Name = 'receipt-outside'; Extra = @('-EvaluateRuntimeFixture', $runtimePath, '-PreflightOnly', '-WslConfig', $safeConfig, '-ReceiptDirectory', $outsidePath) },
    [pscustomobject]@{ Name = 'runtime-outside'; Extra = @('-EvaluateRuntimeFixture', $outsidePath, '-PreflightOnly', '-WslConfig', $safeConfig, '-ReceiptDirectory', $safeReceiptRoot) },
    [pscustomobject]@{ Name = 'dry-config-outside'; Extra = @('-EvaluateRuntimeFixture', $runtimePath, '-DryRunConfig', $outsidePath, '-WslConfig', $safeConfig, '-ReceiptDirectory', $safeReceiptRoot) },
    [pscustomobject]@{ Name = 'pid-outside'; Extra = @('-EvaluateExternalTimeoutFixture', $outsidePath, '-WslConfig', $safeConfig, '-ReceiptDirectory', $safeReceiptRoot) }
  )
  foreach ($pathCase in $confinedPathCases) {
    $before = Get-PathState $testRoot
    $pathRefusal = Invoke-Ps51 $launcher (@(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha
    ) + $pathCase.Extra)
    Assert-True ($pathRefusal.ExitCode -eq 2) "fixture path case $($pathCase.Name) did not exit 2"
    Assert-True (($pathRefusal.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) "fixture path case $($pathCase.Name) lacked its refusal"
    Assert-True (-not (Test-Path -LiteralPath $outsidePath)) "fixture path case $($pathCase.Name) wrote outside FixtureRoot"
    Assert-PathState $testRoot $before "fixture path case $($pathCase.Name) changed FixtureRoot"
    $refusalCaseCount += 1
  }

  $hostilePathCases = @(
    '\\fixture.invalid\share\target',
    '\\?\C:\fixture\target',
    ($testRoot + '\..\escape'),
    ($testRoot + '\stream:ads'),
    'C:\PROGRA~1\fixture',
    ($testRoot + '\CON.txt'),
    ($testRoot + '\prn.log'),
    ($testRoot + '\CLOCK$.txt'),
    ($testRoot + '\com1.fixture'),
    ($testRoot + '\LPT9.bin'),
    ($testRoot + '\trailing-dot.'),
    ($testRoot + '\trailing-space '),
    ($testRoot + '\invalid<name'),
    ($testRoot + '\control-' + [char]1 + '-name')
  )
  foreach ($hostilePath in $hostilePathCases) {
    $before = Get-PathState $testRoot
    $pathRefusal = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateRuntimeFixture', $runtimePath,
      '-PreflightOnly',
      '-WslConfig', $hostilePath,
      '-ReceiptDirectory', $safeReceiptRoot
    )
    Assert-True ($pathRefusal.ExitCode -eq 2) 'hostile path did not exit 2'
    Assert-True (($pathRefusal.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) 'hostile path lacked its fixture refusal'
    Assert-PathState $testRoot $before 'hostile path refusal changed FixtureRoot'
    $refusalCaseCount += 1
  }

  $markerPath = Join-Path $testRoot '.ramshared-kernel-fixture-root'
  $markerHardLink = Join-Path $testRoot 'fixture-root-marker-hardlink'
  New-Item -ItemType HardLink -Path $markerHardLink -Target $markerPath | Out-Null
  try {
    $hardLinkRefusal = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateExternalFailureFixture'
    )
    Assert-True ($hardLinkRefusal.ExitCode -eq 2) 'hard-linked fixture marker was accepted'
    Assert-True (($hardLinkRefusal.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) 'hard-linked fixture marker lacked its refusal'
    Assert-True ((Get-Sha256 $safeSentinel) -ceq $safeSentinelSha) 'hard-linked marker refusal changed the sentinel'
    $refusalCaseCount += 1
  } finally {
    Remove-Item -LiteralPath $markerHardLink -Force
  }

  $junctionTarget = Join-Path $testRoot 'junction-target'
  $junctionPath = Join-Path $testRoot 'junction-ancestor'
  New-Item -ItemType Directory -Path $junctionTarget | Out-Null
  New-Item -ItemType Junction -Path $junctionPath -Target $junctionTarget | Out-Null
  try {
    $junctionTargetState = Get-PathState $junctionTarget
    $junctionRefusal = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateRuntimeFixture', $runtimePath,
      '-PreflightOnly',
      '-WslConfig', (Join-Path $junctionPath 'target.wslconfig'),
      '-ReceiptDirectory', $safeReceiptRoot
    )
    Assert-True ($junctionRefusal.ExitCode -eq 2) 'reparse-point ancestor was accepted'
    Assert-True (($junctionRefusal.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) 'reparse ancestor lacked its refusal'
    Assert-PathState $junctionTarget $junctionTargetState 'reparse ancestor refusal mutated its target'
    $refusalCaseCount += 1
  } finally {
    Remove-Item -LiteralPath $junctionPath -Force
    Remove-Item -LiteralPath $junctionTarget -Force
  }

  $rootCredentialCases = @(
    [pscustomobject]@{ Name = 'missing-nonce'; Extra = @('-FixtureRoot', $testRoot) },
    [pscustomobject]@{ Name = 'wrong-nonce'; Extra = @('-FixtureRoot', $testRoot, '-FixtureNonce', ('0' * 32)) },
    [pscustomobject]@{ Name = 'wrong-case-root'; Extra = @('-FixtureRoot', $testRoot.ToUpperInvariant(), '-FixtureNonce', $fixtureNonce) }
  )
  foreach ($rootCase in $rootCredentialCases) {
    $before = Get-PathState $testRoot
    $rootRefusal = Invoke-Ps51 $launcher ($commonArgs + $rootCase.Extra)
    Assert-True ($rootRefusal.ExitCode -eq 2) "fixture root credential case $($rootCase.Name) did not exit 2"
    Assert-True (($rootRefusal.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) "fixture root credential case $($rootCase.Name) lacked its refusal"
    Assert-PathState $testRoot $before "fixture root credential case $($rootCase.Name) changed FixtureRoot"
    $refusalCaseCount += 1
  }

  $good = Invoke-Ps51 $launcher $commonArgs
  Assert-True ($good.ExitCode -eq 0) ('valid canary transaction failed: ' + ($good.Output -join ' | '))
  Assert-PathState $safeConfig $safeConfigState 'legitimate canary fixture changed the configured host target'
  Assert-PathState $safeReceiptRoot $safeReceiptState 'legitimate canary fixture changed the receipt target'
  Assert-True ((Get-Sha256 $safeSentinel) -ceq $safeSentinelSha) 'legitimate canary fixture changed the sentinel hash'

  $goodRollback = Invoke-Ps51 $launcher ($commonArgs + @('-EvaluateRollbackFixture', $rollbackPath))
  Assert-True ($goodRollback.ExitCode -eq 0) ('valid bundled rollback identity failed: ' + ($goodRollback.Output -join ' | '))

  $wrongRollbackKernelPath = Join-Path $testRoot 'rollback-wrong-kernel.txt'
  Write-Ascii $wrongRollbackKernelPath ($rollback.Replace('CANARY_UNAME=6.6.87.2-microsoft-standard-WSL2', "CANARY_UNAME=$release"))
  $wrongRollbackKernel = Invoke-Ps51 $launcher ($commonArgs + @('-EvaluateRollbackFixture', $wrongRollbackKernelPath))
  Assert-True ($wrongRollbackKernel.ExitCode -ne 0) 'rollback accepted a kernel identity different from the bundled baseline'

  $staleRollbackBootPath = Join-Path $testRoot 'rollback-stale-boot.txt'
  Write-Ascii $staleRollbackBootPath ($rollback.Replace('99999999-8888-4777-8666-555555555555', 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'))
  $staleRollbackBoot = Invoke-Ps51 $launcher ($commonArgs + @('-EvaluateRollbackFixture', $staleRollbackBootPath))
  Assert-True ($staleRollbackBoot.ExitCode -ne 0) 'rollback accepted the candidate boot ID'

  $localizedRuntime = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateRuntimeFixture', $localizedRuntimePath,
    '-PreflightOnly'
  )
  Assert-True ($localizedRuntime.ExitCode -eq 0) ('localized WSL version labels failed: ' + ($localizedRuntime.Output -join ' | '))

  $strictPath = Join-Path $testRoot 'candidate-unknown-key.txt'
  Write-Ascii $strictPath ($candidate + "CANARY_UNKNOWN=1`r`n")
  $strict = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateRuntimeFixture', $runtimePath,
    '-BaselineCanaryFixture', $baselinePath,
    '-EvaluateCanaryFixture', $strictPath
  )
  Assert-True ($strict.ExitCode -ne 0) 'strict canary parser accepted an unknown key'

  $wslgPath = Join-Path $testRoot 'candidate-wslg-failed.txt'
  Write-Ascii $wslgPath ($candidate.Replace('CANARY_WSLG_TRANSACTION=ok', 'CANARY_WSLG_TRANSACTION=failed'))
  $wslg = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateRuntimeFixture', $runtimePath,
    '-BaselineCanaryFixture', $baselinePath,
    '-EvaluateCanaryFixture', $wslgPath
  )
  Assert-True ($wslg.ExitCode -ne 0) 'failed WSLg transaction was accepted'

  $gettyPath = Join-Path $testRoot 'candidate-getty.txt'
  $getty = $candidate.Replace('CANARY_SYSTEMD=running', 'CANARY_SYSTEMD=degraded').Replace('CANARY_FAILED_UNITS=none', 'CANARY_FAILED_UNITS=wsl-bootstrap.service')
  Write-Ascii $gettyPath $getty
  $gettyRejected = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateRuntimeFixture', $runtimePath,
    '-BaselineCanaryFixture', $baselinePath,
    '-EvaluateCanaryFixture', $gettyPath
  )
  Assert-True ($gettyRejected.ExitCode -ne 0) 'getty failure was silently accepted'
  $gettyApproved = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateRuntimeFixture', $runtimePath,
    '-BaselineCanaryFixture', $baselinePath,
    '-EvaluateCanaryFixture', $gettyPath,
    '-ApprovedFailedUnits', 'wsl-bootstrap.service'
  )
  Assert-True ($gettyApproved.ExitCode -eq 0) ('exact getty exception failed: ' + ($gettyApproved.Output -join ' | '))

  $dryConfig = Join-Path $testRoot '.wslconfig'
  Write-Ascii $dryConfig "[wsl2]`r`nmemory=8GB`r`nkernel=C:/stale/kernel`r`nkernelModules=C:/stale/modules.vhdx`r`n"
  $dry = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateRuntimeFixture', $runtimePath,
    '-DryRunConfig', $dryConfig
  )
  Assert-True ($dry.ExitCode -eq 0) ('pair config fixture failed: ' + ($dry.Output -join ' | '))
  $dryText = Get-Content -LiteralPath $dryConfig -Raw
  Assert-True (-not ($dryText -match '(?m)^\s*kernel\s*=')) 'pair rollback retained kernel='
  Assert-True (-not ($dryText -match '(?m)^\s*kernelModules\s*=')) 'pair rollback retained kernelModules='
  Assert-True ($dryText.Contains('memory=8GB')) 'pair rollback lost an unrelated config key'

  $duplicateConfig = Join-Path $testRoot '.wslconfig-duplicate'
  Write-Ascii $duplicateConfig "[wsl2]`r`nmemory=8GB`r`n[wsl2]`r`nprocessors=4`r`n"
  $duplicate = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateRuntimeFixture', $runtimePath,
    '-DryRunConfig', $duplicateConfig
  )
  Assert-True ($duplicate.ExitCode -ne 0) 'duplicate [wsl2] sections were accepted for pair arm'

  $external = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateExternalFailureFixture'
  )
  Assert-True ($external.ExitCode -eq 0) ('external failure fixture failed: ' + ($external.Output -join ' | '))

  $timeoutPid = Join-Path $testRoot 'timeout-child.pid'
  $timeoutTree = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateExternalTimeoutFixture', $timeoutPid
  )
  Assert-True ($timeoutTree.ExitCode -eq 0) ('external process-tree timeout fixture failed: ' + ($timeoutTree.Output -join ' | '))

  $assignFailureEvidence = Join-Path $testRoot 'assign-failure-must-remain-absent.pid'
  $assignFailure = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateExternalTimeoutFixture', $assignFailureEvidence,
    '-InjectAssignFailureFixture'
  )
  Assert-True ($assignFailure.ExitCode -eq 0) ('assignment-failure custody fixture failed: ' + ($assignFailure.Output -join ' | '))
  Assert-True (($assignFailure.Output -join "`n").Contains('PROCESS_ASSIGN_FAILURE_FIXTURE=PASS root-reaped-by-handle-before-resume')) 'assignment-failure fixture lacked exact handle-reap evidence'
  Assert-True (-not (Test-Path -LiteralPath $assignFailureEvidence)) 'assignment-failure fixture resumed a root or descendant'

  $resumeFailureEvidence = Join-Path $testRoot 'resume-failure-must-remain-absent.pid'
  $resumeFailure = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateExternalTimeoutFixture', $resumeFailureEvidence,
    '-InjectResumeFailureFixture'
  )
  Assert-True ($resumeFailure.ExitCode -eq 0) ('resume-failure custody fixture failed: ' + ($resumeFailure.Output -join ' | '))
  Assert-True (($resumeFailure.Output -join "`n").Contains('PROCESS_RESUME_FAILURE_FIXTURE=PASS job-empty-root-reaped-before-resume')) 'resume-failure fixture lacked exact empty-job handle-reap evidence'
  Assert-True (-not (Test-Path -LiteralPath $resumeFailureEvidence)) 'resume-failure fixture ran child code'

  $terminateFailureEvidence = Join-Path $testRoot 'terminate-failure-descendant.pid'
  $terminateFailure = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateExternalTimeoutFixture', $terminateFailureEvidence,
    '-InjectTerminateFailureFixture'
  )
  Assert-True ($terminateFailure.ExitCode -eq 0) ('termination-failure custody fixture failed: ' + ($terminateFailure.Output -join ' | '))
  Assert-True (($terminateFailure.Output -join "`n").Contains('EXTERNAL_TIMEOUT_FIXTURE=PASS JOB_EMPTY=1 HANDLE_IDENTITY=PROVEN')) 'termination-failure fixture lacked empty-job evidence'
  Assert-True (Test-Path -LiteralPath $terminateFailureEvidence -PathType Leaf) 'termination-failure fixture never resumed its descendant probe'

  $creationMismatchEvidence = Join-Path $testRoot 'creation-mismatch-descendant.pid'
  $creationMismatch = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateExternalTimeoutFixture', $creationMismatchEvidence,
    '-InjectRootCreationMismatchFixture'
  )
  Assert-True ($creationMismatch.ExitCode -eq 0) ('creation-mismatch custody fixture failed: ' + ($creationMismatch.Output -join ' | '))
  Assert-True (($creationMismatch.Output -join "`n").Contains('PROCESS_CREATION_IDENTITY_FIXTURE=PASS mismatch-refused-job-empty')) 'creation-mismatch fixture lacked exact empty-job refusal'

  $startupBefore = Get-PathState $testRoot
  $startupDeadline = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateRuntimeFixture', $runtimePath,
    '-PreflightOnly',
    '-EvaluateDeadlineFixtureSec', '1',
    '-EvaluateSlowStartupFixture'
  )
  Assert-True ($startupDeadline.ExitCode -eq 2) ('pre-child deadline fixture failed: ' + ($startupDeadline.Output -join ' | '))
  Assert-True (($startupDeadline.Output -join "`n").Contains('PRE_CHILD_DEADLINE=REFUSED NO_CHILD_CREATED=1')) 'pre-child deadline fixture lacked exact no-child proof'
  Assert-PathState $testRoot $startupBefore 'pre-child deadline fixture changed FixtureRoot'

  $slowHashBefore = Get-PathState $testRoot
  $slowHashClock = [System.Diagnostics.Stopwatch]::StartNew()
  $slowHash = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateRuntimeFixture', $runtimePath,
    '-PreflightOnly',
    '-EvaluateDeadlineFixtureSec', '5',
    '-EvaluateSlowHashFixture'
  )
  $slowHashClock.Stop()
  Assert-True ($slowHash.ExitCode -ne 0) 'slow-hash fixture escaped its outer deadline'
  $slowHashText = $slowHash.Output -join "`n"
  Assert-True ($slowHashText.Contains('SLOW_HASH_SEAM=ENTERED')) ('slow-hash fixture never entered its blocked-I/O seam: ' + $slowHashText)
  Assert-True ($slowHashText -match '(?s)timed out.*JOB_EMPTY=1.*ROOT_CREATION=') ('slow-hash fixture lacked checked empty-job timeout proof: ' + $slowHashText)
  Assert-True ($slowHashClock.Elapsed.TotalSeconds -lt 15) 'slow-hash fixture exceeded its bounded outer cleanup window'
  Assert-PathState $testRoot $slowHashBefore 'slow-hash cancellation changed FixtureRoot'

  $layoutCases = @(
    [pscustomobject]@{ Name = 'unified-2.7.12'; Layout = 'unified_release_v1'; Releases = 1; Nested = 0; Release = $release },
    [pscustomobject]@{ Name = 'unified-6.18.40.1'; Layout = 'unified_release_v1'; Releases = 1; Nested = 0; Release = '6.18.40.1-microsoft-standard-WSL2+' },
    [pscustomobject]@{ Name = 'double-nested'; Layout = 'unified_release_v1'; Releases = 1; Nested = 1; Release = $release },
    [pscustomobject]@{ Name = 'legacy-nested'; Layout = 'legacy_flat_v1'; Releases = 1; Nested = 0; Release = $release }
  )
  foreach ($layoutCase in $layoutCases) {
    $caseRoot = Join-Path $testRoot $layoutCase.Name
    New-Item -ItemType Directory -Path $caseRoot | Out-Null
    Copy-Item -LiteralPath $kernelPath, $modulesPath -Destination $caseRoot
    $caseManifest = Join-Path $caseRoot 'kernel-pair.manifest'
    Write-PairManifest $caseManifest $layoutCase.Release $layoutCase.Layout $layoutCase.Releases $layoutCase.Nested
    $caseResult = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $caseManifest,
      '-ExpectedKernelManifestSha256', (Get-Sha256 $caseManifest),
      '-EvaluateRuntimeFixture', $runtimePath,
      '-PreflightOnly'
    )
    Assert-True ($caseResult.ExitCode -ne 0) ("layout case $($layoutCase.Name) was accepted")
  }

  $oldRuntime = Join-Path $testRoot 'wsl-old.txt'
  Write-Ascii $oldRuntime ((Get-Content -LiteralPath $runtimePath -Raw).Replace('2.7.12.0', '2.7.11.0'))
  $oldResult = Invoke-Ps51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateRuntimeFixture', $oldRuntime,
    '-PreflightOnly'
  )
  Assert-True ($oldResult.ExitCode -ne 0) 'runtime below manifest minimum was accepted'

  $transactionBoundaries = @(
    'after_capture_before_validation', 'after_receipt_root', 'after_snapshot', 'after_bundled_config',
    'after_current_receipt', 'after_transaction_receipt', 'after_candidate_config',
    'after_ready_receipt', 'atomic_after_temp_flush', 'atomic_after_replace',
    'post_check_mutation'
  )
  $transactionBoundaryCaseCount = 0
  foreach ($initialState in @('absent', 'present')) {
    $transactionConfig = Join-Path $testRoot ("transaction-$initialState.wslconfig")
    $transactionReceipts = Join-Path $testRoot ("transaction-$initialState-receipts")
    if ($initialState -ceq 'present') {
      Write-Ascii $transactionConfig "[wsl2]`r`nmemory=6144MB`r`n# exact-original`r`n"
      New-Item -ItemType Directory -Path $transactionReceipts | Out-Null
      Write-Ascii (Join-Path $transactionReceipts 'promotion-current.json') '{"status":"ORIGINAL"}'
      Write-Ascii (Join-Path $transactionReceipts 'unrelated.sentinel') 'transaction-sentinel'
    }
    foreach ($boundary in $transactionBoundaries) {
      $before = Get-PathState $testRoot
      $transactionResult = Invoke-Ps51 $launcher @(
        '-KernelPairManifest', $manifestPath,
        '-ExpectedKernelManifestSha256', $manifestSha,
        '-EvaluateTransactionFixture', $transactionConfig,
        '-InjectFailureBoundary', $boundary,
        '-WslConfig', $transactionConfig,
        '-ReceiptDirectory', $transactionReceipts,
        '-TransactionLockTimeoutSec', '2'
      )
      Assert-True ($transactionResult.ExitCode -eq 0) ("transaction boundary $initialState/$boundary failed: " + ($transactionResult.Output -join ' | '))
      Assert-True (($transactionResult.Output -join "`n").Contains("TRANSACTION_FIXTURE=PASS boundary=$boundary")) "transaction boundary $initialState/$boundary lacked exact PASS"
      Assert-PathState $testRoot $before "transaction boundary $initialState/$boundary did not restore exact bytes/absence"
      $transactionBoundaryCaseCount++
    }
    $before = Get-PathState $testRoot
    $transactionSuccess = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateTransactionFixture', $transactionConfig,
      '-WslConfig', $transactionConfig,
      '-ReceiptDirectory', $transactionReceipts,
      '-TransactionLockTimeoutSec', '2'
    )
    Assert-True ($transactionSuccess.ExitCode -eq 0) ("transaction success fixture $initialState failed: " + ($transactionSuccess.Output -join ' | '))
    Assert-True (($transactionSuccess.Output -join "`n").Contains('TRANSACTION_FIXTURE=PASS boundary=none')) "transaction success fixture $initialState lacked PASS"
    Assert-PathState $testRoot $before "transaction success fixture $initialState did not restore exact bytes/absence"
    $transactionBoundaryCaseCount++
    if ($initialState -ceq 'present') {
      $transactionReceiptIdentity = Get-OwnedTreeIdentity $transactionReceipts
      Remove-OwnedTree $transactionReceipts $transactionReceiptIdentity
      Remove-Item -LiteralPath $transactionConfig -Force
    }
  }
  Write-Host ("TRANSACTION_ROLLBACK_MATRIX=PASS cases=$transactionBoundaryCaseCount capture_before_validation=covered")

  $concurrentConfig = Join-Path $testRoot 'concurrent-transaction.wslconfig'
  $concurrentReceipts = Join-Path $testRoot 'concurrent-transaction-receipts'
  $concurrentEvidence = Join-Path $testRoot 'concurrent-lock.evidence'
  Write-Ascii $concurrentConfig "[wsl2]`r`nmemory=4096MB`r`n"
  $concurrentConfigState = Get-PathState $concurrentConfig
  $firstInvocation = Start-WatchedPs51 $launcher @(
    '-KernelPairManifest', $manifestPath,
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-EvaluateTransactionFixture', $concurrentConfig,
    '-WslConfig', $concurrentConfig,
    '-ReceiptDirectory', $concurrentReceipts,
    '-TransactionLockTimeoutSec', '2',
    '-HoldTransactionLockMilliseconds', '4000',
    '-TransactionLockEvidenceFixture', $concurrentEvidence,
    '-FixtureRoot', $testRoot,
    '-FixtureNonce', $fixtureNonce
  )
  $firstFinished = $false
  try {
    $evidenceClock = [System.Diagnostics.Stopwatch]::StartNew()
    while (-not (Test-Path -LiteralPath $concurrentEvidence -PathType Leaf) -and
        -not $firstInvocation.Process.HasExited -and $evidenceClock.Elapsed.TotalSeconds -lt 12) {
      Start-Sleep -Milliseconds 20
    }
    $evidenceClock.Stop()
    Assert-True (Test-Path -LiteralPath $concurrentEvidence -PathType Leaf) 'first transaction did not publish confined lock evidence'
    $evidenceState = Get-PathState $concurrentEvidence
    $receiptStateDuringLock = Get-PathState $concurrentReceipts
    $secondTransaction = Invoke-Ps51 $launcher @(
      '-KernelPairManifest', $manifestPath,
      '-ExpectedKernelManifestSha256', $manifestSha,
      '-EvaluateTransactionFixture', $concurrentConfig,
      '-WslConfig', $concurrentConfig,
      '-ReceiptDirectory', $concurrentReceipts,
      '-TransactionLockTimeoutSec', '1'
    )
    Assert-True ($secondTransaction.ExitCode -ne 0) 'concurrent transaction unexpectedly acquired the same canonical lock'
    Assert-True (($secondTransaction.Output -join "`n").Contains('TRANSACTION_LOCK=REFUSED')) 'concurrent transaction lacked the exact lock refusal'
    Assert-PathState $concurrentConfig $concurrentConfigState 'refused concurrent transaction changed config bytes'
    Assert-PathState $concurrentReceipts $receiptStateDuringLock 'refused concurrent transaction changed receipt state'
    Assert-PathState $concurrentEvidence $evidenceState 'refused concurrent transaction changed lock evidence'
    $firstExit = Wait-WatchedPs51 $firstInvocation 20
    $firstFinished = $true
    Assert-True ($firstExit -eq 0) 'first concurrent transaction did not complete cleanly'
  } finally {
    if (-not $firstFinished) { [void](Wait-WatchedPs51 $firstInvocation 20) }
  }
  Assert-PathState $concurrentConfig $concurrentConfigState 'first concurrent transaction did not restore config bytes'
  Assert-True (-not (Test-Path -LiteralPath $concurrentReceipts)) 'first concurrent transaction left a receipt root'
  Assert-True (-not (Test-Path -LiteralPath $concurrentEvidence)) 'first concurrent transaction left lock evidence'
  Write-Host 'TRANSACTION_LOCK_MATRIX=PASS winner=1 refused=1 mutation_on_refusal=0'
  Remove-Item -LiteralPath $concurrentConfig -Force

  $installerSourceRoot = Join-Path $testRoot 'installer-sources'
  New-Item -ItemType Directory -Path $installerSourceRoot | Out-Null
  $fixtureWrapperSource = Join-Path $installerSourceRoot 'boot-kernel-logged.ps1'
  $fixtureLauncherSource = Join-Path $installerSourceRoot 'boot-kernel-safe.ps1'
  Copy-Item -LiteralPath $wrapper -Destination $fixtureWrapperSource
  Copy-Item -LiteralPath $launcher -Destination $fixtureLauncherSource
  $installArguments = @(
    '-SourceWrapper', $fixtureWrapperSource,
    '-SourceLauncher', $fixtureLauncherSource,
    '-SourceKernelManifest', $manifestPath,
    '-SourceKernel', $kernelPath,
    '-SourceModules', $modulesPath,
    '-SourceLayoutInventory', $layoutPath,
    '-SourceQemuStamp', $qemuPath,
    '-ExpectedWrapperSha256', (Get-Sha256 $fixtureWrapperSource),
    '-ExpectedLauncherSha256', (Get-Sha256 $fixtureLauncherSource),
    '-ExpectedKernelManifestSha256', $manifestSha,
    '-ExpectedKernelSha256', $kernelSha,
    '-ExpectedModulesSha256', $modulesSha,
    '-ExpectedLayoutInventorySha256', $layoutSha,
    '-ExpectedQemuStampSha256', $qemuSha
  )
  $installGateParent = Join-Path $testRoot 'installer-gate'
  New-Item -ItemType Directory -Path $installGateParent | Out-Null
  $installSentinel = Join-Path $installGateParent 'sentinel.txt'
  Write-Ascii $installSentinel 'installer-gate-sentinel'
  $installSentinelSha = Get-Sha256 $installSentinel
  $installToken = 'INSTALL_IMMUTABLE_KERNEL_LAUNCHER_BUNDLE'
  $installRefusals = @(
    [pscustomobject]@{ Name = 'default'; Extra = @() },
    [pscustomobject]@{ Name = 'run-missing-token'; Extra = @('-Run') },
    [pscustomobject]@{ Name = 'run-blank-token'; Extra = @('-Run', '-ConfirmationToken', '   ') },
    [pscustomobject]@{ Name = 'run-malformed-token'; Extra = @('-Run', '-ConfirmationToken', 'INSTALL BUNDLE') },
    [pscustomobject]@{ Name = 'run-wrong-token'; Extra = @('-Run', '-ConfirmationToken', 'INSTALL_KERNEL_BUNDLE') },
    [pscustomobject]@{ Name = 'run-wrong-case-token'; Extra = @('-Run', '-ConfirmationToken', $installToken.ToLowerInvariant()) },
    [pscustomobject]@{ Name = 'token-without-run'; Extra = @('-ConfirmationToken', $installToken) }
  )
  foreach ($case in $installRefusals) {
    $caseRoot = Join-Path $installGateParent $case.Name
    $parentState = Get-PathState $installGateParent
    $result = Invoke-Ps51 $installer ($installArguments + @('-InstallRoot', $caseRoot) + $case.Extra)
    Assert-True ($result.ExitCode -ne 0) "installer gate case $($case.Name) was accepted"
    Assert-True (($result.Output -join "`n").Contains('LIVE_GATE=REFUSED')) "installer gate case $($case.Name) did not report gate refusal"
    Assert-True (-not (Test-Path -LiteralPath $caseRoot)) "installer gate case $($case.Name) created its install root"
    Assert-PathState $installGateParent $parentState "installer gate case $($case.Name) changed the install parent"
    Assert-True ((Get-Sha256 $installSentinel) -ceq $installSentinelSha) "installer gate case $($case.Name) changed the sentinel hash"
    $refusalCaseCount += 1
  }

  $installerPositiveBefore = Get-PathState $testRoot
  $installerPositiveGate = Invoke-Ps51 $installer ($installArguments + @(
    '-Run', '-ConfirmationToken', $installToken
  ))
  Assert-True ($installerPositiveGate.ExitCode -eq 2) ('installer exact gate did not stop at staging custody: ' + ($installerPositiveGate.Output -join ' | '))
  Assert-True (($installerPositiveGate.Output -join "`n").Contains('LIVE_GATE=ACCEPTED exact-run-and-confirmation')) 'installer exact gate was not recognized'
  Assert-True (($installerPositiveGate.Output -join "`n").Contains('STAGING_CUSTODY=NO_GO')) 'installer exact gate did not stop before copy/publication'
  Assert-PathState $testRoot $installerPositiveBefore 'installer exact positive gate changed fixture state'

  foreach ($hostilePath in $hostilePathCases) {
    $before = Get-PathState $testRoot
    $installerHostile = Invoke-Ps51 $installer ($installArguments + @(
      '-InstallRoot', $hostilePath,
      '-EvaluateInstallFixture',
      '-FixtureRoot', $testRoot,
      '-FixtureNonce', $fixtureNonce
    ))
    Assert-True ($installerHostile.ExitCode -eq 2) 'installer hostile lexical path did not exit 2'
    Assert-True (($installerHostile.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) 'installer hostile lexical path lacked refusal'
    Assert-PathState $testRoot $before 'installer hostile lexical path changed FixtureRoot'
  }

  $fixtureInstallAuthorityRoot = Join-Path $testRoot 'fixture-install-authority'
  $fixtureInstallAuthorityCases = @(
    [pscustomobject]@{ Name = 'run-only'; Extra = @('-Run') },
    [pscustomobject]@{ Name = 'exact'; Extra = @('-Run', '-ConfirmationToken', $installToken) },
    [pscustomobject]@{ Name = 'wrong'; Extra = @('-Run', '-ConfirmationToken', 'INSTALL_KERNEL_BUNDLE') },
    [pscustomobject]@{ Name = 'token-only'; Extra = @('-ConfirmationToken', $installToken) },
    [pscustomobject]@{ Name = 'duplicate'; Extra = @('-Run', '-ConfirmationToken', $installToken, '-ConfirmationToken', $installToken) }
  )
  foreach ($authorityCase in $fixtureInstallAuthorityCases) {
    $before = Get-PathState $testRoot
    $authorityResult = Invoke-Ps51 $installer ($installArguments + @(
      '-InstallRoot', $fixtureInstallAuthorityRoot,
      '-EvaluateInstallFixture',
      '-FixtureRoot', $testRoot,
      '-FixtureNonce', $fixtureNonce
    ) + $authorityCase.Extra)
    Assert-True ($authorityResult.ExitCode -ne 0) "installer fixture/live case $($authorityCase.Name) was accepted"
    if ($authorityCase.Name -cne 'duplicate') {
      Assert-True (($authorityResult.Output -join "`n").Contains('FIXTURE_LIVE_AUTHORITY=REFUSED')) "installer fixture/live case $($authorityCase.Name) lacked refusal"
    }
    Assert-True (-not (Test-Path -LiteralPath $fixtureInstallAuthorityRoot)) "installer fixture/live case $($authorityCase.Name) created its root"
    Assert-PathState $testRoot $before "installer fixture/live case $($authorityCase.Name) changed FixtureRoot"
    $refusalCaseCount += 1
  }

  $outsideInstallResult = Invoke-Ps51 $installer ($installArguments + @(
    '-InstallRoot', $outsidePath,
    '-EvaluateInstallFixture',
    '-FixtureRoot', $testRoot,
    '-FixtureNonce', $fixtureNonce
  ))
  Assert-True ($outsideInstallResult.ExitCode -eq 2) 'installer accepted an output root outside FixtureRoot'
  Assert-True (($outsideInstallResult.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) 'installer outside-root refusal was not exact'
  Assert-True (-not (Test-Path -LiteralPath $outsidePath)) 'installer wrote to its outside-root probe'

  $sourceHardLink = Join-Path $installerSourceRoot 'wrapper-hardlink.ps1'
  New-Item -ItemType HardLink -Path $sourceHardLink -Target $fixtureWrapperSource | Out-Null
  try {
    $hardLinkInstallRoot = Join-Path $testRoot 'hardlink-install-root'
    $hardLinkResult = Invoke-Ps51 $installer ($installArguments + @(
      '-InstallRoot', $hardLinkInstallRoot,
      '-EvaluateInstallFixture',
      '-FixtureRoot', $testRoot,
      '-FixtureNonce', $fixtureNonce
    ))
    Assert-True ($hardLinkResult.ExitCode -ne 0) 'installer accepted a multiply-linked source'
    Assert-True (-not (Test-Path -LiteralPath $hardLinkInstallRoot)) 'hard-link refusal created an install root'
  } finally {
    Remove-Item -LiteralPath $sourceHardLink -Force
  }

  $installResult = Invoke-Ps51 $installer ($installArguments + @(
    '-InstallRoot', $installRoot,
    '-EvaluateInstallFixture',
    '-FixtureRoot', $testRoot,
    '-FixtureNonce', $fixtureNonce
  ))
  Assert-True ($installResult.ExitCode -eq 0) ('bundle installation failed: ' + ($installResult.Output -join ' | '))
  $installMap = @{}
  foreach ($line in $installResult.Output) {
    if ($line -match '^(RAMSHARED_[A-Z0-9_]+)=(.*)$') { $installMap[$Matches[1]] = $Matches[2] }
  }
  foreach ($key in @('RAMSHARED_INSTALLED_WRAPPER', 'RAMSHARED_DEPLOYMENT_MANIFEST', 'RAMSHARED_DEPLOYMENT_SHA256')) {
    Assert-True ($installMap.ContainsKey($key)) "installer output is missing $key"
  }

  $wrapperConfig = Join-Path $testRoot 'gate-wrapper.wslconfig'
  Write-Ascii $wrapperConfig "[wsl2]`r`nmemory=8GB`r`n"
  $wrapperReceiptRoot = Join-Path $testRoot 'gate-wrapper-receipts'
  $wrapperLog = Join-Path $testRoot 'gate-wrapper.log'
  $wrapperConfigState = Get-PathState $wrapperConfig
  $wrapperReceiptState = Get-PathState $wrapperReceiptRoot
  $wrapperGateBase = @(
    '-DeploymentManifest', $installMap.RAMSHARED_DEPLOYMENT_MANIFEST,
    '-ExpectedDeploymentSha256', $installMap.RAMSHARED_DEPLOYMENT_SHA256,
    '-WslConfig', $wrapperConfig,
    '-ReceiptDirectory', $wrapperReceiptRoot,
    '-LogPath', $wrapperLog
  )
  foreach ($case in $safeRefusals) {
    $result = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER ($wrapperGateBase + $case.Extra)
    Assert-True ($result.ExitCode -ne 0) "wrapper gate case $($case.Name) was accepted"
    Assert-True (($result.Output -join "`n").Contains('LIVE_GATE=REFUSED')) "wrapper gate case $($case.Name) did not report gate refusal"
    Assert-True (-not (Test-Path -LiteralPath $wrapperLog)) "wrapper gate case $($case.Name) created its log"
    Assert-PathState $wrapperConfig $wrapperConfigState "wrapper gate case $($case.Name) changed the config sentinel"
    Assert-PathState $wrapperReceiptRoot $wrapperReceiptState "wrapper gate case $($case.Name) changed the receipt root"
    $refusalCaseCount += 1
  }
  $defaultWrapperLogRoot = Join-Path $installRoot 'receipts'
  $defaultWrapperLogState = Get-PathState $defaultWrapperLogRoot
  $wrapperDefaultLogRefusal = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER @(
    '-DeploymentManifest', $installMap.RAMSHARED_DEPLOYMENT_MANIFEST,
    '-ExpectedDeploymentSha256', $installMap.RAMSHARED_DEPLOYMENT_SHA256
  )
  Assert-True ($wrapperDefaultLogRefusal.ExitCode -ne 0) 'wrapper default gate was accepted with its implicit log root'
  Assert-True (($wrapperDefaultLogRefusal.Output -join "`n").Contains('LIVE_GATE=REFUSED')) 'wrapper default gate did not report refusal'
  Assert-PathState $defaultWrapperLogRoot $defaultWrapperLogState 'wrapper default refusal created its implicit log root'
  $wrapperGatePass = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER ($wrapperGateBase + @('-Run', '-ConfirmationToken', $promotionToken))
  Assert-True ($wrapperGatePass.ExitCode -eq 2) ('wrapper exact gate did not reach direct-custody NO-GO: ' + ($wrapperGatePass.Output -join ' | '))
  Assert-True (($wrapperGatePass.Output -join "`n").Contains('LIVE_PROMOTION=NO_GO direct-suspended-supervision-and-handle-execution-unproven')) 'wrapper exact gate lacked the direct-custody NO-GO'
  Assert-True (-not (Test-Path -LiteralPath $wrapperLog)) 'wrapper exact gate created its log'
  Assert-PathState $wrapperConfig $wrapperConfigState 'wrapper exact gate changed the config sentinel'
  Assert-PathState $wrapperReceiptRoot $wrapperReceiptState 'wrapper exact gate changed the receipt root'

  $wrapperFixtureAuthorityBase = @(
    '-DeploymentManifest', (Join-Path $testRoot 'fixture-authority-must-not-read-deployment.json'),
    '-ExpectedDeploymentSha256', ('0' * 64),
    '-WslConfig', $wrapperConfig,
    '-ReceiptDirectory', $wrapperReceiptRoot,
    '-LogPath', $wrapperLog
  )
  foreach ($case in $fixtureAuthorityCases) {
    $before = Get-PathState $testRoot
    $result = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER ($wrapperFixtureAuthorityBase + $case.Arguments + @('-Run', '-ConfirmationToken', $promotionToken))
    Assert-True ($result.ExitCode -eq 2) "wrapper fixture authority case $($case.Name) did not exit 2"
    Assert-True (($result.Output -join "`n").Contains('FIXTURE_LIVE_AUTHORITY=REFUSED')) "wrapper fixture authority case $($case.Name) did not refuse before deployment validation"
    Assert-PathState $testRoot $before "wrapper fixture authority case $($case.Name) changed the temporary root"
    Assert-True (-not (Test-Path -LiteralPath $wrapperLog)) "wrapper fixture authority case $($case.Name) created its log"
    $refusalCaseCount += 1
  }
  foreach ($case in $additionalCustodyAuthorityCases) {
    $before = Get-PathState $testRoot
    $result = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER ($wrapperFixtureAuthorityBase + $case.Arguments + @('-Run', '-ConfirmationToken', $promotionToken))
    Assert-True ($result.ExitCode -eq 2) "wrapper additional custody authority case $($case.Name) did not exit 2"
    Assert-True (($result.Output -join "`n").Contains('FIXTURE_LIVE_AUTHORITY=REFUSED')) "wrapper additional custody authority case $($case.Name) did not refuse before deployment work"
    Assert-PathState $testRoot $before "wrapper additional custody authority case $($case.Name) changed FixtureRoot"
    Assert-True (-not (Test-Path -LiteralPath $wrapperLog)) "wrapper additional custody authority case $($case.Name) created its log"
    $refusalCaseCount++
  }
  foreach ($case in $fixtureCredentialCases) {
    $before = Get-PathState $testRoot
    $result = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER ($wrapperFixtureAuthorityBase + @('-EvaluateRuntimeFixture', $runtimePath) + $case.Extra)
    Assert-True ($result.ExitCode -ne 0) "wrapper fixture credential case $($case.Name) was accepted"
    if (-not [string]::IsNullOrWhiteSpace($case.Marker)) {
      Assert-True (($result.Output -join "`n").Contains($case.Marker)) "wrapper fixture credential case $($case.Name) lacked fixture refusal"
    }
    Assert-PathState $testRoot $before "wrapper fixture credential case $($case.Name) changed the temporary root"
    Assert-True (-not (Test-Path -LiteralPath $wrapperLog)) "wrapper fixture credential case $($case.Name) created its log"
    $refusalCaseCount += 1
  }
  foreach ($descriptor in @($fixtureDescriptors | Where-Object { $_.Name -in @('baseline', 'rollback', 'runtime') })) {
    $before = Get-PathState $testRoot
    $result = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER ($wrapperGateBase + $descriptor.Arguments)
    Assert-True ($result.ExitCode -eq 2) "wrapper companion-only fixture $($descriptor.Name) did not exit 2"
    Assert-True (($result.Output -join "`n").Contains('FIXTURE_COMBINATION=REFUSED')) "wrapper companion-only fixture $($descriptor.Name) did not refuse in the bundled launcher"
    Assert-PathState $testRoot $before "wrapper companion-only fixture $($descriptor.Name) changed the temporary root"
    Assert-True (-not (Test-Path -LiteralPath $wrapperLog)) "wrapper companion-only fixture $($descriptor.Name) created its log"
    $refusalCaseCount += 1
  }

  foreach ($preflightArgs in @(
    @('-PreflightOnly'),
    @('-PreflightOnly', '-EvaluateRuntimeFixture', '   ')
  )) {
    $before = Get-PathState $testRoot
    $wrapperPreflight = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER (@(
      '-DeploymentManifest', (Join-Path $testRoot 'wrapper-preflight-must-not-read.json'),
      '-ExpectedDeploymentSha256', ('0' * 64)
    ) + $preflightArgs)
    Assert-True ($wrapperPreflight.ExitCode -eq 2) 'wrapper preflight without runtime fixture did not exit 2'
    Assert-True (($wrapperPreflight.Output -join "`n").Contains('PREFLIGHT_RUNTIME_FIXTURE=REFUSED')) 'wrapper preflight missing-runtime refusal was not early and exact'
    Assert-PathState $testRoot $before 'wrapper missing-runtime refusal changed FixtureRoot'
    Assert-True (-not (Test-Path -LiteralPath $wrapperLog)) 'wrapper missing-runtime refusal created a log'
    $refusalCaseCount += 1
  }

  $wrapperOutsideCases = @(
    [pscustomobject]@{ Name = 'log'; Extra = @('-EvaluateRuntimeFixture', $runtimePath, '-LogPath', $outsidePath) },
    [pscustomobject]@{ Name = 'config'; Extra = @('-EvaluateRuntimeFixture', $runtimePath, '-WslConfig', $outsidePath) },
    [pscustomobject]@{ Name = 'receipt'; Extra = @('-EvaluateRuntimeFixture', $runtimePath, '-ReceiptDirectory', $outsidePath) },
    [pscustomobject]@{ Name = 'pid'; Extra = @('-EvaluateExternalTimeoutFixture', $outsidePath) }
  )
  foreach ($outsideCase in $wrapperOutsideCases) {
    $before = Get-PathState $testRoot
    $outsideResult = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER (@(
      '-DeploymentManifest', $installMap.RAMSHARED_DEPLOYMENT_MANIFEST,
      '-ExpectedDeploymentSha256', $installMap.RAMSHARED_DEPLOYMENT_SHA256
    ) + $outsideCase.Extra)
    Assert-True ($outsideResult.ExitCode -eq 2) "wrapper outside fixture path $($outsideCase.Name) did not exit 2"
    Assert-True (($outsideResult.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) "wrapper outside fixture path $($outsideCase.Name) lacked refusal"
    Assert-True (-not (Test-Path -LiteralPath $outsidePath)) "wrapper outside fixture path $($outsideCase.Name) was written"
    Assert-PathState $testRoot $before "wrapper outside fixture path $($outsideCase.Name) changed FixtureRoot"
    $refusalCaseCount += 1
  }

  foreach ($hostilePath in $hostilePathCases) {
    $before = Get-PathState $testRoot
    $wrapperHostile = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER @(
      '-DeploymentManifest', $installMap.RAMSHARED_DEPLOYMENT_MANIFEST,
      '-ExpectedDeploymentSha256', $installMap.RAMSHARED_DEPLOYMENT_SHA256,
      '-EvaluateRuntimeFixture', $runtimePath,
      '-PreflightOnly',
      '-LogPath', $hostilePath
    )
    Assert-True ($wrapperHostile.ExitCode -eq 2) 'wrapper hostile lexical path did not exit 2'
    Assert-True (($wrapperHostile.Output -join "`n").Contains('FIXTURE_PATH=REFUSED')) 'wrapper hostile lexical path lacked refusal'
    Assert-PathState $testRoot $before 'wrapper hostile lexical path changed FixtureRoot'
  }
  Write-Host ("PATH_LEXICAL_MATRIX=PASS forms=$($hostilePathCases.Count) entrypoints=3")

  $fixtureChainLog = Join-Path $receiptRoot 'wrapper-chain.log'
  $fixtureReceiptState = Get-PathState $receiptRoot
  $fixtureChainArgs = @(
    '-DeploymentManifest', $installMap.RAMSHARED_DEPLOYMENT_MANIFEST,
    '-ExpectedDeploymentSha256', $installMap.RAMSHARED_DEPLOYMENT_SHA256,
    '-EvaluateRuntimeFixture', $runtimePath,
    '-BaselineCanaryFixture', $baselinePath,
    '-EvaluateCanaryFixture', $candidatePath,
    '-WslConfig', $wrapperConfig,
    '-ReceiptDirectory', $receiptRoot,
    '-LogPath', $fixtureChainLog
  )
  $wrapperFixtureAuthority = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER ($fixtureChainArgs + @('-Run', '-ConfirmationToken', $promotionToken))
  Assert-True ($wrapperFixtureAuthority.ExitCode -ne 0) 'wrapper fixture accepted live authority'
  Assert-True (($wrapperFixtureAuthority.Output -join "`n").Contains('FIXTURE_LIVE_AUTHORITY=REFUSED')) 'wrapper fixture did not report live-authority refusal'
  Assert-True (-not (Test-Path -LiteralPath $fixtureChainLog)) 'wrapper fixture live-authority refusal created its log'
  Assert-PathState $wrapperConfig $wrapperConfigState 'wrapper fixture live-authority refusal changed the config sentinel'
  Assert-PathState $receiptRoot $fixtureReceiptState 'wrapper fixture live-authority refusal changed the receipt root'

  $chain = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER $fixtureChainArgs
  Assert-True ($chain.ExitCode -eq 0) ('real installed wrapper-to-launcher chain failed: ' + ($chain.Output -join ' | '))
  Assert-True (-not (Test-Path -LiteralPath $fixtureChainLog)) 'hermetic wrapper chain created a log without live authority'
  Assert-PathState $wrapperConfig $wrapperConfigState 'hermetic wrapper chain changed the config sentinel'
  Assert-PathState $receiptRoot $fixtureReceiptState 'hermetic wrapper chain changed the receipt root'

  $rollbackChain = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER ($fixtureChainArgs + @('-EvaluateRollbackFixture', $rollbackPath))
  Assert-True ($rollbackChain.ExitCode -eq 0) ('installed wrapper rollback fixture chain failed: ' + ($rollbackChain.Output -join ' | '))
  Assert-True (-not (Test-Path -LiteralPath $fixtureChainLog)) 'wrapper rollback fixture chain created a log'
  Assert-PathState $wrapperConfig $wrapperConfigState 'wrapper rollback fixture chain changed the config sentinel'
  Assert-PathState $receiptRoot $fixtureReceiptState 'wrapper rollback fixture chain changed the receipt root'

  $wrapperDryConfig = Join-Path $testRoot 'wrapper-dry-config.ini'
  Write-Ascii $wrapperDryConfig "[wsl2]`r`nmemory=8GB`r`nkernel=C:/stale/kernel`r`nkernelModules=C:/stale/modules.vhdx`r`n"
  $wrapperDryLog = Join-Path $testRoot 'wrapper-dry.log'
  $wrapperDry = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER @(
    '-DeploymentManifest', $installMap.RAMSHARED_DEPLOYMENT_MANIFEST,
    '-ExpectedDeploymentSha256', $installMap.RAMSHARED_DEPLOYMENT_SHA256,
    '-EvaluateRuntimeFixture', $runtimePath,
    '-DryRunConfig', $wrapperDryConfig,
    '-LogPath', $wrapperDryLog
  )
  Assert-True ($wrapperDry.ExitCode -eq 0) ('installed wrapper dry-config fixture failed: ' + ($wrapperDry.Output -join ' | '))
  $wrapperDryText = Get-Content -LiteralPath $wrapperDryConfig -Raw
  Assert-True (-not ($wrapperDryText -match '(?m)^\s*kernel(?:Modules)?\s*=')) 'wrapper dry-config fixture retained a kernel pair key'
  Assert-True ($wrapperDryText.Contains('memory=8GB')) 'wrapper dry-config fixture lost unrelated configuration'
  Assert-True (-not (Test-Path -LiteralPath $wrapperDryLog)) 'wrapper dry-config fixture created a log'

  $wrapperExternalLog = Join-Path $testRoot 'wrapper-external.log'
  $wrapperExternal = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER @(
    '-DeploymentManifest', $installMap.RAMSHARED_DEPLOYMENT_MANIFEST,
    '-ExpectedDeploymentSha256', $installMap.RAMSHARED_DEPLOYMENT_SHA256,
    '-EvaluateExternalFailureFixture',
    '-LogPath', $wrapperExternalLog
  )
  Assert-True ($wrapperExternal.ExitCode -eq 0) ('installed wrapper external-failure fixture failed: ' + ($wrapperExternal.Output -join ' | '))
  Assert-True (-not (Test-Path -LiteralPath $wrapperExternalLog)) 'wrapper external-failure fixture created a log'

  $wrapperTimeoutPid = Join-Path $testRoot 'wrapper-timeout-child.pid'
  $wrapperTimeoutLog = Join-Path $testRoot 'wrapper-timeout.log'
  $wrapperTimeout = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER @(
    '-DeploymentManifest', $installMap.RAMSHARED_DEPLOYMENT_MANIFEST,
    '-ExpectedDeploymentSha256', $installMap.RAMSHARED_DEPLOYMENT_SHA256,
    '-EvaluateExternalTimeoutFixture', $wrapperTimeoutPid,
    '-LogPath', $wrapperTimeoutLog
  )
  Assert-True ($wrapperTimeout.ExitCode -eq 0) ('installed wrapper external-timeout fixture failed: ' + ($wrapperTimeout.Output -join ' | '))
  Assert-True (Test-Path -LiteralPath $wrapperTimeoutPid -PathType Leaf) 'wrapper external-timeout fixture did not write its hermetic PID evidence'
  Assert-True (-not (Test-Path -LiteralPath $wrapperTimeoutLog)) 'wrapper external-timeout fixture created a log'

  $badDeployment = Invoke-Ps51 $installMap.RAMSHARED_INSTALLED_WRAPPER @(
    '-DeploymentManifest', $installMap.RAMSHARED_DEPLOYMENT_MANIFEST,
    '-ExpectedDeploymentSha256', ('0' * 64),
    '-EvaluateRuntimeFixture', $runtimePath,
    '-BaselineCanaryFixture', $baselinePath,
    '-EvaluateCanaryFixture', $candidatePath,
    '-LogPath', (Join-Path $receiptRoot 'wrapper-reject.log')
  )
  Assert-True ($badDeployment.ExitCode -ne 0) 'wrapper accepted a deployment hash mismatch'
} finally {
  Remove-OwnedTree $testRoot $testRootIdentity
}

Assert-True ($refusalCaseCount -ge 102) "only $refusalCaseCount refusal cases executed; at least 102 are required"
if ([string]::IsNullOrWhiteSpace($SpecTest)) {
  Write-Host 'SPEC_TEST=direct_entrypoints_refuse_before_mutation PASS reason=sentinel-refusal-matrix'
  Write-Host 'SPEC_TEST=legitimate_gate_and_fixture_paths_pass_without_live_effects PASS reason=confined-fixtures'
  Write-Host 'SPEC_TEST=fixture_parameters_cannot_carry_live_authority PASS reason=authority-matrix'
} else {
  Write-Host ("SPEC_TEST=$SpecTest PASS reason=independently-selected-full-custody-suite")
}
Write-Host 'Test-BootKernelSafeStatic: PASS (Windows PowerShell 5.1)'
exit 0
