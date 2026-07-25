#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Manifest,
    [ValidateRange(3, 3)]
    [int]$ColdBoots = 3,
    [switch]$ApprovePhysicalHost,
    [switch]$Resume,
    [string]$Controller = "C:\ramshared\bin\ramshared-winsvc.exe",
    [string]$CampaignRoot = "C:\ProgramData\RamShared\physical-autonomous-gate"
)

$ErrorActionPreference = "Stop"
$TaskName = "RamSharedPhysicalAutonomousGate"
$StatePath = Join-Path $CampaignRoot "state.json"
$ResultsPath = Join-Path $CampaignRoot "results.jsonl"
$WatchdogMarker = Join-Path $CampaignRoot "watchdog.armed"

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
function Write-State([hashtable]$State) {
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
function Invoke-Bounded([scriptblock]$Operation, [int]$Seconds,
    [object[]]$Arguments = @()) {
    $job = Start-Job -ScriptBlock $Operation -ArgumentList $Arguments
    try {
        if (-not (Wait-Job $job -Timeout $Seconds)) {
            throw "bounded operation timed out after $Seconds seconds"
        }
        Receive-Job $job -ErrorAction Stop
    }
    finally { Remove-Job $job -Force -ErrorAction SilentlyContinue }
}
function Get-RamSharedDiskRows {
    @(Get-CimInstance Win32_DiskDrive -ErrorAction Stop | Where-Object {
            $_.Model -match "RAMSHARE|VRAMDISK|RamShared" -or
            $_.Caption -match "RAMSHARE|VRAMDISK|RamShared"
        } | Select-Object Index, Model, SerialNumber, Size)
}
function Assert-ZeroResidue {
    $rows = @(Invoke-Bounded {
            @(Get-CimInstance Win32_DiskDrive -ErrorAction Stop | Where-Object {
                    $_.Model -match "RAMSHARE|VRAMDISK|RamShared" -or
                    $_.Caption -match "RAMSHARE|VRAMDISK|RamShared"
                } | Select-Object Index, Model, SerialNumber, Size)
        } 10)
    if ($rows.Count -ne 0) {
        throw "zero-residue gate failed: Win32_DiskDrive count=$($rows.Count)"
    }
    foreach ($name in @("RamSharedWinSvc", "RamSharedBroker")) {
        $service = Get-Service $name -ErrorAction SilentlyContinue
        if ($service -and $service.Status -eq "Running") {
            throw "zero-residue gate failed: $name is still running"
        }
    }
}
function Arm-Watchdog {
    [IO.File]::WriteAllText($WatchdogMarker, (Get-Date).ToString("o"))
    $marker = $WatchdogMarker.Replace("'", "''")
    $log = (Join-Path $CampaignRoot "watchdog.log").Replace("'", "''")
    $command = @"
Start-Sleep -Seconds 600
if (Test-Path -LiteralPath '$marker') {
  [IO.File]::AppendAllText('$log', (Get-Date).ToString('o') + " watchdog_shutdown`r`n")
  shutdown.exe /s /t 0 /f
}
"@
    Start-Process powershell.exe -WindowStyle Hidden -ArgumentList @(
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-Command", $command) | Out-Null
}
function Disarm-Watchdog {
    Remove-Item $WatchdogMarker -Force -ErrorAction SilentlyContinue
}
function Get-VersionRoot($ManifestDocument) {
    "C:\Program Files\RamShared\versions\$($ManifestDocument.version)-$($ManifestDocument.commit.Substring(0,12))"
}
function Get-ManifestVolumeLetter($ManifestDocument, [string]$ManifestPath) {
    $artifact = $ManifestDocument.artifacts |
        Where-Object role -eq "winsvc_config"
    if (-not $artifact) { throw "manifest has no winsvc_config artifact" }
    $configPath = Join-Path (Split-Path $ManifestPath -Parent) $artifact.relative_path
    $text = Get-Content $configPath -Raw
    $match = [regex]::Match($text, '(?m)^volume_letter\s*=\s*"([A-Z])"\s*$')
    if (-not $match.Success) {
        throw "winsvc_config has no exact uppercase volume_letter"
    }
    $match.Groups[1].Value
}
function Get-LoadedDriverPath {
    $driver = Get-CimInstance Win32_SystemDriver | Where-Object Name -eq "ramshared"
    if (-not $driver -or $driver.State -ne "Running") {
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

Assert-PhysicalApproval
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
$targetLetter = Get-ManifestVolumeLetter $manifestDocument $Manifest
if ($manifestDocument.start_policy -ne "demand") {
    throw "physical gate requires demand start until promotion"
}

if (-not $Resume) {
    if (Test-Path $StatePath) {
        throw "existing physical campaign state must be resolved first: $StatePath"
    }
    $bootConfig = (& bcdedit.exe /enum "{current}" | Out-String)
    if ($bootConfig -notmatch "(?im)testsigning\s+(Yes|Sim)") {
        throw "physical Test Mode gate requires testsigning enabled"
    }
    $pagefiles = @((Get-CimInstance Win32_PageFileUsage -ErrorAction Stop).Name)
    if ($pagefiles -match "^$targetLetter`:\\") {
        throw "pagefile targets product volume letter $targetLetter"
    }
    if ([IO.DriveInfo]::GetDrives().Name -contains "$targetLetter`:\") {
        throw "target volume letter $targetLetter is already in use"
    }
    Assert-ZeroResidue
    Assert-DriverBinaryMatch $manifestDocument (Split-Path $Manifest -Parent)
    & $Controller install --manifest $Manifest
    if ($LASTEXITCODE -ne 0) { throw "product install failed: $LASTEXITCODE" }
    $state = @{
        schema = 1
        next_boot = 1
        cold_boots = $ColdBoots
        manifest = (Resolve-Path $Manifest).Path
        manifest_sha256 = $manifestHash
        last_completed_boot = 0
        status = "scheduled"
    }
    Write-State $state
    Remove-Item $ResultsPath -Force -ErrorAction SilentlyContinue
    $arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" " +
        "-Manifest `"$($state.manifest)`" -ColdBoots $ColdBoots " +
        "-ApprovePhysicalHost -Resume -Controller `"$Controller`" " +
        "-CampaignRoot `"$CampaignRoot`""
    $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument $arguments
    $trigger = New-ScheduledTaskTrigger -AtStartup
    $principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -LogonType ServiceAccount `
        -RunLevel Highest
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger `
        -Principal $principal -Force | Out-Null
    shutdown.exe /r /t 5 /f
    Write-Host "Physical gate scheduled; host will reboot. Evidence: $CampaignRoot"
    return
}

$state = Get-Content $StatePath -Raw | ConvertFrom-Json
if ($state.schema -ne 1 -or $state.manifest_sha256 -ne $manifestHash -or
    $state.next_boot -ne ($state.last_completed_boot + 1)) {
    throw "resume_marker_is_monotonic failed"
}
if ($state.next_boot -gt $ColdBoots) {
    throw "campaign already completed"
}
Arm-Watchdog
$boot = [int]$state.next_boot
$started = Get-Date
try {
    Start-Sleep -Seconds 15
    if ((Get-Service RamSharedWinSvc).Status -ne "Stopped" -or
        (Get-Service RamSharedBroker).Status -ne "Stopped") {
        throw "cold boot did not preserve demand-stopped state"
    }
    Assert-ZeroResidue
    $startWatch = [Diagnostics.Stopwatch]::StartNew()
    Start-Service RamSharedWinSvc
    (Get-Service RamSharedWinSvc).WaitForStatus("Running", [timespan]::FromSeconds(30))
    $diskRows = @(Invoke-Bounded {
            @(Get-CimInstance Win32_DiskDrive -ErrorAction Stop | Where-Object {
                    $_.Model -match "RAMSHARE|VRAMDISK"
                })
        } 30)
    if ($diskRows.Count -ne 1) { throw "exact product disk count=$($diskRows.Count)" }
    $readinessMs = [int]$startWatch.Elapsed.TotalMilliseconds

    $root = Get-VersionRoot $manifestDocument
    foreach ($name in @("RamSharedBroker", "RamSharedWinSvc")) {
        $svc = Get-CimInstance Win32_Service -Filter "Name='$name'"
        $process = Get-Process -Id $svc.ProcessId
        $role = if ($name -eq "RamSharedBroker") { "broker_exe" } else { "winsvc_exe" }
        $relative = ($manifestDocument.artifacts | Where-Object role -eq $role).relative_path
        if ((Get-FileHash $process.Path -Algorithm SHA256).Hash -ne
            (Get-FileHash (Join-Path $root $relative) -Algorithm SHA256).Hash) {
            throw "$name BINARY_MATCH failed"
        }
    }
    Assert-DriverBinaryMatch $manifestDocument $root

    $disk = Get-Disk -Number $diskRows[0].Index
    if ([uint64]$disk.Size -ne 67108864) {
        throw "product disk size mismatch: $($disk.Size)"
    }
    if ($disk.PartitionStyle -eq "RAW") {
        Invoke-Bounded {
            param($diskNumber, $driveLetter)
            $ErrorActionPreference = "Stop"
            Initialize-Disk $diskNumber -PartitionStyle GPT -PassThru |
                New-Partition -UseMaximumSize -DriveLetter $driveLetter |
                Format-Volume -FileSystem NTFS -NewFileSystemLabel RAMSHARE `
                    -Confirm:$false -Force | Out-Null
        } 60 @($disk.Number, $targetLetter)
    }
    $bound = Get-Partition -DriveLetter $targetLetter | Get-Disk
    if ($bound.Number -ne $disk.Number) {
        throw "foreign $targetLetter`: volume binding"
    }
    $hashes = @()
    for ($round = 1; $round -le 3; $round++) {
        $path = "$targetLetter`:\physical-$boot-$round.bin"
        $bytes = New-Object byte[] (8MB)
        $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
        try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
        [IO.File]::WriteAllBytes($path, $bytes)
        $writeHash = (Get-FileHash $path -Algorithm SHA256).Hash
        $read = [IO.File]::ReadAllBytes($path)
        $temp = Join-Path $env:TEMP "ramshared-physical-read.bin"
        [IO.File]::WriteAllBytes($temp, $read)
        $readHash = (Get-FileHash $temp -Algorithm SHA256).Hash
        Remove-Item $path, $temp -Force
        if ($writeHash -ne $readHash) { throw "SHA mismatch boot=$boot round=$round" }
        $hashes += $writeHash
    }
    $stopWatch = [Diagnostics.Stopwatch]::StartNew()
    $stopRequestError = $null
    try { Stop-Service RamSharedWinSvc -ErrorAction Stop }
    catch { $stopRequestError = $_.Exception.Message }
    (Get-Service RamSharedWinSvc).WaitForStatus("Stopped", [timespan]::FromSeconds(30))
    $consumerStopMs = [int]$stopWatch.Elapsed.TotalMilliseconds
    Stop-Service RamSharedBroker
    (Get-Service RamSharedBroker).WaitForStatus("Stopped", [timespan]::FromSeconds(15))
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
    $state.status = if ($boot -eq $ColdBoots) { "complete" } else { "rebooting" }
    Write-State @{
        schema = 1
        next_boot = $state.next_boot
        cold_boots = $ColdBoots
        manifest = $state.manifest
        manifest_sha256 = $manifestHash
        last_completed_boot = $state.last_completed_boot
        status = $state.status
    }
    Disarm-Watchdog
    if ($boot -lt $ColdBoots) {
        shutdown.exe /r /t 5 /f
    }
    else {
        Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
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
    throw
}
