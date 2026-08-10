#Requires -Version 5.1
[CmdletBinding(DefaultParameterSetName = "Controller")]
param(
    [Parameter(Mandatory = $true, ParameterSetName = "Controller")]
    [ValidateNotNullOrEmpty()][string]$VMName,
    [Parameter(Mandatory = $true, ParameterSetName = "Controller")]
    [ValidateNotNullOrEmpty()][string]$ExpectedVMId,
    [Parameter(Mandatory = $true, ParameterSetName = "Controller")]
    [ValidateNotNullOrEmpty()][string]$ExpectedSwitchName,
    [Parameter(Mandatory = $true, ParameterSetName = "Controller")]
    [ValidateNotNullOrEmpty()][string]$ReadinessReceiptPath,
    [Parameter(Mandatory = $true, ParameterSetName = "Controller")]
    [ValidatePattern('^[A-Fa-f0-9]{64}$')]
    [string]$ExpectedReadinessReceiptSha256,
    [Parameter(Mandatory = $true, ParameterSetName = "Controller")]
    [ValidateNotNullOrEmpty()][string]$BaseRoot,
    [Parameter(Mandatory = $true, ParameterSetName = "Controller")]
    [ValidateNotNullOrEmpty()][string]$User,
    [Parameter(Mandatory = $true, ParameterSetName = "Controller")]
    [ValidateNotNullOrEmpty()][string]$PasswordFile,
    [Parameter(ParameterSetName = "Controller")]
    [ValidateRange(15, 30)][int]$ShutdownDelaySeconds = 20,
    [Parameter(ParameterSetName = "Controller")]
    [ValidateRange(30, 600)][int]$ShutdownTimeoutSeconds = 180,
    [Parameter(ParameterSetName = "Controller")]
    [ValidateRange(60, 3600)][int]$CopyTimeoutSeconds = 3600,
    [Parameter(ParameterSetName = "Controller")][switch]$ApproveSeal,
    [Parameter(Mandatory = $true, ParameterSetName = "Worker")]
    [Alias("WorkerMode")][ValidateSet("Copy")][string]$WorkerEntryMode,
    [Parameter(Mandatory = $true, ParameterSetName = "Worker")]
    [Alias("SourcePath")][ValidateNotNullOrEmpty()][string]$WorkerEntrySourcePath,
    [Parameter(Mandatory = $true, ParameterSetName = "Worker")]
    [Alias("DestinationPath")][ValidateNotNullOrEmpty()][string]$WorkerEntryDestinationPath,
    [Parameter(Mandatory = $true, ParameterSetName = "Worker")]
    [Alias("ResultPath")][ValidateNotNullOrEmpty()][string]$WorkerEntryResultPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-Win11LabReadyBaseSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "ready_base_input_missing"
    }
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "ready_base_reparse_input_refused"
    }
    $hash = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256 -ErrorAction Stop).Hash
    if ($hash -notmatch '^[A-Fa-f0-9]{64}$') {
        throw "ready_base_sha256_invalid"
    }
    $hash.ToUpperInvariant()
}

