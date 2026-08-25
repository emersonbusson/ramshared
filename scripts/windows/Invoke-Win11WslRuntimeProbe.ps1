#Requires -Version 5.1
<#
.SYNOPSIS
  Inspect or probe WSL readiness inside one approved Windows lab VM.

.DESCRIPTION
  The default action is plan-only. Status reads Hyper-V state without starting
  the VM. Probe requires -Run, -ApproveGuestWslProbe, and the exact observed VM
  ID. All guest work uses the shared bounded PowerShell Direct helper. If this
  harness starts the VM, it requests a guest-only graceful shutdown and proves
  that the VM returns to Off. It never formats or changes a disk, creates a
  checkpoint, repairs WSL, or force-powers off a VM.
#>
[CmdletBinding()]
param(
    [ValidateSet("plan", "status", "probe")]
    [string]$Action = "plan",
    [ValidateSet("win11-drill", "win11-wsl2-lab")]
    [string]$VMName = "win11-wsl2-lab",
    [string]$ExpectedVMId = "",
    [string]$ExpectedDistro = "Ubuntu-24.04",
    [string]$User = "",
    [string]$Password = "",
    [string]$PasswordFile = "C:\ramshared\bin\.drill-pw",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [ValidateRange(5, 120)]
    [int]$GuestCommandTimeoutSeconds = 15,
    [ValidateRange(2, 600)]
    [int]$PsDirectTimeoutSeconds = 120,
    [ValidateRange(1, 180)]
    [int]$PsDirectConnectTimeoutSeconds = 60,
    [ValidateRange(10, 300)]
    [int]$VmShutdownTimeoutSeconds = 120,
    [switch]$Start,
    [switch]$Run,
    [switch]$ApproveGuestWslProbe,
    [switch]$UseBlankPassword
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "Invoke-GuestPsDirectBounded.ps1")

function Get-LocalDrillPassword {
    param([string]$InitialPassword, [string]$LocalPasswordFile)
    if (-not [string]::IsNullOrEmpty($InitialPassword)) { return $InitialPassword }
    foreach ($scope in @("Machine", "User")) {
        $value = [Environment]::GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)
        if (-not [string]::IsNullOrEmpty($value)) { return $value }
    }
    if (-not [string]::IsNullOrEmpty($env:RAMSHARED_DRILL_PASSWORD)) {
        return $env:RAMSHARED_DRILL_PASSWORD
    }
    if (Test-Path -LiteralPath $LocalPasswordFile -PathType Leaf) {
        return (Get-Content -LiteralPath $LocalPasswordFile -Raw).Trim()
    }
    return ""
}

function Get-ApprovedGuestUser {
    param([string]$Name, [string]$RequestedUser)
    $expected = if ($Name -ceq "win11-wsl2-lab") {
        "WIN11-WSL2-LAB\drilladmin"
    } else {
        "WIN11-DRILL\drilladmin"
    }
    if ([string]::IsNullOrWhiteSpace($RequestedUser)) { return $expected }
    if ($RequestedUser -cne $expected) { throw "guest_user_identity_mismatch" }
    return $expected
}

function Get-LabObservation {
    param([string]$Name)
    $vmRows = @(Get-VM -Name $Name -ErrorAction Stop)
    if ($vmRows.Count -ne 1 -or [string]$vmRows[0].Name -cne $Name) {
        throw "vm_identity_ambiguous"
    }
    $vm = $vmRows[0]
    $processors = @(Get-VMProcessor -VMName $Name -ErrorAction Stop)
    $snapshots = @(Get-VMSnapshot -VMName $Name -ErrorAction Stop)
    $disks = @(Get-VMHardDiskDrive -VMName $Name -ErrorAction Stop)
    if ($processors.Count -ne 1) { throw "vm_processor_identity_ambiguous" }

    $diskContractOk = $false
    if ($disks.Count -eq 1 -and -not [string]::IsNullOrWhiteSpace([string]$disks[0].Path)) {
        $diskPath = [IO.Path]::GetFullPath([string]$disks[0].Path)
        $diskContractOk = [IO.Path]::GetExtension($diskPath) -ceq ".vhdx" -and
            $diskPath.IndexOf($Name, [StringComparison]::OrdinalIgnoreCase) -ge 0
        if ($Name -ceq "win11-wsl2-lab") {
            $approvedRoot = "C:\ramshared-hyperv\win11-wsl2-lab\"
            $diskContractOk = $diskContractOk -and
                $diskPath.StartsWith($approvedRoot, [StringComparison]::OrdinalIgnoreCase)
        }
    }

    [pscustomobject]@{
        vm_name = [string]$vm.Name
        vm_id = ([guid]$vm.Id).ToString("D").ToUpperInvariant()
        state = [string]$vm.State
        generation = [int]$vm.Generation
        processor_count = [int]$processors[0].Count
        nested_virtualization = [bool]$processors[0].ExposeVirtualizationExtensions
        automatic_checkpoints_enabled = [bool]$vm.AutomaticCheckpointsEnabled
        checkpoint_type = [string]$vm.CheckpointType
        snapshot_count = [int]$snapshots.Count
        disk_count = [int]$disks.Count
        disk_contract_ok = [bool]$diskContractOk
    }
}

