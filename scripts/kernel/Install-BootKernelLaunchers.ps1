<#
.SYNOPSIS
  Installs one immutable, hash-bound RamShared kernel promotion bundle.

.DESCRIPTION
  Copies the reviewed wrapper, launcher, kernel-pair manifest, kernel image,
  modules VHDX, layout inventory, and QEMU receipt into one versioned Windows
  directory. The directory is published with a same-volume rename and is never
  overwritten. Installation must finish before WSL shutdown because repository
  UNC paths disappear with the WSL VM.

  This script does not change .wslconfig, start or stop WSL, or touch a kernel,
  VM, service, device, swap, GPU, pressure workload, or Docker state.
  The default is PLAN/refusal; installation requires -Run and the exact
  case-sensitive installation confirmation token.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$SourceWrapper,
  [Parameter(Mandatory = $true)][string]$SourceLauncher,
  [Parameter(Mandatory = $true)][string]$SourceKernelManifest,
  [Parameter(Mandatory = $true)][string]$SourceKernel,
  [Parameter(Mandatory = $true)][string]$SourceModules,
  [Parameter(Mandatory = $true)][string]$SourceLayoutInventory,
  [Parameter(Mandatory = $true)][string]$SourceQemuStamp,
  [Parameter(Mandatory = $true)][string]$ExpectedWrapperSha256,
  [Parameter(Mandatory = $true)][string]$ExpectedLauncherSha256,
  [Parameter(Mandatory = $true)][string]$ExpectedKernelManifestSha256,
  [Parameter(Mandatory = $true)][string]$ExpectedKernelSha256,
  [Parameter(Mandatory = $true)][string]$ExpectedModulesSha256,
  [Parameter(Mandatory = $true)][string]$ExpectedLayoutInventorySha256,
  [Parameter(Mandatory = $true)][string]$ExpectedQemuStampSha256,
  [string]$InstallRoot = 'C:\wsl\ramshared-launchers',
  [switch]$EvaluateInstallFixture,
  [string]$FixtureRoot = '',
  [string]$FixtureNonce = '',
  [switch]$Run,
  [string]$ConfirmationToken = '',
  [switch]$InternalSuspendedWorker,
  [long]$InternalJobHandle = 0
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$script:InstallerClock = [System.Diagnostics.Stopwatch]::StartNew()
$script:MaximumInputBytes = [long]68719476736

function Initialize-InstallerJobType {
  if ($null -ne ('RamSharedSuspendedJobProcess' -as [type])) { return }
  Add-Type -TypeDefinition @'
// RAMSHARED_SUSPENDED_JOB_CSHARP_BEGIN
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
// RAMSHARED_SUSPENDED_JOB_CSHARP_END
'@
}

function ConvertTo-InstallerWorkerArgument([string]$Value) {
  if ($Value.Length -gt 0 -and $Value -cnotmatch '[\s"]') { return $Value }
  $escaped = $Value -replace '(\\*)"', '$1$1\"'
  $escaped = $escaped -replace '(\\+)$', '$1$1'
  return '"' + $escaped + '"'
}

$requiredConfirmationToken = 'INSTALL_IMMUTABLE_KERNEL_LAUNCHER_BUNDLE'
$liveAuthoritySupplied = $PSBoundParameters.ContainsKey('Run') -or $PSBoundParameters.ContainsKey('ConfirmationToken')
$fixtureInputPresent = $EvaluateInstallFixture.IsPresent -or $PSBoundParameters.ContainsKey('FixtureRoot') -or $PSBoundParameters.ContainsKey('FixtureNonce')
if ($fixtureInputPresent -and $liveAuthoritySupplied) {
  Write-Output 'FIXTURE_LIVE_AUTHORITY=REFUSED'
  exit 2
}
if ($fixtureInputPresent -and -not $EvaluateInstallFixture.IsPresent) {
  Write-Output 'FIXTURE_COMBINATION=REFUSED install-fixture-switch-required'
  exit 2
}
if (-not $EvaluateInstallFixture.IsPresent -and
    (-not $Run.IsPresent -or $ConfirmationToken -cne $requiredConfirmationToken)) {
  Write-Output 'PLAN: immutable kernel launcher bundle installation is inert.'
  Write-Output ('LIVE_GATE=REFUSED require=-Run token=' + $requiredConfirmationToken)
  exit 2
}

