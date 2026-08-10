#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$VMName,
    [Parameter(Mandatory = $true)][guid]$ExpectedVMId,
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$CurrentSwitchName,
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$TargetSwitchName,
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$ReadyReceiptPath,
    [switch]$ApproveTransition
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Assert-Win11LabSealedReadyReceipt {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$Receipt,
        [Parameter(Mandatory = $true)][string]$ExpectedVMName,
        [Parameter(Mandatory = $true)][guid]$ExpectedVMId,
        [Parameter(Mandatory = $true)][string]$ExpectedSwitchName
    )

    try {
        $receiptVmId = [guid]([string]$Receipt.after.host.vm_id)
        $valid = [int]$Receipt.schema -eq 1 -and
            [string]$Receipt.status -ceq "READY" -and
            [string]$Receipt.vm_name -ceq $ExpectedVMName -and
            [string]$Receipt.expected_switch_name -ceq $ExpectedSwitchName -and
            [string]$Receipt.network_policy -ceq "SealedOffline" -and
            [bool]$Receipt.after.ready -and
            $receiptVmId -eq $ExpectedVMId -and
            [string]$Receipt.after.host.vm_state -ceq "Running" -and
            [string]$Receipt.after.guest.image_state -ceq "IMAGE_STATE_COMPLETE" -and
            [int]$Receipt.after.guest.system_setup_in_progress -eq 0 -and
            [int]$Receipt.after.guest.oobe_in_progress -eq 0 -and
            [int]$Receipt.after.guest.setup_phase -eq 0 -and
            [int]$Receipt.after.guest.setup_type -eq 0 -and
            [int]$Receipt.after.guest.routable_ipv4_count -eq 0
    }
    catch {
        throw "network_transition_ready_receipt_invalid"
    }
    if (-not $valid) {
        throw "network_transition_ready_receipt_invalid"
    }
    $true
}

function Invoke-Win11LabNetworkTransitionState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$VMName,
        [Parameter(Mandatory = $true)][guid]$ExpectedVMId,
        [Parameter(Mandatory = $true)][string]$CurrentSwitchName,
        [Parameter(Mandatory = $true)][string]$TargetSwitchName,
        [Parameter(Mandatory = $true)][bool]$ApproveTransition,
        [Parameter(Mandatory = $true)][scriptblock]$GetVm,
        [Parameter(Mandatory = $true)][scriptblock]$GetSnapshots,
        [Parameter(Mandatory = $true)][scriptblock]$GetSwitch,
        [Parameter(Mandatory = $true)][scriptblock]$GetAdapters,
        [Parameter(Mandatory = $true)][scriptblock]$ConnectAdapter
    )

    if (-not $ApproveTransition) {
        throw "network_transition_approval_required"
    }
    foreach ($value in @($VMName, $CurrentSwitchName, $TargetSwitchName)) {
        if ([string]::IsNullOrWhiteSpace($value) -or $value -cne $value.Trim()) {
            throw "network_transition_identity_invalid"
        }
    }
    if ($CurrentSwitchName -ceq $TargetSwitchName) {
        throw "network_transition_switches_must_differ"
    }

    try { $vms = @(& $GetVm $VMName | Where-Object { $null -ne $_ }) }
    catch { throw "network_transition_vm_provider_failed" }
    if ($vms.Count -ne 1 -or [string]$vms[0].Name -cne $VMName) {
        throw "network_transition_vm_identity_invalid"
    }
    try { $actualVmId = [guid]$vms[0].Id }
    catch { throw "network_transition_vm_identity_invalid" }
    if ($actualVmId -ne $ExpectedVMId -or [string]$vms[0].State -cne "Running") {
        throw "network_transition_vm_identity_invalid"
    }
    try { $snapshots = @(& $GetSnapshots $VMName | Where-Object { $null -ne $_ }) }
    catch { throw "network_transition_snapshot_provider_failed" }
    if ($snapshots.Count -ne 0) {
        throw "network_transition_snapshot_residue"
    }

    try {
        $sourceSwitches = @(& $GetSwitch $CurrentSwitchName | Where-Object { $null -ne $_ })
        $targetSwitches = @(& $GetSwitch $TargetSwitchName | Where-Object { $null -ne $_ })
    }
    catch { throw "network_transition_switch_provider_failed" }
    if ($sourceSwitches.Count -ne 1 -or
        [string]$sourceSwitches[0].Name -cne $CurrentSwitchName -or
        [string]$sourceSwitches[0].SwitchType -cne "Private") {
        throw "network_transition_source_switch_invalid"
    }
    if ($targetSwitches.Count -ne 1 -or
        [string]$targetSwitches[0].Name -cne $TargetSwitchName -or
        [string]$targetSwitches[0].SwitchType -cne "External") {
        throw "network_transition_target_switch_type_invalid"
    }

    try { $adapters = @(& $GetAdapters $VMName | Where-Object { $null -ne $_ }) }
    catch { throw "network_transition_adapter_provider_failed" }
    if ($adapters.Count -ne 1 -or
        [string]::IsNullOrWhiteSpace([string]$adapters[0].Id) -or
        [string]$adapters[0].SwitchName -cne $CurrentSwitchName) {
        throw "network_transition_adapter_identity_invalid"
    }
    $adapter = $adapters[0]
    $adapterId = [string]$adapter.Id
    $mutationAttempted = $false
    try {
        $mutationAttempted = $true
        & $ConnectAdapter $adapter $TargetSwitchName | Out-Null
        $afterAdapters = @(& $GetAdapters $VMName | Where-Object { $null -ne $_ })
        if ($afterAdapters.Count -ne 1 -or
            [string]$afterAdapters[0].Id -cne $adapterId -or
            [string]$afterAdapters[0].SwitchName -cne $TargetSwitchName) {
            throw "network_transition_postread_failed"
        }
        return [pscustomobject][ordered]@{
            schema = [int]1
            vm_name = $VMName
            vm_id = $ExpectedVMId.ToString()
            adapter_id = $adapterId
            before_switch = $CurrentSwitchName
            after_switch = $TargetSwitchName
            restored = $false
            completed_utc = [DateTime]::UtcNow.ToString("o")
        }
    }
    catch {
        if ($mutationAttempted) {
            try {
                & $ConnectAdapter $adapter $CurrentSwitchName | Out-Null
                $restoredAdapters = @(& $GetAdapters $VMName | Where-Object { $null -ne $_ })
                if ($restoredAdapters.Count -ne 1 -or
                    [string]$restoredAdapters[0].Id -cne $adapterId -or
                    [string]$restoredAdapters[0].SwitchName -cne $CurrentSwitchName) {
                    throw "restore readback invalid"
                }
            }
            catch {
                throw "network_transition_restore_failed"
            }
            throw "network_transition_postread_failed_restored"
        }
        throw "network_transition_connect_failed"
    }
}