function Write-TypedResult {
    param([string]$Status, [string]$Reason, [hashtable]$Extra = @{}, [int]$Depth = 8)
    $record = [ordered]@{
        schema = 1
        status = $Status
        reason = $Reason
        action = $Action
        vm_name = $VMName
        expected_distro = $ExpectedDistro
        DISK_MUTATION = $false
    }
    foreach ($key in @($Extra.Keys)) { $record[$key] = $Extra[$key] }
    $record | ConvertTo-Json -Depth $Depth
}

function Wait-LabVmOff {
    param([DateTime]$DeadlineUtc)
    do {
        $rows = @(Get-VM -Name $VMName -ErrorAction Stop)
        if ($rows.Count -ne 1) { throw "vm_identity_ambiguous_during_shutdown" }
        if ([string]$rows[0].State -ceq "Off") { return $true }
        Start-Sleep -Seconds 2
    } while ([DateTime]::UtcNow -lt $DeadlineUtc)
    return $false
}

function Invoke-GracefulHostShutdownFallback {
    param([string]$ExpectedId)
    $worker = @'
$ErrorActionPreference = "Stop"
$name = [Environment]::GetEnvironmentVariable("RAMSHARED_LAB_VM_NAME")
$expected = [Environment]::GetEnvironmentVariable("RAMSHARED_LAB_VM_ID")
if ([string]::IsNullOrWhiteSpace($name) -or [string]::IsNullOrWhiteSpace($expected)) {
    throw "bounded host shutdown identity is incomplete"
}
$rows = @(Get-VM -Name $name -ErrorAction Stop)
if ($rows.Count -ne 1 -or ([guid]$rows[0].Id).ToString("D").ToUpperInvariant() -cne $expected) {
    throw "bounded host shutdown VM identity mismatch"
}
Stop-VM -VM $rows[0] -Confirm:$false -ErrorAction Stop
'@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($worker))
    $deadlineUtc = [DateTime]::UtcNow.AddSeconds($VmShutdownTimeoutSeconds)
    do {
        # The integration shutdown channel can lag a just-started VM. If it is
        # already off, or becomes ready during the deadline, do not escalate.
        if (Wait-LabVmOff -DeadlineUtc ([DateTime]::UtcNow)) {
            return "restored_off_host_fallback"
        }
        $execution = $null
        try {
            $remainingSeconds = [Math]::Max(1, [int][Math]::Ceiling(
                ($deadlineUtc - [DateTime]::UtcNow).TotalSeconds))
            $attemptTimeoutSeconds = [Math]::Min(20, $remainingSeconds)
            $execution = Invoke-BoundedGuestProcess -FilePath (Join-Path $PSHOME "powershell.exe") `
                -Arguments ("-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand " + $encoded) `
                -TimeoutSeconds $attemptTimeoutSeconds -Environment @{
                    RAMSHARED_LAB_VM_NAME = $VMName
                    RAMSHARED_LAB_VM_ID = $ExpectedId
                }
        }
        catch {
            $execution = $null
        }
        if ($null -ne $execution -and $execution.completed -and $execution.exit_code -eq 0) {
            if (Wait-LabVmOff -DeadlineUtc $deadlineUtc) {
                return "restored_off_host_fallback"
            }
            return "host_graceful_shutdown_timeout"
        }
        if ([DateTime]::UtcNow -lt $deadlineUtc) {
            Start-Sleep -Seconds 3
        }
    } while ([DateTime]::UtcNow -lt $deadlineUtc)
    return "host_graceful_shutdown_timeout"
}

