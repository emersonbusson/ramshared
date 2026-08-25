<#
.SYNOPSIS
  Verifies and runs one installed RamShared kernel promotion launcher.

.DESCRIPTION
  This wrapper has no mutable launcher default. It accepts only the deployment
  manifest installed beside itself, verifies every hash binding, and invokes
  the exact bundled launcher with Windows PowerShell 5.1. The repository path
  is not needed after WSL shutdown. The default is PLAN/refusal; live promotion
  requires -Run and the exact case-sensitive promotion confirmation token.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$DeploymentManifest,
  [Parameter(Mandatory = $true)][string]$ExpectedDeploymentSha256,
  [string]$LogPath = '',
  [string]$WslConfig = "$env:USERPROFILE\.wslconfig",
  [int]$TimeoutSec = 60,
  [string]$Distro = 'Ubuntu-24.04',
  [string]$ApprovedFailedUnits = '',
  [string]$ReceiptDirectory = 'C:\wsl\ramshared-receipts',
  [string]$EvaluateCanaryFixture = '',
  [string]$BaselineCanaryFixture = '',
  [string]$EvaluateRollbackFixture = '',
  [string]$EvaluateRuntimeFixture = '',
  [string]$DryRunConfig = '',
  [switch]$EvaluateExternalFailureFixture,
  [string]$EvaluateExternalTimeoutFixture = '',
  [switch]$InjectAssignFailureFixture,
  [switch]$InjectResumeFailureFixture,
  [switch]$InjectTerminateFailureFixture,
  [switch]$InjectRootCreationMismatchFixture,
  [int]$EvaluateDeadlineFixtureSec = 0,
  [switch]$EvaluateSlowStartupFixture,
  [switch]$EvaluateSlowHashFixture,
  [string]$EvaluateTransactionFixture = '',
  [string]$InjectFailureBoundary = '',
  [int]$TransactionLockTimeoutSec = 3,
  [int]$HoldTransactionLockMilliseconds = 0,
  [string]$TransactionLockEvidenceFixture = '',
  [string]$FixtureRoot = '',
  [string]$FixtureNonce = '',
  [switch]$PreflightOnly,
  [switch]$EvaluateLiveGateFixture,
  [switch]$Run,
  [string]$ConfirmationToken = '',
  [switch]$InternalSuspendedWorker,
  [long]$InternalJobHandle = 0
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$script:WrapperClock = [System.Diagnostics.Stopwatch]::StartNew()
$script:MaximumInputBytes = [long]68719476736