function Assert-Win11LabReadyBaseReceipt {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$Receipt,
        [Parameter(Mandatory = $true)][string]$ExpectedName,
        [Parameter(Mandatory = $true)][guid]$ExpectedId,
        [Parameter(Mandatory = $true)][string]$ExpectedSwitch
    )
    foreach ($property in @("schema", "status", "terminal_code", "vm_name",
            "expected_switch_name", "network_policy", "after")) {
        if ($null -eq $Receipt.PSObject.Properties[$property]) {
            throw "ready_base_receipt_incomplete"
        }
    }
    $after = $Receipt.after
    if ($null -eq $after -or $null -eq $after.host -or $null -eq $after.guest) {
        throw "ready_base_receipt_incomplete"
    }
    $guest = $after.guest
    foreach ($property in @("image_state", "system_setup_in_progress", "oobe_in_progress",
            "setup_phase", "setup_type", "routable_ipv4_count", "provider_error_count",
            "package_count", "service_count", "root_count", "ramshared_disk_count",
            "ramshared_pnp_disk_count", "verifier_target_count", "verifier_all_drivers",
            "testsigning_enabled", "root_expected_thumbprint_count",
            "trusted_publisher_expected_thumbprint_count", "root_foreign_subject_count",
            "trusted_publisher_foreign_subject_count")) {
        if ($null -eq $guest.PSObject.Properties[$property]) {
            throw "ready_base_receipt_incomplete"
        }
    }
    try {
        $receiptVmId = [guid]([string]$after.host.vm_id)
    }
    catch {
        throw "ready_base_receipt_vm_id_invalid"
    }
    if ([int]$Receipt.schema -ne 1 -or [string]$Receipt.status -cne "READY" -or
        [string]$Receipt.terminal_code -cne "ready" -or
        [string]$Receipt.vm_name -cne $ExpectedName -or $receiptVmId -ne $ExpectedId -or
        [string]$Receipt.expected_switch_name -cne $ExpectedSwitch -or
        [string]$Receipt.network_policy -cne "SealedOffline" -or
        [string]$guest.image_state -cne "IMAGE_STATE_COMPLETE" -or
        [int]$guest.system_setup_in_progress -ne 0 -or [int]$guest.oobe_in_progress -ne 0 -or
        [int]$guest.setup_phase -ne 0 -or [int]$guest.setup_type -ne 0 -or
        [int]$guest.routable_ipv4_count -ne 0 -or [int]$guest.provider_error_count -ne 0 -or
        [int]$guest.package_count -ne 0 -or [int]$guest.service_count -ne 0 -or
        [int]$guest.root_count -ne 0 -or [int]$guest.ramshared_disk_count -ne 0 -or
        [int]$guest.ramshared_pnp_disk_count -ne 0 -or [int]$guest.verifier_target_count -ne 0 -or
        $guest.verifier_all_drivers -isnot [bool] -or [bool]$guest.verifier_all_drivers -or
        $guest.testsigning_enabled -isnot [bool] -or [bool]$guest.testsigning_enabled -or
        [int]$guest.root_expected_thumbprint_count -ne 0 -or
        [int]$guest.trusted_publisher_expected_thumbprint_count -ne 0 -or
        [int]$guest.root_foreign_subject_count -ne 0 -or
        [int]$guest.trusted_publisher_foreign_subject_count -ne 0) {
        throw "ready_base_receipt_not_clean_sealed_ready"
    }
    $true
}

function Assert-Win11LabReadyBaseShutdownReceipt {
    param([object]$Receipt, [int]$ExpectedDelaySeconds)
    if ($null -eq $Receipt -or
        $null -eq $Receipt.PSObject.Properties["shutdown_scheduled"] -or
        $null -eq $Receipt.PSObject.Properties["delay_seconds"] -or
        $Receipt.shutdown_scheduled -isnot [bool] -or -not [bool]$Receipt.shutdown_scheduled -or
        $Receipt.delay_seconds -isnot [int] -or [int]$Receipt.delay_seconds -ne $ExpectedDelaySeconds) {
        throw "ready_base_shutdown_receipt_invalid"
    }
    $true
}

function Invoke-Win11LabReadyBaseCopyWorker {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath,
        [Parameter(Mandatory = $true)][string]$DestinationPath,
        [Parameter(Mandatory = $true)][string]$ResultPath
    )
    $source = Get-Item -LiteralPath $SourcePath -Force -ErrorAction Stop
    if (($source.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
        -not [string]$source.Extension.Equals(".vhdx", [StringComparison]::OrdinalIgnoreCase)) {
        throw "ready_base_source_vhd_invalid"
    }
    if (Test-Path -LiteralPath $DestinationPath) {
        throw "ready_base_copy_destination_exists"
    }
    $destinationParent = Split-Path -Parent $DestinationPath
    if (-not (Test-Path -LiteralPath $destinationParent -PathType Container)) {
        throw "ready_base_copy_parent_missing"
    }
    $sourceHash = Get-Win11LabReadyBaseSha256 -Path $source.FullName
    Copy-Item -LiteralPath $source.FullName -Destination $DestinationPath -ErrorAction Stop
    $destination = Get-Item -LiteralPath $DestinationPath -Force -ErrorAction Stop
    $destinationHash = Get-Win11LabReadyBaseSha256 -Path $destination.FullName
    if ([int64]$source.Length -ne [int64]$destination.Length -or
        $sourceHash -cne $destinationHash) {
        throw "ready_base_copy_hash_or_size_mismatch"
    }
    $result = [ordered]@{
        schema = 1
        source_vhd_bytes = [int64]$source.Length
        copied_vhd_bytes = [int64]$destination.Length
        source_vhd_sha256 = $sourceHash
        copied_vhd_sha256 = $destinationHash
    } | ConvertTo-Json -Depth 4 -Compress
    [IO.File]::WriteAllText($ResultPath, $result + "`n", (New-Object Text.UTF8Encoding($false)))
}