function Restore-StartedLabVmOff {
    param(
        [string]$ExpectedId,
        [string]$GuestUser,
        [AllowEmptyString()]
        [string]$GuestPassword,
        [switch]$GuestPasswordMayBeEmpty
    )
    $guestOutcome = "guest_shutdown_failed"
    try {
        $shutdownRows = Invoke-GuestPsDirectBounded -VMName $VMName -User $GuestUser `
            -Password $GuestPassword -AllowEmptyPassword:$GuestPasswordMayBeEmpty `
            -Operation invoke -TimeoutSeconds 45 `
            -ConnectTimeoutSeconds 20 -ScriptBlock {
                $output = & shutdown.exe /s /t 5 /d p:4:1 `
                    /c "RamShared bounded WSL readiness probe complete" 2>&1 | Out-String
                if ([int]$LASTEXITCODE -ne 0) { throw "guest shutdown scheduling failed" }
                [pscustomobject]@{
                    shutdown_scheduled=$true
                    output_present=(-not [string]::IsNullOrWhiteSpace($output))
                }
            }
        if (@($shutdownRows).Count -ne 1 -or -not [bool]@($shutdownRows)[0].shutdown_scheduled) {
            $guestOutcome = "guest_shutdown_receipt_invalid"
        } elseif (Wait-LabVmOff -DeadlineUtc ([DateTime]::UtcNow.AddSeconds($VmShutdownTimeoutSeconds))) {
            return "restored_off_guest"
        } else {
            $guestOutcome = "guest_shutdown_timeout"
        }
    } catch {
        $guestOutcome = "guest_shutdown_failed"
    }

    try {
        return Invoke-GracefulHostShutdownFallback -ExpectedId $ExpectedId
    } catch {
        return "host_graceful_shutdown_failed"
    }
}

$observation = Get-LabObservation -Name $VMName
if ($Action -ceq "status") {
    Write-TypedResult -Status "STATUS" -Reason "host_observation_complete" -Extra @{
        observation = $observation
    }
    exit 0
}
if ($Action -ceq "plan" -or ($Action -ceq "probe" -and -not $Run)) {
    Write-TypedResult -Status "PLAN" -Reason "no_guest_or_vm_mutation" -Extra @{
        observation = $observation
        would_start_vm = [bool]($Start -and $observation.state -ceq "Off")
        requires_run = $true
        requires_approval = "ApproveGuestWslProbe"
        requires_expected_vm_id = $true
        remote_transport = "bounded PowerShell Direct"
    }
    exit 0
}

if (-not $ApproveGuestWslProbe) {
    Write-TypedResult -Status "PARTIAL" -Reason "guest_probe_approval_required"
    exit 2
}
if ([string]::IsNullOrWhiteSpace($ExpectedVMId)) {
    Write-TypedResult -Status "PARTIAL" -Reason "expected_vm_id_required"
    exit 2
}
try {
    $expectedCanonical = ([guid]$ExpectedVMId).ToString("D").ToUpperInvariant()
} catch {
    Write-TypedResult -Status "PARTIAL" -Reason "expected_vm_id_invalid"
    exit 2
}
if ($observation.vm_id -cne $expectedCanonical) {
    Write-TypedResult -Status "PARTIAL" -Reason "vm_identity_mismatch"
    exit 2
}
if ($observation.generation -ne 2) {
    Write-TypedResult -Status "PARTIAL" -Reason "vm_generation_mismatch"
    exit 2
}
if ($observation.snapshot_count -ne 0 -or $observation.automatic_checkpoints_enabled -or
    $observation.checkpoint_type -cne "Disabled") {
    Write-TypedResult -Status "PARTIAL" -Reason "snapshot_residue"
    exit 2
}
if (-not $observation.nested_virtualization) {
    Write-TypedResult -Status "PARTIAL" -Reason "nested_virtualization_unavailable"
    exit 2
}
if ($observation.disk_count -ne 1 -or -not $observation.disk_contract_ok) {
    Write-TypedResult -Status "PARTIAL" -Reason "guest_disk_identity_mismatch"
    exit 2
}
if ($PsDirectTimeoutSeconds -le $PsDirectConnectTimeoutSeconds) {
    Write-TypedResult -Status "PARTIAL" -Reason "psdirect_deadline_invalid"
    exit 2
}
if ($observation.state -cnotin @("Off", "Running")) {
    Write-TypedResult -Status "PARTIAL" -Reason "vm_state_ineligible"
    exit 2
}
if ($observation.state -ceq "Off" -and -not $Start) {
    Write-TypedResult -Status "PARTIAL" -Reason "vm_is_off_and_start_not_approved"
    exit 2
}

