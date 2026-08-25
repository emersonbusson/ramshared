<#
.SYNOPSIS
  Promotes one sealed WSL kernel and modules pair with an automatic rollback.

.DESCRIPTION
  Performs an attended bundled-kernel/candidate A/B transaction. The launcher
  validates an immutable kernel-pair manifest, stages no files, writes both
  kernel= and kernelModules= atomically, checks every wsl.exe exit, and keeps a
  hash-bound receipt. Any uncertainty restores a freshly derived bundled
  configuration and proves a new bundled-kernel boot before returning failure.

  The launcher never loads a module, activates RamShared, touches swap or block
  devices, starts pressure, or starts Docker. Repository UNC paths are not used.
  The default is PLAN/refusal; live promotion requires -Run and the exact
  case-sensitive promotion confirmation token.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$KernelPairManifest,
  [Parameter(Mandatory = $true)][string]$ExpectedKernelManifestSha256,
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
  [ValidateSet('', 'after_capture_before_validation', 'after_receipt_root', 'after_snapshot', 'after_bundled_config', 'after_current_receipt', 'after_transaction_receipt', 'after_candidate_config', 'after_ready_receipt', 'atomic_after_temp_flush', 'atomic_after_replace', 'post_check_mutation')]
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

$script:WslExe = Join-Path $env:SystemRoot 'System32\wsl.exe'
$script:Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$script:MaximumCapturedOutputBytes = 1024 * 1024
$script:MaximumInputBytes = [long]68719476736
$script:TransactionDeadlineSec = 300
$script:TransactionClock = [System.Diagnostics.Stopwatch]::StartNew()
$script:AtomicFailureBoundary = ''
$script:InjectAssignFailure = $InjectAssignFailureFixture.IsPresent
$script:InjectResumeFailure = $InjectResumeFailureFixture.IsPresent
$script:InjectTerminateFailure = $InjectTerminateFailureFixture.IsPresent
$script:InjectRootCreationMismatch = $InjectRootCreationMismatchFixture.IsPresent
$script:InjectSlowHash = $false
$script:SlowHashRequested = $EvaluateSlowHashFixture.IsPresent

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
  $segments = @($Path.Substring(3) -split '[\\/]')
  foreach ($segment in $segments) {
    if ($segment.Length -eq 0 -or $segment.EndsWith('.') -or $segment.EndsWith(' ') -or
        $segment -match '~[0-9]+' ) {
      throw "$Name contains an empty, trailing-dot/space, or 8.3 segment"
    }
    $stem = ($segment -split '\.', 2)[0]
    if ($stem -match '^(CON|PRN|AUX|NUL|CLOCK\$|COM[1-9]|LPT[1-9])$') {
      throw "$Name DOS_DEVICE_SEGMENT is forbidden: $segment"
    }
  }
}