if (-not $ApproveTransition) {
    throw "network_transition_approval_required"
}
if (-not (Test-Path -LiteralPath $ReadyReceiptPath -PathType Leaf)) {
    throw "network_transition_ready_receipt_missing"
}
$receiptItem = Get-Item -LiteralPath $ReadyReceiptPath -Force -ErrorAction Stop
if (($receiptItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
    $receiptItem.Length -lt 2 -or $receiptItem.Length -gt 2MB) {
    throw "network_transition_ready_receipt_file_invalid"
}
try {
    $readyReceipt = [IO.File]::ReadAllText($receiptItem.FullName) | ConvertFrom-Json -ErrorAction Stop
}
catch {
    throw "network_transition_ready_receipt_parse_failed"
}
Assert-Win11LabSealedReadyReceipt -Receipt $readyReceipt -ExpectedVMName $VMName `
    -ExpectedVMId $ExpectedVMId -ExpectedSwitchName $CurrentSwitchName | Out-Null
$readyReceiptSha256 = (Get-FileHash -LiteralPath $receiptItem.FullName -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()

$transitionReceipt = Invoke-Win11LabNetworkTransitionState -VMName $VMName `
    -ExpectedVMId $ExpectedVMId -CurrentSwitchName $CurrentSwitchName `
    -TargetSwitchName $TargetSwitchName -ApproveTransition ([bool]$ApproveTransition) `
    -GetVm { param($name) @(Get-VM -Name $name -ErrorAction Stop) } `
    -GetSnapshots { param($name) @(Get-VMSnapshot -VMName $name -ErrorAction Stop) } `
    -GetSwitch { param($name) @(Get-VMSwitch -Name $name -ErrorAction Stop) } `
    -GetAdapters { param($name) @(Get-VMNetworkAdapter -VMName $name -ErrorAction Stop) } `
    -ConnectAdapter {
        param($adapter, $switchName)
        Connect-VMNetworkAdapter -VMNetworkAdapter $adapter -SwitchName $switchName -ErrorAction Stop
    }
$transitionReceipt | Add-Member -NotePropertyName ready_receipt_sha256 -NotePropertyValue $readyReceiptSha256
$transitionReceipt | ConvertTo-Json -Depth 5 -Compress