try {
    $User = Get-ApprovedGuestUser -Name $VMName -RequestedUser $User
} catch {
    Write-TypedResult -Status "PARTIAL" -Reason "guest_user_identity_mismatch"
    exit 2
}
if ($UseBlankPassword -and $VMName -cne "win11-wsl2-lab") {
    Write-TypedResult -Status "PARTIAL" -Reason "blank_password_probe_not_approved_vm"
    exit 2
}
if ($UseBlankPassword -and -not [string]::IsNullOrEmpty($Password)) {
    Write-TypedResult -Status "PARTIAL" -Reason "blank_password_probe_explicit_password_forbidden"
    exit 2
}
if ($UseBlankPassword) {
    $Password = ""
} else {
    $Password = Get-LocalDrillPassword -InitialPassword $Password -LocalPasswordFile $PasswordFile
    if ([string]::IsNullOrEmpty($Password)) {
        Write-TypedResult -Status "PARTIAL" -Reason "missing_guest_credential"
        exit 2
    }
}

$artifactDir = Join-Path $ArtifactRoot ("win11-wsl-runtime-probe-" +
    [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssZ") + "-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $artifactDir -ErrorAction Stop | Out-Null
$startedHere = $false
$probe = $null
$failureReason = ""
$shutdownReason = "not_required"

try {
    if ($observation.state -ceq "Off") {
        Start-VM -Name $VMName -ErrorAction Stop | Out-Null
        $startedHere = $true
    }
    $rows = Invoke-GuestPsDirectBounded -VMName $VMName -User $User -Password $Password `
        -AllowEmptyPassword:$UseBlankPassword -Operation invoke -TimeoutSeconds $PsDirectTimeoutSeconds `
        -ConnectTimeoutSeconds $PsDirectConnectTimeoutSeconds -ArgumentList @(
            $GuestCommandTimeoutSeconds, $ExpectedDistro
        ) -ScriptBlock {
            param($TimeoutSec, $DistroName)
            $ErrorActionPreference = "Stop"

            function Invoke-WslWithTimeout {
                param([string]$Exe, [string]$Arguments, [int]$TimeoutSec)
                $info = New-Object System.Diagnostics.ProcessStartInfo
                $info.FileName = $Exe
                $info.Arguments = $Arguments
                $info.UseShellExecute = $false
                $info.CreateNoWindow = $true
                $info.RedirectStandardOutput = $true
                $info.RedirectStandardError = $true
                # wsl.exe emits UTF-16LE when its streams are redirected.
                $info.StandardOutputEncoding = [Text.Encoding]::Unicode
                $info.StandardErrorEncoding = [Text.Encoding]::Unicode
                $process = New-Object System.Diagnostics.Process
                $process.StartInfo = $info
                try {
                    if (-not $process.Start()) {
                        return [pscustomobject]@{ done=$false; exit=$null; outcome="start_failed"; stdout="" }
                    }
                    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
                    $stderrTask = $process.StandardError.ReadToEndAsync()
                    $done = $process.WaitForExit($TimeoutSec * 1000)
                    $outcome = "completed"
                    if (-not $done) {
                        $outcome = "timeout"
                        try { $process.Kill() } catch { $outcome = "timeout_kill_failed" }
                        if (-not $process.WaitForExit(5000)) { $outcome = "timeout_process_still_running" }
                    }
                    $streamsDone = [Threading.Tasks.Task]::WaitAll(
                        [Threading.Tasks.Task[]]@($stdoutTask, $stderrTask), 5000)
                    if (-not $streamsDone) { $outcome = "stream_drain_timeout" }
                    [pscustomobject]@{
                        done = [bool]$done
                        exit = if ($done) { [int]$process.ExitCode } else { $null }
                        outcome = $outcome
                        stdout = if ($streamsDone) { [string]$stdoutTask.Result } else { "" }
                    }
                } finally {
                    $process.Dispose()
                }
            }

            $features = @{}
            foreach ($featureName in @("Microsoft-Windows-Subsystem-Linux", "VirtualMachinePlatform")) {
                try {
                    $feature = Get-WindowsOptionalFeature -Online -FeatureName $featureName -ErrorAction Stop
                    $features[$featureName] = [string]$feature.State
                } catch {
                    $features[$featureName] = "provider_error"
                }
            }
            $wslService = Get-CimInstance Win32_Service -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -eq "WslService" } | Select-Object -First 1
            $wslExe = if (Test-Path -LiteralPath "C:\Program Files\WSL\wsl.exe" -PathType Leaf) {
                "C:\Program Files\WSL\wsl.exe"
            } else {
                "wsl.exe"
            }
            $status = if ($null -ne $wslService -and [string]$wslService.State -ceq "Running") {
                Invoke-WslWithTimeout -Exe $wslExe -Arguments "--status" -TimeoutSec $TimeoutSec
            } else {
                [pscustomobject]@{ done=$false; exit=$null; outcome="service_unavailable"; stdout="" }
            }
            $list = if ($null -ne $wslService -and [string]$wslService.State -ceq "Running") {
                Invoke-WslWithTimeout -Exe $wslExe -Arguments "-l -v" -TimeoutSec $TimeoutSec
            } else {
                [pscustomobject]@{ done=$false; exit=$null; outcome="service_unavailable"; stdout="" }
            }
            [pscustomobject]@{
                host = [string]$env:COMPUTERNAME
                feature_wsl = [string]$features["Microsoft-Windows-Subsystem-Linux"]
                feature_vmp = [string]$features["VirtualMachinePlatform"]
                service_present = [bool]($null -ne $wslService)
                service_state = if ($null -ne $wslService) { [string]$wslService.State } else { "missing" }
                status_timeout = [bool](-not $status.done)
                status_exit = $status.exit
                status_outcome = [string]$status.outcome
                list_timeout = [bool](-not $list.done)
                list_exit = $list.exit
                list_outcome = [string]$list.outcome
                expected_distro_present = [bool]($list.stdout -match [regex]::Escape($DistroName))
            }
        }
    $probeRows = @($rows)
    if ($probeRows.Count -ne 1) {
        $failureReason = "guest_probe_result_ambiguous"
    } else {
        $probe = $probeRows[0]
    }
} catch {
    $failureText = [string]$_.Exception.Message
    if ($failureText -match "credential|logon failure|user name or password|usu.rio ou senha|credencial") {
        $failureReason = "powershell_direct_auth_failed"
    } elseif ($failureText -match "deadline|PowerShell Direct unavailable|timed out|timeout|process_tree_terminated") {
        $failureReason = "powershell_direct_unavailable"
    } else {
        $failureReason = "guest_probe_failed"
    }
} finally {
    if ($startedHere) {
        $shutdownReason = Restore-StartedLabVmOff -ExpectedId $observation.vm_id `
            -GuestUser $User -GuestPassword $Password `
            -GuestPasswordMayBeEmpty:$UseBlankPassword
    }
}

$probeReason = $failureReason
if ([string]::IsNullOrWhiteSpace($probeReason)) {
    if ($null -eq $probe) {
        $probeReason = "guest_probe_result_missing"
    } elseif (-not [bool]$probe.service_present) {
        $probeReason = "guest_wsl_service_missing"
    } elseif ([string]$probe.service_state -cne "Running") {
        $probeReason = "guest_wsl_service_not_running"
    } elseif ([bool]$probe.status_timeout -or [bool]$probe.list_timeout -or
        [int]$probe.status_exit -ne 0 -or [int]$probe.list_exit -ne 0 -or
        -not [bool]$probe.expected_distro_present) {
        $probeReason = "guest_wsl_runtime_unavailable"
    } else {
        $probeReason = "wsl_runtime_ready"
    }
}

$cleanupComplete = -not $startedHere -or $shutdownReason -cin @(
    "restored_off_guest", "restored_off_host_fallback"
)
$reason = if ($cleanupComplete) { $probeReason } else { "cleanup_incomplete" }

$probeRecord = [ordered]@{
    schema = 1
    vm_id = $observation.vm_id
    started_here = [bool]$startedHere
    probe_reason = $probeReason
    cleanup_reason = $shutdownReason
    probe = $probe
}
$probeRecord | ConvertTo-Json -Depth 8 |
    Set-Content -LiteralPath (Join-Path $artifactDir "probe.json") -Encoding UTF8

$finalStatus = if ($reason -ceq "wsl_runtime_ready") { "PASS" } else { "PARTIAL" }
$summary = [ordered]@{
    schema = 1
    status = $finalStatus
    reason = $reason
    action = $Action
    vm_name = $VMName
    vm_id = $observation.vm_id
    expected_distro = $ExpectedDistro
    artifact = $artifactDir
    started_here = [bool]$startedHere
    shutdown = $shutdownReason
    probe_reason = $probeReason
    cleanup_reason = $shutdownReason
    DISK_MUTATION = $false
}
$summary | ConvertTo-Json -Depth 6 |
    Set-Content -LiteralPath (Join-Path $artifactDir "summary.json") -Encoding UTF8
$summary | ConvertTo-Json -Depth 6
if ($finalStatus -ceq "PASS") { exit 0 }
exit 2