function Assert-NoUnsafeWindowsPathSyntax([string]$Path, [string]$Name) {
  Assert-WindowsPathLexicalSyntax $Path $Name
  if ([string]::IsNullOrWhiteSpace($Path) -or -not [System.IO.Path]::IsPathRooted($Path)) {
    throw "$Name must use one non-blank absolute Windows drive path"
  }
  if ($Path.StartsWith('\\') -or $Path.StartsWith('//') -or
      $Path.StartsWith('\\?\') -or $Path.StartsWith('\\.\') -or
      $Path -match '(^|[\\/])\.\.?([\\/]|$)' -or
      $Path -match '(^|[\\/])[^\\/]*~[0-9]+(?:[\\/]|$)' -or
      $Path -cnotmatch '^[A-Za-z]:[\\/][^:]*$') {
    throw "$Name contains a UNC, device, ADS, short-name, or traversal form"
  }
  $fullPath = [System.IO.Path]::GetFullPath($Path)
  if ($fullPath -cne $Path.TrimEnd('\', '/')) {
    throw "$Name is not a canonical absolute path"
  }
  return $fullPath
}

function Assert-NoReparseAncestors([string]$Path, [string]$Name) {
  $fullPath = [System.IO.Path]::GetFullPath($Path)
  $cursor = if (Test-Path -LiteralPath $fullPath) {
    $fullPath
  } else {
    Split-Path -Parent $fullPath
  }
  while (-not [string]::IsNullOrWhiteSpace($cursor)) {
    if (Test-Path -LiteralPath $cursor) {
      $item = Get-Item -LiteralPath $cursor -Force
      if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Name has a reparse-point ancestor: $cursor"
      }
      $pathRoot = [System.IO.Path]::GetPathRoot($cursor).TrimEnd('\', '/')
      $canonicalCursor = [System.IO.Path]::GetFullPath($cursor).TrimEnd('\', '/')
      if ($canonicalCursor -cne $pathRoot -and $item.FullName.TrimEnd('\', '/') -cne $canonicalCursor) {
        throw "$Name has a case-colliding ancestor: $cursor"
      }
    }
    $parent = Split-Path -Parent $cursor
    if ([string]::IsNullOrWhiteSpace($parent) -or $parent -ceq $cursor) {
      break
    }
    # Preserve a drive root as C:\ rather than converting it to the
    # drive-relative C: form, which is neither canonical nor safe to walk.
    $cursor = $parent
  }
}

function Assert-FixtureContext([string]$Root, [string]$Nonce) {
  if ($Nonce -cnotmatch '^[0-9a-f]{32}$') {
    throw 'FixtureNonce must be one lowercase 128-bit nonce'
  }
  $fullRoot = Assert-NoUnsafeWindowsPathSyntax $Root 'FixtureRoot'
  $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\', '/')
  $parent = (Split-Path -Parent $fullRoot).TrimEnd('\', '/')
  $leaf = Split-Path -Leaf $fullRoot
  if ($parent -cne $tempRoot -or $leaf -cne ('ramshared-kernel-static-' + $Nonce)) {
    throw 'FixtureRoot must be one canonical fresh direct child of the Windows temporary directory'
  }
  if (-not (Test-Path -LiteralPath $fullRoot -PathType Container)) {
    throw 'FixtureRoot must already exist'
  }
  Assert-NoReparseAncestors $fullRoot 'FixtureRoot'
  $marker = Join-Path $fullRoot '.ramshared-kernel-fixture-root'
  if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) {
    throw 'FixtureRoot marker is missing'
  }
  Assert-NoReparseAncestors $marker 'FixtureRoot marker'
  if ((Get-FileIdentity $marker).NumberOfLinks -ne 1) {
    throw 'FixtureRoot marker must have exactly one filesystem link'
  }
  $expectedMarker = 'ramshared.kernel-fixture.v1:' + $Nonce
  if ([System.IO.File]::ReadAllText($marker) -cne $expectedMarker) {
    throw 'FixtureRoot marker does not bind the supplied nonce'
  }
  return $fullRoot
}

function Assert-FixturePath([string]$Path, [string]$Name, [string]$Root) {
  $fullPath = Assert-NoUnsafeWindowsPathSyntax $Path $Name
  $prefix = $Root.TrimEnd('\', '/') + '\'
  if (-not $fullPath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "$Name must remain beneath FixtureRoot"
  }
  Assert-NoReparseAncestors $fullPath $Name
  return $fullPath
}

function Assert-ExactLivePaths([string]$ConfigPath, [string]$ReceiptPath) {
  $config = Assert-NoUnsafeWindowsPathSyntax $ConfigPath 'WslConfig'
  $receipts = Assert-NoUnsafeWindowsPathSyntax $ReceiptPath 'ReceiptDirectory'
  $expectedConfig = [System.IO.Path]::GetFullPath((Join-Path $env:USERPROFILE '.wslconfig'))
  $expectedReceipts = [System.IO.Path]::GetFullPath('C:\wsl\ramshared-receipts')
  if ($config -cne $expectedConfig -or $receipts -cne $expectedReceipts) {
    throw 'live promotion paths must use the canonical user .wslconfig and receipt root'
  }
  Assert-NoReparseAncestors $config 'WslConfig'
  Assert-NoReparseAncestors $receipts 'ReceiptDirectory'
}

function Initialize-ProcessJobType {
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

function Initialize-FileIdentityType {
  if ($null -ne ('RamSharedFileIdentity' -as [type])) {
    return
  }
  Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

public static class RamSharedFileIdentity {
    [StructLayout(LayoutKind.Sequential)]
    public struct FILETIME { public uint Low; public uint High; }

    [StructLayout(LayoutKind.Sequential)]
    public struct BY_HANDLE_FILE_INFORMATION {
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

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool GetFileInformationByHandle(
        SafeFileHandle handle,
        out BY_HANDLE_FILE_INFORMATION information);

    public static ulong[] Read(SafeFileHandle handle) {
        BY_HANDLE_FILE_INFORMATION information;
        if (!GetFileInformationByHandle(handle, out information)) {
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        }
        ulong index = ((ulong)information.FileIndexHigh << 32) | information.FileIndexLow;
        return new ulong[] { information.VolumeSerialNumber, index, information.NumberOfLinks };
    }
}
'@
}

function Get-FileIdentity([string]$Path) {
  Initialize-FileIdentityType
  $stream = [System.IO.File]::Open(
    $Path,
    [System.IO.FileMode]::Open,
    [System.IO.FileAccess]::Read,
    [System.IO.FileShare]::Read
  )
  try {
    $values = [RamSharedFileIdentity]::Read($stream.SafeFileHandle)
    return [pscustomobject]@{
      Volume = $values[0]
      Index = $values[1]
      NumberOfLinks = $values[2]
    }
  } finally {
    $stream.Dispose()
  }
}

function Get-RemainingTransactionMilliseconds([int]$RequestedSeconds, [string]$Label) {
  $remaining = ($script:TransactionDeadlineSec * 1000) - [int]$script:TransactionClock.ElapsedMilliseconds
  $requested = $RequestedSeconds * 1000
  $bounded = [Math]::Min($remaining, $requested)
  if ($bounded -lt 1) {
    throw "$Label refused because the end-to-end transaction deadline expired"
  }
  return $bounded
}

function Get-BytesSha256([byte[]]$Bytes) {
  $algorithm = [System.Security.Cryptography.SHA256]::Create()
  try {
    return ([System.BitConverter]::ToString($algorithm.ComputeHash($Bytes))).Replace('-', '').ToLowerInvariant()
  } finally {
    $algorithm.Dispose()
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
        if ($script:TransactionClock.ElapsedMilliseconds -ge ($script:TransactionDeadlineSec * 1000)) {
          throw 'hash input exceeded the end-to-end transaction deadline'
        }
        if ($script:InjectSlowHash) {
          $script:InjectSlowHash = $false
          [Console]::Error.WriteLine('SLOW_HASH_SEAM=ENTERED')
          Start-Sleep -Seconds 30
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

function Assert-RegularFile([string]$Path, [string]$Name) {
  $fullPath = Assert-NoUnsafeWindowsPathSyntax $Path $Name
  Assert-NoReparseAncestors $fullPath $Name
  if (-not (Test-Path -LiteralPath $fullPath -PathType Leaf)) {
    throw "$Name is missing: $fullPath"
  }
  $item = Get-Item -LiteralPath $fullPath -Force
  if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "$Name must not be a reparse point"
  }
  $identity = Get-FileIdentity $item.FullName
  if ($identity.NumberOfLinks -ne 1) {
    throw "$Name must have exactly one filesystem link"
  }
  return $item
}

function ConvertFrom-StrictKeyValue(
  [string]$Text,
  [string[]]$ExpectedKeys,
  [string]$Label,
  [string]$KeyPattern
) {
  $map = @{}
  foreach ($line in @($Text -split "`r?`n")) {
    if ($line.Length -eq 0) {
      continue
    }
    if ($line -cnotmatch ('^(' + $KeyPattern + ')=([' + [char]32 + '-' + [char]126 + ']*)$')) {
      throw "$Label contains a malformed or non-ASCII line"
    }
    $key = $Matches[1]
    if ($map.ContainsKey($key)) {
      throw "$Label contains duplicate key $key"
    }
    $map[$key] = $Matches[2]
  }
  $actual = @($map.Keys | Sort-Object)
  $expected = @($ExpectedKeys | Sort-Object)
  if (($actual -join "`n") -cne ($expected -join "`n")) {
    throw "$Label contains missing or unknown keys"
  }
  return $map
}

function ConvertTo-PositiveInt64([string]$Value, [string]$Name) {
  $number = [long]0
  if (-not [long]::TryParse($Value, [ref]$number) -or $number -le 0) {
    throw "$Name must be a positive integer"
  }
  return $number
}

function ConvertTo-NonNegativeInt([string]$Value, [string]$Name) {
  $number = [int]0
  if (-not [int]::TryParse($Value, [ref]$number) -or $number -lt 0) {
    throw "$Name must be a non-negative integer"
  }
  return $number
}

function Read-KernelPairManifest([string]$Path, [string]$ExpectedSha256) {
  Assert-Sha256 $ExpectedSha256 'ExpectedKernelManifestSha256'
  $item = Assert-RegularFile $Path 'kernel-pair manifest'
  $actualHash = Get-Sha256 $item.FullName
  if ($actualHash -cne $ExpectedSha256) {
    throw "kernel-pair manifest hash mismatch: expected=$ExpectedSha256 actual=$actualHash"
  }
  $keys = @(
    'schema', 'pair_id', 'release',
    'kernel_file', 'kernel_sha256', 'kernel_size_bytes',
    'modules_file', 'modules_sha256', 'modules_size_bytes',
    'modules_layout', 'layout_release_directory_count',
    'layout_nested_release_directory_count', 'layout_inventory_sha256',
    'module_name', 'module_vermagic', 'minimum_wsl_version',
    'qemu_stamp_sha256', 'qemu_kernel_sha256', 'qemu_release'
  )
  $map = ConvertFrom-StrictKeyValue ([System.IO.File]::ReadAllText($item.FullName)) $keys 'kernel-pair manifest' '[a-z][a-z0-9_]+'
  if ($map.schema -cne 'ramshared.kernel-pair.v1') {
    throw 'unsupported kernel-pair manifest schema'
  }
  if ($map.release -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$') {
    throw 'kernel release is not canonical'
  }
  foreach ($name in @('kernel_file', 'modules_file')) {
    if ($map[$name] -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$' -or
        [System.IO.Path]::GetFileName($map[$name]) -cne $map[$name]) {
      throw "$name must be one safe basename"
    }
  }
  if ($map.kernel_file -cne 'kernel.bzImage' -or $map.modules_file -cne 'modules.vhdx') {
    throw 'kernel-pair artifact basenames are not canonical'
  }
  foreach ($name in @(
    'kernel_sha256', 'modules_sha256', 'layout_inventory_sha256',
    'qemu_stamp_sha256', 'qemu_kernel_sha256'
  )) {
    Assert-Sha256 $map[$name] $name
  }
  $kernelSize = ConvertTo-PositiveInt64 $map.kernel_size_bytes 'kernel_size_bytes'
  $modulesSize = ConvertTo-PositiveInt64 $map.modules_size_bytes 'modules_size_bytes'
  $releaseCount = ConvertTo-NonNegativeInt $map.layout_release_directory_count 'layout_release_directory_count'
  $nestedCount = ConvertTo-NonNegativeInt $map.layout_nested_release_directory_count 'layout_nested_release_directory_count'
  if ($map.modules_layout -cnotmatch '^(legacy_flat_v1|unified_release_v1)$') {
    throw 'modules_layout is unsupported'
  }
  if ($map.modules_layout -ceq 'legacy_flat_v1' -and ($releaseCount -ne 0 -or $nestedCount -ne 0)) {
    throw 'legacy modules layout contains release-directory nesting'
  }
  if ($map.modules_layout -ceq 'unified_release_v1' -and ($releaseCount -ne 1 -or $nestedCount -ne 0)) {
    throw 'unified modules layout is missing one release directory or is double nested'
  }
  if ($map.module_name -cnotmatch '^[A-Za-z0-9_+-]+$') {
    throw 'module_name is unsafe'
  }
  $vermagicRelease = @($map.module_vermagic -split '\s+')[0]
  if ($vermagicRelease -cne $map.release) {
    throw 'module vermagic does not bind the exact kernel release'
  }
  $minimumVersion = $null
  if (-not [version]::TryParse($map.minimum_wsl_version, [ref]$minimumVersion)) {
    throw 'minimum_wsl_version is invalid'
  }
  if ($map.qemu_kernel_sha256 -cne $map.kernel_sha256 -or $map.qemu_release -cne $map.release) {
    throw 'QEMU receipt does not bind the exact kernel hash and release'
  }
  $expectedPairId = 'v1-' + $map.kernel_sha256.Substring(0, 16) + '-' + $map.modules_sha256.Substring(0, 16)
  if ($map.pair_id -cne $expectedPairId) {
    throw 'pair_id does not bind the exact kernel and modules hashes'
  }
  return [pscustomobject]@{
    Map = $map
    ManifestPath = $item.FullName
    ManifestSha256 = $actualHash
    Directory = $item.DirectoryName
    KernelPath = Join-Path $item.DirectoryName $map.kernel_file
    ModulesPath = Join-Path $item.DirectoryName $map.modules_file
    LayoutInventoryPath = Join-Path $item.DirectoryName 'modules-layout.manifest'
    QemuStampPath = Join-Path $item.DirectoryName 'qemu-pass.stamp'
    KernelSize = $kernelSize
    ModulesSize = $modulesSize
    ReleaseDirectoryCount = $releaseCount
    NestedReleaseDirectoryCount = $nestedCount
    MinimumWslVersion = $minimumVersion
  }
}

function Assert-KernelPairArtifacts([object]$Pair) {
  $kernel = Assert-RegularFile $Pair.KernelPath 'sealed kernel image'
  $modules = Assert-RegularFile $Pair.ModulesPath 'sealed modules VHDX'
  $layoutInventory = Assert-RegularFile $Pair.LayoutInventoryPath 'sealed modules layout inventory'
  $qemuStamp = Assert-RegularFile $Pair.QemuStampPath 'sealed QEMU validation stamp'
  if ($kernel.Length -le 1048576 -or $kernel.Length -ne $Pair.KernelSize -or
      (Get-Sha256 $kernel.FullName) -cne $Pair.Map.kernel_sha256) {
    throw 'sealed kernel image size or hash differs from the manifest'
  }
  if ($modules.Length -ne $Pair.ModulesSize -or (Get-Sha256 $modules.FullName) -cne $Pair.Map.modules_sha256) {
    throw 'sealed modules VHDX size or hash differs from the manifest'
  }
  if ((Get-Sha256 $qemuStamp.FullName) -cne $Pair.Map.qemu_stamp_sha256) {
    throw 'sealed QEMU validation stamp hash differs from the manifest'
  }
  if ((Get-Sha256 $layoutInventory.FullName) -cne $Pair.Map.layout_inventory_sha256) {
    throw 'sealed modules layout inventory hash differs from the manifest'
  }
  $layoutKeys = @(
    'schema', 'layout', 'release', 'release_directory_count',
    'nested_release_directory_count', 'modules_sha256', 'modules_size_bytes'
  )
  $layout = ConvertFrom-StrictKeyValue ([System.IO.File]::ReadAllText($layoutInventory.FullName)) $layoutKeys 'modules layout inventory' '[a-z][a-z0-9_]+'
  if ($layout.schema -cne 'ramshared.modules-layout.v1' -or
      $layout.layout -cne $Pair.Map.modules_layout -or
      $layout.release -cne $Pair.Map.release -or
      $layout.release_directory_count -cne $Pair.Map.layout_release_directory_count -or
      $layout.nested_release_directory_count -cne $Pair.Map.layout_nested_release_directory_count -or
      $layout.modules_sha256 -cne $Pair.Map.modules_sha256 -or
      $layout.modules_size_bytes -cne $Pair.Map.modules_size_bytes) {
    throw 'sealed modules layout inventory differs from the kernel-pair manifest'
  }
  $stampKeys = @('REL', 'KERNEL_SHA256', 'HEAD', 'DATE', 'VALIDATE')
  $stamp = ConvertFrom-StrictKeyValue ([System.IO.File]::ReadAllText($qemuStamp.FullName)) $stampKeys 'QEMU validation stamp' '[A-Z][A-Z0-9_]+'
  if ($stamp.REL -cne $Pair.Map.release -or
      $stamp.KERNEL_SHA256 -cne $Pair.Map.kernel_sha256 -or
      $stamp.HEAD -cnotmatch '^[0-9a-f]{7,64}$' -or
      $stamp.DATE -cnotmatch '^[0-9]{4}-[0-9]{2}-[0-9]{2}T' -or
      $stamp.VALIDATE -cne 'qemu-validate.sh') {
    throw 'QEMU validation stamp semantics differ from the sealed pair'
  }
}

function ConvertFrom-WslVersionText([string]$Text) {
  $map = @{}
  foreach ($line in @(($Text -replace "`0", '') -split "`r?`n")) {
    if ($line -match '^([^:]+):\s*([0-9]+(?:\.[0-9]+){1,3}(?:[-+][A-Za-z0-9._-]+)?)\s*$') {
      $label = $Matches[1].Trim()
      $versionValue = $Matches[2]
      $key = $null
      if ($label -match '(?i)wslg') {
        $key = 'wslg'
      } elseif ($label -match '(?i)\bwsl\b') {
        $key = 'wsl'
      } elseif ($label -match '(?i)kernel') {
        $key = 'kernel'
      } elseif ($label -match '(?i)windows') {
        $key = 'windows'
      }
      if ($null -ne $key) {
        if ($map.ContainsKey($key)) {
          throw "wsl.exe --version contains duplicate $key version"
        }
        $map[$key] = $versionValue
      }
    }
  }
  foreach ($required in @('wsl', 'kernel', 'wslg', 'windows')) {
    if (-not $map.ContainsKey($required)) {
      throw "wsl.exe --version is missing $required version"
    }
  }
  return $map
}

function Assert-RuntimeAdmission([object]$Pair, [string]$WslVersionText) {
  $versions = ConvertFrom-WslVersionText $WslVersionText
  $runtimeVersion = $null
  if (-not [version]::TryParse($versions.wsl, [ref]$runtimeVersion)) {
    throw 'installed WSL runtime version is invalid'
  }
  if ($runtimeVersion -lt $Pair.MinimumWslVersion) {
    throw "installed WSL runtime $runtimeVersion predates required runtime $($Pair.MinimumWslVersion)"
  }
  if ($Pair.Map.modules_layout -ceq 'unified_release_v1') {
    throw 'unified modules artifacts are not admitted: WSL unified-layout support was merged after 2.7.12 and no reviewed released runtime is allowlisted'
  }
  if ($Pair.Map.modules_layout -cne 'legacy_flat_v1' -or
      $Pair.ReleaseDirectoryCount -ne 0 -or
      $Pair.NestedReleaseDirectoryCount -ne 0) {
    throw 'kernel modules layout is incompatible with the admitted legacy runtime contract'
  }
  return $versions
}

function ConvertTo-ProcessArgument([string]$Value) {
  if ($Value.Length -gt 0 -and $Value -cnotmatch '[\s"]') {
    return $Value
  }
  $escaped = $Value -replace '(\\*)"', '$1$1\"'
  $escaped = $escaped -replace '(\\+)$', '$1$1'
  return '"' + $escaped + '"'
}

function Invoke-BoundedProcess(
  [string]$FilePath,
  [string[]]$Arguments,
  [int]$DeadlineSec,
  [string]$Label
) {
  if ($DeadlineSec -lt 1 -or $DeadlineSec -gt 180) {
    throw "$Label deadline is outside 1..180 seconds"
  }
  Initialize-ProcessJobType
  $deadlineMilliseconds = Get-RemainingTransactionMilliseconds $DeadlineSec $Label
  $argumentLine = (@($Arguments | ForEach-Object { ConvertTo-ProcessArgument $_ }) -join ' ')
  $ownedProcess = $null
  try {
    $ownedProcess = [RamSharedSuspendedJobProcess]::Start(
      $FilePath,
      $argumentLine,
      $deadlineMilliseconds,
      $script:MaximumCapturedOutputBytes,
      $script:InjectAssignFailure,
      $script:InjectResumeFailure,
      $script:InjectTerminateFailure,
      $script:InjectRootCreationMismatch,
      $Label
    )
    $result = $ownedProcess.Wait()
    if (-not $result.JobEmpty) {
      throw "$Label returned without exact empty-job custody proof"
    }
    if ($result.ExitCode -ne 0) {
      $detail = $result.Stderr.Trim()
      if ($detail.Length -gt 2048) {
        $detail = $detail.Substring(0, 2048)
      }
      throw "$Label failed with exit $($result.ExitCode): $detail"
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

function Get-ConfigText([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path)) {
    return ''
  }
  $item = Assert-RegularFile ([System.IO.Path]::GetFullPath($Path)) '.wslconfig'
  return [System.IO.File]::ReadAllText($item.FullName)
}

function Remove-KernelPairFromConfigText([string]$Text) {
  $kept = New-Object 'System.Collections.Generic.List[string]'
  foreach ($line in @($Text -split "`r?`n")) {
    if ($line -match '^\s*(kernel|kernelModules)\s*=') {
      continue
    }
    $kept.Add($line)
  }
  while ($kept.Count -gt 0 -and $kept[$kept.Count - 1] -eq '') {
    $kept.RemoveAt($kept.Count - 1)
  }
  if ($kept.Count -eq 0) {
    return "[wsl2]`r`n"
  }
  return (($kept.ToArray() -join "`r`n") + "`r`n")
}

function Set-KernelPairInConfigText([string]$Text, [string]$KernelPath, [string]$ModulesPath) {
  $wsl2SectionCount = @($Text -split "`r?`n" | Where-Object { $_ -match '^\s*\[wsl2\]\s*$' }).Count
  if ($wsl2SectionCount -gt 1) {
    throw '.wslconfig contains more than one [wsl2] section'
  }
  $clean = Remove-KernelPairFromConfigText $Text
  $lines = @($clean -split "`r?`n")
  $kernelLine = 'kernel=' + ($KernelPath -replace '\\', '/')
  $modulesLine = 'kernelModules=' + ($ModulesPath -replace '\\', '/')
  $output = New-Object 'System.Collections.Generic.List[string]'
  $inWsl2 = $false
  $foundWsl2 = $false
  $inserted = $false
  foreach ($line in $lines) {
    if ($line -match '^\s*\[wsl2\]\s*$') {
      $inWsl2 = $true
      $foundWsl2 = $true
      $output.Add($line)
      continue
    }
    if ($line -match '^\s*\[') {
      if ($inWsl2 -and -not $inserted) {
        $output.Add($kernelLine)
        $output.Add($modulesLine)
        $inserted = $true
      }
      $inWsl2 = $false
    }
    $output.Add($line)
  }
  if ($inWsl2 -and -not $inserted) {
    while ($output.Count -gt 0 -and $output[$output.Count - 1] -eq '') {
      $output.RemoveAt($output.Count - 1)
    }
    $output.Add($kernelLine)
    $output.Add($modulesLine)
    $inserted = $true
  }
  if (-not $foundWsl2) {
    $prefixed = New-Object 'System.Collections.Generic.List[string]'
    $prefixed.Add('[wsl2]')
    $prefixed.Add($kernelLine)
    $prefixed.Add($modulesLine)
    foreach ($line in $output) {
      $prefixed.Add($line)
    }
    $output = $prefixed
  }
  while ($output.Count -gt 0 -and $output[$output.Count - 1] -eq '') {
    $output.RemoveAt($output.Count - 1)
  }
  return (($output.ToArray() -join "`r`n") + "`r`n")
}

function Assert-ConfigPairCardinality([string]$Text, [int]$ExpectedCount) {
  $kernelCount = @($Text -split "`r?`n" | Where-Object { $_ -match '^\s*kernel\s*=' }).Count
  $modulesCount = @($Text -split "`r?`n" | Where-Object { $_ -match '^\s*kernelModules\s*=' }).Count
  if ($kernelCount -ne $ExpectedCount -or $modulesCount -ne $ExpectedCount) {
    throw "config pair cardinality mismatch: kernel=$kernelCount kernelModules=$modulesCount expected=$ExpectedCount"
  }
}

function Test-TransientSharingException([System.Exception]$Exception) {
  $cursor = $Exception
  while ($null -ne $cursor) {
    $nativeCode = $cursor.HResult -band 0xffff
    if ($nativeCode -eq 32 -or $nativeCode -eq 33) {
      return $true
    }
    $cursor = $cursor.InnerException
  }
  return $false
}

function Invoke-AtomicFailureBoundary([string]$Name) {
  if ($script:AtomicFailureBoundary -ceq $Name) {
    throw "injected deterministic atomic failure: $Name"
  }
}

function Restore-AtomicOriginal(
  [string]$Path,
  [bool]$OriginalExisted,
  [string]$Backup,
  [long]$OriginalLength,
  [string]$OriginalSha256
) {
  if ($OriginalExisted) {
    if (-not (Test-Path -LiteralPath $Backup -PathType Leaf)) {
      throw 'atomic replacement lost the original backup'
    }
    if (Test-Path -LiteralPath $Path -PathType Leaf) {
      [System.IO.File]::Replace($Backup, $Path, $null, $true)
    } else {
      [System.IO.File]::Move($Backup, $Path)
    }
    $restored = Assert-RegularFile $Path 'atomic restoration readback'
    if ($restored.Length -ne $OriginalLength -or (Get-Sha256 $Path) -cne $OriginalSha256) {
      throw 'atomic restoration did not reproduce the exact original bytes'
    }
  } else {
    if (Test-Path -LiteralPath $Path) {
      Remove-Item -LiteralPath $Path -Force
    }
    if (Test-Path -LiteralPath $Path) {
      throw 'atomic restoration did not reproduce original absence'
    }
  }
}

function Write-AtomicBytes([string]$Path, [byte[]]$Bytes) {
  if (-not [System.IO.Path]::IsPathRooted($Path)) {
    throw "atomic write target must be absolute: $Path"
  }
  $fullPath = [System.IO.Path]::GetFullPath($Path)
  $parent = Split-Path -Parent $fullPath
  if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
    throw 'atomic write parent must exist before the transaction starts'
  }
  Assert-NoReparseAncestors $fullPath 'atomic write target'
  $expectedHash = Get-BytesSha256 $Bytes
  for ($attempt = 1; $attempt -le 3; $attempt++) {
    $temporary = Join-Path $parent ('.ramshared-' + [guid]::NewGuid().ToString('N') + '.tmp')
    $backup = Join-Path $parent ('.ramshared-' + [guid]::NewGuid().ToString('N') + '.bak')
    $originalExisted = Test-Path -LiteralPath $fullPath -PathType Leaf
    $originalLength = [long]0
    $originalSha256 = ''
    $replacementAttempted = $false
    $replacementCommitted = $false
    if ($originalExisted) {
      $originalItem = Assert-RegularFile $fullPath 'atomic write original'
      $originalLength = $originalItem.Length
      $originalSha256 = Get-Sha256 $fullPath
    } elseif (Test-Path -LiteralPath $fullPath) {
      throw 'atomic write target exists but is not a regular file'
    }
    try {
      $stream = New-Object System.IO.FileStream($temporary, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
      try {
        $stream.Write($Bytes, 0, $Bytes.Length)
        $stream.Flush($true)
      } finally {
        $stream.Dispose()
      }
      Invoke-AtomicFailureBoundary 'atomic_after_temp_flush'
      $temporaryItem = Assert-RegularFile $temporary 'atomic write staging file'
      if ($temporaryItem.Length -ne $Bytes.Length -or (Get-Sha256 $temporary) -cne $expectedHash) {
        throw 'atomic write staging readback hash mismatch'
      }
      $replacementAttempted = $true
      if ($originalExisted) {
        [System.IO.File]::Replace($temporary, $fullPath, $backup, $true)
      } else {
        [System.IO.File]::Move($temporary, $fullPath)
      }
      $replacementCommitted = $true
      Invoke-AtomicFailureBoundary 'atomic_after_replace'
      $readback = Assert-RegularFile $fullPath 'atomic write readback'
      if ($readback.Length -ne $Bytes.Length -or (Get-Sha256 $fullPath) -cne $expectedHash) {
        throw 'atomic write readback hash mismatch'
      }
      if ($originalExisted -and (Test-Path -LiteralPath $backup -PathType Leaf)) {
        Remove-Item -LiteralPath $backup -Force
      }
      return $expectedHash
    } catch {
      $failure = $_
      if ($replacementAttempted) {
        try {
          if ($originalExisted -and -not (Test-Path -LiteralPath $backup -PathType Leaf)) {
            $currentOriginal = Assert-RegularFile $fullPath 'atomic uncertain replacement original check'
            if ($currentOriginal.Length -ne $originalLength -or
                (Get-Sha256 $fullPath) -cne $originalSha256) {
              throw 'replacement failed without a backup or exact original readback'
            }
          } else {
            Restore-AtomicOriginal $fullPath $originalExisted $backup $originalLength $originalSha256
          }
        } catch {
          throw "atomic write failed and exact restoration failed: $($failure.Exception.Message); restore=$($_.Exception.Message)"
        }
      }
      foreach ($candidate in @($temporary, $backup)) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
          Remove-Item -LiteralPath $candidate -Force
        }
      }
      if (-not $replacementAttempted -and
          $attempt -lt 3 -and
          (Test-TransientSharingException $failure.Exception)) {
        Start-Sleep -Milliseconds (100 * $attempt)
        continue
      }
      throw $failure
    }
  }
  throw 'atomic write exhausted only classified transient sharing retries'
}

function Write-AtomicText([string]$Path, [string]$Text) {
  return Write-AtomicBytes $Path $script:Utf8NoBom.GetBytes($Text)
}

function Write-AtomicJson([string]$Path, [object]$Value) {
  $text = ($Value | ConvertTo-Json -Depth 12) + "`r`n"
  return Write-AtomicText $Path $text
}

function Enter-TransactionLock([string]$ConfigPath, [int]$WaitSeconds) {
  if ($WaitSeconds -lt 1 -or $WaitSeconds -gt 10) {
    throw 'TransactionLockTimeoutSec must be between 1 and 10 seconds'
  }
  $identity = [System.IO.Path]::GetFullPath($ConfigPath).ToLowerInvariant()
  $identityHash = Get-BytesSha256 $script:Utf8NoBom.GetBytes($identity)
  $name = 'Global\RamSharedKernelPromotion-' + $identityHash
  $mutex = New-Object System.Threading.Mutex($false, $name)
  $acquired = $false
  try {
    try {
      $acquired = $mutex.WaitOne($WaitSeconds * 1000, $false)
    } catch [System.Threading.AbandonedMutexException] {
      $acquired = $true
    }
    if (-not $acquired) {
      throw 'TRANSACTION_LOCK=REFUSED another canonical target transaction owns the bounded lock'
    }
    return [pscustomobject]@{
      Mutex = $mutex
      Name = $name
      IdentitySha256 = $identityHash
      Acquired = $true
    }
  } catch {
    if (-not $acquired) {
      $mutex.Dispose()
    }
    throw
  }
}

function Exit-TransactionLock([object]$Lock) {
  if ($null -eq $Lock) {
    return
  }
  if ($Lock.Acquired) {
    $Lock.Mutex.ReleaseMutex()
  }
  $Lock.Mutex.Dispose()
}

function Invoke-TransactionFailureBoundary([string]$Name, [string]$Selected) {
  if ($Selected -ceq $Name) {
    throw "injected deterministic transaction failure: $Name"
  }
}

function Restore-ExactFileState([string]$Path, [bool]$Existed, [byte[]]$Bytes) {
  $savedBoundary = $script:AtomicFailureBoundary
  $script:AtomicFailureBoundary = ''
  try {
    if ($Existed) {
      [void](Write-AtomicBytes $Path $Bytes)
      if ((Get-BytesSha256 ([System.IO.File]::ReadAllBytes($Path))) -cne (Get-BytesSha256 $Bytes)) {
        throw 'exact file-state restoration hash mismatch'
      }
    } elseif (Test-Path -LiteralPath $Path) {
      Remove-Item -LiteralPath $Path -Force
      if (Test-Path -LiteralPath $Path) {
        throw 'exact file-state restoration did not restore absence'
      }
    }
  } finally {
    $script:AtomicFailureBoundary = $savedBoundary
  }
}

function Assert-TransactionReadyIdentity(
  [object]$Pair,
  [string]$ConfigPath,
  [string]$ExpectedConfigSha256,
  [object]$Lock,
  [hashtable]$InitialArtifactIdentity
) {
  if ($null -eq $Lock -or -not $Lock.Acquired) {
    throw 'READY requires an actively held canonical transaction lock'
  }
  $reloaded = Read-KernelPairManifest $Pair.ManifestPath $Pair.ManifestSha256
  Assert-KernelPairArtifacts $reloaded
  if ((Get-Sha256 $ConfigPath) -cne $ExpectedConfigSha256) {
    throw 'READY target configuration changed after candidate verification'
  }
  $canonicalConfigIdentity = Get-BytesSha256 $script:Utf8NoBom.GetBytes(
    [System.IO.Path]::GetFullPath($ConfigPath).ToLowerInvariant()
  )
  if ($Lock.IdentitySha256 -cne $canonicalConfigIdentity) {
    throw 'READY canonical target identity differs from the held transaction lock'
  }
  if ($reloaded.Map.pair_id -cne $Pair.Map.pair_id -or
      $reloaded.KernelPath -cne $Pair.KernelPath -or
      $reloaded.ModulesPath -cne $Pair.ModulesPath) {
    throw 'READY artifact identity changed after the transaction began'
  }
  foreach ($artifactPath in @(
    $Pair.ManifestPath,
    $Pair.KernelPath,
    $Pair.ModulesPath,
    $Pair.LayoutInventoryPath,
    $Pair.QemuStampPath
  )) {
    $canonicalArtifactPath = [System.IO.Path]::GetFullPath($artifactPath)
    $before = $InitialArtifactIdentity[$canonicalArtifactPath]
    $after = Get-FileIdentity $canonicalArtifactPath
    if ($null -eq $before -or
        $after.NumberOfLinks -ne 1 -or
        $after.Volume -ne $before.Volume -or
        $after.Index -ne $before.Index) {
      throw 'READY artifact filesystem identity changed after the transaction began'
    }
  }
}

function Get-TransactionArtifactIdentity([object]$Pair) {
  $identity = @{}
  foreach ($artifactPath in @(
    $Pair.ManifestPath,
    $Pair.KernelPath,
    $Pair.ModulesPath,
    $Pair.LayoutInventoryPath,
    $Pair.QemuStampPath
  )) {
    $canonicalArtifactPath = [System.IO.Path]::GetFullPath($artifactPath)
    $value = Get-FileIdentity $canonicalArtifactPath
    if ($value.NumberOfLinks -ne 1) {
      throw 'transaction artifact must have exactly one filesystem link'
    }
    $identity[$canonicalArtifactPath] = $value
  }
  return $identity
}

function Invoke-HermeticTransactionFixture(
  [object]$Pair,
  [string]$ConfigPath,
  [string]$ReceiptRoot,
  [string]$FailureBoundary,
  [int]$LockTimeoutSeconds,
  [int]$HoldMilliseconds,
  [string]$LockEvidencePath
) {
  if ($HoldMilliseconds -lt 0 -or $HoldMilliseconds -gt 10000) {
    throw 'HoldTransactionLockMilliseconds must be between 0 and 10000'
  }
  $configParent = Split-Path -Parent $ConfigPath
  $receiptParent = Split-Path -Parent $ReceiptRoot
  if (-not (Test-Path -LiteralPath $configParent -PathType Container) -or
      -not (Test-Path -LiteralPath $receiptParent -PathType Container)) {
    throw 'transaction fixture parents must exist before lock acquisition'
  }

  $lock = $null
  $receiptRootCreated = $false
  $snapshotPath = ''
  $transactionReceiptPath = ''
  $currentReceiptPath = Join-Path $ReceiptRoot 'promotion-current.json'
  $currentExisted = $false
  $currentBytes = [byte[]]@()
  $configExisted = $false
  $configBytes = [byte[]]@()
  $captureComplete = $false
  $mutationStarted = $false
  $lockEvidenceMutationAttempted = $false
  $configMutationAttempted = $false
  $currentReceiptMutationAttempted = $false
  $snapshotMutationAttempted = $false
  $transactionReceiptMutationAttempted = $false
  $caught = $false
  try {
    $lock = Enter-TransactionLock $ConfigPath $LockTimeoutSeconds
    $currentExisted = Test-Path -LiteralPath $currentReceiptPath -PathType Leaf
    if ($currentExisted) {
      [void](Assert-RegularFile $currentReceiptPath 'transaction current receipt')
      $currentBytes = [System.IO.File]::ReadAllBytes($currentReceiptPath)
    } elseif (Test-Path -LiteralPath $currentReceiptPath) {
      throw 'transaction current receipt target is not a regular file'
    }
    $configExisted = Test-Path -LiteralPath $ConfigPath -PathType Leaf
    if ($configExisted) {
      [void](Assert-RegularFile $ConfigPath 'transaction config target')
      $configBytes = [System.IO.File]::ReadAllBytes($ConfigPath)
    } elseif (Test-Path -LiteralPath $ConfigPath) {
      throw 'transaction config target is not a regular file'
    }
    $captureComplete = $true
    Invoke-TransactionFailureBoundary 'after_capture_before_validation' $FailureBoundary
    Assert-NoReparseAncestors $ConfigPath 'transaction fixture config target'
    Assert-NoReparseAncestors $ReceiptRoot 'transaction fixture receipt target'
    $initialArtifactIdentity = Get-TransactionArtifactIdentity $Pair
    $revalidatedPair = Read-KernelPairManifest $Pair.ManifestPath $Pair.ManifestSha256
    Assert-KernelPairArtifacts $revalidatedPair
    if (-not [string]::IsNullOrWhiteSpace($LockEvidencePath)) {
      if (Test-Path -LiteralPath $LockEvidencePath) {
        throw 'transaction lock evidence path must begin absent'
      }
      $mutationStarted = $true
      $lockEvidenceMutationAttempted = $true
      [void](Write-AtomicText $LockEvidencePath ('LOCKED:' + $lock.IdentitySha256))
    }
    if ($HoldMilliseconds -gt 0) {
      Start-Sleep -Milliseconds $HoldMilliseconds
    }
    if ($FailureBoundary -match '^atomic_') {
      $script:AtomicFailureBoundary = $FailureBoundary
    }
    if (-not (Test-Path -LiteralPath $ReceiptRoot)) {
      $mutationStarted = $true
      New-Item -ItemType Directory -Path $ReceiptRoot | Out-Null
      $receiptRootCreated = $true
    }
    Assert-NoReparseAncestors $ReceiptRoot 'transaction fixture receipt root'
    Invoke-TransactionFailureBoundary 'after_receipt_root' $FailureBoundary

    $transactionId = 'fixture-' + [guid]::NewGuid().ToString('N')
    $snapshotPath = Join-Path $ReceiptRoot ('wslconfig-snapshot-' + $transactionId + '.bin')
    $transactionReceiptPath = Join-Path $ReceiptRoot ('promotion-' + $transactionId + '.json')
    $mutationStarted = $true
    $snapshotMutationAttempted = $true
    [void](Write-AtomicBytes $snapshotPath $configBytes)
    Invoke-TransactionFailureBoundary 'after_snapshot' $FailureBoundary

    $originalText = if ($configExisted) { $script:Utf8NoBom.GetString($configBytes) } else { '' }
    $bundledText = Remove-KernelPairFromConfigText $originalText
    $configMutationAttempted = $true
    [void](Write-AtomicText $ConfigPath $bundledText)
    Invoke-TransactionFailureBoundary 'after_bundled_config' $FailureBoundary

    $receipt = [ordered]@{ schema = 'ramshared.kernel-transaction-fixture.v1'; status = 'IN_PROGRESS'; pair_id = $Pair.Map.pair_id }
    $currentReceiptMutationAttempted = $true
    [void](Write-AtomicJson $currentReceiptPath $receipt)
    Invoke-TransactionFailureBoundary 'after_current_receipt' $FailureBoundary
    $transactionReceiptMutationAttempted = $true
    [void](Write-AtomicJson $transactionReceiptPath $receipt)
    Invoke-TransactionFailureBoundary 'after_transaction_receipt' $FailureBoundary

    $candidateText = Set-KernelPairInConfigText $bundledText $Pair.KernelPath $Pair.ModulesPath
    $candidateHash = Write-AtomicText $ConfigPath $candidateText
    Invoke-TransactionFailureBoundary 'after_candidate_config' $FailureBoundary
    if ($FailureBoundary -ceq 'post_check_mutation') {
      [System.IO.File]::AppendAllText($ConfigPath, "# injected post-check mutation`r`n", $script:Utf8NoBom)
    }
    Assert-TransactionReadyIdentity $Pair $ConfigPath $candidateHash $lock $initialArtifactIdentity
    $receipt.status = 'READY'
    [void](Write-AtomicJson $transactionReceiptPath $receipt)
    Invoke-TransactionFailureBoundary 'after_ready_receipt' $FailureBoundary
    if (-not [string]::IsNullOrWhiteSpace($FailureBoundary)) {
      throw "requested transaction failure boundary was not reached: $FailureBoundary"
    }
  } catch {
    if ($_.Exception.Message -match '^TRANSACTION_LOCK=REFUSED') {
      throw
    }
    if ([string]::IsNullOrWhiteSpace($FailureBoundary)) {
      throw
    }
    $expectedFailure = switch -Regex ($FailureBoundary) {
      '^atomic_' { '^injected deterministic atomic failure: ' + [regex]::Escape($FailureBoundary) + '$'; break }
      '^post_check_mutation$' { '^READY target configuration changed after candidate verification$'; break }
      default { '^injected deterministic transaction failure: ' + [regex]::Escape($FailureBoundary) + '$' }
    }
    if ($_.Exception.Message -cnotmatch $expectedFailure) {
      throw
    }
    $caught = $true
  } finally {
    if ($null -ne $lock) {
      try {
        $script:AtomicFailureBoundary = ''
        if ($captureComplete -and $mutationStarted -and
            $lockEvidenceMutationAttempted -and
            -not [string]::IsNullOrWhiteSpace($LockEvidencePath) -and
            (Test-Path -LiteralPath $LockEvidencePath -PathType Leaf)) {
          Remove-Item -LiteralPath $LockEvidencePath -Force
        }
        if ($captureComplete -and $mutationStarted) {
          if ($configMutationAttempted) {
            Restore-ExactFileState $ConfigPath $configExisted $configBytes
          }
          if ($currentReceiptMutationAttempted) {
            Restore-ExactFileState $currentReceiptPath $currentExisted $currentBytes
          }
          if ($snapshotMutationAttempted -and
              -not [string]::IsNullOrWhiteSpace($snapshotPath) -and
              (Test-Path -LiteralPath $snapshotPath -PathType Leaf)) {
            Remove-Item -LiteralPath $snapshotPath -Force
          }
          if ($transactionReceiptMutationAttempted -and
              -not [string]::IsNullOrWhiteSpace($transactionReceiptPath) -and
              (Test-Path -LiteralPath $transactionReceiptPath -PathType Leaf)) {
            Remove-Item -LiteralPath $transactionReceiptPath -Force
          }
          if ($receiptRootCreated -and (Test-Path -LiteralPath $ReceiptRoot -PathType Container)) {
            $remaining = @(Get-ChildItem -LiteralPath $ReceiptRoot -Force)
            if ($remaining.Count -eq 0) {
              Remove-Item -LiteralPath $ReceiptRoot -Force
            }
          }
        }
      } finally {
        Exit-TransactionLock $lock
      }
    }
  }
  if (-not [string]::IsNullOrWhiteSpace($FailureBoundary) -and -not $caught) {
    throw 'transaction fixture did not observe its requested failure'
  }
  Write-Host ('TRANSACTION_FIXTURE=PASS boundary=' + $(if ([string]::IsNullOrWhiteSpace($FailureBoundary)) { 'none' } else { $FailureBoundary }))
}

function ConvertFrom-CanaryPayload([string]$Payload) {
  $keys = @(
    'CANARY_SCHEMA', 'CANARY_PHASE', 'CANARY_WSL_EXIT', 'CANARY_BOOT_ID',
    'CANARY_UNAME', 'CANARY_SYSTEMD', 'CANARY_FAILED_UNITS',
    'CANARY_DXG_NODE', 'CANARY_DXG_DEV_T', 'CANARY_DXG_COUNT',
    'CANARY_XWAYLAND_COUNT_BEFORE', 'CANARY_XWAYLAND_COUNT_AFTER',
    'CANARY_WSLG_TRANSACTION', 'CANARY_GPU_DRIVER', 'CANARY_DXG_PROBE',
    'CANARY_MODULES', 'CANARY_MODULE_VERMAGIC', 'CANARY_MODULE_TREE',
    'CANARY_DISTRO_ID', 'CANARY_DISTRO_VERSION_ID',
    'CANARY_DMESG_READABLE', 'CANARY_DMESG_SHA256',
    'CANARY_DXG_FORTIFY_WARNINGS', 'CANARY_WAIT_FOR_BOOT_FAILURES',
    'CANARY_JOURNAL_UNCLEAN', 'CANARY_P9_CANCELLED',
    'CANARY_KERNEL_FATALS', 'CANARY_DXG_QUERY_ERRORS'
  )
  return ConvertFrom-StrictKeyValue $Payload $keys 'canary payload' 'CANARY_[A-Z0-9_]+'
}

function Get-ApprovedUnitSet([string]$Text) {
  if ([string]::IsNullOrWhiteSpace($Text)) {
    return @()
  }
  $units = @($Text -split ',' | ForEach-Object { $_.Trim() } | Sort-Object -Unique)
  foreach ($unit in $units) {
    if ($unit -cnotmatch '^[A-Za-z0-9_.@:-]+\.service$') {
      throw "approved failed unit is invalid: $unit"
    }
  }
  return $units
}

function Test-CanaryPayload([string]$Payload, [string]$Phase, [object]$Pair) {
  $reasons = New-Object 'System.Collections.Generic.List[string]'
  try {
    $map = ConvertFrom-CanaryPayload $Payload
  } catch {
    return [pscustomobject]@{ Ok = $false; Reasons = @($_.Exception.Message); Values = @{} }
  }
  if ($map.CANARY_SCHEMA -cne '1') { $reasons.Add('canary schema mismatch') }
  if ($map.CANARY_PHASE -cne $Phase) { $reasons.Add('canary phase mismatch') }
  if ($map.CANARY_WSL_EXIT -cne '0') { $reasons.Add('wsl canary command failed') }
  if ($map.CANARY_BOOT_ID -cnotmatch '^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$') {
    $reasons.Add('boot ID is invalid')
  }
  $approved = @(Get-ApprovedUnitSet $ApprovedFailedUnits)
  [string[]]$failed = @()
  if ($map.CANARY_FAILED_UNITS -cne 'none') {
    $failed = @($map.CANARY_FAILED_UNITS -split ',' | Sort-Object -Unique)
  }
  if ($map.CANARY_SYSTEMD -ceq 'running') {
    if ($failed.Count -ne 0) { $reasons.Add('systemd reports running with failed units') }
  } elseif ($map.CANARY_SYSTEMD -ceq 'degraded') {
    if ($approved.Count -eq 0 -or ($failed -join ',') -cne ($approved -join ',')) {
      $reasons.Add('degraded systemd units do not exactly match the explicit WSL-only exception')
    }
  } else {
    $reasons.Add('systemd did not reach running or an explicitly approved degraded state')
  }
  if ($map.CANARY_DXG_NODE -cne 'char') { $reasons.Add('/dev/dxg is not a character device') }
  if ($map.CANARY_DXG_DEV_T -cnotmatch '^[0-9a-f]+:[0-9a-f]+$') { $reasons.Add('/dev/dxg dev_t is invalid') }
  $dxgCount = ConvertTo-NonNegativeInt $map.CANARY_DXG_COUNT 'CANARY_DXG_COUNT'
  $xwaylandAfter = ConvertTo-NonNegativeInt $map.CANARY_XWAYLAND_COUNT_AFTER 'CANARY_XWAYLAND_COUNT_AFTER'
  [void](ConvertTo-NonNegativeInt $map.CANARY_XWAYLAND_COUNT_BEFORE 'CANARY_XWAYLAND_COUNT_BEFORE')
  if ($dxgCount -ne 1) { $reasons.Add("DXG character-device cardinality is $dxgCount, expected 1") }
  if ($map.CANARY_WSLG_TRANSACTION -cne 'ok' -or $xwaylandAfter -lt 1) {
    $reasons.Add('bounded WSLg/Xwayland transaction did not complete')
  }
  if ($map.CANARY_DXG_PROBE -cne 'ok' -or $map.CANARY_GPU_DRIVER -cnotmatch '^[A-Za-z0-9._,-]+$') {
    $reasons.Add('bounded DXG driver query failed or returned an invalid driver version')
  }
  if ($map.CANARY_DISTRO_ID -cnotmatch '^[a-z0-9._-]+$' -or
      $map.CANARY_DISTRO_VERSION_ID -cnotmatch '^[A-Za-z0-9._-]+$') {
    $reasons.Add('distro identity is invalid')
  }
  if ($map.CANARY_DMESG_READABLE -cne '1' -or $map.CANARY_DMESG_SHA256 -cnotmatch '^[0-9a-f]{64}$') {
    $reasons.Add('current-boot dmesg evidence is unreadable or unbound')
  }
  foreach ($key in @(
    'CANARY_DXG_FORTIFY_WARNINGS', 'CANARY_WAIT_FOR_BOOT_FAILURES',
    'CANARY_JOURNAL_UNCLEAN', 'CANARY_P9_CANCELLED', 'CANARY_KERNEL_FATALS'
  )) {
    $number = ConvertTo-NonNegativeInt $map[$key] $key
    if ($number -ne 0) { $reasons.Add("$key is non-zero ($number)") }
  }
  [void](ConvertTo-NonNegativeInt $map.CANARY_DXG_QUERY_ERRORS 'CANARY_DXG_QUERY_ERRORS')
  if ($Phase -ceq 'candidate') {
    if ($map.CANARY_UNAME -cne $Pair.Map.release) { $reasons.Add('candidate uname does not match the sealed release') }
    if ($map.CANARY_MODULES -cne 'ok') { $reasons.Add('candidate module metadata is unavailable') }
    if ($map.CANARY_MODULE_TREE -cne 'ok') { $reasons.Add('candidate module tree is missing or double nested') }
    if ($map.CANARY_MODULE_VERMAGIC -cne $Pair.Map.module_vermagic) {
      $reasons.Add('candidate module vermagic differs from the sealed pair')
    }
  } elseif ($Phase -cne 'bundled') {
    $reasons.Add('unknown canary phase')
  }
  return [pscustomobject]@{ Ok = ($reasons.Count -eq 0); Reasons = @($reasons); Values = $map }
}

function Test-CanaryComparison([object]$Baseline, [object]$Candidate) {
  $reasons = New-Object 'System.Collections.Generic.List[string]'
  foreach ($key in @('CANARY_DISTRO_ID', 'CANARY_DISTRO_VERSION_ID', 'CANARY_GPU_DRIVER', 'CANARY_DXG_COUNT')) {
    if ($Baseline.Values[$key] -cne $Candidate.Values[$key]) {
      $reasons.Add("candidate differs from bundled baseline in $key")
    }
  }
  if ($Baseline.Values.CANARY_BOOT_ID -ceq $Candidate.Values.CANARY_BOOT_ID) {
    $reasons.Add('candidate did not produce a fresh boot ID')
  }
  $baselineErrors = ConvertTo-NonNegativeInt $Baseline.Values.CANARY_DXG_QUERY_ERRORS 'baseline DXG query errors'
  $candidateErrors = ConvertTo-NonNegativeInt $Candidate.Values.CANARY_DXG_QUERY_ERRORS 'candidate DXG query errors'
  if ($candidateErrors -gt $baselineErrors) {
    $reasons.Add("candidate DXG query errors exceed bundled baseline ($candidateErrors > $baselineErrors)")
  }
  return [pscustomobject]@{ Ok = ($reasons.Count -eq 0); Reasons = @($reasons) }
}

function Test-RollbackComparison([object]$Baseline, [object]$Rollback, [object]$Candidate) {
  $reasons = New-Object 'System.Collections.Generic.List[string]'
  if ($null -eq $Baseline -or -not $Baseline.Ok) {
    $reasons.Add('a valid bundled baseline is unavailable')
    return [pscustomobject]@{ Ok = $false; Reasons = @($reasons) }
  }
  foreach ($key in @(
    'CANARY_UNAME', 'CANARY_DISTRO_ID', 'CANARY_DISTRO_VERSION_ID',
    'CANARY_GPU_DRIVER', 'CANARY_DXG_COUNT'
  )) {
    if ($Rollback.Values[$key] -cne $Baseline.Values[$key]) {
      $reasons.Add("rollback differs from bundled baseline in $key")
    }
  }
  if ($Rollback.Values.CANARY_BOOT_ID -ceq $Baseline.Values.CANARY_BOOT_ID) {
    $reasons.Add('rollback did not produce a fresh boot ID after the bundled baseline')
  }
  if ($null -ne $Candidate -and
      $Candidate.Values.ContainsKey('CANARY_BOOT_ID') -and
      $Candidate.Values.CANARY_BOOT_ID -match '^[0-9a-f-]{36}$' -and
      $Rollback.Values.CANARY_BOOT_ID -ceq $Candidate.Values.CANARY_BOOT_ID) {
    $reasons.Add('rollback retained the candidate boot ID')
  }
  return [pscustomobject]@{ Ok = ($reasons.Count -eq 0); Reasons = @($reasons) }
}

function New-CanaryGuestCommand([string]$Phase, [string]$ModuleName) {
  if ($Phase -cnotmatch '^(bundled|candidate)$' -or $ModuleName -cnotmatch '^[A-Za-z0-9_+-]+$') {
    throw 'unsafe canary command input'
  }
  $guestScript = @'
set -u
export LC_ALL=C
phase='__PHASE__'
module='__MODULE__'
uname_value="$(uname -r 2>/dev/null || printf unavailable)"
boot_id="$(cat /proc/sys/kernel/random/boot_id 2>/dev/null || printf unavailable)"
systemd_state="$(timeout -k 2s 20s systemctl is-system-running --wait 2>/dev/null || true)"
failed_units="$(systemctl --failed --no-legend --plain --no-pager 2>/dev/null | awk '{print $1}' | sort -u | paste -sd, -)"
[ -n "$failed_units" ] || failed_units=none
if [ -c /dev/dxg ]; then dxg_node=char; else dxg_node=missing; fi
dxg_dev_t="$(stat -Lc '%t:%T' /dev/dxg 2>/dev/null || printf unavailable)"
dxg_count="$(find /dev -maxdepth 1 -type c -name dxg 2>/dev/null | wc -l | tr -d ' ')"
xwayland_before="$(pgrep -xc Xwayland 2>/dev/null || true)"
if command -v timeout >/dev/null 2>&1 && command -v xdpyinfo >/dev/null 2>&1 && [ -n "${DISPLAY:-}" ] && timeout -k 2s 8s xdpyinfo -display "$DISPLAY" >/dev/null 2>&1; then
  wslg_transaction=ok
else
  wslg_transaction=failed
fi
xwayland_after="$(pgrep -xc Xwayland 2>/dev/null || true)"
if command -v timeout >/dev/null 2>&1 && command -v nvidia-smi >/dev/null 2>&1; then
  gpu_driver="$(timeout -k 2s 10s nvidia-smi --query-gpu=driver_version --format=csv,noheader,nounits 2>/dev/null | sort -u | paste -sd, - || true)"
else
  gpu_driver=''
fi
if [ -n "$gpu_driver" ]; then dxg_probe=ok; else dxg_probe=failed; gpu_driver=unavailable; fi
module_vermagic="$(modinfo -F vermagic "$module" 2>/dev/null | head -n 1 || true)"
if [ -n "$module_vermagic" ]; then modules_state=ok; else modules_state=missing; module_vermagic=unavailable; fi
if [ -r "/lib/modules/$uname_value/modules.dep" ] && [ ! -d "/lib/modules/$uname_value/$uname_value" ]; then module_tree=ok; else module_tree=failed; fi
distro_id=unavailable
distro_version_id=unavailable
if [ -r /etc/os-release ]; then
  . /etc/os-release
  distro_id="${ID:-unavailable}"
  distro_version_id="${VERSION_ID:-unavailable}"
fi
if dmesg_text="$(dmesg --level=err,warn 2>&1)"; then dmesg_readable=1; else dmesg_readable=0; fi
dmesg_sha="$(printf '%s' "$dmesg_text" | sha256sum 2>/dev/null | awk '{print $1}')"
[ -n "$dmesg_sha" ] || dmesg_sha=unavailable
dxg_fortify="$(printf '%s\n' "$dmesg_text" | grep -F -c 'memcpy: detected field-spanning write' || true)"
wait_boot="$(printf '%s\n' "$dmesg_text" | grep -F -c 'WaitForBootProcess: /sbin/init failed to start within' || true)"
journal_unclean="$(printf '%s\n' "$dmesg_text" | grep -Eic 'journal.*(unclean|corrupt)|uncleanly shut down' || true)"
p9_cancelled="$(printf '%s\n' "$dmesg_text" | grep -Eic 'p9.*Operation canceled' || true)"
kernel_fatals="$(printf '%s\n' "$dmesg_text" | grep -Eic 'kernel panic|BUG:|Oops:|general protection fault' || true)"
dxg_query_errors="$(printf '%s\n' "$dmesg_text" | grep -Ec 'dxgkio_(query_adapter_info|is_feature_enabled): Ioctl failed: -(22|2)' || true)"
printf 'CANARY_SCHEMA=1\n'
printf 'CANARY_PHASE=%s\n' "$phase"
printf 'CANARY_BOOT_ID=%s\n' "$boot_id"
printf 'CANARY_UNAME=%s\n' "$uname_value"
printf 'CANARY_SYSTEMD=%s\n' "$systemd_state"
printf 'CANARY_FAILED_UNITS=%s\n' "$failed_units"
printf 'CANARY_DXG_NODE=%s\n' "$dxg_node"
printf 'CANARY_DXG_DEV_T=%s\n' "$dxg_dev_t"
printf 'CANARY_DXG_COUNT=%s\n' "$dxg_count"
printf 'CANARY_XWAYLAND_COUNT_BEFORE=%s\n' "$xwayland_before"
printf 'CANARY_XWAYLAND_COUNT_AFTER=%s\n' "$xwayland_after"
printf 'CANARY_WSLG_TRANSACTION=%s\n' "$wslg_transaction"
printf 'CANARY_GPU_DRIVER=%s\n' "$gpu_driver"
printf 'CANARY_DXG_PROBE=%s\n' "$dxg_probe"
printf 'CANARY_MODULES=%s\n' "$modules_state"
printf 'CANARY_MODULE_VERMAGIC=%s\n' "$module_vermagic"
printf 'CANARY_MODULE_TREE=%s\n' "$module_tree"
printf 'CANARY_DISTRO_ID=%s\n' "$distro_id"
printf 'CANARY_DISTRO_VERSION_ID=%s\n' "$distro_version_id"
printf 'CANARY_DMESG_READABLE=%s\n' "$dmesg_readable"
printf 'CANARY_DMESG_SHA256=%s\n' "$dmesg_sha"
printf 'CANARY_DXG_FORTIFY_WARNINGS=%s\n' "$dxg_fortify"
printf 'CANARY_WAIT_FOR_BOOT_FAILURES=%s\n' "$wait_boot"
printf 'CANARY_JOURNAL_UNCLEAN=%s\n' "$journal_unclean"
printf 'CANARY_P9_CANCELLED=%s\n' "$p9_cancelled"
printf 'CANARY_KERNEL_FATALS=%s\n' "$kernel_fatals"
printf 'CANARY_DXG_QUERY_ERRORS=%s\n' "$dxg_query_errors"
'@
  $guestScript = $guestScript.Replace('__PHASE__', $Phase).Replace('__MODULE__', $ModuleName)
  $encoded = [Convert]::ToBase64String($script:Utf8NoBom.GetBytes($guestScript))
  return "printf '%s' '$encoded' | base64 -d | sh"
}

function Invoke-WslCanary([string]$Phase, [object]$Pair) {
  $command = New-CanaryGuestCommand $Phase $Pair.Map.module_name
  $guestDeadline = [Math]::Max(3, $TimeoutSec - 5)
  $result = Invoke-BoundedProcess $script:WslExe @(
    '-d', $Distro, '--', 'timeout', '-k', '2s', ($guestDeadline.ToString() + 's'),
    'sh', '-lc', $command
  ) $TimeoutSec "WSL $Phase canary"
  return ($result.Stdout.TrimEnd() + "`nCANARY_WSL_EXIT=0`n")
}

function Invoke-WslShutdownChecked {
  [void](Invoke-BoundedProcess $script:WslExe @('--shutdown') 30 'wsl.exe --shutdown')
}

function Assert-WslStopped {
  for ($attempt = 1; $attempt -le 5; $attempt++) {
    $running = Invoke-BoundedProcess $script:WslExe @('--list', '--running', '--quiet') 10 'wsl.exe running-distro probe'
    if (($running.Stdout -replace "`0", '').Trim().Length -eq 0) {
      return
    }
    Start-Sleep -Seconds 1
  }
  throw 'WSL stopped-state gate failed: at least one distro remains running'
}

function Get-HostIdentity([string]$WslVersionText, [object]$Versions) {
  $windows = Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
  $identity = [ordered]@{
    windows_product_name = [string]$windows.ProductName
    windows_display_version = [string]$windows.DisplayVersion
    windows_current_build = [string]$windows.CurrentBuild
    windows_ubr = [string]$windows.UBR
    wsl_version = [string]$Versions.wsl
    wsl_kernel_version = [string]$Versions.kernel
    wslg_version = [string]$Versions.wslg
    wsl_reported_windows_version = [string]$Versions.windows
    wsl_version_output_sha256 = Get-BytesSha256 $script:Utf8NoBom.GetBytes($WslVersionText)
  }
  foreach ($key in $identity.Keys) {
    if ([string]::IsNullOrWhiteSpace($identity[$key]) -or $identity[$key] -match "[`r`n`t]") {
      throw "host identity field is unavailable or malformed: $key"
    }
  }
  return $identity
}

function Assert-Inputs {
  if ($Distro -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._ -]{0,127}$') {
    throw 'Distro must be one exact, safe WSL distribution name'
  }
  if ($TimeoutSec -lt 10 -or $TimeoutSec -gt 120) {
    throw 'TimeoutSec must be between 10 and 120 seconds'
  }
  [void](Get-ApprovedUnitSet $ApprovedFailedUnits)
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
  $liveManifest = Assert-NoUnsafeWindowsPathSyntax $KernelPairManifest 'KernelPairManifest'
  $liveConfig = Assert-NoUnsafeWindowsPathSyntax $WslConfig 'WslConfig'
  $liveReceipts = Assert-NoUnsafeWindowsPathSyntax $ReceiptDirectory 'ReceiptDirectory'
  Assert-Sha256 $ExpectedKernelManifestSha256 'ExpectedKernelManifestSha256'
  $expectedConfig = [System.IO.Path]::GetFullPath((Join-Path $env:USERPROFILE '.wslconfig'))
  $expectedReceipts = [System.IO.Path]::GetFullPath('C:\wsl\ramshared-receipts')
  if ($liveConfig -cne $expectedConfig -or $liveReceipts -cne $expectedReceipts) {
    throw 'live promotion paths must use the canonical user .wslconfig and receipt root'
  }
  Write-Output 'LIVE_GATE=ACCEPTED exact-run-and-confirmation'
  Write-Output 'LIVE_PROMOTION=NO_GO direct-suspended-supervision-and-handle-execution-unproven'
  exit 2
}
$terminalFixturePresent = (
  $EvaluateExternalFailureFixture.IsPresent -or
  -not [string]::IsNullOrWhiteSpace($EvaluateExternalTimeoutFixture) -or
  -not [string]::IsNullOrWhiteSpace($EvaluateTransactionFixture) -or
  -not [string]::IsNullOrWhiteSpace($DryRunConfig) -or
  -not [string]::IsNullOrWhiteSpace($EvaluateCanaryFixture) -or
  $PreflightOnly.IsPresent
)
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
if ($PSBoundParameters.ContainsKey('EvaluateDeadlineFixtureSec')) {
  if ($EvaluateDeadlineFixtureSec -lt 1 -or $EvaluateDeadlineFixtureSec -gt 5) {
    Write-Output 'FIXTURE_COMBINATION=REFUSED deadline-fixture-must-be-1..5-seconds'
    exit 2
  }
  $script:TransactionDeadlineSec = $EvaluateDeadlineFixtureSec
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
if ($fixtureInputPresent -and -not $terminalFixturePresent) {
  Write-Output 'FIXTURE_COMBINATION=REFUSED companion-input-requires-terminal-fixture'
  exit 2
}

if ($PreflightOnly.IsPresent -and
    (-not $PSBoundParameters.ContainsKey('EvaluateRuntimeFixture') -or
     [string]::IsNullOrWhiteSpace($EvaluateRuntimeFixture))) {
  Write-Output 'PREFLIGHT_RUNTIME_FIXTURE=REFUSED required-before-any-wsl.exe-access'
  exit 2
}

if ($EvaluateSlowStartupFixture.IsPresent) {
  $startupRemainingMs = ($script:TransactionDeadlineSec * 1000) -
    [int]$script:TransactionClock.ElapsedMilliseconds
  if ($startupRemainingMs -gt 0) {
    Start-Sleep -Milliseconds $startupRemainingMs
  }
  Write-Output 'PRE_CHILD_DEADLINE=REFUSED NO_CHILD_CREATED=1'
  exit 2
}

if ($InternalSuspendedWorker.IsPresent) {
  try {
    Initialize-ProcessJobType
    [RamSharedSuspendedJobProcess]::ValidateAndCloseInheritedJobHandle($InternalJobHandle)
  } catch {
    Write-Output ('INTERNAL_CUSTODY=REFUSED ' + $_.Exception.Message)
    exit 2
  }
} else {
  Initialize-ProcessJobType
  $workerArguments = @(
    '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
    '-File', $PSCommandPath,
    '-InternalSuspendedWorker', '-InternalJobHandle', '{RAMSHARED_JOB_HANDLE}'
  )
  foreach ($entry in $PSBoundParameters.GetEnumerator()) {
    if ($entry.Key -ceq 'InternalSuspendedWorker' -or $entry.Key -ceq 'InternalJobHandle') {
      continue
    }
    if ($entry.Value -is [System.Management.Automation.SwitchParameter]) {
      if ($entry.Value.IsPresent) { $workerArguments += ('-' + $entry.Key) }
    } else {
      $workerArguments += @([string]('-' + $entry.Key), [string]$entry.Value)
    }
  }
  $remainingWorkerMs = ($script:TransactionDeadlineSec * 1000) - [int]$script:TransactionClock.ElapsedMilliseconds
  if ($remainingWorkerMs -lt 1) {
    throw 'direct launcher supervision deadline expired before worker creation'
  }
  $workerLine = (@($workerArguments | ForEach-Object { ConvertTo-ProcessArgument $_ }) -join ' ')
  $worker = $null
  try {
    $worker = [RamSharedSuspendedJobProcess]::Start(
      (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'),
      $workerLine,
      $remainingWorkerMs,
      $script:MaximumCapturedOutputBytes,
      $false,
      $false,
      $false,
      $false,
      'direct launcher suspended worker'
    )
    $workerResult = $worker.Wait()
    if (-not $workerResult.JobEmpty) {
      throw 'direct launcher worker returned without empty-job proof'
    }
    if (-not [string]::IsNullOrWhiteSpace($workerResult.Stdout)) {
      Write-Output $workerResult.Stdout.TrimEnd()
    }
    if (-not [string]::IsNullOrWhiteSpace($workerResult.Stderr)) {
      Write-Output $workerResult.Stderr.TrimEnd()
    }
    exit $workerResult.ExitCode
  } finally {
    if ($null -ne $worker) { $worker.Dispose() }
  }
}

$fixtureRootFull = ''
if ($hermeticEvaluation) {
  try {
    $fixtureRootFull = Assert-FixtureContext $FixtureRoot $FixtureNonce
    foreach ($requiredFixturePath in @(
      [pscustomobject]@{ Value = $KernelPairManifest; Name = 'KernelPairManifest' },
      [pscustomobject]@{ Value = $WslConfig; Name = 'WslConfig' },
      [pscustomobject]@{ Value = $ReceiptDirectory; Name = 'ReceiptDirectory' }
    )) {
      try {
        $validatedFixturePath = Assert-FixturePath $requiredFixturePath.Value $requiredFixturePath.Name $fixtureRootFull
      } catch {
        throw ($requiredFixturePath.Name + ' path validation failed: ' + $_.Exception.Message)
      }
      switch ($requiredFixturePath.Name) {
        'KernelPairManifest' { $KernelPairManifest = $validatedFixturePath }
        'WslConfig' { $WslConfig = $validatedFixturePath }
        'ReceiptDirectory' { $ReceiptDirectory = $validatedFixturePath }
      }
    }
    foreach ($fixturePath in @(
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
        try {
          [void](Assert-FixturePath $fixturePath.Value $fixturePath.Name $fixtureRootFull)
        } catch {
          throw ($fixturePath.Name + ' path validation failed: ' + $_.Exception.Message)
        }
      }
    }
    if (-not [string]::IsNullOrWhiteSpace($EvaluateTransactionFixture) -and
        ([System.IO.Path]::GetFullPath($EvaluateTransactionFixture) -cne [System.IO.Path]::GetFullPath($WslConfig))) {
      throw 'EvaluateTransactionFixture must name the same confined target as WslConfig'
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

$script:InjectSlowHash = $script:SlowHashRequested
Assert-Inputs

if ($EvaluateExternalFailureFixture) {
  $caught = $false
  try {
    [void](Invoke-BoundedProcess $env:ComSpec @('/d', '/c', 'exit', '17') 10 'external failure fixture')
  } catch {
    if ($_.Exception.Message -match 'exit 17') {
      $caught = $true
    } else {
      throw
    }
  }
  if (-not $caught) {
    throw 'external non-zero exit was not rejected'
  }
  Write-Host 'EXTERNAL_FAILURE_FIXTURE=PASS'
  exit 0
}

if (-not [string]::IsNullOrWhiteSpace($EvaluateExternalTimeoutFixture)) {
  $pidFile = [System.IO.Path]::GetFullPath($EvaluateExternalTimeoutFixture)
  $pidParent = Split-Path -Parent $pidFile
  if (-not (Test-Path -LiteralPath $pidParent -PathType Container)) {
    throw 'external timeout fixture parent is missing'
  }
  $escapedPidFile = $pidFile.Replace("'", "''")
  $fixtureCommand = @"
`$child = Start-Process -FilePath `$env:ComSpec -ArgumentList @('/d','/c','ping -t 127.0.0.1') -PassThru -NoNewWindow
[System.IO.File]::WriteAllText('$escapedPidFile', `$child.Id.ToString(), [System.Text.Encoding]::ASCII)
exit 0
"@
  $encodedFixture = [Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($fixtureCommand))
  $custodyOutcome = ''
  try {
    [void](Invoke-BoundedProcess (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') @(
      '-NoProfile', '-NonInteractive', '-EncodedCommand', $encodedFixture
    ) 2 'external process-tree timeout fixture')
  } catch {
    $custodyOutcome = $_.Exception.Message
  }
  if ($InjectAssignFailureFixture.IsPresent) {
    if ($custodyOutcome -cnotmatch 'INJECTED_ASSIGN_FAILURE' -or
        (Test-Path -LiteralPath $pidFile)) {
      throw 'assignment-failure fixture was resumed or lacked exact handle-reap proof'
    }
    Write-Host 'PROCESS_ASSIGN_FAILURE_FIXTURE=PASS root-reaped-by-handle-before-resume'
    exit 0
  }
  if ($InjectResumeFailureFixture.IsPresent) {
    if ($custodyOutcome -cnotmatch 'INJECTED_RESUME_FAILURE job-empty-root-reaped-before-resume' -or
        (Test-Path -LiteralPath $pidFile)) {
      throw 'resume-failure fixture ran child code or lacked exact empty-job handle-reap proof'
    }
    Write-Host 'PROCESS_RESUME_FAILURE_FIXTURE=PASS job-empty-root-reaped-before-resume'
    exit 0
  }
  if ($InjectRootCreationMismatchFixture.IsPresent) {
    if ($custodyOutcome -cnotmatch 'root creation identity mismatch' -or
        $custodyOutcome -cnotmatch 'ROOT_CREATION_MISMATCH=REFUSED' -or
        $custodyOutcome -cnotmatch 'JOB_EMPTY=1' -or
        $custodyOutcome -cnotmatch 'ROOT_CREATION=[0-9]+') {
      throw 'creation-identity fixture lacked exact mismatch and empty-job proof'
    }
    Write-Host 'PROCESS_CREATION_IDENTITY_FIXTURE=PASS mismatch-refused-job-empty'
    exit 0
  }
  if ($custodyOutcome -cnotmatch 'timed out' -or
      $custodyOutcome -cnotmatch 'JOB_EMPTY=1' -or
      $custodyOutcome -cnotmatch 'ROOT_CREATION=[0-9]+' -or
      -not (Test-Path -LiteralPath $pidFile -PathType Leaf)) {
    throw 'external timeout fixture lacked exact handle/job/creation custody proof'
  }
  if ($InjectTerminateFailureFixture.IsPresent -and
      $custodyOutcome -cnotmatch 'INJECTED_TERMINATE_FAILURE=OBSERVED cleanup-terminate-checked') {
    throw 'termination-failure fixture lacked checked fallback termination evidence'
  }
  Write-Host 'EXTERNAL_TIMEOUT_FIXTURE=PASS JOB_EMPTY=1 HANDLE_IDENTITY=PROVEN'
  exit 0
}

$pair = Read-KernelPairManifest $KernelPairManifest $ExpectedKernelManifestSha256
Assert-KernelPairArtifacts $pair

if (-not $hermeticEvaluation) {
  Assert-ExactLivePaths $WslConfig $ReceiptDirectory
  Write-Output 'MODULE_VHDX_PROVENANCE=REFUSED cryptographic-containment-not-verifiable'
  Write-Output 'LIVE_PROMOTION=NO_GO'
  exit 2
}

if (-not [string]::IsNullOrWhiteSpace($EvaluateTransactionFixture)) {
  Invoke-HermeticTransactionFixture $pair ([System.IO.Path]::GetFullPath($EvaluateTransactionFixture)) ([System.IO.Path]::GetFullPath($ReceiptDirectory)) $InjectFailureBoundary $TransactionLockTimeoutSec $HoldTransactionLockMilliseconds $TransactionLockEvidenceFixture
  exit 0
}

$runtimeText = ''
if (-not [string]::IsNullOrWhiteSpace($EvaluateRuntimeFixture)) {
  $runtimeItem = Assert-RegularFile ([System.IO.Path]::GetFullPath($EvaluateRuntimeFixture)) 'runtime fixture'
  $runtimeText = [System.IO.File]::ReadAllText($runtimeItem.FullName)
} elseif (-not $hermeticEvaluation) {
  $runtimeText = (Invoke-BoundedProcess $script:WslExe @('--version') 15 'wsl.exe --version').Stdout
}

$versions = $null
if ($runtimeText.Length -gt 0) {
  $versions = Assert-RuntimeAdmission $pair $runtimeText
}

if (-not [string]::IsNullOrWhiteSpace($DryRunConfig)) {
  $dryPath = [System.IO.Path]::GetFullPath($DryRunConfig)
  $original = Get-ConfigText $dryPath
  $armed = Set-KernelPairInConfigText $original $pair.KernelPath $pair.ModulesPath
  Assert-ConfigPairCardinality $armed 1
  [void](Write-AtomicText $dryPath $armed)
  $armedReadback = Get-ConfigText $dryPath
  Assert-ConfigPairCardinality $armedReadback 1
  $disarmed = Remove-KernelPairFromConfigText $armedReadback
  Assert-ConfigPairCardinality $disarmed 0
  [void](Write-AtomicText $dryPath $disarmed)
  Assert-ConfigPairCardinality (Get-ConfigText $dryPath) 0
  Write-Host 'CONFIG_PAIR_FIXTURE=PASS'
  exit 0
}

if (-not [string]::IsNullOrWhiteSpace($EvaluateCanaryFixture)) {
  if ([string]::IsNullOrWhiteSpace($BaselineCanaryFixture) -or $runtimeText.Length -eq 0) {
    throw 'canary fixture evaluation requires baseline and runtime fixtures'
  }
  $baselinePayload = [System.IO.File]::ReadAllText((Assert-RegularFile ([System.IO.Path]::GetFullPath($BaselineCanaryFixture)) 'baseline canary fixture').FullName)
  $candidatePayload = [System.IO.File]::ReadAllText((Assert-RegularFile ([System.IO.Path]::GetFullPath($EvaluateCanaryFixture)) 'candidate canary fixture').FullName)
  $baselineVerdict = Test-CanaryPayload $baselinePayload 'bundled' $pair
  $candidateVerdict = Test-CanaryPayload $candidatePayload 'candidate' $pair
  $comparison = if ($baselineVerdict.Ok -and $candidateVerdict.Ok) { Test-CanaryComparison $baselineVerdict $candidateVerdict } else { [pscustomobject]@{ Ok = $false; Reasons = @('phase verdict failed') } }
  $rollbackComparison = [pscustomobject]@{ Ok = $true; Reasons = @() }
  if (-not [string]::IsNullOrWhiteSpace($EvaluateRollbackFixture)) {
    $rollbackPayload = [System.IO.File]::ReadAllText((Assert-RegularFile ([System.IO.Path]::GetFullPath($EvaluateRollbackFixture)) 'rollback canary fixture').FullName)
    $rollbackVerdict = Test-CanaryPayload $rollbackPayload 'bundled' $pair
    $rollbackComparison = if ($rollbackVerdict.Ok) { Test-RollbackComparison $baselineVerdict $rollbackVerdict $candidateVerdict } else { [pscustomobject]@{ Ok = $false; Reasons = @('rollback phase verdict failed') } }
  }
  if ($baselineVerdict.Ok -and $candidateVerdict.Ok -and $comparison.Ok -and $rollbackComparison.Ok) {
    Write-Host 'CANARY_TRANSACTION_FIXTURE=PASS'
    exit 0
  }
  $reasons = @($baselineVerdict.Reasons) + @($candidateVerdict.Reasons) + @($comparison.Reasons) + @($rollbackComparison.Reasons)
  Write-Host ('CANARY_TRANSACTION_FIXTURE=FAIL reasons=' + ($reasons -join '; '))
  exit 1
}

if ($PreflightOnly) {
  $temporaryConfig = Join-Path $fixtureRootFull ('preflight-' + [guid]::NewGuid().ToString('N') + '.wslconfig')
  try {
    [void](Write-AtomicText $temporaryConfig "[wsl2]`r`nmemory=8GB`r`n")
    $armed = Set-KernelPairInConfigText (Get-ConfigText $temporaryConfig) $pair.KernelPath $pair.ModulesPath
    Assert-ConfigPairCardinality $armed 1
    [void](Write-AtomicText $temporaryConfig $armed)
    $disarmed = Remove-KernelPairFromConfigText (Get-ConfigText $temporaryConfig)
    Assert-ConfigPairCardinality $disarmed 0
    [void](Write-AtomicText $temporaryConfig $disarmed)
  } finally {
    if (Test-Path -LiteralPath $temporaryConfig -PathType Leaf) {
      Remove-Item -LiteralPath $temporaryConfig -Force
    }
  }
  Write-Host ('PREFLIGHT=PASS pair_id=' + $pair.Map.pair_id + ' wsl=' + $versions.wsl + ' layout=' + $pair.Map.modules_layout)
  exit 0
}

throw 'internal invariant: live promotion cannot pass the module-to-VHDX provenance refusal'
