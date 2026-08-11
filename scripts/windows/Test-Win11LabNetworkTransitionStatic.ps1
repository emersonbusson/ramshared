#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$SealedSwitchPath = "",
    [string]$NetworkTransitionPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($SealedSwitchPath)) {
    $SealedSwitchPath = Join-Path $PSScriptRoot "Ensure-Win11LabSealedSwitch.ps1"
}
if ([string]::IsNullOrWhiteSpace($NetworkTransitionPath)) {
    $NetworkTransitionPath = Join-Path $PSScriptRoot "Set-Win11LabNetworkTransition.ps1"
}
if (-not (Test-Path -LiteralPath $SealedSwitchPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $NetworkTransitionPath -PathType Leaf)) {
    throw "win11_lab_network_transition_static: production harness missing"
}

function Import-Win11LabFunction {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Source
    )
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput(
        $Source, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "win11_lab_network_transition_static: parser failed"
    }
    $definition = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -ceq $Name
        }, $true)
    if ($null -eq $definition) {
        throw "win11_lab_network_transition_static: function missing $Name"
    }
    $body = $definition.Body.Extent.Text.Trim()
    Set-Item -Path ("Function:\script:{0}" -f $Name) -Value (
        [scriptblock]::Create($body.Substring(1, $body.Length - 2)))
}

function Assert-ThrowsCode {
    param([scriptblock]$Action, [string]$Code)
    try { & $Action }
    catch {
        if ($_.Exception.Message -notmatch [regex]::Escape($Code)) {
            throw "win11_lab_network_transition_static: expected $Code, got $($_.Exception.Message)"
        }
        return
    }
    throw "win11_lab_network_transition_static: expected refusal $Code"
}

$sealedText = Get-Content -LiteralPath $SealedSwitchPath -Raw
$transitionText = Get-Content -LiteralPath $NetworkTransitionPath -Raw
Import-Win11LabFunction -Name "Ensure-Win11LabSealedSwitchState" -Source $sealedText
Import-Win11LabFunction -Name "Assert-Win11LabSealedReadyReceipt" -Source $transitionText
Import-Win11LabFunction -Name "Invoke-Win11LabNetworkTransitionState" -Source $transitionText

$sealedName = "RamShared-SealedOffline"
$existing = Ensure-Win11LabSealedSwitchState -SwitchName $sealedName -ApproveCreate:$false `
    -GetSwitch { param($name) [pscustomobject]@{ Name = $name; Id = [guid]::NewGuid(); SwitchType = "Private" } } `
    -CreateSwitch { param($name) throw "must not create" }
if ($existing.action -cne "existing" -or $existing.switch_type -cne "Private") {
    throw "sealed_switch_existing_private_is_noop"
}
Write-Output "PASS sealed_switch_existing_private_is_noop"