if (-not [string]::IsNullOrWhiteSpace($WorkerEntryMode)) {
    Invoke-Win11LabReadyBaseCopyWorker -SourcePath $WorkerEntrySourcePath `
        -DestinationPath $WorkerEntryDestinationPath -ResultPath $WorkerEntryResultPath
    exit 0
}

. (Join-Path $PSScriptRoot "Invoke-GuestPsDirectBounded.ps1")

$requiredHostCommands = @(
    "Get-VM",
    "Get-VMSnapshot",
    "Get-VMHardDiskDrive",
    "Get-VMNetworkAdapter",
    "Get-VMSwitch",
    "Get-VMProcessor",
    "Get-VMFirmware",
    "Get-VMSecurity"
)
foreach ($requiredHostCommand in $requiredHostCommands) {
    if ($null -eq (Get-Command -Name $requiredHostCommand -CommandType Cmdlet -ErrorAction SilentlyContinue)) {
        throw ("ready_base_required_host_command_missing:" + $requiredHostCommand)
    }
}

if (-not $ApproveSeal) {
    throw "ready_base_approval_required"
}
if (Test-Path -LiteralPath $BaseRoot) {
    throw "ready_base_destination_exists"
}
$baseParent = Split-Path -Parent $BaseRoot
if ([string]::IsNullOrWhiteSpace($baseParent) -or
    -not (Test-Path -LiteralPath $baseParent -PathType Container)) {
    throw "ready_base_parent_missing"
}
$baseParentItem = Get-Item -LiteralPath $baseParent -Force -ErrorAction Stop
if (($baseParentItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "ready_base_parent_reparse_refused"
}
if (-not (Test-Path -LiteralPath $ReadinessReceiptPath -PathType Leaf)) {
    throw "ready_base_receipt_missing"
}
$receiptItem = Get-Item -LiteralPath $ReadinessReceiptPath -Force -ErrorAction Stop
if ($receiptItem.Length -le 0 -or $receiptItem.Length -gt 2MB -or
    ($receiptItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "ready_base_receipt_file_invalid"
}
$receiptHash = Get-Win11LabReadyBaseSha256 -Path $receiptItem.FullName
if ($receiptHash -cne $ExpectedReadinessReceiptSha256.ToUpperInvariant()) {
    throw "ready_base_receipt_sha256_mismatch"
}
try {
    $expectedGuid = [guid]$ExpectedVMId
    $receipt = [IO.File]::ReadAllText($receiptItem.FullName) | ConvertFrom-Json -ErrorAction Stop
}
catch {
    throw "ready_base_receipt_parse_or_vm_id_invalid"
}
Assert-Win11LabReadyBaseReceipt -Receipt $receipt -ExpectedName $VMName `
    -ExpectedId $expectedGuid -ExpectedSwitch $ExpectedSwitchName | Out-Null

$vmRows = @(Get-VM -Name $VMName -ErrorAction Stop)
if ($vmRows.Count -ne 1 -or [guid]$vmRows[0].Id -ne $expectedGuid -or
    [string]$vmRows[0].State -cne "Running") {
    throw "ready_base_host_vm_identity_or_state_invalid"
}
$vm = $vmRows[0]
if (@(Get-VMSnapshot -VMName $VMName -ErrorAction Stop).Count -ne 0) {
    throw "ready_base_checkpoint_refused"
}
$drives = @(Get-VMHardDiskDrive -VMName $VMName -ErrorAction Stop)
$adapters = @(Get-VMNetworkAdapter -VMName $VMName -ErrorAction Stop)
$switches = @(Get-VMSwitch -Name $ExpectedSwitchName -ErrorAction Stop)
if ($drives.Count -ne 1 -or [string]::IsNullOrWhiteSpace([string]$drives[0].Path) -or
    $adapters.Count -ne 1 -or [string]$adapters[0].SwitchName -cne $ExpectedSwitchName -or
    $switches.Count -ne 1 -or [string]$switches[0].Name -cne $ExpectedSwitchName -or
    [string]$switches[0].SwitchType -cne "Private") {
    throw "ready_base_host_storage_or_network_identity_invalid"
}
$sourceVhd = Get-Item -LiteralPath ([string]$drives[0].Path) -Force -ErrorAction Stop
if (($sourceVhd.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
    -not [string]$sourceVhd.Extension.Equals(".vhdx", [StringComparison]::OrdinalIgnoreCase)) {
    throw "ready_base_source_vhd_invalid"
}
$passwordItem = Get-Item -LiteralPath $PasswordFile -Force -ErrorAction Stop
if (($passwordItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
    $passwordItem.Length -le 0 -or $passwordItem.Length -gt 4096) {
    throw "ready_base_password_file_invalid"
}
$password = [IO.File]::ReadAllText($passwordItem.FullName).Trim()
if ([string]::IsNullOrWhiteSpace($password)) {
    throw "ready_base_password_missing"
}

$runId = [guid]::NewGuid().ToString("N")
$stagingRoot = Join-Path $baseParent ((Split-Path -Leaf $BaseRoot) + ".staging." + $runId)
$copyResultPath = Join-Path $stagingRoot "copy-result.json"
$copiedVhdPath = Join-Path $stagingRoot "base.vhdx"
$promoted = $false
try {
    $shutdownReceipt = Invoke-GuestPsDirectBounded -VMName $VMName -User $User `
        -Password $password -Operation invoke -TimeoutSeconds 120 -ConnectTimeoutSeconds 60 `
        -ScriptBlock {
            param([int]$DelaySeconds)
            & shutdown.exe /s /t $DelaySeconds | Out-Null
            if ($LASTEXITCODE -ne 0) {
                throw "ready base guest shutdown scheduling failed"
            }
            [pscustomobject]@{
                shutdown_scheduled = $true
                delay_seconds = $DelaySeconds
            }
        } -ArgumentList @($ShutdownDelaySeconds)
    Assert-Win11LabReadyBaseShutdownReceipt -Receipt $shutdownReceipt `
        -ExpectedDelaySeconds $ShutdownDelaySeconds | Out-Null

    $offDeadline = [DateTime]::UtcNow.AddSeconds($ShutdownTimeoutSeconds)
    do {
        Start-Sleep -Seconds 2
        $current = Get-VM -Name $VMName -ErrorAction Stop
        if ([guid]$current.Id -ne $expectedGuid) {
            throw "ready_base_vm_identity_drift"
        }
        if ([string]$current.State -ceq "Off") {
            break
        }
    } while ([DateTime]::UtcNow -lt $offDeadline)
    if ([string]$current.State -cne "Off") {
        throw "ready_base_guest_shutdown_timeout"
    }

    New-Item -ItemType Directory -Path $stagingRoot -ErrorAction Stop | Out-Null
    $powerShellPath = Join-Path $PSHOME "powershell.exe"
    $argumentValues = @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $PSCommandPath,
        "-WorkerMode", "Copy", "-SourcePath", $sourceVhd.FullName,
        "-DestinationPath", $copiedVhdPath, "-ResultPath", $copyResultPath
    )
    $quotedArguments = @($argumentValues | ForEach-Object {
            Quote-GuestProcessArgument -Value ([string]$_)
        })
    $arguments = $quotedArguments -join " "
    $execution = Invoke-BoundedGuestProcess -FilePath $powerShellPath -Arguments $arguments `
        -TimeoutSeconds $CopyTimeoutSeconds
    if (-not [bool]$execution.completed -or [int]$execution.exit_code -ne 0) {
        throw "ready_base_copy_child_failed_or_timed_out"
    }
    if (-not (Test-Path -LiteralPath $copyResultPath -PathType Leaf)) {
        throw "ready_base_copy_receipt_missing"
    }
    $copyReceipt = [IO.File]::ReadAllText($copyResultPath) | ConvertFrom-Json -ErrorAction Stop
    if ([int]$copyReceipt.schema -ne 1 -or
        [int64]$copyReceipt.source_vhd_bytes -ne [int64]$copyReceipt.copied_vhd_bytes -or
        [string]$copyReceipt.source_vhd_sha256 -cne [string]$copyReceipt.copied_vhd_sha256 -or
        [string]$copyReceipt.source_vhd_sha256 -notmatch '^[A-F0-9]{64}$') {
        throw "ready_base_copy_receipt_invalid"
    }
    [IO.File]::Delete($copyResultPath)

    $processor = Get-VMProcessor -VMName $VMName -ErrorAction Stop
    $firmware = Get-VMFirmware -VMName $VMName -ErrorAction Stop
    $security = Get-VMSecurity -VMName $VMName -ErrorAction Stop
    $manifest = [ordered]@{
        schema = 1
        kind = "ramshared-isolated-win11-lab-base"
        source_vm_name = $VMName
        source_vm_id = $expectedGuid.ToString()
        source_ready_receipt_sha256 = $receiptHash
        network_policy = "SealedOffline"
        switch_name = $ExpectedSwitchName
        source_vhd_bytes = [int64]$copyReceipt.source_vhd_bytes
        copied_vhd_bytes = [int64]$copyReceipt.copied_vhd_bytes
        source_vhd_sha256 = [string]$copyReceipt.source_vhd_sha256
        copied_vhd_sha256 = [string]$copyReceipt.copied_vhd_sha256
        processor_count = [int]$processor.Count
        dynamic_memory_enabled = [bool]$vm.DynamicMemoryEnabled
        memory_startup_bytes = [int64]$vm.MemoryStartup
        memory_minimum_bytes = [int64]$vm.MemoryMinimum
        memory_maximum_bytes = [int64]$vm.MemoryMaximum
        secure_boot_enabled = ([string]$firmware.SecureBoot -ceq "On")
        tpm_enabled = [bool]$security.TpmEnabled
        checkpoints = 0
        guest_shutdown = "supported_deferred"
        base_usage = "isolated_lab_only"
        base_files_read_only = $true
        sealed_utc = [DateTime]::UtcNow.ToString("o")
    }
    $manifestPath = Join-Path $stagingRoot "base-manifest.json"
    [IO.File]::WriteAllText($manifestPath,
        (($manifest | ConvertTo-Json -Depth 6 -Compress) + "`n"),
        (New-Object Text.UTF8Encoding($false)))
    Set-ItemProperty -LiteralPath $copiedVhdPath -Name IsReadOnly -Value $true -ErrorAction Stop
    Set-ItemProperty -LiteralPath $manifestPath -Name IsReadOnly -Value $true -ErrorAction Stop
    if (-not (Get-Item -LiteralPath $copiedVhdPath -Force -ErrorAction Stop).IsReadOnly -or
        -not (Get-Item -LiteralPath $manifestPath -Force -ErrorAction Stop).IsReadOnly) {
        throw "ready_base_readonly_seal_failed"
    }
    if (Test-Path -LiteralPath $BaseRoot) {
        throw "ready_base_destination_exists"
    }
    [IO.Directory]::Move($stagingRoot, $BaseRoot)
    $promoted = $true
    [pscustomobject]$manifest | ConvertTo-Json -Depth 6 -Compress
}
finally {
    $password = $null
    if (-not $promoted -and (Test-Path -LiteralPath $stagingRoot)) {
        Remove-Item -LiteralPath $stagingRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