function Initialize-WrapperJobType {
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

function ConvertTo-WrapperWorkerArgument([string]$Value) {
  if ($Value.Length -gt 0 -and $Value -cnotmatch '[\s"]') { return $Value }
  $escaped = $Value -replace '(\\*)"', '$1$1\"'
  $escaped = $escaped -replace '(\\+)$', '$1$1'
  return '"' + $escaped + '"'
}

$requiredConfirmationToken = 'PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL'
$liveGateValid = $Run.IsPresent -and $ConfirmationToken -ceq $requiredConfirmationToken
$liveAuthoritySupplied = (
  $PSBoundParameters.ContainsKey('Run') -or
  $PSBoundParameters.ContainsKey('ConfirmationToken')
)
$fixtureInputPresent = (
  $PSBoundParameters.ContainsKey('EvaluateCanaryFixture') -or
  $PSBoundParameters.ContainsKey('BaselineCanaryFixture') -or
  $PSBoundParameters.ContainsKey('EvaluateRollbackFixture') -or
  $PSBoundParameters.ContainsKey('EvaluateRuntimeFixture') -or
  $PSBoundParameters.ContainsKey('DryRunConfig') -or
  $PSBoundParameters.ContainsKey('EvaluateExternalFailureFixture') -or
  $PSBoundParameters.ContainsKey('EvaluateExternalTimeoutFixture') -or
  $PSBoundParameters.ContainsKey('InjectAssignFailureFixture') -or
  $PSBoundParameters.ContainsKey('InjectResumeFailureFixture') -or
  $PSBoundParameters.ContainsKey('InjectTerminateFailureFixture') -or
  $PSBoundParameters.ContainsKey('InjectRootCreationMismatchFixture') -or
  $PSBoundParameters.ContainsKey('EvaluateDeadlineFixtureSec') -or
  $PSBoundParameters.ContainsKey('EvaluateSlowStartupFixture') -or
  $PSBoundParameters.ContainsKey('EvaluateSlowHashFixture') -or
  $PSBoundParameters.ContainsKey('EvaluateTransactionFixture') -or
  $PSBoundParameters.ContainsKey('InjectFailureBoundary') -or
  $PSBoundParameters.ContainsKey('TransactionLockTimeoutSec') -or
  $PSBoundParameters.ContainsKey('HoldTransactionLockMilliseconds') -or
  $PSBoundParameters.ContainsKey('TransactionLockEvidenceFixture') -or
  $PSBoundParameters.ContainsKey('FixtureRoot') -or
  $PSBoundParameters.ContainsKey('FixtureNonce') -or
  $EvaluateLiveGateFixture.IsPresent
)
$nonLiveModePresent = $fixtureInputPresent -or $PreflightOnly.IsPresent
$hermeticEvaluation = $nonLiveModePresent
if ($nonLiveModePresent -and $liveAuthoritySupplied) {
  Write-Output 'FIXTURE_LIVE_AUTHORITY=REFUSED'
  exit 2
}
if ($EvaluateLiveGateFixture.IsPresent) {
  Write-Output 'FIXTURE_COMBINATION=REFUSED live-gate-fixture-retired'
  exit 2
}
if (-not $hermeticEvaluation -and -not $liveGateValid) {
  Write-Output 'PLAN: WSL kernel promotion and all-WSL shutdown are inert.'
  Write-Output ('LIVE_GATE=REFUSED require=-Run token=' + $requiredConfirmationToken)
  exit 2
}
if (-not $hermeticEvaluation) {
  Write-Output 'LIVE_GATE=ACCEPTED exact-run-and-confirmation'
  Write-Output 'LIVE_PROMOTION=NO_GO direct-suspended-supervision-and-handle-execution-unproven'
  exit 2
}
if ($PreflightOnly.IsPresent -and
    (-not $PSBoundParameters.ContainsKey('EvaluateRuntimeFixture') -or
     [string]::IsNullOrWhiteSpace($EvaluateRuntimeFixture))) {
  Write-Output 'PREFLIGHT_RUNTIME_FIXTURE=REFUSED required-before-deployment-or-wsl.exe-access'
  exit 2
}
$processCustodyInjectionCount = 0
if ($InjectAssignFailureFixture.IsPresent) { $processCustodyInjectionCount++ }
if ($InjectResumeFailureFixture.IsPresent) { $processCustodyInjectionCount++ }
if ($InjectTerminateFailureFixture.IsPresent) { $processCustodyInjectionCount++ }
if ($InjectRootCreationMismatchFixture.IsPresent) { $processCustodyInjectionCount++ }
if (($processCustodyInjectionCount -gt 0) -and
    [string]::IsNullOrWhiteSpace($EvaluateExternalTimeoutFixture)) {
  Write-Output 'FIXTURE_COMBINATION=REFUSED process-custody-injection-requires-timeout-fixture'
  exit 2
}
if ($processCustodyInjectionCount -gt 1) {
  Write-Output 'FIXTURE_COMBINATION=REFUSED process-custody-injections-are-mutually-exclusive'
  exit 2
}
if ($PSBoundParameters.ContainsKey('EvaluateDeadlineFixtureSec') -and
    ($EvaluateDeadlineFixtureSec -lt 1 -or $EvaluateDeadlineFixtureSec -gt 5)) {
  Write-Output 'FIXTURE_COMBINATION=REFUSED deadline-fixture-must-be-1..5-seconds'
  exit 2
}
if ($EvaluateSlowHashFixture.IsPresent -and
    -not $PSBoundParameters.ContainsKey('EvaluateDeadlineFixtureSec')) {
  Write-Output 'FIXTURE_COMBINATION=REFUSED slow-hash-fixture-requires-bounded-deadline'
  exit 2
}
if ($EvaluateSlowStartupFixture.IsPresent -and
    -not $PSBoundParameters.ContainsKey('EvaluateDeadlineFixtureSec')) {
  Write-Output 'FIXTURE_COMBINATION=REFUSED slow-startup-fixture-requires-bounded-deadline'
  exit 2
}

if ($EvaluateSlowStartupFixture.IsPresent) {
  $startupRemainingMs = ($EvaluateDeadlineFixtureSec * 1000) -
    [int]$script:WrapperClock.ElapsedMilliseconds
  if ($startupRemainingMs -gt 0) {
    Start-Sleep -Milliseconds $startupRemainingMs
  }
  Write-Output 'PRE_CHILD_DEADLINE=REFUSED NO_CHILD_CREATED=1'
  exit 2
}

if ($InternalSuspendedWorker.IsPresent) {
  try {
    Initialize-WrapperJobType
    [RamSharedSuspendedJobProcess]::ValidateAndCloseInheritedJobHandle($InternalJobHandle)
  } catch {
    Write-Output ('INTERNAL_CUSTODY=REFUSED ' + $_.Exception.Message)
    exit 2
  }
} else {
  Initialize-WrapperJobType
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
  $wrapperWorkerBudgetSec = if ($PSBoundParameters.ContainsKey('EvaluateDeadlineFixtureSec')) {
    $EvaluateDeadlineFixtureSec
  } else {
    [Math]::Min(180, [Math]::Max(15, $TimeoutSec + 60))
  }
  $workerDeadlineMs = ($wrapperWorkerBudgetSec * 1000) -
    [int]$script:WrapperClock.ElapsedMilliseconds
  if ($workerDeadlineMs -lt 1) { throw 'wrapper deadline expired before suspended worker creation' }
  $workerLine = (@($workerArguments | ForEach-Object { ConvertTo-WrapperWorkerArgument $_ }) -join ' ')
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
      'direct wrapper suspended worker'
    )
    $workerResult = $worker.Wait()
    if (-not $workerResult.JobEmpty) { throw 'direct wrapper worker lacked empty-job proof' }
    if (-not [string]::IsNullOrWhiteSpace($workerResult.Stdout)) { Write-Output $workerResult.Stdout.TrimEnd() }
    if (-not [string]::IsNullOrWhiteSpace($workerResult.Stderr)) { Write-Output $workerResult.Stderr.TrimEnd() }
    exit $workerResult.ExitCode
  } finally {
    if ($null -ne $worker) { $worker.Dispose() }
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
        if ($script:WrapperClock.ElapsedMilliseconds -ge 180000) {
          throw 'hash input exceeded the wrapper deadline'
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
  if ((Get-FileLinkCount $marker) -ne 1) {
    throw 'FixtureRoot marker must have exactly one filesystem link'
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

function Initialize-FileIdentityType {
  if ($null -ne ('RamSharedFileIdentity' -as [type])) { return }
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class RamSharedFileIdentity {
    [StructLayout(LayoutKind.Sequential)] public struct FT { public uint Low; public uint High; }
    [StructLayout(LayoutKind.Sequential)] public struct INFO {
        public uint Attr; public FT Create; public FT Access; public FT Write;
        public uint Volume; public uint SizeHigh; public uint SizeLow;
        public uint Links; public uint IndexHigh; public uint IndexLow;
    }
    [DllImport("kernel32.dll", SetLastError=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool GetFileInformationByHandle(SafeFileHandle handle, out INFO info);
    public static uint Links(SafeFileHandle handle) {
        INFO info;
        if (!GetFileInformationByHandle(handle, out info)) {
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        }
        return info.Links;
    }
}
'@
}

function Get-FileLinkCount([string]$Path) {
  Initialize-FileIdentityType
  $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
  try { return [RamSharedFileIdentity]::Links($stream.SafeFileHandle) } finally { $stream.Dispose() }
}

function Assert-LeafHash([string]$Path, [string]$Expected, [string]$Name) {
  if ($Expected -cnotmatch '^[0-9a-f]{64}$') {
    throw "$Name expected hash is invalid"
  }
  $fullPath = Assert-SafeWindowsPath $Path $Name
  Assert-NoReparseAncestors $fullPath $Name
  if (-not (Test-Path -LiteralPath $fullPath -PathType Leaf)) {
    throw "$Name is missing: $fullPath"
  }
  $item = Get-Item -LiteralPath $fullPath -Force
  if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "$Name must not be a reparse point"
  }
  if ((Get-FileLinkCount $item.FullName) -ne 1) {
    throw "$Name must have exactly one filesystem link"
  }
  $actual = Get-Sha256 $item.FullName
  if ($actual -cne $Expected) {
    throw "$Name hash mismatch: expected=$Expected actual=$actual"
  }
}

function Assert-ExactProperties([object]$Object, [string[]]$Expected) {
  $actual = @($Object.PSObject.Properties.Name | Sort-Object)
  $wanted = @($Expected | Sort-Object)
  if (($actual -join "`n") -cne ($wanted -join "`n")) {
    throw 'deployment manifest contains missing or unknown properties'
  }
}

$fixtureRootFull = ''
if ($hermeticEvaluation) {
  try {
    $fixtureRootFull = Assert-FixtureContext $FixtureRoot $FixtureNonce
    $DeploymentManifest = Assert-FixturePath $DeploymentManifest 'DeploymentManifest' $fixtureRootFull
    $WslConfig = Assert-FixturePath $WslConfig 'WslConfig' $fixtureRootFull
    $ReceiptDirectory = Assert-FixturePath $ReceiptDirectory 'ReceiptDirectory' $fixtureRootFull
    foreach ($fixturePath in @(
      [pscustomobject]@{ Value = $LogPath; Name = 'LogPath' },
      [pscustomobject]@{ Value = $EvaluateCanaryFixture; Name = 'EvaluateCanaryFixture' },
      [pscustomobject]@{ Value = $BaselineCanaryFixture; Name = 'BaselineCanaryFixture' },
      [pscustomobject]@{ Value = $EvaluateRollbackFixture; Name = 'EvaluateRollbackFixture' },
      [pscustomobject]@{ Value = $EvaluateRuntimeFixture; Name = 'EvaluateRuntimeFixture' },
      [pscustomobject]@{ Value = $DryRunConfig; Name = 'DryRunConfig' },
      [pscustomobject]@{ Value = $EvaluateExternalTimeoutFixture; Name = 'EvaluateExternalTimeoutFixture' },
      [pscustomobject]@{ Value = $EvaluateTransactionFixture; Name = 'EvaluateTransactionFixture' }
      [pscustomobject]@{ Value = $TransactionLockEvidenceFixture; Name = 'TransactionLockEvidenceFixture' }
    )) {
      if (-not [string]::IsNullOrWhiteSpace($fixturePath.Value)) {
        [void](Assert-FixturePath $fixturePath.Value $fixturePath.Name $fixtureRootFull)
      }
    }
    if (-not [string]::IsNullOrWhiteSpace($TransactionLockEvidenceFixture) -and
        [string]::IsNullOrWhiteSpace($EvaluateTransactionFixture)) {
      throw 'TransactionLockEvidenceFixture requires EvaluateTransactionFixture'
    }
  } catch {
    Write-Output ('FIXTURE_PATH=REFUSED ' + $_.Exception.Message)
    exit 2
  }
}

if ($ExpectedDeploymentSha256 -cnotmatch '^[0-9a-f]{64}$') {
  throw 'ExpectedDeploymentSha256 must be one lowercase SHA-256 value'
}
$selfDirectory = [System.IO.Path]::GetFullPath($PSScriptRoot)
if ($hermeticEvaluation) {
  [void](Assert-FixturePath $selfDirectory 'installed wrapper directory' $fixtureRootFull)
}
$expectedManifestPath = [System.IO.Path]::GetFullPath((Join-Path $selfDirectory 'deployment.json'))
$providedManifestPath = Assert-SafeWindowsPath $DeploymentManifest 'DeploymentManifest'
if ($providedManifestPath -cne $expectedManifestPath) {
  throw 'deployment manifest must be the file installed beside this wrapper'
}
Assert-LeafHash $providedManifestPath $ExpectedDeploymentSha256 'deployment manifest'

$deployment = Get-Content -LiteralPath $providedManifestPath -Raw | ConvertFrom-Json
$properties = @(
  'schema', 'bundle_id',
  'wrapper_file', 'wrapper_sha256',
  'launcher_file', 'launcher_sha256',
  'kernel_manifest_file', 'kernel_manifest_sha256',
  'kernel_file', 'kernel_sha256',
  'modules_file', 'modules_sha256',
  'layout_inventory_file', 'layout_inventory_sha256',
  'qemu_stamp_file', 'qemu_stamp_sha256'
)
Assert-ExactProperties $deployment $properties
if ($deployment.schema -cne 'ramshared.kernel-launcher-deployment.v1') {
  throw 'unsupported deployment manifest schema'
}
if ($deployment.bundle_id -cne (Split-Path -Leaf $selfDirectory)) {
  throw 'deployment bundle identity differs from its immutable directory'
}
$expectedNames = @{
  wrapper_file = 'boot-kernel-logged.ps1'
  launcher_file = 'boot-kernel-safe.ps1'
  kernel_manifest_file = 'kernel-pair.manifest'
  kernel_file = 'kernel.bzImage'
  modules_file = 'modules.vhdx'
  layout_inventory_file = 'modules-layout.manifest'
  qemu_stamp_file = 'qemu-pass.stamp'
}
foreach ($property in $expectedNames.Keys) {
  if ($deployment.$property -cne $expectedNames[$property]) {
    throw "deployment manifest has an unsafe $property"
  }
}

$wrapper = Join-Path $selfDirectory $deployment.wrapper_file
$launcher = Join-Path $selfDirectory $deployment.launcher_file
$kernelManifest = Join-Path $selfDirectory $deployment.kernel_manifest_file
$kernel = Join-Path $selfDirectory $deployment.kernel_file
$modules = Join-Path $selfDirectory $deployment.modules_file
$layoutInventory = Join-Path $selfDirectory $deployment.layout_inventory_file
$qemuStamp = Join-Path $selfDirectory $deployment.qemu_stamp_file
Assert-LeafHash $wrapper $deployment.wrapper_sha256 'wrapper'
Assert-LeafHash $launcher $deployment.launcher_sha256 'launcher'
Assert-LeafHash $kernelManifest $deployment.kernel_manifest_sha256 'kernel-pair manifest'
Assert-LeafHash $kernel $deployment.kernel_sha256 'kernel image'
Assert-LeafHash $modules $deployment.modules_sha256 'modules VHDX'
Assert-LeafHash $layoutInventory $deployment.layout_inventory_sha256 'modules layout inventory'
Assert-LeafHash $qemuStamp $deployment.qemu_stamp_sha256 'QEMU validation stamp'

if (-not $hermeticEvaluation) {
  Write-Output 'MODULE_VHDX_PROVENANCE=REFUSED cryptographic-containment-not-verifiable'
  Write-Output 'LIVE_PROMOTION=NO_GO'
  exit 2
}

$powerShell51 = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
if (-not (Test-Path -LiteralPath $powerShell51 -PathType Leaf)) {
  throw 'Windows PowerShell 5.1 executable is unavailable'
}

function ConvertTo-ProcessArgument([string]$Value) {
  if ($Value.Length -gt 0 -and $Value -cnotmatch '[\s"]') { return $Value }
  $escaped = $Value -replace '(\\*)"', '$1$1\"'
  $escaped = $escaped -replace '(\\+)$', '$1$1'
  return '"' + $escaped + '"'
}


function Invoke-BoundedPowerShell([string[]]$Arguments, [int]$DeadlineSec) {
  if ($DeadlineSec -lt 1 -or $DeadlineSec -gt 180) {
    throw 'wrapper deadline is outside 1..180 seconds'
  }
  Initialize-WrapperJobType
  $argumentLine = (@($Arguments | ForEach-Object { ConvertTo-ProcessArgument $_ }) -join ' ')
  $ownedProcess = $null
  try {
    $ownedProcess = [RamSharedSuspendedJobProcess]::Start(
      $powerShell51,
      $argumentLine,
      ($DeadlineSec * 1000),
      1048576,
      $false,
      $false,
      $false,
      $false,
      'wrapper child'
    )
    $result = $ownedProcess.Wait()
    if (-not $result.JobEmpty) {
      throw 'wrapper child returned without exact empty-job custody proof'
    }
    return [pscustomobject]@{
      ExitCode = $result.ExitCode
      Stdout = $result.Stdout
      Stderr = $result.Stderr
      ProcessId = $result.ProcessId
      CreationTimeUtcFileTime = $result.CreationTimeUtcFileTime
      JobEmpty = $result.JobEmpty
    }
  } finally {
    if ($null -ne $ownedProcess) {
      $ownedProcess.Dispose()
    }
  }
}
$boundArgs = @(
  '-NoProfile',
  '-NonInteractive',
  '-ExecutionPolicy', 'Bypass',
  '-File', $launcher,
  '-KernelPairManifest', $kernelManifest,
  '-ExpectedKernelManifestSha256', $deployment.kernel_manifest_sha256,
  '-WslConfig', $WslConfig,
  '-TimeoutSec', $TimeoutSec.ToString(),
  '-Distro', $Distro,
  '-ReceiptDirectory', $ReceiptDirectory
)
if (-not [string]::IsNullOrWhiteSpace($ApprovedFailedUnits)) {
  $boundArgs += @('-ApprovedFailedUnits', $ApprovedFailedUnits)
}
if (-not [string]::IsNullOrWhiteSpace($EvaluateCanaryFixture)) {
  $boundArgs += @('-EvaluateCanaryFixture', $EvaluateCanaryFixture)
}
if (-not [string]::IsNullOrWhiteSpace($BaselineCanaryFixture)) {
  $boundArgs += @('-BaselineCanaryFixture', $BaselineCanaryFixture)
}
if (-not [string]::IsNullOrWhiteSpace($EvaluateRollbackFixture)) {
  $boundArgs += @('-EvaluateRollbackFixture', $EvaluateRollbackFixture)
}
if (-not [string]::IsNullOrWhiteSpace($EvaluateRuntimeFixture)) {
  $boundArgs += @('-EvaluateRuntimeFixture', $EvaluateRuntimeFixture)
}
if (-not [string]::IsNullOrWhiteSpace($DryRunConfig)) {
  $boundArgs += @('-DryRunConfig', $DryRunConfig)
}
if ($EvaluateExternalFailureFixture) { $boundArgs += '-EvaluateExternalFailureFixture' }
if (-not [string]::IsNullOrWhiteSpace($EvaluateExternalTimeoutFixture)) {
  $boundArgs += @('-EvaluateExternalTimeoutFixture', $EvaluateExternalTimeoutFixture)
}
if ($InjectAssignFailureFixture.IsPresent) { $boundArgs += '-InjectAssignFailureFixture' }
if ($InjectResumeFailureFixture.IsPresent) { $boundArgs += '-InjectResumeFailureFixture' }
if ($InjectTerminateFailureFixture.IsPresent) { $boundArgs += '-InjectTerminateFailureFixture' }
if ($InjectRootCreationMismatchFixture.IsPresent) { $boundArgs += '-InjectRootCreationMismatchFixture' }
if ($PSBoundParameters.ContainsKey('EvaluateDeadlineFixtureSec')) {
  $boundArgs += @('-EvaluateDeadlineFixtureSec', $EvaluateDeadlineFixtureSec.ToString())
}
if ($EvaluateSlowStartupFixture.IsPresent) { $boundArgs += '-EvaluateSlowStartupFixture' }
if ($EvaluateSlowHashFixture.IsPresent) { $boundArgs += '-EvaluateSlowHashFixture' }
if (-not [string]::IsNullOrWhiteSpace($EvaluateTransactionFixture)) {
  $boundArgs += @('-EvaluateTransactionFixture', $EvaluateTransactionFixture)
}
if (-not [string]::IsNullOrWhiteSpace($TransactionLockEvidenceFixture)) {
  $boundArgs += @('-TransactionLockEvidenceFixture', $TransactionLockEvidenceFixture)
}
if (-not [string]::IsNullOrWhiteSpace($InjectFailureBoundary)) {
  $boundArgs += @('-InjectFailureBoundary', $InjectFailureBoundary)
}
if ($PSBoundParameters.ContainsKey('TransactionLockTimeoutSec')) {
  $boundArgs += @('-TransactionLockTimeoutSec', $TransactionLockTimeoutSec.ToString())
}
if ($PSBoundParameters.ContainsKey('HoldTransactionLockMilliseconds')) {
  $boundArgs += @('-HoldTransactionLockMilliseconds', $HoldTransactionLockMilliseconds.ToString())
}
if ($hermeticEvaluation) {
  $boundArgs += @('-FixtureRoot', $fixtureRootFull, '-FixtureNonce', $FixtureNonce)
}
if ($PreflightOnly) { $boundArgs += '-PreflightOnly' }
if ($EvaluateLiveGateFixture.IsPresent) {
  $boundArgs += @('-EvaluateLiveGateFixture', '-Run', '-ConfirmationToken', $ConfirmationToken)
} elseif (-not $hermeticEvaluation) {
  $boundArgs += @('-Run', '-ConfirmationToken', $ConfirmationToken)
}

Assert-LeafHash $providedManifestPath $ExpectedDeploymentSha256 'deployment manifest immediate pre-use'
Assert-LeafHash $launcher $deployment.launcher_sha256 'launcher immediate pre-use'
$child = Invoke-BoundedPowerShell $boundArgs ([Math]::Min(180, [Math]::Max(15, $TimeoutSec + 30)))
if (-not [string]::IsNullOrWhiteSpace($child.Stdout)) { Write-Output $child.Stdout.TrimEnd() }
if (-not [string]::IsNullOrWhiteSpace($child.Stderr)) { Write-Output $child.Stderr.TrimEnd() }
Assert-LeafHash $providedManifestPath $ExpectedDeploymentSha256 'deployment manifest post-use'
Assert-LeafHash $launcher $deployment.launcher_sha256 'launcher post-use'
exit $child.ExitCode
