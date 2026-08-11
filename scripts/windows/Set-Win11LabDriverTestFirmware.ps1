#Requires -Version 5.1
<#
.SYNOPSIS
  Reversibly changes Secure Boot on one approved disposable Driver Verifier VM.

.DESCRIPTION
  The transition schedules a graceful guest shutdown through bounded
  PowerShell Direct, changes firmware only after the exact VM is Off, reads
  the value back, and starts only that VM. It never changes the physical host
  boot configuration and never force-stops a VM.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$VMName,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedVMId,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$User,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$Password,
    [Parameter(Mandatory = $true)]
    [ValidateSet("Off", "On")]
    [string]$DesiredSecureBoot,
    [Parameter(Mandatory = $true)]
    [switch]$ApproveGuestFirmwareTransition,
    [ValidateRange(15, 30)]
    [int]$GuestShutdownDelaySeconds = 15,
    [ValidateRange(60, 900)]
    [int]$VmStateTimeoutSeconds = 300,
    [ValidateRange(1, 180)]
    [int]$PsDirectConnectTimeoutSeconds = 60,
    [ValidateNotNullOrEmpty()]
    [string]$ArtifactRoot = "C:\ramshared\artifacts"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "Invoke-GuestPsDirectBounded.ps1")

function Get-Win11DriverTestFirmwareVm {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedId,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedState
    )

    $expectedCanonical = ([guid]$ExpectedId).ToString("D").ToUpperInvariant()
    $matches = @(Get-VM -Name $Name -ErrorAction Stop)
    if ($matches.Count -ne 1) {
        throw "driver-test firmware VM identity is zero or ambiguous"
    }
    $vm = $matches[0]
    $actualCanonical = ([guid]$vm.Id).ToString("D").ToUpperInvariant()
    if ([string]$vm.Name -cne $Name -or $actualCanonical -cne $expectedCanonical -or
        [int]$vm.Generation -ne 2 -or [string]$vm.State -cne $ExpectedState) {
        throw "driver-test firmware VM identity, generation, or state is not exact"
    }
    $vm
}

function Wait-Win11DriverTestFirmwareVmState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExpectedState,
        [Parameter(Mandatory = $true)]
        [datetime]$DeadlineUtc
    )

    while ([DateTime]::UtcNow -lt $DeadlineUtc) {
        $vm = @(Get-VM -Name $VMName -ErrorAction Stop)
        if ($vm.Count -ne 1 -or ([guid]$vm[0].Id).ToString("D").ToUpperInvariant() -cne
            ([guid]$ExpectedVMId).ToString("D").ToUpperInvariant()) {
            throw "driver-test firmware VM identity changed while waiting"
        }
        if ([string]$vm[0].State -ceq $ExpectedState) {
            return $vm[0]
        }
        Start-Sleep -Seconds 2
    }
    throw "driver-test firmware VM state deadline exceeded"
}

function Test-Win11DriverTestFirmwareAdapterStatus {
    [CmdletBinding()]
    param([AllowEmptyString()][string]$Status)

    [string]::Equals($Status, "OK", [StringComparison]::InvariantCultureIgnoreCase)
}