Assert-ThrowsCode -Code "sealed_switch_create_approval_required" -Action {
    Ensure-Win11LabSealedSwitchState -SwitchName $sealedName -ApproveCreate:$false `
        -GetSwitch { param($name) @() } -CreateSwitch { param($name) } | Out-Null
}
Write-Output "PASS sealed_switch_creation_requires_approval"

Assert-ThrowsCode -Code "sealed_switch_type_invalid" -Action {
    Ensure-Win11LabSealedSwitchState -SwitchName $sealedName -ApproveCreate:$false `
        -GetSwitch { param($name) [pscustomobject]@{ Name = $name; Id = [guid]::NewGuid(); SwitchType = "Internal" } } `
        -CreateSwitch { param($name) } | Out-Null
}
Write-Output "PASS sealed_switch_foreign_type_is_refused"

$script:sealedCreated = $false
$created = Ensure-Win11LabSealedSwitchState -SwitchName $sealedName -ApproveCreate:$true `
    -GetSwitch {
        param($name)
        if ($script:sealedCreated) {
            [pscustomobject]@{ Name = $name; Id = [guid]"11111111-1111-1111-1111-111111111111"; SwitchType = "Private" }
        }
        else { @() }
    } `
    -CreateSwitch { param($name) $script:sealedCreated = $true }
if ($created.action -cne "created" -or
    $created.switch_id -cne "11111111-1111-1111-1111-111111111111") {
    throw "sealed_switch_postcreate_identity_is_exact"
}
Write-Output "PASS sealed_switch_postcreate_identity_is_exact"

$vmName = "manufactured-clean-vm"
$vmId = [guid]"22222222-2222-2222-2222-222222222222"
$adapterId = "Microsoft:$($vmId.ToString().ToUpperInvariant())\33333333-3333-3333-3333-333333333333"
$receipt = [pscustomobject]@{
    schema = 1
    status = "READY"
    vm_name = $vmName
    expected_switch_name = $sealedName
    network_policy = "SealedOffline"
    after = [pscustomobject]@{
        ready = $true
        host = [pscustomobject]@{ vm_id = $vmId.ToString(); vm_state = "Running" }
        guest = [pscustomobject]@{
            image_state = "IMAGE_STATE_COMPLETE"
            system_setup_in_progress = 0
            oobe_in_progress = 0
            setup_phase = 0
            setup_type = 0
            routable_ipv4_count = 0
        }
    }
}
Assert-Win11LabSealedReadyReceipt -Receipt $receipt -ExpectedVMName $vmName `
    -ExpectedVMId $vmId -ExpectedSwitchName $sealedName | Out-Null
Write-Output "PASS network_transition_requires_sealed_ready_receipt"

$script:adapterSwitch = $sealedName
$transition = Invoke-Win11LabNetworkTransitionState -VMName $vmName -ExpectedVMId $vmId `
    -CurrentSwitchName $sealedName -TargetSwitchName "External" -ApproveTransition:$true `
    -GetVm { param($name) [pscustomobject]@{ Name = $name; Id = $vmId; State = "Running" } } `
    -GetSnapshots { param($name) @() } `
    -GetSwitch { param($name) [pscustomobject]@{ Name = $name; SwitchType = if ($name -ceq $sealedName) { "Private" } else { "External" } } } `
    -GetAdapters { param($name) [pscustomobject]@{ Id = $adapterId; SwitchName = $script:adapterSwitch } } `
    -ConnectAdapter { param($adapter, $switchName) $script:adapterSwitch = $switchName }
if ($transition.before_switch -cne $sealedName -or
    $transition.after_switch -cne "External" -or
    $transition.adapter_id -cne $adapterId) {
    throw "network_transition_binds_exact_vm_and_adapter"
}
Write-Output "PASS network_transition_binds_exact_vm_and_adapter"

Assert-ThrowsCode -Code "network_transition_target_switch_type_invalid" -Action {
    Invoke-Win11LabNetworkTransitionState -VMName $vmName -ExpectedVMId $vmId `
        -CurrentSwitchName $sealedName -TargetSwitchName "Internal" -ApproveTransition:$true `
        -GetVm { param($name) [pscustomobject]@{ Name = $name; Id = $vmId; State = "Running" } } `
        -GetSnapshots { param($name) @() } `
        -GetSwitch { param($name) [pscustomobject]@{ Name = $name; SwitchType = if ($name -ceq $sealedName) { "Private" } else { "Internal" } } } `
        -GetAdapters { param($name) [pscustomobject]@{ Id = $adapterId; SwitchName = $sealedName } } `
        -ConnectAdapter { param($adapter, $switchName) } | Out-Null
}
Write-Output "PASS network_transition_target_must_be_external"

$script:restoreCalls = New-Object System.Collections.Generic.List[string]
$script:adapterSwitch = $sealedName
$script:postTargetRead = $false
Assert-ThrowsCode -Code "network_transition_postread_failed_restored" -Action {
    Invoke-Win11LabNetworkTransitionState -VMName $vmName -ExpectedVMId $vmId `
        -CurrentSwitchName $sealedName -TargetSwitchName "External" -ApproveTransition:$true `
        -GetVm { param($name) [pscustomobject]@{ Name = $name; Id = $vmId; State = "Running" } } `
        -GetSnapshots { param($name) @() } `
        -GetSwitch { param($name) [pscustomobject]@{ Name = $name; SwitchType = if ($name -ceq $sealedName) { "Private" } else { "External" } } } `
        -GetAdapters {
            param($name)
            if ($script:adapterSwitch -ceq "External" -and -not $script:postTargetRead) {
                $script:postTargetRead = $true
                return [pscustomobject]@{ Id = $adapterId; SwitchName = "foreign" }
            }
            [pscustomobject]@{ Id = $adapterId; SwitchName = $script:adapterSwitch }
        } `
        -ConnectAdapter {
            param($adapter, $switchName)
            [void]$script:restoreCalls.Add($switchName)
            $script:adapterSwitch = $switchName
        } | Out-Null
}
if ($script:adapterSwitch -cne $sealedName -or
    $script:restoreCalls.Count -ne 2 -or
    $script:restoreCalls[1] -cne $sealedName) {
    throw "network_transition_failure_restores_sealed_switch"
}
Write-Output "PASS network_transition_failure_restores_sealed_switch"

foreach ($forbidden in @("Restart-VM", "Stop-VM", "Start-VM", "Password", "Credential", "Invoke-Command", "New-PSSession")) {
    if ($transitionText -match [regex]::Escape($forbidden)) {
        throw "network_transition_never_reboots_or_accepts_credentials"
    }
}
Write-Output "PASS network_transition_never_reboots_or_accepts_credentials"
Write-Output "PASS Test-Win11LabNetworkTransitionStatic"