if ($EvaluateInstallFixture.IsPresent) {
  if ($InternalSuspendedWorker.IsPresent) {
    try {
      Initialize-InstallerJobType
      [RamSharedSuspendedJobProcess]::ValidateAndCloseInheritedJobHandle($InternalJobHandle)
    } catch {
      Write-Output ('INTERNAL_CUSTODY=REFUSED ' + $_.Exception.Message)
      exit 2
    }
  } else {
    Initialize-InstallerJobType
    $workerArguments = @(
      '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
      '-File', $PSCommandPath,
      '-InternalSuspendedWorker', '-InternalJobHandle', '{RAMSHARED_JOB_HANDLE}'
    )
    foreach ($entry in $PSBoundParameters.GetEnumerator()) {
      if ($entry.Key -ceq 'InternalSuspendedWorker' -or $entry.Key -ceq 'InternalJobHandle') { continue }
      if ($entry.Value -is [System.Management.Automation.SwitchParameter]) {
        if ($entry.Value.IsPresent) { $workerArguments += ('-' + $entry.Key) }
      } else {
        $workerArguments += @([string]('-' + $entry.Key), [string]$entry.Value)
      }
    }
    $workerDeadlineMs = 180000 - [int]$script:InstallerClock.ElapsedMilliseconds
    if ($workerDeadlineMs -lt 1) { throw 'installer deadline expired before suspended worker creation' }
    $workerLine = (@($workerArguments | ForEach-Object { ConvertTo-InstallerWorkerArgument $_ }) -join ' ')
    $worker = $null
    try {
      $worker = [RamSharedSuspendedJobProcess]::Start(
        (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'),
        $workerLine,
        $workerDeadlineMs,
        1048576,
        $false,
        $false,
        $false,
        $false,
        'direct installer suspended worker'
      )
      $workerResult = $worker.Wait()
      if (-not $workerResult.JobEmpty) { throw 'direct installer worker lacked empty-job proof' }
      if (-not [string]::IsNullOrWhiteSpace($workerResult.Stdout)) { Write-Output $workerResult.Stdout.TrimEnd() }
      if (-not [string]::IsNullOrWhiteSpace($workerResult.Stderr)) { Write-Output $workerResult.Stderr.TrimEnd() }
      exit $workerResult.ExitCode
    } finally {
      if ($null -ne $worker) { $worker.Dispose() }
    }
  }
}

function Get-Sha256([string]$Path) {
  $stream = [System.IO.File]::Open(
    $Path,
    [System.IO.FileMode]::Open,
    [System.IO.FileAccess]::Read,
    [System.IO.FileShare]::Read
  )
  try {
    if ($stream.Length -gt $script:MaximumInputBytes) {
      throw 'hash input exceeds the 64 GiB invocation limit'
    }
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
      $buffer = New-Object 'byte[]' 1048576
      while ($true) {
        if ($script:InstallerClock.ElapsedMilliseconds -ge 180000) {
          throw 'hash input exceeded the installer deadline'
        }
        $count = $stream.Read($buffer, 0, $buffer.Length)
        if ($count -eq 0) { break }
        [void]$algorithm.TransformBlock($buffer, 0, $count, $buffer, 0)
      }
      [void]$algorithm.TransformFinalBlock((New-Object 'byte[]' 0), 0, 0)
      return ([System.BitConverter]::ToString($algorithm.Hash)).Replace('-', '').ToLowerInvariant()
    } finally {
      $algorithm.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

function Assert-Sha256([string]$Value, [string]$Name) {
  if ($Value -cnotmatch '^[0-9a-f]{64}$') {
    throw "$Name must be one lowercase SHA-256 value"
  }
}

function Assert-WindowsPathLexicalSyntax([string]$Path, [string]$Name) {
  if ([string]::IsNullOrWhiteSpace($Path)) {
    throw "$Name must use one non-blank absolute Windows drive path"
  }
  if ($Path -match '[\x00-\x1f<>"|?*]' -or
      $Path.StartsWith('\\') -or $Path.StartsWith('//') -or
      $Path.StartsWith('\\?\') -or $Path.StartsWith('\\.\') -or
      $Path -cnotmatch '^[A-Za-z]:[\\/]' -or
      $Path.Substring(2).Contains(':') -or
      $Path -match '(^|[\\/])\.\.?([\\/]|$)') {
    throw "$Name contains a namespace, UNC, ADS, traversal, control, or invalid-character form"
  }
  foreach ($segment in @($Path.Substring(3) -split '[\\/]')) {
    if ($segment.Length -eq 0 -or $segment.EndsWith('.') -or $segment.EndsWith(' ') -or
        $segment -match '~[0-9]+') {
      throw "$Name contains an empty, trailing-dot/space, or 8.3 segment"
    }
    $stem = ($segment -split '\.', 2)[0]
    if ($stem -match '^(CON|PRN|AUX|NUL|CLOCK\$|COM[1-9]|LPT[1-9])$') {
      throw "$Name DOS_DEVICE_SEGMENT is forbidden: $segment"
    }
  }
}

function Assert-SafeWindowsPath([string]$Path, [string]$Name) {
  Assert-WindowsPathLexicalSyntax $Path $Name
  if ([string]::IsNullOrWhiteSpace($Path) -or -not [System.IO.Path]::IsPathRooted($Path) -or
      $Path.StartsWith('\\') -or $Path.StartsWith('//') -or
      $Path.StartsWith('\\?\') -or $Path.StartsWith('\\.\') -or
      $Path -match '(^|[\\/])\.\.?([\\/]|$)' -or
      $Path -match '(^|[\\/])[^\\/]*~[0-9]+(?:[\\/]|$)' -or
      $Path -cnotmatch '^[A-Za-z]:[\\/][^:]*$') {
    throw "$Name contains an unsafe or non-canonical Windows path form"
  }
  $fullPath = [System.IO.Path]::GetFullPath($Path)
  if ($fullPath -cne $Path.TrimEnd('\', '/')) {
    throw "$Name is not a canonical absolute path"
  }
  return $fullPath
}

function Assert-NoReparseAncestors([string]$Path, [string]$Name) {
  $cursor = if (Test-Path -LiteralPath $Path) { $Path } else { Split-Path -Parent $Path }
  while (-not [string]::IsNullOrWhiteSpace($cursor)) {
    if (Test-Path -LiteralPath $cursor) {
      $item = Get-Item -LiteralPath $cursor -Force
      if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Name has a reparse-point ancestor"
      }
      $root = [System.IO.Path]::GetPathRoot($cursor).TrimEnd('\', '/')
      $canonical = [System.IO.Path]::GetFullPath($cursor).TrimEnd('\', '/')
      if ($canonical -cne $root -and $item.FullName.TrimEnd('\', '/') -cne $canonical) {
        throw "$Name has a case-colliding ancestor"
      }
    }
    $parent = Split-Path -Parent $cursor
    if ([string]::IsNullOrWhiteSpace($parent) -or $parent -ceq $cursor) { break }
    # Keep the rooted form (C:\); trimming it produces drive-relative C:.
    $cursor = $parent
  }
}

function Assert-FixtureContext([string]$Root, [string]$Nonce) {
  if ($Nonce -cnotmatch '^[0-9a-f]{32}$') {
    throw 'FixtureNonce must be one lowercase 128-bit nonce'
  }
  $fullRoot = Assert-SafeWindowsPath $Root 'FixtureRoot'
  $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\', '/')
  if ((Split-Path -Parent $fullRoot).TrimEnd('\', '/') -cne $tempRoot -or
      (Split-Path -Leaf $fullRoot) -cne ('ramshared-kernel-static-' + $Nonce)) {
    throw 'FixtureRoot must be one canonical fresh direct child of the Windows temporary directory'
  }
  if (-not (Test-Path -LiteralPath $fullRoot -PathType Container)) {
    throw 'FixtureRoot must already exist'
  }
  Assert-NoReparseAncestors $fullRoot 'FixtureRoot'
  $marker = Join-Path $fullRoot '.ramshared-kernel-fixture-root'
  if (-not (Test-Path -LiteralPath $marker -PathType Leaf) -or
      [System.IO.File]::ReadAllText($marker) -cne ('ramshared.kernel-fixture.v1:' + $Nonce)) {
    throw 'FixtureRoot marker does not bind the supplied nonce'
  }
  Assert-NoReparseAncestors $marker 'FixtureRoot marker'
  $markerStream = [System.IO.File]::Open($marker, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
  try {
    if ((Get-OpenFileIdentity $markerStream).NumberOfLinks -ne 1) {
      throw 'FixtureRoot marker must have exactly one filesystem link'
    }
  } finally {
    $markerStream.Dispose()
  }
  return $fullRoot
}

function Assert-FixturePath([string]$Path, [string]$Name, [string]$Root) {
  $fullPath = Assert-SafeWindowsPath $Path $Name
  if (-not $fullPath.StartsWith($Root.TrimEnd('\', '/') + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "$Name must remain beneath FixtureRoot"
  }
  Assert-NoReparseAncestors $fullPath $Name
  return $fullPath
}

function Initialize-OwnedTreeDeletionType {
  if ($null -ne ('RamSharedOwnedTreeDeletion' -as [type])) { return }
  Add-Type -TypeDefinition @'
// RAMSHARED_OWNED_TREE_CSHARP_BEGIN
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;

public static class RamSharedOwnedTreeDeletion {
    const uint DELETE = 0x00010000;
    const uint FILE_READ_ATTRIBUTES = 0x00000080;
    const uint FILE_WRITE_ATTRIBUTES = 0x00000100;
    const uint FILE_SHARE_ALL = 0x00000007;
    const uint OPEN_EXISTING = 3;
    const uint FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000;
    const uint FILE_FLAG_BACKUP_SEMANTICS = 0x02000000;
    const uint FILE_ATTRIBUTE_DIRECTORY = 0x00000010;
    const uint FILE_ATTRIBUTE_REPARSE_POINT = 0x00000400;
    const uint FILE_ATTRIBUTE_READONLY = 0x00000001;
    const int FileBasicInfo = 0;
    const int FileDispositionInfo = 4;

    [StructLayout(LayoutKind.Sequential)]
    internal struct FILETIME { public uint Low; public uint High; }

    [StructLayout(LayoutKind.Sequential)]
    internal struct BY_HANDLE_FILE_INFORMATION {
        public uint FileAttributes;
        public FILETIME CreationTime;
        public FILETIME LastAccessTime;
        public FILETIME LastWriteTime;
        public uint VolumeSerialNumber;
        public uint FileSizeHigh;
        public uint FileSizeLow;
        public uint NumberOfLinks;
        public uint FileIndexHigh;
        public uint FileIndexLow;
    }

    public sealed class OwnedIdentity : IDisposable {
        internal IntPtr Handle;
        internal BY_HANDLE_FILE_INFORMATION Snapshot;

        internal OwnedIdentity(IntPtr handle,
            BY_HANDLE_FILE_INFORMATION snapshot) {
            Handle = handle;
            Snapshot = snapshot;
        }

        public ulong Volume {
            get { return Snapshot.VolumeSerialNumber; }
        }

        public ulong Index {
            get { return RamSharedOwnedTreeDeletion.Index(Snapshot); }
        }

        public ulong Creation {
            get { return RamSharedOwnedTreeDeletion.FileTimeValue(Snapshot.CreationTime); }
        }

        internal void AssertActive() {
            if (Handle == IntPtr.Zero || Handle == new IntPtr(-1))
                throw new ObjectDisposedException("owned cleanup identity");
        }

        public void Dispose() {
            RamSharedOwnedTreeDeletion.CheckedClose(ref Handle);
            GC.SuppressFinalize(this);
        }

        ~OwnedIdentity() {
            RamSharedOwnedTreeDeletion.CloseNoThrow(ref Handle);
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    struct FILE_BASIC_INFO {
        public long CreationTime;
        public long LastAccessTime;
        public long LastWriteTime;
        public long ChangeTime;
        public uint FileAttributes;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct FILE_DISPOSITION_INFO {
        [MarshalAs(UnmanagedType.Bool)] public bool DeleteFile;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateFileW(string name, uint access, uint share,
        IntPtr securityAttributes, uint creation, uint flags, IntPtr template);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool GetFileInformationByHandle(IntPtr handle,
        out BY_HANDLE_FILE_INFORMATION information);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool SetFileInformationByHandle(IntPtr handle, int informationClass,
        IntPtr information, uint informationLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool CloseHandle(IntPtr handle);

    static Exception NativeFailure(string operation) {
        return new Win32Exception(Marshal.GetLastWin32Error(), operation + " failed");
    }

    static IntPtr OpenNode(string path, bool deleteAccess) {
        uint access = FILE_READ_ATTRIBUTES;
        if (deleteAccess) access |= DELETE | FILE_WRITE_ATTRIBUTES;
        IntPtr handle = CreateFileW(path, access, FILE_SHARE_ALL, IntPtr.Zero,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            IntPtr.Zero);
        if (handle == new IntPtr(-1)) throw NativeFailure("CreateFileW(owned node)");
        return handle;
    }

    static BY_HANDLE_FILE_INFORMATION ReadInfo(IntPtr handle) {
        BY_HANDLE_FILE_INFORMATION information;
        if (!GetFileInformationByHandle(handle, out information))
            throw NativeFailure("GetFileInformationByHandle(owned node)");
        if ((information.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0)
            throw new InvalidOperationException("owned cleanup refuses a reparse point");
        ulong index = ((ulong)information.FileIndexHigh << 32) | information.FileIndexLow;
        if (index == 0) throw new InvalidOperationException("owned cleanup refuses an empty file identity");
        if ((information.FileAttributes & FILE_ATTRIBUTE_DIRECTORY) == 0 &&
                information.NumberOfLinks != 1)
            throw new InvalidOperationException("owned cleanup refuses a multiply-linked file");
        return information;
    }

    static ulong Index(BY_HANDLE_FILE_INFORMATION information) {
        return ((ulong)information.FileIndexHigh << 32) | information.FileIndexLow;
    }

    internal static ulong FileTimeValue(FILETIME value) {
        return ((ulong)value.High << 32) | value.Low;
    }

    static bool SameIdentity(BY_HANDLE_FILE_INFORMATION left,
        BY_HANDLE_FILE_INFORMATION right) {
        return left.VolumeSerialNumber == right.VolumeSerialNumber &&
            Index(left) == Index(right) &&
            FileTimeValue(left.CreationTime) == FileTimeValue(right.CreationTime) &&
            ((left.FileAttributes ^ right.FileAttributes) & FILE_ATTRIBUTE_DIRECTORY) == 0;
    }

    internal static void CheckedClose(ref IntPtr handle) {
        if (handle == IntPtr.Zero || handle == new IntPtr(-1)) return;
        IntPtr value = handle;
        handle = IntPtr.Zero;
        if (!CloseHandle(value)) throw NativeFailure("CloseHandle(owned node)");
    }

    internal static void CloseNoThrow(ref IntPtr handle) {
        if (handle == IntPtr.Zero || handle == new IntPtr(-1)) return;
        IntPtr value = handle;
        handle = IntPtr.Zero;
        CloseHandle(value);
    }

    static void AssertPathStillNames(string path,
        BY_HANDLE_FILE_INFORMATION expected) {
        IntPtr probe = IntPtr.Zero;
        try {
            probe = OpenNode(path, false);
            BY_HANDLE_FILE_INFORMATION actual = ReadInfo(probe);
            if (!SameIdentity(expected, actual))
                throw new InvalidOperationException(
                    "owned cleanup path identity changed; replacement was not touched");
            CheckedClose(ref probe);
        } finally {
            CloseNoThrow(ref probe);
        }
    }

    static void ClearReadOnly(IntPtr handle,
        BY_HANDLE_FILE_INFORMATION information) {
        if ((information.FileAttributes & FILE_ATTRIBUTE_READONLY) == 0) return;
        FILE_BASIC_INFO basic = new FILE_BASIC_INFO();
        basic.FileAttributes = information.FileAttributes & ~FILE_ATTRIBUTE_READONLY;
        int size = Marshal.SizeOf(typeof(FILE_BASIC_INFO));
        IntPtr buffer = Marshal.AllocHGlobal(size);
        try {
            Marshal.StructureToPtr(basic, buffer, false);
            if (!SetFileInformationByHandle(handle, FileBasicInfo, buffer, (uint)size))
                throw NativeFailure("SetFileInformationByHandle(FileBasicInfo)");
        } finally {
            Marshal.FreeHGlobal(buffer);
        }
    }

    static void MarkDelete(IntPtr handle) {
        FILE_DISPOSITION_INFO disposition = new FILE_DISPOSITION_INFO();
        disposition.DeleteFile = true;
        int size = Marshal.SizeOf(typeof(FILE_DISPOSITION_INFO));
        IntPtr buffer = Marshal.AllocHGlobal(size);
        try {
            Marshal.StructureToPtr(disposition, buffer, false);
            if (!SetFileInformationByHandle(handle, FileDispositionInfo,
                    buffer, (uint)size))
                throw NativeFailure("SetFileInformationByHandle(FileDispositionInfo)");
        } finally {
            Marshal.FreeHGlobal(buffer);
        }
    }

    static string replacementFixtureStage;
    static string replacementFixtureRoot;
    static string replacementFixtureMoved;
    static string replacementFixtureSentinel;
    static string injectedReplacementStage;

    static void MaybeInjectReplacement(string stage, string path) {
        if (replacementFixtureStage == null ||
                !String.Equals(replacementFixtureStage, stage,
                    StringComparison.Ordinal) ||
                !String.Equals(replacementFixtureRoot, path,
                    StringComparison.OrdinalIgnoreCase)) return;
        replacementFixtureStage = null;
        Directory.Move(path, replacementFixtureMoved);
        Directory.CreateDirectory(path);
        File.WriteAllText(replacementFixtureSentinel,
            "replacement-must-survive:" + stage);
        injectedReplacementStage = stage;
    }

    static void AssertPathAtStage(string stage, string path,
        BY_HANDLE_FILE_INFORMATION expected) {
        MaybeInjectReplacement(stage, path);
        try {
            AssertPathStillNames(path, expected);
        } catch (InvalidOperationException error) {
            if (injectedReplacementStage != null)
                throw new InvalidOperationException(
                    "owned cleanup identity changed at stage=" +
                    injectedReplacementStage +
                    "; replacement was not touched", error);
            throw;
        }
    }

    static void DeleteNode(string path, string fixtureRoot,
        ref IntPtr retained, BY_HANDLE_FILE_INFORMATION expected) {
        AssertPathAtStage("before-node-validation", path, expected);
        if ((expected.FileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0) {
            if (String.Equals(path, fixtureRoot, StringComparison.OrdinalIgnoreCase))
                AssertPathAtStage("before-enumeration", path, expected);
            string[] entries = Directory.GetFileSystemEntries(path);
            foreach (string entry in entries) {
                if (String.Equals(path, fixtureRoot, StringComparison.OrdinalIgnoreCase)) {
                    AssertPathAtStage("before-child-open", path, expected);
                    AssertPathAtStage("before-child-delete", path, expected);
                } else {
                    AssertPathStillNames(path, expected);
                }
                IntPtr child = IntPtr.Zero;
                try {
                    child = OpenNode(entry, true);
                    BY_HANDLE_FILE_INFORMATION childInfo = ReadInfo(child);
                    AssertPathStillNames(path, expected);
                    AssertPathStillNames(entry, childInfo);
                    DeleteNode(entry, fixtureRoot, ref child, childInfo);
                } finally {
                    CloseNoThrow(ref child);
                }
            }
            AssertPathStillNames(path, expected);
            if (Directory.GetFileSystemEntries(path).Length != 0)
                throw new InvalidOperationException("owned cleanup directory changed during deletion");
        }
        ClearReadOnly(retained, expected);
        if (String.Equals(path, fixtureRoot, StringComparison.OrdinalIgnoreCase))
            AssertPathAtStage("before-root-delete", path, expected);
        else
            AssertPathStillNames(path, expected);
        MarkDelete(retained);
        CheckedClose(ref retained);
    }

    public static OwnedIdentity Capture(string path) {
        IntPtr handle = IntPtr.Zero;
        try {
            handle = OpenNode(path, true);
            BY_HANDLE_FILE_INFORMATION information = ReadInfo(handle);
            OwnedIdentity identity = new OwnedIdentity(handle, information);
            handle = IntPtr.Zero;
            return identity;
        } finally {
            CloseNoThrow(ref handle);
        }
    }

    static void ValidateRetainedRoot(OwnedIdentity identity,
        out BY_HANDLE_FILE_INFORMATION information) {
        if (identity == null) throw new ArgumentNullException("identity");
        identity.AssertActive();
        information = ReadInfo(identity.Handle);
        if ((information.FileAttributes & FILE_ATTRIBUTE_DIRECTORY) == 0 ||
                !SameIdentity(identity.Snapshot, information))
            throw new InvalidOperationException(
                "owned cleanup retained identity changed; replacement was not touched");
    }

    public static void DeleteTree(string path, OwnedIdentity identity) {
        BY_HANDLE_FILE_INFORMATION information;
        ValidateRetainedRoot(identity, out information);
        AssertPathAtStage("before-root-validation", path, information);
        DeleteNode(path, path, ref identity.Handle, information);
        GC.SuppressFinalize(identity);
    }

    public static void DeleteTreeWithReplacementFixture(string path,
        OwnedIdentity identity, string stage, string movedPath,
        string sentinelPath) {
        if (String.IsNullOrEmpty(stage) || String.IsNullOrEmpty(movedPath) ||
                String.IsNullOrEmpty(sentinelPath))
            throw new ArgumentException("replacement fixture arguments are required");
        replacementFixtureStage = stage;
        replacementFixtureRoot = path;
        replacementFixtureMoved = movedPath;
        replacementFixtureSentinel = sentinelPath;
        injectedReplacementStage = null;
        try {
            DeleteTree(path, identity);
        } finally {
            replacementFixtureStage = null;
            replacementFixtureRoot = null;
            replacementFixtureMoved = null;
            replacementFixtureSentinel = null;
            injectedReplacementStage = null;
        }
    }

    public static void DeleteEmptyDirectory(string path,
        OwnedIdentity identity) {
        BY_HANDLE_FILE_INFORMATION information;
        ValidateRetainedRoot(identity, out information);
        AssertPathStillNames(path, information);
        MarkDelete(identity.Handle);
        CheckedClose(ref identity.Handle);
        GC.SuppressFinalize(identity);
    }
}
// RAMSHARED_OWNED_TREE_CSHARP_END
'@
}

function Get-OwnedTreeIdentity([string]$Path) {
  Initialize-OwnedTreeDeletionType
  return [RamSharedOwnedTreeDeletion]::Capture($Path)
}

function Remove-OwnedTree([string]$Path, [object]$Identity) {
  Initialize-OwnedTreeDeletionType
  [RamSharedOwnedTreeDeletion]::DeleteTree($Path, $Identity)
}

function Remove-OwnedEmptyDirectory([string]$Path, [object]$Identity) {
  Initialize-OwnedTreeDeletionType
  [RamSharedOwnedTreeDeletion]::DeleteEmptyDirectory($Path, $Identity)
}

function Initialize-FileIdentityType {
  if ($null -ne ('RamSharedInstallerFileIdentity' -as [type])) { return }
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class RamSharedInstallerFileIdentity {
    [StructLayout(LayoutKind.Sequential)] public struct FT { public uint Low; public uint High; }
    [StructLayout(LayoutKind.Sequential)] public struct INFO {
        public uint Attr; public FT Create; public FT Access; public FT Write;
        public uint Volume; public uint SizeHigh; public uint SizeLow;
        public uint Links; public uint IndexHigh; public uint IndexLow;
    }
    [DllImport("kernel32.dll", SetLastError=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool GetFileInformationByHandle(SafeFileHandle handle, out INFO info);
    public static ulong[] Read(SafeFileHandle handle) {
        INFO info;
        if (!GetFileInformationByHandle(handle, out info)) {
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        }
        ulong index = ((ulong)info.IndexHigh << 32) | info.IndexLow;
        return new ulong[] { info.Volume, index, info.Links };
    }
}
'@
}

function Get-OpenFileIdentity([System.IO.FileStream]$Stream) {
  Initialize-FileIdentityType
  $value = [RamSharedInstallerFileIdentity]::Read($Stream.SafeFileHandle)
  return [pscustomobject]@{ Volume = $value[0]; Index = $value[1]; NumberOfLinks = $value[2] }
}

function Enter-InstallLock([string]$CanonicalInstallRoot) {
  $algorithm = [System.Security.Cryptography.SHA256]::Create()
  try {
    $bytes = (New-Object System.Text.UTF8Encoding($false)).GetBytes($CanonicalInstallRoot.ToLowerInvariant())
    $hash = ([System.BitConverter]::ToString($algorithm.ComputeHash($bytes))).Replace('-', '')
  } finally {
    $algorithm.Dispose()
  }
  $mutex = New-Object System.Threading.Mutex($false, ('Global\RamSharedKernelInstall-' + $hash))
  $acquired = $false
  try {
    try { $acquired = $mutex.WaitOne(3000, $false) } catch [System.Threading.AbandonedMutexException] { $acquired = $true }
    if (-not $acquired) {
      throw 'INSTALL_LOCK=REFUSED another canonical install transaction owns the bounded lock'
    }
    return [pscustomobject]@{ Mutex = $mutex; Acquired = $true }
  } catch {
    if (-not $acquired) { $mutex.Dispose() }
    throw
  }
}

function Exit-InstallLock([object]$Lock) {
  if ($null -ne $Lock) {
    if ($Lock.Acquired) { $Lock.Mutex.ReleaseMutex() }
    $Lock.Mutex.Dispose()
  }
}

function Assert-RegularSource([string]$Path, [string]$ExpectedSha256, [string]$Name) {
  $fullPath = Assert-SafeWindowsPath $Path $Name
  Assert-NoReparseAncestors $fullPath $Name
  if (-not (Test-Path -LiteralPath $fullPath -PathType Leaf)) {
    throw "$Name is missing: $fullPath"
  }
  $item = Get-Item -LiteralPath $fullPath -Force
  if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "$Name must not be a reparse point"
  }
  if ($item.Length -le 0) {
    throw "$Name must not be empty"
  }
  $stream = [System.IO.File]::Open($item.FullName, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
  try {
    if ((Get-OpenFileIdentity $stream).NumberOfLinks -ne 1) {
      throw "$Name must have exactly one filesystem link"
    }
  } finally {
    $stream.Dispose()
  }
  $actual = Get-Sha256 $item.FullName
  if ($actual -cne $ExpectedSha256) {
    throw "$Name SHA-256 mismatch: expected=$ExpectedSha256 actual=$actual"
  }
  return $item.FullName
}

function Write-Utf8NoBom([string]$Path, [string]$Text) {
  $encoding = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $Text, $encoding)
}

function Copy-SealedSource(
  [string]$Source,
  [string]$Destination,
  [string]$ExpectedSha256,
  [string]$Name
) {
  $sourceStream = $null
  $destinationStream = $null
  $algorithm = $null
  try {
    Assert-NoReparseAncestors $Source $Name
    Assert-NoReparseAncestors $Destination "$Name destination"
    $sourceStream = [System.IO.File]::Open(
      $Source,
      [System.IO.FileMode]::Open,
      [System.IO.FileAccess]::Read,
      [System.IO.FileShare]::Read
    )
    $identityBefore = Get-OpenFileIdentity $sourceStream
    if ($identityBefore.NumberOfLinks -ne 1) {
      throw "$Name must have exactly one filesystem link"
    }
    $destinationStream = New-Object System.IO.FileStream(
      $Destination,
      [System.IO.FileMode]::CreateNew,
      [System.IO.FileAccess]::Write,
      [System.IO.FileShare]::None
    )
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    $buffer = New-Object 'byte[]' 1048576
    $copied = [long]0
    while (($count = $sourceStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
      $destinationStream.Write($buffer, 0, $count)
      [void]$algorithm.TransformBlock($buffer, 0, $count, $buffer, 0)
      $copied += $count
    }
    [void]$algorithm.TransformFinalBlock((New-Object 'byte[]' 0), 0, 0)
    $destinationStream.Flush($true)
    $destinationStream.Dispose()
    $destinationStream = $null
    $actualSha256 = ([System.BitConverter]::ToString($algorithm.Hash)).Replace('-', '').ToLowerInvariant()
    $identityAfter = Get-OpenFileIdentity $sourceStream
    if ($identityAfter.Volume -ne $identityBefore.Volume -or
        $identityAfter.Index -ne $identityBefore.Index -or
        $identityAfter.NumberOfLinks -ne 1) {
      throw "$Name identity changed while it was copied"
    }
    if ($copied -le 0 -or $actualSha256 -cne $ExpectedSha256) {
      throw "$Name changed or differs from its expected SHA-256 during sealed copy"
    }
    $readback = Get-Item -LiteralPath $Destination -Force
    if ($readback.Length -ne $copied -or (Get-Sha256 $Destination) -cne $ExpectedSha256) {
      throw "$Name destination readback differs from the locked source handle"
    }
    return $copied
  } catch {
    if (Test-Path -LiteralPath $Destination -PathType Leaf) {
      Remove-Item -LiteralPath $Destination -Force
    }
    throw
  } finally {
    if ($null -ne $algorithm) { $algorithm.Dispose() }
    if ($null -ne $destinationStream) { $destinationStream.Dispose() }
    if ($null -ne $sourceStream) { $sourceStream.Dispose() }
  }
}

function Assert-NoReparseDirectory([string]$Path, [string]$Name) {
  Assert-NoReparseAncestors $Path $Name
  if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
    throw "$Name is missing: $Path"
  }
  $item = Get-Item -LiteralPath $Path -Force
  if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "$Name must not be a reparse point"
  }
}

function Assert-InstalledBundle(
  [string]$Directory,
  [string]$WrapperSha256,
  [string]$LauncherSha256,
  [string]$KernelManifestSha256,
  [string]$KernelSha256,
  [string]$ModulesSha256,
  [string]$LayoutInventorySha256,
  [string]$QemuStampSha256
) {
  Assert-NoReparseDirectory $Directory 'installed launcher directory'
  $expected = @{
    'boot-kernel-logged.ps1' = $WrapperSha256
    'boot-kernel-safe.ps1' = $LauncherSha256
    'kernel-pair.manifest' = $KernelManifestSha256
    'kernel.bzImage' = $KernelSha256
    'modules.vhdx' = $ModulesSha256
    'modules-layout.manifest' = $LayoutInventorySha256
    'qemu-pass.stamp' = $QemuStampSha256
  }
  foreach ($name in $expected.Keys) {
    $path = Join-Path $Directory $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
      throw "installed promotion bundle is incomplete: $name"
    }
    $item = Get-Item -LiteralPath $path -Force
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
      throw "installed promotion bundle contains a reparse point: $name"
    }
    $stream = [System.IO.File]::Open($item.FullName, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
    try {
      if ((Get-OpenFileIdentity $stream).NumberOfLinks -ne 1) {
        throw "installed promotion bundle contains a hard link: $name"
      }
    } finally {
      $stream.Dispose()
    }
    if ((Get-Sha256 $path) -cne $expected[$name]) {
      throw "installed promotion bundle hash mismatch: $name"
    }
  }
}

function Assert-DeploymentManifest(
  [string]$Path,
  [string]$BundleId,
  [string]$WrapperSha256,
  [string]$LauncherSha256,
  [string]$KernelManifestSha256,
  [string]$KernelSha256,
  [string]$ModulesSha256,
  [string]$LayoutInventorySha256,
  [string]$QemuStampSha256
) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw 'published launcher deployment manifest is missing'
  }
  $data = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
  $expectedProperties = @(
    'schema', 'bundle_id',
    'wrapper_file', 'wrapper_sha256',
    'launcher_file', 'launcher_sha256',
    'kernel_manifest_file', 'kernel_manifest_sha256',
    'kernel_file', 'kernel_sha256',
    'modules_file', 'modules_sha256',
    'layout_inventory_file', 'layout_inventory_sha256',
    'qemu_stamp_file', 'qemu_stamp_sha256'
  )
  $actualProperties = @($data.PSObject.Properties.Name | Sort-Object)
  if (($actualProperties -join "`n") -cne (($expectedProperties | Sort-Object) -join "`n")) {
    throw 'published launcher deployment manifest contains missing or unknown properties'
  }
  if ($data.schema -cne 'ramshared.kernel-launcher-deployment.v1' -or
      $data.bundle_id -cne $BundleId -or
      $data.wrapper_file -cne 'boot-kernel-logged.ps1' -or
      $data.wrapper_sha256 -cne $WrapperSha256 -or
      $data.launcher_file -cne 'boot-kernel-safe.ps1' -or
      $data.launcher_sha256 -cne $LauncherSha256 -or
      $data.kernel_manifest_file -cne 'kernel-pair.manifest' -or
      $data.kernel_manifest_sha256 -cne $KernelManifestSha256 -or
      $data.kernel_file -cne 'kernel.bzImage' -or
      $data.kernel_sha256 -cne $KernelSha256 -or
      $data.modules_file -cne 'modules.vhdx' -or
      $data.modules_sha256 -cne $ModulesSha256 -or
      $data.layout_inventory_file -cne 'modules-layout.manifest' -or
      $data.layout_inventory_sha256 -cne $LayoutInventorySha256 -or
      $data.qemu_stamp_file -cne 'qemu-pass.stamp' -or
      $data.qemu_stamp_sha256 -cne $QemuStampSha256) {
    throw 'published launcher deployment manifest does not bind the exact bundle'
  }
}

$installRootFull = ''
try {
  $installRootFull = Assert-SafeWindowsPath $InstallRoot 'InstallRoot'
} catch {
  if ($EvaluateInstallFixture.IsPresent) {
    Write-Output ('FIXTURE_PATH=REFUSED ' + $_.Exception.Message)
    exit 2
  }
  throw
}
if ($EvaluateInstallFixture.IsPresent) {
  try {
    $fixtureRootFull = Assert-FixtureContext $FixtureRoot $FixtureNonce
    $installRootFull = Assert-FixturePath $installRootFull 'InstallRoot' $fixtureRootFull
    $SourceWrapper = Assert-FixturePath $SourceWrapper 'SourceWrapper' $fixtureRootFull
    $SourceLauncher = Assert-FixturePath $SourceLauncher 'SourceLauncher' $fixtureRootFull
    $SourceKernelManifest = Assert-FixturePath $SourceKernelManifest 'SourceKernelManifest' $fixtureRootFull
    $SourceKernel = Assert-FixturePath $SourceKernel 'SourceKernel' $fixtureRootFull
    $SourceModules = Assert-FixturePath $SourceModules 'SourceModules' $fixtureRootFull
    $SourceLayoutInventory = Assert-FixturePath $SourceLayoutInventory 'SourceLayoutInventory' $fixtureRootFull
    $SourceQemuStamp = Assert-FixturePath $SourceQemuStamp 'SourceQemuStamp' $fixtureRootFull
  } catch {
    Write-Output ('FIXTURE_PATH=REFUSED ' + $_.Exception.Message)
    exit 2
  }
} elseif ($installRootFull -cne [System.IO.Path]::GetFullPath('C:\wsl\ramshared-launchers')) {
  throw 'live InstallRoot must use the canonical C:\wsl\ramshared-launchers path'
}

Assert-Sha256 $ExpectedWrapperSha256 'ExpectedWrapperSha256'
Assert-Sha256 $ExpectedLauncherSha256 'ExpectedLauncherSha256'
Assert-Sha256 $ExpectedKernelManifestSha256 'ExpectedKernelManifestSha256'
Assert-Sha256 $ExpectedKernelSha256 'ExpectedKernelSha256'
Assert-Sha256 $ExpectedModulesSha256 'ExpectedModulesSha256'
Assert-Sha256 $ExpectedLayoutInventorySha256 'ExpectedLayoutInventorySha256'
Assert-Sha256 $ExpectedQemuStampSha256 'ExpectedQemuStampSha256'

if (-not $EvaluateInstallFixture.IsPresent) {
  foreach ($liveSourcePath in @(
    [pscustomobject]@{ Value = $SourceWrapper; Name = 'SourceWrapper' },
    [pscustomobject]@{ Value = $SourceLauncher; Name = 'SourceLauncher' },
    [pscustomobject]@{ Value = $SourceKernelManifest; Name = 'SourceKernelManifest' },
    [pscustomobject]@{ Value = $SourceKernel; Name = 'SourceKernel' },
    [pscustomobject]@{ Value = $SourceModules; Name = 'SourceModules' },
    [pscustomobject]@{ Value = $SourceLayoutInventory; Name = 'SourceLayoutInventory' },
    [pscustomobject]@{ Value = $SourceQemuStamp; Name = 'SourceQemuStamp' }
  )) {
    [void](Assert-SafeWindowsPath $liveSourcePath.Value $liveSourcePath.Name)
  }
  Write-Output 'LIVE_GATE=ACCEPTED exact-run-and-confirmation'
  Write-Output 'STAGING_CUSTODY=NO_GO direct-suspended-supervision-and-handle-execution-unproven'
  exit 2
}

$wrapperSource = Assert-RegularSource $SourceWrapper $ExpectedWrapperSha256 'SourceWrapper'
$launcherSource = Assert-RegularSource $SourceLauncher $ExpectedLauncherSha256 'SourceLauncher'
$kernelManifestSource = Assert-RegularSource $SourceKernelManifest $ExpectedKernelManifestSha256 'SourceKernelManifest'
$kernelSource = Assert-RegularSource $SourceKernel $ExpectedKernelSha256 'SourceKernel'
$modulesSource = Assert-RegularSource $SourceModules $ExpectedModulesSha256 'SourceModules'
$layoutInventorySource = Assert-RegularSource $SourceLayoutInventory $ExpectedLayoutInventorySha256 'SourceLayoutInventory'
$qemuStampSource = Assert-RegularSource $SourceQemuStamp $ExpectedQemuStampSha256 'SourceQemuStamp'
if ((Get-Item -LiteralPath $kernelSource -Force).Length -le 1048576) {
  throw 'SourceKernel must exceed 1 MiB'
}

$installLock = Enter-InstallLock $installRootFull
$installRootCreated = $false
$installSucceeded = $false
$publishedHere = $false
$installRootIdentity = $null
$stagingIdentity = $null
$publishedIdentity = $null
try {
  if (-not (Test-Path -LiteralPath $installRootFull)) {
    New-Item -ItemType Directory -Path $installRootFull | Out-Null
    $installRootCreated = $true
    $installRootIdentity = Get-OwnedTreeIdentity $installRootFull
  }
  Assert-NoReparseAncestors $installRootFull 'InstallRoot'
  Assert-NoReparseDirectory $installRootFull 'InstallRoot'

$bundleId = 'v1-' + $ExpectedKernelManifestSha256.Substring(0, 16) + '-' + $ExpectedWrapperSha256.Substring(0, 16) + '-' + $ExpectedLauncherSha256.Substring(0, 16) + '-' + $ExpectedKernelSha256.Substring(0, 16) + '-' + $ExpectedModulesSha256.Substring(0, 16)
$finalDirectory = Join-Path $installRootFull $bundleId
$stagingDirectory = Join-Path $installRootFull ('.staging-' + $bundleId + '-' + [guid]::NewGuid().ToString('N'))

try {
  if (Test-Path -LiteralPath $finalDirectory) {
    Assert-InstalledBundle $finalDirectory $ExpectedWrapperSha256 $ExpectedLauncherSha256 $ExpectedKernelManifestSha256 $ExpectedKernelSha256 $ExpectedModulesSha256 $ExpectedLayoutInventorySha256 $ExpectedQemuStampSha256
    Assert-DeploymentManifest (Join-Path $finalDirectory 'deployment.json') $bundleId $ExpectedWrapperSha256 $ExpectedLauncherSha256 $ExpectedKernelManifestSha256 $ExpectedKernelSha256 $ExpectedModulesSha256 $ExpectedLayoutInventorySha256 $ExpectedQemuStampSha256
  } else {
    New-Item -ItemType Directory -Path $stagingDirectory | Out-Null
    $stagingIdentity = Get-OwnedTreeIdentity $stagingDirectory
    [void](Copy-SealedSource $wrapperSource (Join-Path $stagingDirectory 'boot-kernel-logged.ps1') $ExpectedWrapperSha256 'SourceWrapper')
    [void](Copy-SealedSource $launcherSource (Join-Path $stagingDirectory 'boot-kernel-safe.ps1') $ExpectedLauncherSha256 'SourceLauncher')
    [void](Copy-SealedSource $kernelManifestSource (Join-Path $stagingDirectory 'kernel-pair.manifest') $ExpectedKernelManifestSha256 'SourceKernelManifest')
    $copiedKernelLength = Copy-SealedSource $kernelSource (Join-Path $stagingDirectory 'kernel.bzImage') $ExpectedKernelSha256 'SourceKernel'
    [void](Copy-SealedSource $modulesSource (Join-Path $stagingDirectory 'modules.vhdx') $ExpectedModulesSha256 'SourceModules')
    [void](Copy-SealedSource $layoutInventorySource (Join-Path $stagingDirectory 'modules-layout.manifest') $ExpectedLayoutInventorySha256 'SourceLayoutInventory')
    [void](Copy-SealedSource $qemuStampSource (Join-Path $stagingDirectory 'qemu-pass.stamp') $ExpectedQemuStampSha256 'SourceQemuStamp')
    if ($copiedKernelLength -le 1048576) {
      throw 'SourceKernel locked-handle copy must exceed 1 MiB'
    }

    Assert-InstalledBundle $stagingDirectory $ExpectedWrapperSha256 $ExpectedLauncherSha256 $ExpectedKernelManifestSha256 $ExpectedKernelSha256 $ExpectedModulesSha256 $ExpectedLayoutInventorySha256 $ExpectedQemuStampSha256

    $deployment = [ordered]@{
      schema = 'ramshared.kernel-launcher-deployment.v1'
      bundle_id = $bundleId
      wrapper_file = 'boot-kernel-logged.ps1'
      wrapper_sha256 = $ExpectedWrapperSha256
      launcher_file = 'boot-kernel-safe.ps1'
      launcher_sha256 = $ExpectedLauncherSha256
      kernel_manifest_file = 'kernel-pair.manifest'
      kernel_manifest_sha256 = $ExpectedKernelManifestSha256
      kernel_file = 'kernel.bzImage'
      kernel_sha256 = $ExpectedKernelSha256
      modules_file = 'modules.vhdx'
      modules_sha256 = $ExpectedModulesSha256
      layout_inventory_file = 'modules-layout.manifest'
      layout_inventory_sha256 = $ExpectedLayoutInventorySha256
      qemu_stamp_file = 'qemu-pass.stamp'
      qemu_stamp_sha256 = $ExpectedQemuStampSha256
    }
    $deploymentText = ($deployment | ConvertTo-Json -Compress) + [Environment]::NewLine
    Write-Utf8NoBom (Join-Path $stagingDirectory 'deployment.json') $deploymentText
    Assert-DeploymentManifest (Join-Path $stagingDirectory 'deployment.json') $bundleId $ExpectedWrapperSha256 $ExpectedLauncherSha256 $ExpectedKernelManifestSha256 $ExpectedKernelSha256 $ExpectedModulesSha256 $ExpectedLayoutInventorySha256 $ExpectedQemuStampSha256

    foreach ($stagedFile in @(Get-ChildItem -LiteralPath $stagingDirectory -Force -File)) {
      $stagedFile.IsReadOnly = $true
    }
    [System.IO.Directory]::Move($stagingDirectory, $finalDirectory)
    $publishedIdentity = Get-OwnedTreeIdentity $finalDirectory
    if ($publishedIdentity.Volume -ne $stagingIdentity.Volume -or
        $publishedIdentity.Index -ne $stagingIdentity.Index) {
      throw 'published directory identity changed across the same-volume rename'
    }
    $stagingIdentity = $null
    $publishedHere = $true
    Assert-InstalledBundle $finalDirectory $ExpectedWrapperSha256 $ExpectedLauncherSha256 $ExpectedKernelManifestSha256 $ExpectedKernelSha256 $ExpectedModulesSha256 $ExpectedLayoutInventorySha256 $ExpectedQemuStampSha256
    Assert-DeploymentManifest (Join-Path $finalDirectory 'deployment.json') $bundleId $ExpectedWrapperSha256 $ExpectedLauncherSha256 $ExpectedKernelManifestSha256 $ExpectedKernelSha256 $ExpectedModulesSha256 $ExpectedLayoutInventorySha256 $ExpectedQemuStampSha256
  }
} finally {
  if ($null -ne $stagingIdentity) {
    Remove-OwnedTree $stagingDirectory $stagingIdentity
    $stagingIdentity = $null
  }
}

$deploymentPath = Join-Path $finalDirectory 'deployment.json'
$deploymentSha256 = Get-Sha256 $deploymentPath

Write-Output 'RAMSHARED_INSTALL_SCHEMA=1'
Write-Output ('RAMSHARED_BUNDLE_ID=' + $bundleId)
Write-Output ('RAMSHARED_INSTALLED_WRAPPER=' + (Join-Path $finalDirectory 'boot-kernel-logged.ps1'))
Write-Output ('RAMSHARED_DEPLOYMENT_MANIFEST=' + $deploymentPath)
Write-Output ('RAMSHARED_DEPLOYMENT_SHA256=' + $deploymentSha256)
$installSucceeded = $true
} finally {
  if (-not $installSucceeded -and $publishedHere -and $null -ne $publishedIdentity) {
    Remove-OwnedTree $finalDirectory $publishedIdentity
    $publishedIdentity = $null
  }
  if ($installRootCreated -and -not $installSucceeded -and $null -ne $installRootIdentity) {
    Remove-OwnedEmptyDirectory $installRootFull $installRootIdentity
    $installRootIdentity = $null
  }
  Exit-InstallLock $installLock
}