function Wait-Win11DriverTestFirmwareHostContact {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [datetime]$DeadlineUtc
    )

    $expectedComponents = @(
        "6C09BB55-D683-4DA0-8931-C9BF705F6480",
        "84EAAE65-2F2E-45F5-9BB5-0E857DC8EB47",
        "2A34B1C2-FD73-4043-8A5B-DD2159BC743F",
        "9F8233AC-BE49-4C79-8EE3-E7E1985B2077",
        "2497F4DE-E9FA-4204-80E4-4B75C46419C0",
        "5CED1297-4598-4915-A5FC-AD21BB4D02A4"
    ) | Sort-Object
    $expectedId = ([guid]$ExpectedVMId).ToString("D").ToUpperInvariant()

    while ([DateTime]::UtcNow -lt $DeadlineUtc) {
        [void](Get-Win11DriverTestFirmwareVm -Name $VMName -ExpectedId $expectedId `
                -ExpectedState "Running")
        $services = @(Get-VMIntegrationService -VMName $VMName -ErrorAction Stop)
        $components = @()
        $servicesValid = $services.Count -eq 6
        foreach ($service in $services) {
            $match = [regex]::Match([string]$service.Id,
                '(?i)(?<component>[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$')
            if (-not $match.Success) {
                throw "driver-test firmware integration-service identity is malformed"
            }
            $components += ([guid]$match.Groups["component"].Value).ToString("D").ToUpperInvariant()
            $serviceVmId = if ($null -eq $service.PSObject.Properties["VMId"]) {
                $expectedId
            }
            else {
                ([guid]$service.VMId).ToString("D").ToUpperInvariant()
            }
            if ($serviceVmId -cne $expectedId -or -not [bool]$service.Enabled -or
                [string]$service.PrimaryStatusDescription -cne "OK") {
                $servicesValid = $false
            }
        }
        $components = @($components | Sort-Object)
        if (@(Compare-Object -ReferenceObject $expectedComponents -DifferenceObject $components).Count -ne 0) {
            $servicesValid = $false
        }

        $adapters = @(Get-VMNetworkAdapter -VMName $VMName -ErrorAction Stop)
        $adapterValid = $adapters.Count -eq 1 -and [bool]$adapters[0].Connected -and
            (Test-Win11DriverTestFirmwareAdapterStatus -Status ([string]$adapters[0].Status))
        if ($servicesValid -and $adapterValid) {
            return [pscustomobject]@{
                integration_component_ids = $components
                integration_service_count = [int]$services.Count
                network_adapter_count = [int]$adapters.Count
                network_adapter_connected = [bool]$adapters[0].Connected
                network_adapter_status = [string]$adapters[0].Status
            }
        }
        Start-Sleep -Seconds 2
    }
    throw "driver-test firmware host contact deadline exceeded"
}

function ConvertFrom-Win11DriverTestFirmwareRows {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][object[]]$Rows)

    $json = (@($Rows) | ForEach-Object { [string]$_ }) -join [Environment]::NewLine
    if ([string]::IsNullOrWhiteSpace($json)) {
        throw "driver-test firmware guest shutdown emitted no receipt"
    }
    try {
        $json | ConvertFrom-Json -ErrorAction Stop
    }
    catch {
        throw "driver-test firmware guest shutdown emitted malformed receipt"
    }
}

if (-not $ApproveGuestFirmwareTransition) {
    throw "ApproveGuestFirmwareTransition is required for this guest-only firmware transition"
}

$expectedVmIdCanonical = ([guid]$ExpectedVMId).ToString("D").ToUpperInvariant()
$runId = [guid]::NewGuid().ToString("D")
$artifactDirectory = Join-Path $ArtifactRoot ("driver-test-firmware-" + $runId)
New-Item -ItemType Directory -Path $artifactDirectory -ErrorAction Stop | Out-Null
$startedUtc = [DateTime]::UtcNow
$priorSecureBoot = ""
$firmwareChanged = $false
$priorFirmwareRestored = $false
$startedAfterChange = $false
$shutdownReceipt = $null
$failure = $null

try {
    $vm = Get-Win11DriverTestFirmwareVm -Name $VMName -ExpectedId $expectedVmIdCanonical -ExpectedState "Running"
    $snapshots = @(Get-VMSnapshot -VMName $VMName -ErrorAction Stop)
    if ($snapshots.Count -ne 0) {
        throw "driver-test firmware transition refuses VM snapshot residue"
    }
    $firmwareBefore = Get-VMFirmware -VMName $VMName -ErrorAction Stop
    $priorSecureBoot = [string]$firmwareBefore.SecureBoot
    if ($priorSecureBoot -cnotin @("Off", "On") -or $priorSecureBoot -ceq $DesiredSecureBoot) {
        throw "driver-test firmware prior state is invalid or already requested"
    }

    $rows = Invoke-GuestPsDirectBounded -VMName $VMName -User $User -Password $Password `
        -Operation invoke -TimeoutSeconds 120 -ConnectTimeoutSeconds $PsDirectConnectTimeoutSeconds `
        -ScriptBlock {
            param($RequestedDelay)
            $output = & shutdown.exe /s /t $RequestedDelay /d p:4:1 `
                /c "RamShared disposable driver-test firmware boundary" 2>&1 | Out-String
            $exitCode = [int]$LASTEXITCODE
            if ($exitCode -ne 0) {
                throw "guest shutdown schedule failed exit=$exitCode"
            }
            [pscustomobject]@{
                schema = [int]1
                shutdown_scheduled = $true
                action = "shutdown"
                delay_seconds = [int]$RequestedDelay
                command_exit_code = $exitCode
            } | ConvertTo-Json -Compress
        } -ArgumentList @($GuestShutdownDelaySeconds)
    $shutdownReceipt = ConvertFrom-Win11DriverTestFirmwareRows -Rows $rows
    if ([int]$shutdownReceipt.schema -ne 1 -or
        ($shutdownReceipt.shutdown_scheduled -isnot [bool]) -or
        [bool]$shutdownReceipt.shutdown_scheduled -ne $true -or
        [string]$shutdownReceipt.action -cne "shutdown" -or
        ($shutdownReceipt.delay_seconds -isnot [int]) -or
        [int]$shutdownReceipt.delay_seconds -ne $GuestShutdownDelaySeconds -or
        [int]$shutdownReceipt.command_exit_code -ne 0) {
        throw "driver-test firmware guest shutdown receipt is invalid"
    }

    $vm = Wait-Win11DriverTestFirmwareVmState -ExpectedState "Off" `
        -DeadlineUtc ([DateTime]::UtcNow.AddSeconds($VmStateTimeoutSeconds))
    if ($vm.State -cne "Off") {
        throw "driver-test firmware requires exact VM Off state"
    }

    Set-VMFirmware -VMName $VMName -EnableSecureBoot $DesiredSecureBoot -ErrorAction Stop
    $firmwareChanged = $true
    $firmwareReadback = Get-VMFirmware -VMName $VMName -ErrorAction Stop
    if ([string]$firmwareReadback.SecureBoot -cne $DesiredSecureBoot) {
        throw "driver-test firmware readback does not match the requested state"
    }

    Start-VM -Name $VMName -ErrorAction Stop | Out-Null
    $startedAfterChange = $true
    $runningDeadlineUtc = [DateTime]::UtcNow.AddSeconds($VmStateTimeoutSeconds)
    $vm = Wait-Win11DriverTestFirmwareVmState -ExpectedState "Running" `
        -DeadlineUtc $runningDeadlineUtc
    $hostContact = Wait-Win11DriverTestFirmwareHostContact -DeadlineUtc $runningDeadlineUtc

    $receipt = [pscustomobject]@{
        schema = [int]1
        run_id = $runId
        status = "PASS"
        started_utc = $startedUtc.ToString("o")
        completed_utc = [DateTime]::UtcNow.ToString("o")
        vm_name = $VMName
        expected_vm_id = $expectedVmIdCanonical
        generation = [int]$vm.Generation
        prior_secure_boot = $priorSecureBoot
        desired_secure_boot = $DesiredSecureBoot
        readback_secure_boot = [string]$firmwareReadback.SecureBoot
        final_vm_state = [string]$vm.State
        integration_component_ids = @($hostContact.integration_component_ids)
        integration_service_count = [int]$hostContact.integration_service_count
        network_adapter_count = [int]$hostContact.network_adapter_count
        network_adapter_connected = [bool]$hostContact.network_adapter_connected
        network_adapter_status = [string]$hostContact.network_adapter_status
        shutdown = $shutdownReceipt
        prior_firmware_restored = $false
    }
    $receipt | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath `
        (Join-Path $artifactDirectory "summary.json") -Encoding UTF8 -ErrorAction Stop
    Write-Output ("STATUS=PASS vm={0} secure_boot={1} artifacts={2}" -f
        $VMName, $DesiredSecureBoot, $artifactDirectory)
}
catch {
    $failure = $_
    $recoveryErrors = [Collections.Generic.List[string]]::new()
    try {
        $current = @(Get-VM -Name $VMName -ErrorAction Stop)
        if ($current.Count -ne 1 -or ([guid]$current[0].Id).ToString("D").ToUpperInvariant() -cne
            $expectedVmIdCanonical) {
            throw "exact VM identity unavailable during firmware recovery"
        }
        if ([string]$current[0].State -ceq "Off") {
            if ($firmwareChanged) {
                Set-VMFirmware -VMName $VMName -EnableSecureBoot $priorSecureBoot -ErrorAction Stop
                $rollbackReadback = Get-VMFirmware -VMName $VMName -ErrorAction Stop
                if ([string]$rollbackReadback.SecureBoot -cne $priorSecureBoot) {
                    throw "prior firmware readback failed during recovery"
                }
                $priorFirmwareRestored = $true
            }
            Start-VM -Name $VMName -ErrorAction Stop | Out-Null
            $recoveryDeadlineUtc = [DateTime]::UtcNow.AddSeconds($VmStateTimeoutSeconds)
            [void](Wait-Win11DriverTestFirmwareVmState -ExpectedState "Running" `
                    -DeadlineUtc $recoveryDeadlineUtc)
            [void](Wait-Win11DriverTestFirmwareHostContact -DeadlineUtc $recoveryDeadlineUtc)
        }
        elseif ($firmwareChanged -and -not $startedAfterChange) {
            throw "firmware changed but exact VM is not Off for safe recovery"
        }
    }
    catch {
        $recoveryErrors.Add("firmware recovery failed")
    }

    [pscustomobject]@{
        schema = [int]1
        run_id = $runId
        status = "FAIL"
        started_utc = $startedUtc.ToString("o")
        completed_utc = [DateTime]::UtcNow.ToString("o")
        vm_name = $VMName
        expected_vm_id = $expectedVmIdCanonical
        desired_secure_boot = $DesiredSecureBoot
        firmware_changed = [bool]$firmwareChanged
        prior_firmware_restored = [bool]$priorFirmwareRestored
        recovery_error_count = [int]$recoveryErrors.Count
        failure_code = "driver_test_firmware_transition_failed"
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath `
        (Join-Path $artifactDirectory "summary.json") -Encoding UTF8 -ErrorAction Stop
    throw $failure
}
