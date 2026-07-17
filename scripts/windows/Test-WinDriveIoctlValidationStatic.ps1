#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ScriptPath,
    [string]$QueuePath,
    [string]$DriverPath,
    [string]$VirtdiskPath,
    [string]$ControlPath,
    [string]$BuildScriptPath
)

$ErrorActionPreference = "Stop"
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path $scriptRoot "Invoke-WinDriveIoctlValidation.ps1"
}
if ([string]::IsNullOrWhiteSpace($QueuePath)) {
    $QueuePath = Join-Path $scriptRoot "..\..\drivers\windows\ramshared\queue.c"
}
if ([string]::IsNullOrWhiteSpace($DriverPath)) {
    $DriverPath = Join-Path $scriptRoot "..\..\drivers\windows\ramshared\driver.c"
}
if ([string]::IsNullOrWhiteSpace($VirtdiskPath)) {
    $VirtdiskPath = Join-Path $scriptRoot "..\..\drivers\windows\ramshared\virtdisk.c"
}
if ([string]::IsNullOrWhiteSpace($ControlPath)) {
    $ControlPath = Join-Path $scriptRoot "..\..\drivers\windows\ramshared\control.c"
}
if ([string]::IsNullOrWhiteSpace($BuildScriptPath)) {
    $BuildScriptPath = Join-Path $scriptRoot "Build-Drivers.ps1"
}
$source = Get-Content -LiteralPath $ScriptPath -Raw

$requiredImplementations = @(
    "Invoke-ReservedCqeInjection",
    "Invoke-CompletionReentryInjection",
    "Invoke-RundownDuringCopyInjection",
    "Invoke-StartIoReadCopyRaceInjection"
)
foreach ($name in $requiredImplementations) {
    if ($source -notmatch [regex]::Escape("function $name")) {
        throw "missing concurrent injector: $name"
    }
}
$requiredStartIoTokens = @(
    "StartQueuePump",
    "PhysicalReadWithPump",
    "DrainSqPublishCq",
    "Find-RamshareDiskInfo",
    "Wait-MsftDiskWithIoPump",
    "Invoke-EnumerationIoPump",
    "early post-CREATE",
    "STARTIO_READ_COPY_RACE"
)
foreach ($token in $requiredStartIoTokens) {
    if ($source -notmatch [regex]::Escape($token)) {
        throw "StartIo READ-copy race harness missing token: $token"
    }
}

if ($source -match "require concurrent I/O injectors") {
    throw "placeholder concurrent-injector path is still present"
}

$requiredVerdicts = @(
    "REFUSE_RESERVED_CQE",
    "COMPLETION_REENTRY_NO_SLOT_REUSE",
    "RUNDOWN_UNMAP_AFTER_COPY",
    "STARTIO_READ_COPY_RACE"
)
foreach ($name in $requiredVerdicts) {
    if ($source -notmatch [regex]::Escape("`$verdict.$name = 1")) {
        throw "injector never records success: $name"
    }
}

$requiredExactVpdTokens = @(
    '$_.Serial -ieq $expectedSerial',
    '$_.Size -eq [uint64]$SizeBytes'
)
foreach ($token in $requiredExactVpdTokens) {
    if ($source -notmatch [regex]::Escape($token)) {
        throw "VPD verdict lacks exact identity gate: $token"
    }
}
$forbiddenVpdFallbacks = @(
    'Size + name unique',
    'Exactly one live PnP RAMSHARE unit'
)
foreach ($token in $forbiddenVpdFallbacks) {
    if ($source -match [regex]::Escape($token)) {
        throw "unsafe VPD fallback is present: $token"
    }
}
# Capacity must not rely on CHS-derived Win32_DiskDrive.Size alone.
$requiredCapacityTokens = @(
    'IOCTL_DISK_GET_LENGTH_INFO',
    'DiskLenQuery',
    'PhysicalDrive'
)
foreach ($token in $requiredCapacityTokens) {
    if ($source -notmatch [regex]::Escape($token)) {
        throw "VPD capacity gate missing authoritative length surface: $token"
    }
}

