#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$SwitchName,
    [switch]$ApproveCreate
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Ensure-Win11LabSealedSwitchState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$SwitchName,
        [Parameter(Mandatory = $true)][bool]$ApproveCreate,
        [Parameter(Mandatory = $true)][scriptblock]$GetSwitch,
        [Parameter(Mandatory = $true)][scriptblock]$CreateSwitch
    )

    if ([string]::IsNullOrWhiteSpace($SwitchName) -or
        $SwitchName -cne $SwitchName.Trim()) {
        throw "sealed_switch_name_invalid"
    }
    try {
        $rows = @(& $GetSwitch $SwitchName | Where-Object { $null -ne $_ })
    }
    catch {
        throw "sealed_switch_provider_failed"
    }
    if ($rows.Count -gt 1) {
        throw "sealed_switch_ambiguous"
    }
    $action = "existing"
    if ($rows.Count -eq 0) {
        if (-not $ApproveCreate) {
            throw "sealed_switch_create_approval_required"
        }
        try {
            & $CreateSwitch $SwitchName | Out-Null
        }
        catch {
            throw "sealed_switch_create_failed"
        }
        $action = "created"
        try {
            $rows = @(& $GetSwitch $SwitchName | Where-Object { $null -ne $_ })
        }
        catch {
            throw "sealed_switch_postcreate_provider_failed"
        }
        if ($rows.Count -ne 1) {
            throw "sealed_switch_postcreate_identity_invalid"
        }
    }

    $row = $rows[0]
    if ([string]$row.Name -cne $SwitchName) {
        throw "sealed_switch_identity_mismatch"
    }
    if ([string]$row.SwitchType -cne "Private") {
        throw "sealed_switch_type_invalid"
    }
    try {
        $switchId = ([guid]$row.Id).ToString()
    }
    catch {
        throw "sealed_switch_id_invalid"
    }

    [pscustomobject][ordered]@{
        schema = [int]1
        action = $action
        switch_name = $SwitchName
        switch_id = $switchId
        switch_type = "Private"
        observed_utc = [DateTime]::UtcNow.ToString("o")
    }
}

$receipt = Ensure-Win11LabSealedSwitchState -SwitchName $SwitchName `
    -ApproveCreate ([bool]$ApproveCreate) `
    -GetSwitch {
        param($name)
        @(Get-VMSwitch -Name $name -ErrorAction Stop)
    } `
    -CreateSwitch {
        param($name)
        New-VMSwitch -Name $name -SwitchType Private -ErrorAction Stop | Out-Null
    }
$receipt | ConvertTo-Json -Depth 4 -Compress
