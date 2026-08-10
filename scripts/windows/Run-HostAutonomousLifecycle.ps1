#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Manifest,
    [ValidateRange(3, 3)]
    [int]$ColdBoots = 3,
    [switch]$ApprovePhysicalHost,
    [switch]$ApproveReboot,
    [switch]$Resume,
    [string]$ResumeApprovalToken = "",
    [switch]$AllowWatchdogShutdown,
    [string]$Controller = "C:\ramshared\bin\ramshared-winsvc.exe",
    [string]$CampaignRoot = "C:\ProgramData\RamShared\physical-autonomous-gate"
)

$ErrorActionPreference = "Stop"
$TaskName = "RamSharedPhysicalAutonomousGate"
$StatePath = Join-Path $CampaignRoot "state.json"
$ResultsPath = Join-Path $CampaignRoot "results.jsonl"
$WatchdogMarker = Join-Path $CampaignRoot "watchdog.armed"
$CampaignMutexName = "Global\RamSharedPhysicalAutonomousGate.v1"

function Assert-PhysicalApproval {
    if (-not $ApprovePhysicalHost) {
        throw "Physical host mutation requires -ApprovePhysicalHost."
    }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole(
            [Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Elevated administrator token required."
    }
}
function Write-State([object]$State) {
    $temp = "$StatePath.new"
    [IO.File]::WriteAllText($temp, ($State | ConvertTo-Json -Depth 6),
        [Text.UTF8Encoding]::new($false))
    Move-Item $temp $StatePath -Force
}
function Append-Result([hashtable]$Row) {
    $line = ($Row | ConvertTo-Json -Compress -Depth 8)
    if ([Text.Encoding]::UTF8.GetByteCount($line) -gt 16384) {
        throw "physical lifecycle evidence row exceeds 16 KiB"
    }
    [IO.File]::AppendAllText($ResultsPath, $line + [Environment]::NewLine,
        [Text.UTF8Encoding]::new($false))
}

function Get-Sha256Hex {
    param([byte[]]$Bytes)
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        ([BitConverter]::ToString($sha.ComputeHash($Bytes))).Replace("-", "")
    }
    finally { $sha.Dispose() }
}
function Test-IntendedPayloadMatch {
    param([byte[]]$Intended, [byte[]]$Observed)
    if ($null -eq $Intended -or $null -eq $Observed) { return $false }
    (Get-Sha256Hex $Intended) -eq (Get-Sha256Hex $Observed)
}
function Resolve-ExactOnlineDisk {
    param(
        [object[]]$Rows,
        [string]$ExpectedSerial,
        [uint64]$ExpectedSize,
        [uint32]$ExpectedSectorSize
    )
    $matches = @($Rows | Where-Object {
            (([string]$_.FriendlyName -replace '\s+', ' ').Trim() -ceq
                "RAMSHARE VRAMDISK") -and
            (([string]$_.SerialNumber).Trim() -ceq $ExpectedSerial) -and
            ([uint64]$_.Size -eq $ExpectedSize) -and
            (([string]$_.BusType).Trim() -ceq "Virtual") -and
            ([uint32]$_.LogicalSectorSize -eq $ExpectedSectorSize) -and
            ([uint32]$_.PhysicalSectorSize -eq $ExpectedSectorSize) -and
            (([string]$_.PartitionStyle).Trim() -ceq "RAW") -and
            ([int]$_.NumberOfPartitions -eq 0) -and
            (-not [bool]$_.IsBoot) -and (-not [bool]$_.IsSystem)
        })
    if ($matches.Count -ne 1 -or $Rows.Count -ne 1) {
        throw "exact current-run RAW product disk count=$($matches.Count), observed=$($Rows.Count)"
    }
    $matches[0]
}
function Assert-PagefileLetterFree {
    param(
        [string]$Letter,
        [string[]]$ActivePagefiles,
        [string[]]$ConfiguredPagefiles,
        [bool]$QuerySucceeded
    )
    if (-not $QuerySucceeded) { throw "pagefile query failed closed" }
    $prefix = "^$([regex]::Escape($Letter)):\\"
    if (@($ActivePagefiles | Where-Object { $_ -match $prefix }).Count -ne 0) {
        throw "active pagefile targets product volume letter $Letter"
    }
    if (@($ConfiguredPagefiles | Where-Object { $_ -match $prefix }).Count -ne 0) {
        throw "configured pagefile targets product volume letter $Letter"
    }
    $true
}
function Assert-SupportedStop {
    param([string]$CommandError, [string]$FinalState)
    if (-not [string]::IsNullOrWhiteSpace($CommandError)) {
        throw "supported stop command failed: $CommandError"
    }
    if ($FinalState -ne "Stopped") {
        throw "supported stop final state is $FinalState"
    }
    $true
}
function Quote-ProcessArgument {
    param([string]$Value)
    if ($Value -notmatch '[\s"]') { return $Value }
    '"' + ($Value -replace '(\\*)"', '$1$1\"' -replace '(\\+)$', '$1$1') + '"'
}
function Invoke-BoundedProcess {
    param([string]$FilePath, [string[]]$Arguments, [int]$Seconds)
    if ($Seconds -lt 1) { throw "process deadline must be positive" }
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $FilePath
    $info.Arguments = ($Arguments -join " ")
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) { throw "failed to start bounded process" }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($Seconds * 1000)) {
            $taskkill = Join-Path $env:SystemRoot "System32\taskkill.exe"
            $killer = [Diagnostics.Process]::Start($taskkill,
                "/PID $($process.Id) /T /F")
            if (-not $killer.WaitForExit(5000)) {
                try { $killer.Kill() } catch {}
                throw "bounded process-tree termination timed out"
            }
            if ($killer.ExitCode -ne 0) {
                throw "taskkill failed to terminate bounded process tree: exit=$($killer.ExitCode)"
            }
            if (-not $process.WaitForExit(5000)) {
                throw "bounded process tree did not terminate"
            }
            return [pscustomobject]@{
                completed = $false
                exit_code = $null
                stdout = $stdoutTask.Result
                stderr = $stderrTask.Result
                process_tree_terminated = $true
            }
        }
        $process.WaitForExit()
        [pscustomobject]@{
            completed = $true
            exit_code = $process.ExitCode
            stdout = $stdoutTask.Result
            stderr = $stderrTask.Result
            process_tree_terminated = $false
        }
    }
    finally { $process.Dispose() }
}
function Invoke-BoundedPowerShellChild {
    param([string]$ScriptPath, [string[]]$Arguments, [int]$Seconds)
    $powershell = Join-Path $PSHOME "powershell.exe"
    $childArguments = @(
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-File", (Quote-ProcessArgument $ScriptPath)) + $Arguments
    Invoke-BoundedProcess $powershell $childArguments $Seconds
}
function New-RebootApproval {
    param([int]$Boot)
    $bytes = New-Object byte[] 32
    $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
    $token = ([BitConverter]::ToString($bytes)).Replace("-", "")
    [pscustomobject]@{
        boot = $Boot
        token = $token
        token_sha256 = Get-Sha256Hex ([Text.Encoding]::UTF8.GetBytes($token))
        expires_utc = [datetime]::UtcNow.AddMinutes(30).ToString("o")
    }
}
function Use-ResumeApproval {
    param([object]$State, [string]$Token, [int]$Boot)
    if ($State.status -ne "scheduled" -or
        [int]$State.approval_boot -ne $Boot -or
        [bool]$State.approval_consumed -or
        [datetimeoffset]::UtcNow -gt
            [datetimeoffset]::Parse([string]$State.approval_expires_utc) -or
        [string]::IsNullOrWhiteSpace($Token)) {
        throw "resume approval is stale, replayed, or not scheduled"
    }
    $actual = Get-Sha256Hex ([Text.Encoding]::UTF8.GetBytes($Token))
    if ($actual -cne [string]$State.approval_token_sha256) {
        throw "resume approval token mismatch"
    }
    $State.approval_consumed = $true
    $State.approval_token_sha256 = ""
    $State.status = "running"
    $State
}
function Get-ScheduledResumeArguments {
    param(
        [string]$ScriptPath,
        [string]$ManifestPath,
        [int]$Boots,
        [string]$ControllerPath,
        [string]$Root,
        [string]$Token,
        [bool]$WatchdogShutdownApproved
    )
    $items = @(
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-File", (Quote-ProcessArgument $ScriptPath),
        "-Manifest", (Quote-ProcessArgument $ManifestPath),
        "-ColdBoots", [string]$Boots,
        "-Resume", "-ResumeApprovalToken", (Quote-ProcessArgument $Token),
        "-Controller", (Quote-ProcessArgument $ControllerPath),
        "-CampaignRoot", (Quote-ProcessArgument $Root)
    )
    if ($WatchdogShutdownApproved) { $items += "-AllowWatchdogShutdown" }
    $items -join " "
}
function Get-WatchdogTimeoutAction {
    param([bool]$ShutdownApproved)
    if ($ShutdownApproved) { "shutdown" } else { "record_only" }
}
function Get-WatchdogCommand {
    param(
        [string]$MarkerPath,
        [string]$LogPath,
        [string]$ExpectedNonce,
        [bool]$ShutdownApproved
    )
    $watchdogAction = Get-WatchdogTimeoutAction $ShutdownApproved
    $marker = $MarkerPath.Replace("'", "''")
    $log = $LogPath.Replace("'", "''")
    $shutdownLine = if ($ShutdownApproved) { "shutdown.exe /s /t 0 /f" } else { "" }
@"
try {
  Start-Sleep -Seconds 600
  if (Test-Path -LiteralPath '$marker') {
    `$armed = Get-Content -LiteralPath '$marker' -Raw | ConvertFrom-Json
    if (`$armed.nonce -ceq '$ExpectedNonce') {
      [IO.File]::AppendAllText('$log', (Get-Date).ToString('o') + " watchdog_$watchdogAction`r`n")
      $shutdownLine
    }
  }
}
finally {
  Remove-Item -LiteralPath `$MyInvocation.MyCommand.Path -Force -ErrorAction SilentlyContinue
}
"@
}
function Invoke-CampaignSafetyCleanup {
    param(
        [string]$MarkerPath,
        [string]$ScheduledTaskName,
        [scriptblock]$TaskRemover = $null
    )
    Remove-Item -LiteralPath $MarkerPath -Force -ErrorAction SilentlyContinue
    Get-ChildItem -LiteralPath (Split-Path $MarkerPath -Parent) `
        -Filter "watchdog-*.ps1" -File -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
    if ($TaskRemover) {
        & $TaskRemover $ScheduledTaskName
    }
    else {
        Unregister-ScheduledTask -TaskName $ScheduledTaskName `
            -Confirm:$false -ErrorAction SilentlyContinue
    }
}
function Enter-CampaignMutex {
    $mutex = New-Object Threading.Mutex($false, $CampaignMutexName)
    try {
        try { $acquired = $mutex.WaitOne([timespan]::FromSeconds(10)) }
        catch [Threading.AbandonedMutexException] { $acquired = $true }
        if (-not $acquired) { throw "campaign mutex acquisition timed out" }
        $mutex
    }
    catch {
        $mutex.Dispose()
        throw
    }
}
function Exit-CampaignMutex([Threading.Mutex]$Mutex) {
    try { $Mutex.ReleaseMutex() } finally { $Mutex.Dispose() }
}
function Invoke-BoundedPowerShellText {
    param([string]$Text, [string[]]$Arguments, [int]$Seconds)
    $scriptPath = Join-Path $CampaignRoot `
        ("bounded-" + [guid]::NewGuid().ToString("N") + ".ps1")
    [IO.File]::WriteAllText($scriptPath, $Text, [Text.UTF8Encoding]::new($false))
    try {
        $result = Invoke-BoundedPowerShellChild $scriptPath $Arguments $Seconds
        if (-not $result.completed) {
            throw "bounded child timed out; tree_terminated=$($result.process_tree_terminated)"
        }
        if ($result.exit_code -ne 0) {
            throw "bounded child failed exit=$($result.exit_code): $($result.stderr)"
        }
        $result.stdout
    }
    finally { Remove-Item $scriptPath -Force -ErrorAction SilentlyContinue }
}
function Get-PackagedDriveConfig {
    param($ManifestDocument, [string]$ManifestPath)
    $artifacts = @($ManifestDocument.artifacts | Where-Object role -eq "winsvc_config")
    if ($artifacts.Count -ne 1) { throw "manifest must have one winsvc_config" }
    $configPath = Join-Path (Split-Path $ManifestPath -Parent) `
        $artifacts[0].relative_path
    if (-not (Test-Path $configPath -PathType Leaf)) {
        throw "winsvc_config is missing"
    }
    if ((Get-FileHash $configPath -Algorithm SHA256).Hash -ine
        [string]$artifacts[0].sha256) {
        throw "winsvc_config manifest hash mismatch"
    }
    $text = Get-Content $configPath -Raw
    $fields = @{}
    foreach ($field in @("size_bytes", "block_size", "volume_letter", "evidence_path")) {
        $pattern = switch ($field) {
            "volume_letter" { '(?m)^volume_letter\s*=\s*"([A-Z])"\s*$' }
            "evidence_path" { '(?m)^evidence_path\s*=\s*"([^"]+)"\s*$' }
            default { "(?m)^$field\s*=\s*(\d+)\s*$" }
        }
        $matches = [regex]::Matches($text, $pattern)
        if ($matches.Count -ne 1) { throw "winsvc_config must define $field exactly once" }
        $fields[$field] = $matches[0].Groups[1].Value
    }
    $evidence = $fields.evidence_path -replace '\\\\', '\'
    if (-not [IO.Path]::IsPathRooted($evidence)) {
        throw "winsvc_config evidence_path must be absolute"
    }
    [pscustomobject]@{
        path = $configPath
        size_bytes = [uint64]$fields.size_bytes
        block_size = [uint32]$fields.block_size
        volume_letter = [string]$fields.volume_letter
        evidence_path = $evidence
    }
}
function Get-PagefileObservations {
    $script = @'
$ErrorActionPreference = "Stop"
$active = @((Get-CimInstance Win32_PageFileUsage -ErrorAction Stop).Name)
$configured = @((Get-ItemProperty `
    "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management" `
    -Name PagingFiles -ErrorAction Stop).PagingFiles)
[pscustomobject]@{ active = $active; configured = $configured } |
    ConvertTo-Json -Compress -Depth 4
'@
    try {
        $value = Invoke-BoundedPowerShellText $script @() 10 | ConvertFrom-Json
        [pscustomobject]@{
            succeeded = $true
            active = @($value.active)
            configured = @($value.configured)
        }
    }
    catch {
        [pscustomobject]@{ succeeded = $false; active = @(); configured = @() }
    }
}
function Get-ExactDiskObservations {
    $script = @'
$ErrorActionPreference = "Stop"
$rows = @(Get-Disk -ErrorAction Stop | Where-Object {
    (([string]$_.FriendlyName -replace '\s+', ' ').Trim()) -ceq "RAMSHARE VRAMDISK"
} | ForEach-Object {
    [pscustomobject]@{
        Number = $_.Number; FriendlyName = $_.FriendlyName
        SerialNumber = $_.SerialNumber; Size = [uint64]$_.Size
        BusType = [string]$_.BusType
        LogicalSectorSize = [uint32]$_.LogicalSectorSize
        PhysicalSectorSize = [uint32]$_.PhysicalSectorSize
        PartitionStyle = [string]$_.PartitionStyle
        NumberOfPartitions = [int]$_.NumberOfPartitions
        IsBoot = [bool]$_.IsBoot; IsSystem = [bool]$_.IsSystem
    }
})
ConvertTo-Json -InputObject $rows -Compress -Depth 4
'@
    $json = Invoke-BoundedPowerShellText $script @() 30
    @($json | ConvertFrom-Json)
}
function Assert-ZeroResidue {
    $rows = @(Get-ExactDiskObservations)
    if ($rows.Count -ne 0) {
        throw "zero-residue gate failed: exact product disk count=$($rows.Count)"
    }
    foreach ($name in @("RamSharedWinSvc", "RamSharedBroker")) {
        $service = Get-Service $name -ErrorAction SilentlyContinue
        if ($service -and $service.Status -eq "Running") {
            throw "zero-residue gate failed: $name is still running"
        }
    }
}
function Arm-Watchdog([bool]$ShutdownApproved) {
    $watchdogAction = Get-WatchdogTimeoutAction $ShutdownApproved
    $watchdogNonce = [guid]::NewGuid().ToString("N")
    [IO.File]::WriteAllText($WatchdogMarker,
        (([ordered]@{
                    armed_at = (Get-Date).ToString("o")
                    action = $watchdogAction
                    nonce = $watchdogNonce
                }) |
            ConvertTo-Json -Compress), [Text.UTF8Encoding]::new($false))
    $command = Get-WatchdogCommand $WatchdogMarker `
        (Join-Path $CampaignRoot "watchdog.log") $watchdogNonce `
        $ShutdownApproved
    $watchdogScript = Join-Path $CampaignRoot "watchdog-$watchdogNonce.ps1"
    [IO.File]::WriteAllText($watchdogScript, $command,
        [Text.UTF8Encoding]::new($false))
    Start-Process (Join-Path $PSHOME "powershell.exe") -WindowStyle Hidden `
        -ArgumentList @(
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-File", (Quote-ProcessArgument $watchdogScript)) | Out-Null
}
function Disarm-Watchdog {
    Remove-Item $WatchdogMarker -Force -ErrorAction SilentlyContinue
}
function Get-VersionRoot($ManifestDocument) {
    "C:\Program Files\RamShared\versions\$($ManifestDocument.version)-$($ManifestDocument.commit.Substring(0,12))"
}
function Get-ManifestVolumeLetter($ManifestDocument, [string]$ManifestPath) {
    (Get-PackagedDriveConfig $ManifestDocument $ManifestPath).volume_letter
}
function Get-LoadedDriverPath {
    $script = @'
$ErrorActionPreference = "Stop"
$rows = @(Get-CimInstance Win32_SystemDriver -Filter "Name='ramshared'" `
    -ErrorAction Stop | Select-Object Name, State, PathName)
if ($rows.Count -ne 1) { throw "exact ramshared driver identity not found" }
$rows[0] | ConvertTo-Json -Compress
'@
    $driver = Invoke-BoundedPowerShellText $script @() 10 | ConvertFrom-Json
    if ($driver.State -ne "Running") {
        throw "ramshared driver is not Running"
    }
    $path = ([string]$driver.PathName).Trim('"') -replace '^\\\?\?\\', ''
    if ($path -like "\SystemRoot\*") {
        $path = Join-Path $env:SystemRoot $path.Substring(12)
    }
    $path
}
function Assert-DriverBinaryMatch($ManifestDocument, [string]$CandidateRoot) {
    $relative = ($ManifestDocument.artifacts |
        Where-Object role -eq "driver_sys").relative_path
    if (-not $relative) { throw "manifest has no driver_sys artifact" }
    $loadedPath = Get-LoadedDriverPath
    if ((Get-FileHash $loadedPath -Algorithm SHA256).Hash -ne
        (Get-FileHash (Join-Path $CandidateRoot $relative) -Algorithm SHA256).Hash) {
        throw "ramshared driver BINARY_MATCH failed"
    }
}
function Resolve-CurrentOnlineEvidence {
    param([object[]]$Rows, [int]$ServicePid, [int64]$StartedUtcMs)
    $runPattern = "^run-$ServicePid-\d+-\d+$"
    $matches = @($Rows | Where-Object {
            [int]$_.schema -eq 1 -and [int]$_.pid -eq $ServicePid -and
            ([string]$_.phase) -ceq "Online" -and
            ([string]$_.run_id) -cmatch $runPattern -and
            ([string]$_.source_run_id) -ceq ([string]$_.run_id) -and
            [int64]$_.ts_utc_ms -ge ($StartedUtcMs - 2000) -and
            ([string]$_.lun_serial) -cmatch '^[0-9A-F]{16}$'
        })
    $runs = @($matches | Group-Object run_id)
    if ($runs.Count -ne 1) {
        throw "exact current winsvc Online run count=$($runs.Count)"
    }
    @($runs[0].Group | Sort-Object { [int64]$_.ts_utc_ms })[-1]
}
function Get-CurrentOnlineIdentity {
    param([string]$EvidenceRoot, [int]$ServicePid, [datetime]$StartedAt)
    $deadline = (Get-Date).AddSeconds(45)
    do {
        $online = @()
        foreach ($file in @(Get-ChildItem -LiteralPath $EvidenceRoot -Filter "*.jsonl" `
                    -File -ErrorAction SilentlyContinue | Where-Object {
                    $_.LastWriteTimeUtc -ge $StartedAt.ToUniversalTime().AddSeconds(-2)
                })) {
            foreach ($line in @(Get-Content -LiteralPath $file.FullName `
                        -ErrorAction SilentlyContinue)) {
                try { $row = $line | ConvertFrom-Json -ErrorAction Stop } catch { continue }
                Add-Member -InputObject $row -NotePropertyName source_run_id `
                    -NotePropertyValue $file.BaseName -Force
                if ($row.phase -eq "Online" -and [int]$row.pid -eq $ServicePid) {
                    $online += $row
                }
            }
        }
        if ($online.Count -ne 0) {
            $epoch = [datetime]::SpecifyKind([datetime]'1970-01-01',
                [DateTimeKind]::Utc)
            $startedUtcMs = [int64](
                ($StartedAt.ToUniversalTime() - $epoch).TotalMilliseconds)
            $row = Resolve-CurrentOnlineEvidence $online $ServicePid $startedUtcMs
            return [pscustomobject]@{
                run_id = [string]$row.run_id
                serial = [string]$row.lun_serial
                size_bytes = [uint64]$row.lun_size_bytes
            }
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw "current winsvc PID produced no Online evidence within 45 seconds"
}
function Invoke-ExactDiskFormat {
    param([object]$Disk, [object]$Config, [string]$ExpectedSerial)
    $script = @'
param($Number, $Serial, $Size, $Sector, $Letter)
$ErrorActionPreference = "Stop"
$rows = @(Get-Disk -ErrorAction Stop | Where-Object {
    $_.Number -eq [int]$Number -and
    (([string]$_.FriendlyName -replace '\s+', ' ').Trim()) -ceq "RAMSHARE VRAMDISK" -and
    (([string]$_.SerialNumber).Trim()) -ceq $Serial -and
    [uint64]$_.Size -eq [uint64]$Size -and
    ([string]$_.BusType) -ceq "Virtual" -and
    [uint32]$_.LogicalSectorSize -eq [uint32]$Sector -and
    [uint32]$_.PhysicalSectorSize -eq [uint32]$Sector -and
    ([string]$_.PartitionStyle) -ceq "RAW" -and
    [int]$_.NumberOfPartitions -eq 0 -and
    -not $_.IsBoot -and -not $_.IsSystem
})
if ($rows.Count -ne 1) { throw "exact RAW disk changed before mutation" }
if ([IO.DriveInfo]::GetDrives().Name -contains "$Letter`:\") {
    throw "target letter became occupied before mutation"
}
Initialize-Disk -Number ([int]$Number) -PartitionStyle GPT -PassThru |
    New-Partition -UseMaximumSize -DriveLetter $Letter |
    Format-Volume -FileSystem NTFS -NewFileSystemLabel RAMSHARE `
        -Confirm:$false -Force | Out-Null
'@
    $arguments = @(
        (Quote-ProcessArgument ([string]$Disk.Number)),
        (Quote-ProcessArgument $ExpectedSerial),
        (Quote-ProcessArgument ([string]$Config.size_bytes)),
        (Quote-ProcessArgument ([string]$Config.block_size)),
        (Quote-ProcessArgument $Config.volume_letter)
    )
    Invoke-BoundedPowerShellText $script $arguments 60 | Out-Null
}
function Register-OneShotResumeTask {
    param([string]$Arguments)
    $action = New-ScheduledTaskAction `
        -Execute (Join-Path $PSHOME "powershell.exe") -Argument $Arguments
    $trigger = New-ScheduledTaskTrigger -AtStartup
    $principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" `
        -LogonType ServiceAccount -RunLevel Highest
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger `
        -Principal $principal -Force | Out-Null
}
function Get-ServiceProcessIdBounded([string]$Name) {
    $script = @'
param($Name)
$ErrorActionPreference = "Stop"
$rows = @(Get-CimInstance Win32_Service -Filter "Name='$Name'" `
    -ErrorAction Stop | Select-Object Name, State, ProcessId, PathName)
if ($rows.Count -ne 1) { throw "exact service identity not found" }
$rows[0] | ConvertTo-Json -Compress
'@
    $json = Invoke-BoundedPowerShellText $script @((Quote-ProcessArgument $Name)) 10
    $json | ConvertFrom-Json
}
function Assert-FormattedVolumeBinding {
    param([int]$DiskNumber, [string]$Serial, [uint64]$Size, [string]$Letter)
    $script = @'
param($Number, $Serial, $Size, $Letter)
$ErrorActionPreference = "Stop"
$partitions = @(Get-Partition -DriveLetter $Letter -ErrorAction Stop)
if ($partitions.Count -ne 1) { throw "exact formatted partition count mismatch" }
$disk = $partitions[0] | Get-Disk -ErrorAction Stop
$volume = $partitions[0] | Get-Volume -ErrorAction Stop
if ($disk.Number -ne [int]$Number -or
    (([string]$disk.SerialNumber).Trim()) -cne $Serial -or
    [uint64]$disk.Size -ne [uint64]$Size -or
    $volume.FileSystemLabel -cne "RAMSHARE" -or
    $volume.FileSystem -cne "NTFS") {
    throw "formatted volume identity mismatch"
}
'@
    $arguments = @(
        (Quote-ProcessArgument ([string]$DiskNumber)),
        (Quote-ProcessArgument $Serial),
        (Quote-ProcessArgument ([string]$Size)),
        (Quote-ProcessArgument $Letter)
    )
    Invoke-BoundedPowerShellText $script $arguments 15 | Out-Null
}

$isStartupResume = $Resume -and
    -not [string]::IsNullOrWhiteSpace($ResumeApprovalToken)
if ($isStartupResume) {
    # A startup action is one-shot even when manifest/state validation fails.
    Invoke-CampaignSafetyCleanup $WatchdogMarker $TaskName
}
if (-not $isStartupResume) {
    Assert-PhysicalApproval
    if (-not $ApproveReboot) {
        throw "Each reboot requires -ApproveReboot on the current invocation."
    }
}
if (-not [IO.Path]::IsPathRooted($Manifest) -or
    -not (Test-Path $Manifest -PathType Leaf)) {
    throw "Manifest must be an existing absolute file."
}
if (-not (Test-Path $Controller -PathType Leaf)) {
    throw "Controller missing: $Controller"
}
New-Item $CampaignRoot -ItemType Directory -Force | Out-Null
$manifestHash = (Get-FileHash $Manifest -Algorithm SHA256).Hash
$manifestDocument = Get-Content $Manifest -Raw | ConvertFrom-Json
$driveConfig = Get-PackagedDriveConfig $manifestDocument $Manifest
$targetLetter = $driveConfig.volume_letter
if ($manifestDocument.start_policy -ne "demand") {
    throw "physical gate requires demand start until promotion"
}

if (-not $isStartupResume) {
    $campaignMutex = Enter-CampaignMutex
    try {
    if (-not (Test-Path $StatePath)) {
        $bootResult = Invoke-BoundedProcess `
            (Join-Path $env:SystemRoot "System32\bcdedit.exe") `
            @("/enum", '"{current}"') 10
        if (-not $bootResult.completed -or $bootResult.exit_code -ne 0 -or
            $bootResult.stdout -notmatch "(?im)testsigning\s+(Yes|Sim)") {
            throw "physical Test Mode gate requires bounded proof of testsigning enabled"
        }
        $pagefiles = Get-PagefileObservations
        Assert-PagefileLetterFree $targetLetter $pagefiles.active `
            $pagefiles.configured $pagefiles.succeeded | Out-Null
        if ([IO.DriveInfo]::GetDrives().Name -contains "$targetLetter`:\") {
            throw "target volume letter $targetLetter is already in use"
        }
        Assert-ZeroResidue
        $install = Invoke-BoundedProcess $Controller @(
            "install", "--manifest", (Quote-ProcessArgument $Manifest)) 60
        if (-not $install.completed -or $install.exit_code -ne 0) {
            throw "product install failed: exit=$($install.exit_code); $($install.stderr)"
        }
        $state = [ordered]@{
            schema = 2
            next_boot = 1
            cold_boots = $ColdBoots
            manifest = (Resolve-Path $Manifest).Path
            manifest_sha256 = $manifestHash
            last_completed_boot = 0
            status = "awaiting_approval"
            approval_boot = 0
            approval_token_sha256 = ""
            approval_expires_utc = ""
            approval_consumed = $true
        }
        Remove-Item $ResultsPath -Force -ErrorAction SilentlyContinue
    }
    else {
        $state = Get-Content $StatePath -Raw | ConvertFrom-Json
        if ($state.schema -ne 2 -or $state.manifest_sha256 -ne $manifestHash -or
            $state.cold_boots -ne $ColdBoots -or
            $state.next_boot -ne ($state.last_completed_boot + 1) -or
            $state.status -ne "awaiting_approval") {
            throw "campaign is not ready for a fresh reboot approval"
        }
    }
    if ([int]$state.next_boot -gt $ColdBoots) {
        throw "campaign already completed"
    }
    Invoke-CampaignSafetyCleanup $WatchdogMarker $TaskName
    $approval = New-RebootApproval ([int]$state.next_boot)
    $state.status = "scheduled"
    $state.approval_boot = [int]$state.next_boot
    $state.approval_token_sha256 = $approval.token_sha256
    $state.approval_expires_utc = $approval.expires_utc
    $state.approval_consumed = $false
    Write-State $state
    $arguments = Get-ScheduledResumeArguments $PSCommandPath $state.manifest `
        $ColdBoots $Controller $CampaignRoot $approval.token `
        ([bool]$AllowWatchdogShutdown)
    try {
        Register-OneShotResumeTask $arguments
        $reboot = Invoke-BoundedProcess (Join-Path $env:SystemRoot "System32\shutdown.exe") `
            @("/r", "/t", "5", "/f") 10
        if (-not $reboot.completed -or $reboot.exit_code -ne 0) {
            throw "approved reboot request failed: exit=$($reboot.exit_code)"
        }
    }
    catch {
        Invoke-CampaignSafetyCleanup $WatchdogMarker $TaskName
        $state.status = "awaiting_approval"
        $state.approval_token_sha256 = ""
        $state.approval_expires_utc = ""
        $state.approval_consumed = $true
        Write-State $state
        throw
    }
    Write-Host "One cold boot approved and scheduled. Evidence: $CampaignRoot"
    return
    }
    finally { Exit-CampaignMutex $campaignMutex }
}

$campaignMutex = Enter-CampaignMutex
try {
    $state = Get-Content $StatePath -Raw | ConvertFrom-Json
    if ($state.schema -ne 2 -or $state.manifest_sha256 -ne $manifestHash -or
        $state.next_boot -ne ($state.last_completed_boot + 1)) {
        throw "resume_marker_is_monotonic failed"
    }
    if ($state.next_boot -gt $ColdBoots) {
        throw "campaign already completed"
    }
    $boot = [int]$state.next_boot
    $state = Use-ResumeApproval $state $ResumeApprovalToken $boot
    Write-State $state
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false `
        -ErrorAction SilentlyContinue
}
finally { Exit-CampaignMutex $campaignMutex }
$started = Get-Date
try {
    Arm-Watchdog ([bool]$AllowWatchdogShutdown)
    Start-Sleep -Seconds 15
    if ((Get-Service RamSharedWinSvc).Status -ne "Stopped" -or
        (Get-Service RamSharedBroker).Status -ne "Stopped") {
        throw "cold boot did not preserve demand-stopped state"
    }
    Assert-ZeroResidue
    $startWatch = [Diagnostics.Stopwatch]::StartNew()
    $serviceStartedAt = Get-Date
    $sc = Join-Path $env:SystemRoot "System32\sc.exe"
    $consumerStart = Invoke-BoundedProcess $sc @("start", "RamSharedWinSvc") 10
    if (-not $consumerStart.completed -or $consumerStart.exit_code -ne 0) {
        throw "supported start command failed: exit=$($consumerStart.exit_code); $($consumerStart.stderr)"
    }
    (Get-Service RamSharedWinSvc).WaitForStatus("Running", [timespan]::FromSeconds(30))
    $consumerService = Get-ServiceProcessIdBounded "RamSharedWinSvc"
    if ($consumerService.State -ne "Running" -or
        [int]$consumerService.ProcessId -le 0) {
        throw "consumer service has no current running process"
    }
    $online = Get-CurrentOnlineIdentity $driveConfig.evidence_path `
        ([int]$consumerService.ProcessId) $serviceStartedAt
    if ($online.size_bytes -ne $driveConfig.size_bytes) {
        throw "current Online evidence size contradicts immutable config"
    }
    $pagefiles = Get-PagefileObservations
    Assert-PagefileLetterFree $targetLetter $pagefiles.active `
        $pagefiles.configured $pagefiles.succeeded | Out-Null
    $diskRows = @(Get-ExactDiskObservations)
    $disk = Resolve-ExactOnlineDisk $diskRows $online.serial `
        $driveConfig.size_bytes $driveConfig.block_size
    $readinessMs = [int]$startWatch.Elapsed.TotalMilliseconds

    $root = Get-VersionRoot $manifestDocument
    foreach ($name in @("RamSharedBroker", "RamSharedWinSvc")) {
        $svc = Get-ServiceProcessIdBounded $name
        if ($svc.State -ne "Running" -or [int]$svc.ProcessId -le 0) {
            throw "$name has no current running process"
        }
        $process = Get-Process -Id $svc.ProcessId
        $role = if ($name -eq "RamSharedBroker") { "broker_exe" } else { "winsvc_exe" }
        $relative = ($manifestDocument.artifacts | Where-Object role -eq $role).relative_path
        if ((Get-FileHash $process.Path -Algorithm SHA256).Hash -ne
            (Get-FileHash (Join-Path $root $relative) -Algorithm SHA256).Hash) {
            throw "$name BINARY_MATCH failed"
        }
    }
    Assert-DriverBinaryMatch $manifestDocument $root

    Invoke-ExactDiskFormat $disk $driveConfig $online.serial
    Assert-FormattedVolumeBinding ([int]$disk.Number) $online.serial `
        $driveConfig.size_bytes $targetLetter
    $hashes = @()
    for ($round = 1; $round -le 3; $round++) {
        $path = "$targetLetter`:\physical-$boot-$round.bin"
        $bytes = New-Object byte[] (8MB)
        $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
        try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
        $intendedHash = Get-Sha256Hex $bytes
        [IO.File]::WriteAllBytes($path, $bytes)
        $read = [IO.File]::ReadAllBytes($path)
        Remove-Item $path -Force
        if (-not (Test-IntendedPayloadMatch $bytes $read)) {
            throw "intended/read-back SHA mismatch boot=$boot round=$round"
        }
        $hashes += $intendedHash
    }
    $stopWatch = [Diagnostics.Stopwatch]::StartNew()
    $consumerStop = Invoke-BoundedProcess $sc @("stop", "RamSharedWinSvc") 10
    $stopRequestError = if (-not $consumerStop.completed -or
        $consumerStop.exit_code -ne 0) {
        "exit=$($consumerStop.exit_code); $($consumerStop.stderr)"
    } else { "" }
    (Get-Service RamSharedWinSvc).WaitForStatus("Stopped", [timespan]::FromSeconds(30))
    Assert-SupportedStop $stopRequestError `
        ([string](Get-Service RamSharedWinSvc).Status) | Out-Null
    $consumerStopMs = [int]$stopWatch.Elapsed.TotalMilliseconds
    $brokerStop = Invoke-BoundedProcess $sc @("stop", "RamSharedBroker") 10
    $brokerStopError = if (-not $brokerStop.completed -or
        $brokerStop.exit_code -ne 0) {
        "exit=$($brokerStop.exit_code); $($brokerStop.stderr)"
    } else { "" }
    (Get-Service RamSharedBroker).WaitForStatus("Stopped", [timespan]::FromSeconds(15))
    Assert-SupportedStop $brokerStopError `
        ([string](Get-Service RamSharedBroker).Status) | Out-Null
    $productStopMs = [int]$stopWatch.Elapsed.TotalMilliseconds
    Assert-ZeroResidue
    Append-Result @{
        schema = 1
        boot = $boot
        manifest_sha256 = $manifestHash
        readiness_ms = $readinessMs
        consumer_stop_ms = $consumerStopMs
        product_stop_ms = $productStopMs
        sha256 = $hashes
        stop_request_error = $stopRequestError
        forced_kills = 0
        residue = 0
        verdict = "PASS"
        started_at = $started.ToString("o")
        completed_at = (Get-Date).ToString("o")
    }
    $state.last_completed_boot = $boot
    $state.next_boot = $boot + 1
    $state.status = if ($boot -eq $ColdBoots) { "complete" } else { "awaiting_approval" }
    $state.approval_boot = 0
    $state.approval_token_sha256 = ""
    $state.approval_expires_utc = ""
    $state.approval_consumed = $true
    Write-State $state
    if ($boot -eq $ColdBoots) {
        Invoke-CampaignSafetyCleanup $WatchdogMarker $TaskName
        Assert-ZeroResidue
        $completedRows = @(Get-Content $ResultsPath | ForEach-Object {
                $_ | ConvertFrom-Json
            } | Where-Object verdict -eq "PASS")
        if ($completedRows.Count -ne 3 -or (Test-Path $WatchdogMarker) -or
            (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue)) {
            throw "cleanup_artifacts_complete failed"
        }
        Write-Host ("three_cold_boots_same_manifest PASS; " +
            "final_preflight_clean PASS; resume_marker_is_monotonic PASS; " +
            "cleanup_artifacts_complete PASS")
    }
    else {
        Write-Host ("cold boot $boot PASS; campaign is awaiting a fresh " +
            "-ApprovePhysicalHost -ApproveReboot invocation")
    }
}
catch {
    Append-Result @{
        schema = 1
        boot = $boot
        manifest_sha256 = $manifestHash
        verdict = "FAIL"
        error = $_.Exception.Message
        forced_kills = 0
        completed_at = (Get-Date).ToString("o")
    }
    $state.status = "failed"
    $state.approval_boot = 0
    $state.approval_token_sha256 = ""
    $state.approval_expires_utc = ""
    $state.approval_consumed = $true
    Write-State $state
    throw
}
finally {
    Invoke-CampaignSafetyCleanup $WatchdogMarker $TaskName
}