$queueSource = Get-Content -LiteralPath $QueuePath -Raw
$acquires = [regex]::Matches(
    $queueSource,
    [regex]::Escape("ExAcquireRundownProtection(&Q->IoRundown)")
).Count
$releases = [regex]::Matches(
    $queueSource,
    [regex]::Escape("ExReleaseRundownProtection(&Q->IoRundown)")
).Count
# ≥2 acquires (QSubmit + QCommit); many release *sites* are expected (multi-exit).
# Runtime pairing is 1:1 per path; site count must not under-release.
if ($acquires -lt 2 -or $releases -lt $acquires) {
    throw "QSubmit/QCommit rundown incomplete: acquire=$acquires release=$releases"
}
if ($queueSource -notmatch "QSubmit[\s\S]{0,800}?ExAcquireRundownProtection") {
    throw "QSubmit does not acquire IoRundown near entry"
}
if ($queueSource -notmatch "QCommitAndFetch[\s\S]{0,800}?ExAcquireRundownProtection") {
    throw "QCommitAndFetch does not acquire IoRundown near entry"
}
if ($queueSource -notmatch "ExWaitForRundownProtectionRelease\(&Q->IoRundown\)") {
    throw "teardown does not wait IoRundown before unmap"
}
if ($queueSource -match [regex]::Escape("__except (EXCEPTION_EXECUTE_HANDLER)")) {
    throw "QMapUserRegion still catches every exception instead of filtering expected probe faults"
}
if ($queueSource -notmatch "QProbeAndLockExceptionFilter\(\s*_In_ ULONG ExceptionCode\s*\)") {
    throw "QMapUserRegion is missing explicit probe/lock exception filter"
}
if ($queueSource -notmatch "static DRIVER_CANCEL QCommitCancel;") {
    throw "QCommitCancel is missing DRIVER_CANCEL prototype for WDK Code Analysis"
}

$controlSource = Get-Content -LiteralPath $ControlPath -Raw
$requiredDispatchPrototypes = @(
    "static DRIVER_DISPATCH CtlDispatchCreateClose;",
    "static DRIVER_DISPATCH CtlDispatchCleanup;",
    "static DRIVER_DISPATCH CtlDispatchDeviceControl;"
)
foreach ($token in $requiredDispatchPrototypes) {
    if ($controlSource -notmatch [regex]::Escape($token)) {
        throw "control dispatch lacks DRIVER_DISPATCH prototype: $token"
    }
}

$driverSource = Get-Content -LiteralPath $DriverPath -Raw
$requiredDriverTokens = @(
    "HW_INITIALIZATION_DATA hw",
    "STOR_FEATURE_VIRTUAL_MINIPORT",
    "hw.HwAdapterControl = HwStorAdapterControl",
    "hw.HwFreeAdapterResources = HwStorFreeAdapterResources"
)
foreach ($token in $requiredDriverTokens) {
    if ($driverSource -notmatch [regex]::Escape($token)) {
        throw "virtual StorPort initialization missing: $token"
    }
}

$virtdiskSource = Get-Content -LiteralPath $VirtdiskPath -Raw
$requiredVpdLifecycleTokens = @(
    "VdIsAsciiHexSerial(Params->serial)",
    "g_AdapterExt == NULL",
    "STATUS_DEVICE_NOT_READY",
    "VdHandleReportLuns(Srb, FALSE)",
    "VdHandleReportLuns(Srb, TRUE)",
    "Srb->SrbStatus = SRB_STATUS_NO_DEVICE",
    "allocationLen = Srb->Cdb[4]",
    "RtlCopyMemory(&response[4], Disk->serial, 16)",
    "(Srb->Cdb[1] & 0x1F) != 0x10",
    "allocationLen = ((ULONG)Srb->Cdb[10] << 24)",
    "response[11] = (UCHAR)(bs & 0xFF)"
)
foreach ($token in $requiredVpdLifecycleTokens) {
    if ($virtdiskSource -notmatch [regex]::Escape($token)) {
        throw "VPD lifecycle gate missing: $token"
    }
}
$forbiddenVpdLifecycleTokens = @(
    "VdHandleInquiry(NULL, Srb)",
    "RtlFillMemory(&buf[4], 16, '0')",
    "(c >= 'a' && c <= 'f')",
    "VdHandleReadCapacity(NULL, Srb)"
)
foreach ($token in $forbiddenVpdLifecycleTokens) {
    if ($virtdiskSource -cmatch [regex]::Escape($token)) {
        throw "placeholder VPD lifecycle is present: $token"
    }
}
$busChanges = [regex]::Matches(
    $virtdiskSource,
    [regex]::Escape("StorPortNotification(BusChangeDetected")
).Count
if ($busChanges -lt 2) {
    throw "CREATE/DESTROY must both notify BusChangeDetected: count=$busChanges"
}

$buildSource = Get-Content -LiteralPath $BuildScriptPath -Raw
$requiredBuildFlags = @('"/W4"', '"/WX"', '"/wd4324"', '"/Z7"')
foreach ($flag in $requiredBuildFlags) {
    if ($buildSource -notmatch [regex]::Escape($flag)) {
        throw "Windows driver build is missing required flag: $flag"
    }
}
if ($buildSource -match [regex]::Escape('"/Zi"')) {
    throw "Windows driver build uses /Zi and can write vc140.pdb outside the output tree"
}

Write-Host "STATIC_SCSI_LIFECYCLE_TEST=PASS"
Write-Host "STATIC_INJECTOR_TEST=PASS"
Write-Host "STATIC_WDK_FLAGS_TEST=PASS"
