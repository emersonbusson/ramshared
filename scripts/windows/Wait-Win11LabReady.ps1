#Requires -Version 5.1
<#
.SYNOPSIS
  Read-only acceptance gate for a newly provisioned disposable Win11 lab VM.

.DESCRIPTION
  This script observes only the named Hyper-V VM and its guest through the
  shared bounded PowerShell Direct helper. It never creates, starts, stops,
  reboots, checkpoints, installs, or removes anything.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$VMName,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$User,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$Password,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedComputerName,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedOsBuild,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedAdministratorIdentity,
    [Parameter(Mandatory = $true)]
    [datetime]$MinimumGuestBootUtc,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedSignerSubject,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedSignerThumbprint,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedSwitchName,
    [Parameter(Mandatory = $true)]
    [ValidateSet("RequireRoutableIPv4", "SealedOffline")]
    [string]$NetworkPolicy,
    [ValidateRange(15, 3600)]
    [int]$TotalTimeoutSeconds = 900,
    [ValidateRange(5, 600)]
    [int]$PerAttemptTimeoutSeconds = 120,
    [ValidateRange(1, 180)]
    [int]$PsDirectConnectTimeoutSeconds = 60,
    [ValidateRange(1, 30)]
    [int]$PollIntervalSeconds = 5
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$currentPrincipal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw [System.UnauthorizedAccessException]::new("Administrative privileges are required.")
}

if (-not (Get-Module -ListAvailable -Name Hyper-V)) {
    throw "Hyper-V PowerShell module is not available."
}

. (Join-Path $PSScriptRoot "Invoke-GuestPsDirectBounded.ps1")

function Normalize-Win11LabReadyThumbprint {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    $normalized = $Value.Trim().ToUpperInvariant()
    if ($Value -cne $Value.Trim() -or $normalized -notmatch '^[0-9A-F]{40}$') {
        throw "$Name must be one exact SHA-1 certificate thumbprint"
    }
    $normalized
}

function Normalize-Win11LabReadyIdentity {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if ([string]::IsNullOrWhiteSpace($Value) -or $Value -cne $Value.Trim()) {
        throw "$Name must be one exact non-empty identity value"
    }
    $Value
}

function Normalize-Win11LabReadyGuid {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if ([string]::IsNullOrWhiteSpace($Value) -or $Value -cne $Value.Trim() -or
        $Value -notmatch '(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$') {
        throw "$Name must be one canonical GUID"
    }
    $Value.ToUpperInvariant()
}

function Normalize-Win11LabReadyNetworkPolicy {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value
    )

    if ($Value -cnotin @("RequireRoutableIPv4", "SealedOffline")) {
        throw "NetworkPolicy must be RequireRoutableIPv4 or SealedOffline"
    }
    $Value
}

function Get-Win11LabReadyIntegrationComponentId {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if ([string]::IsNullOrWhiteSpace($Value) -or $Value -cne $Value.Trim()) {
        throw "readiness_host_integration_service_refused"
    }
    $suffix = [regex]::Match($Value,
        '(?i)(?<component>[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$')
    if (-not $suffix.Success) {
        throw "readiness_host_integration_service_refused"
    }
    try {
        Normalize-Win11LabReadyGuid -Value $suffix.Groups["component"].Value -Name $Name
    }
    catch {
        throw "readiness_host_integration_service_refused"
    }
}

function Get-Win11LabReadyExpectedIntegrationComponentIds {
    [CmdletBinding()]
    param()

    @(
        "6C09BB55-D683-4DA0-8931-C9BF705F6480",
        "84EAAE65-2F2E-45F5-9BB5-0E857DC8EB47",
        "2A34B1C2-FD73-4043-8A5B-DD2159BC743F",
        "9F8233AC-BE49-4C79-8EE3-E7E1985B2077",
        "2497F4DE-E9FA-4204-80E4-4B75C46419C0",
        "5CED1297-4598-4915-A5FC-AD21BB4D02A4"
    )
}

function Get-Win11LabReadyAttemptBudget {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [int]$RemainingSeconds,
        [Parameter(Mandatory = $true)]
        [int]$PerAttemptTimeoutSeconds,
        [Parameter(Mandatory = $true)]
        [int]$ConnectTimeoutSeconds
    )

    if ($RemainingSeconds -le $ConnectTimeoutSeconds) {
        throw "readiness_attempt_budget_exhausted"
    }
    $budget = [Math]::Min($RemainingSeconds, $PerAttemptTimeoutSeconds)
    if ($budget -le $ConnectTimeoutSeconds) {
        throw "readiness_attempt_budget_exhausted"
    }
    [int]$budget
}

function Get-Win11LabReadyProviderStageNames {
    [CmdletBinding()]
    param()

    @(
        "setup_state",
        "win32_operating_system",
        "windows_identity",
        "driver_store",
        "system_driver",
        "pnp_root",
        "disk",
        "pnp_disk",
        "network_ipv4",
        "root_certificates",
        "trusted_publisher_certificates",
        "verifier_query",
        "testsigning_query"
    )
}

function Get-Win11LabReadyAttemptStageCode {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$OutcomeCode
    )

    if ($OutcomeCode -ceq "host_preflight_refused") {
        return "host_assert"
    }
    if ($OutcomeCode -in @(
            "attempt_budget_exhausted",
            "psdirect_connect_or_outer_timeout",
            "provider_timeout")) {
        return "psdirect_connect_or_outer_timeout"
    }
    if ($OutcomeCode -in @(
            "child_result",
            "provider_child_result",
            "secret_echo_suppressed")) {
        return "child_result"
    }
    "guest_parse_assert"
}

function Test-Win11LabReadyTerminalAttemptOutcome {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$OutcomeCode
    )

    $OutcomeCode -in @(
        "provider_failure",
        "host_preflight_refused",
        "guest_preflight_refused",
        "secret_echo_suppressed",
        "attempt_budget_exhausted",
        "child_result",
        "provider_timeout",
        "provider_child_result",
        "provider_guest_parse_assertion")
}

function Get-Win11LabReadyAttemptFailureCode {
    [CmdletBinding()]
    param(
        [AllowNull()]
        [string]$ExceptionMessage,
        [Parameter(Mandatory = $true)]
        [string]$Password
    )

    if (-not [string]::IsNullOrWhiteSpace($Password) -and
        -not [string]::IsNullOrWhiteSpace($ExceptionMessage) -and
        $ExceptionMessage.IndexOf($Password, [StringComparison]::Ordinal) -ge 0) {
        return "secret_echo_suppressed"
    }
    if ([string]::IsNullOrWhiteSpace($ExceptionMessage)) {
        return "child_result"
    }
    if ($ExceptionMessage -ceq "readiness_attempt_budget_exhausted") {
        return "attempt_budget_exhausted"
    }
    if ($ExceptionMessage -cmatch '^readiness_provider_stage:(?:setup_state|win32_operating_system|windows_identity|driver_store|system_driver|pnp_root|disk|pnp_disk|network_ipv4|root_certificates|trusted_publisher_certificates|verifier_query|testsigning_query):(provider_timeout|provider_child_result|provider_guest_parse_assertion|attempt_budget_exhausted)$') {
        return $Matches[1]
    }
    if ($ExceptionMessage -cmatch '^readiness_host_(?:provider_failure|evidence_malformed|identity_or_cardinality_failed|integration_service_refused|network_adapter_refused)$') {
        return "host_preflight_refused"
    }
    if ($ExceptionMessage -ceq "readiness_provider_failure") {
        return "provider_failure"
    }
    if ($ExceptionMessage -cmatch '^readiness_(?:guest_identity_or_boot_failed|guest_data_malformed|guest_residue|guest_network_policy_violation)$') {
        return "guest_preflight_refused"
    }
    if ($ExceptionMessage -ceq "readiness_network_not_ready") {
        return "network_not_ready"
    }
    if ($ExceptionMessage -ceq "PowerShell Direct invoke outer deadline exceeded" -or
        $ExceptionMessage.StartsWith("PowerShell Direct invoke outer deadline exceeded;", [StringComparison]::Ordinal)) {
        return "psdirect_connect_or_outer_timeout"
    }
    if ($ExceptionMessage.StartsWith("PowerShell Direct invoke child failed exit=", [StringComparison]::Ordinal) -and
        $ExceptionMessage.IndexOf("PowerShell Direct unavailable after", [StringComparison]::Ordinal) -ge 0) {
        return "psdirect_connect_or_outer_timeout"
    }
    if ($ExceptionMessage -cmatch '^PowerShell Direct invoke child failed exit=[0-9]+:') {
        return "child_result"
    }
    "child_result"
}

function Get-Win11LabReadyProviderStageOutcomeCode {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$AttemptFailureCode
    )

    if ($AttemptFailureCode -ceq "attempt_budget_exhausted") {
        return "attempt_budget_exhausted"
    }
    if ($AttemptFailureCode -ceq "psdirect_connect_or_outer_timeout") {
        return "provider_timeout"
    }
    if ($AttemptFailureCode -in @("child_result", "secret_echo_suppressed")) {
        return "provider_child_result"
    }
    "provider_guest_parse_assertion"
}

function Assert-Win11LabReadyProviderStageReceipts {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Receipts,
        [Parameter(Mandatory = $true)]
        [int]$ExpectedAttempt,
        [switch]$RequireComplete
    )

    $expectedProviderCodes = @(Get-Win11LabReadyProviderStageNames)
    $allowedPropertyNames = @(
        "attempt", "provider_code", "stage_code", "outcome_code", "outer_timeout_seconds",
        "started_utc", "completed_utc", "duration_ms"
    )
    $allowedStageCodes = @(
        "host_assert", "psdirect_connect_or_outer_timeout", "child_result", "guest_parse_assert"
    )
    $observedProviderCodes = [Collections.Generic.List[string]]::new()
    foreach ($receipt in @($Receipts | Where-Object { $null -ne $_ })) {
        foreach ($propertyName in $allowedPropertyNames) {
            if ($null -eq $receipt.PSObject.Properties[$propertyName]) {
                throw "readiness provider stage receipt is missing $propertyName"
            }
        }
        foreach ($property in @($receipt.PSObject.Properties)) {
            if ($allowedPropertyNames -cnotcontains [string]$property.Name) {
                throw "readiness provider stage receipt contains an unsafe property"
            }
        }
        $providerCode = [string]$receipt.provider_code
        $stageCode = [string]$receipt.stage_code
        $outcomeCode = [string]$receipt.outcome_code
        if ([int]$receipt.attempt -ne $ExpectedAttempt -or
            $expectedProviderCodes -cnotcontains $providerCode -or
            $allowedStageCodes -cnotcontains $stageCode -or
            [int]$receipt.duration_ms -lt 0 -or
            [string]::IsNullOrWhiteSpace([string]$receipt.started_utc) -or
            [string]::IsNullOrWhiteSpace([string]$receipt.completed_utc)) {
            throw "readiness provider stage receipt is malformed"
        }
        if ($observedProviderCodes -ccontains $providerCode) {
            throw "readiness provider stage receipt is ambiguous"
        }
        $observedProviderCodes.Add($providerCode)
        if ($outcomeCode -ceq "completed") {
            if ($stageCode -cne "guest_parse_assert" -or [int]$receipt.outer_timeout_seconds -le 0) {
                throw "readiness completed provider stage receipt is malformed"
            }
            continue
        }
        if ($outcomeCode -ceq "provider_timeout") {
            if ($stageCode -cne "psdirect_connect_or_outer_timeout" -or [int]$receipt.outer_timeout_seconds -le 0) {
                throw "readiness provider timeout receipt is malformed"
            }
            continue
        }
        if ($outcomeCode -ceq "attempt_budget_exhausted") {
            if ($stageCode -cne "psdirect_connect_or_outer_timeout" -or [int]$receipt.outer_timeout_seconds -ne 0) {
                throw "readiness provider budget receipt is malformed"
            }
            continue
        }
        if ($outcomeCode -ceq "provider_child_result") {
            if ($stageCode -cne "child_result" -or [int]$receipt.outer_timeout_seconds -le 0) {
                throw "readiness provider child receipt is malformed"
            }
            continue
        }
        if ($outcomeCode -ceq "provider_guest_parse_assertion") {
            if ($stageCode -cne "guest_parse_assert" -or [int]$receipt.outer_timeout_seconds -le 0) {
                throw "readiness provider parse receipt is malformed"
            }
            continue
        }
        throw "readiness provider stage outcome is invalid"
    }
    if ($observedProviderCodes.Count -eq 0) {
        throw "readiness provider stage receipt is empty"
    }
    if ($RequireComplete) {
        if ($observedProviderCodes.Count -ne $expectedProviderCodes.Count) {
            throw "readiness provider stages are incomplete"
        }
        foreach ($providerCode in $expectedProviderCodes) {
            $matched = @($Receipts | Where-Object { [string]$_.provider_code -ceq $providerCode })
            if ($matched.Count -ne 1 -or [string]$matched[0].outcome_code -cne "completed" -or
                [string]$matched[0].stage_code -cne "guest_parse_assert") {
                throw "readiness provider stages are incomplete or failed"
            }
        }
    }
    $true
}

function Get-Win11LabReadyHostEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$VMName
    )

    $vms = @(Get-VM -Name $VMName -ErrorAction Stop)
    if ($vms.Count -ne 1) {
        throw "readiness host VM query did not return exactly one named VM"
    }
    $vmId = Normalize-Win11LabReadyGuid -Value ([string]$vms[0].Id) -Name "VM.Id"
    $integrationServices = @(Get-VMIntegrationService -VM $vms[0] -ErrorAction Stop)
    $networkAdapters = @(Get-VMNetworkAdapter -VMName $VMName -ErrorAction Stop)
    [pscustomobject]@{
        schema = [int]1
        observation_utc = [DateTime]::UtcNow.ToString("o")
        vm_name = [string]$vms[0].Name
        vm_id = $vmId
        vm_state = [string]$vms[0].State
        integration_service_count = [int]$integrationServices.Count
        integration_services = @($integrationServices | ForEach-Object {
                [pscustomobject]@{
                    vm_id = $vmId
                    id = [string]$_.Id
                    component_id = Get-Win11LabReadyIntegrationComponentId -Value ([string]$_.Id) -Name "integration-service Id"
                    name = [string]$_.Name
                    enabled = if ($_.Enabled -is [bool]) { [bool]$_.Enabled } else { $null }
                    primary_status_description = [string]$_.PrimaryStatusDescription
                }
            })
        network_adapter_count = [int]$networkAdapters.Count
        network_adapters = @($networkAdapters | ForEach-Object {
                [pscustomobject]@{
                    switch_name = [string]$_.SwitchName
                    connected = if ($_.Connected -is [bool]) { [bool]$_.Connected } else { $null }
                    status = if ($null -eq $_.Status) {
                        ""
                    }
                    else {
                        ([string]$_.Status).Trim().ToUpperInvariant()
                    }
                }
            })
    }
}

function Assert-Win11LabReadyHostEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedVMName,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSwitchName
    )

    foreach ($propertyName in @(
            "schema", "vm_name", "vm_id", "vm_state", "integration_service_count", "integration_services",
            "network_adapter_count", "network_adapters")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "readiness_host_evidence_malformed"
        }
    }
    $expectedComponentIds = @(Get-Win11LabReadyExpectedIntegrationComponentIds)
    $services = @($Evidence.integration_services | Where-Object { $null -ne $_ })
    $networkAdapters = @($Evidence.network_adapters | Where-Object { $null -ne $_ })
    $expectedSwitch = Normalize-Win11LabReadyIdentity $ExpectedSwitchName "expected switch name"
    try {
        $vmId = Normalize-Win11LabReadyGuid -Value ([string]$Evidence.vm_id) -Name "readiness host VM ID"
    }
    catch {
        throw "readiness_host_evidence_malformed"
    }
    if ([int]$Evidence.schema -ne 1 -or [string]$Evidence.vm_name -cne $ExpectedVMName -or
        [string]$Evidence.vm_state -cne "Running" -or [int]$Evidence.integration_service_count -ne 6 -or
        $services.Count -ne 6 -or [int]$Evidence.network_adapter_count -ne 1 -or
        $networkAdapters.Count -ne 1) {
        throw "readiness_host_identity_or_cardinality_failed"
    }
    $observedComponentIds = [Collections.Generic.List[string]]::new()
    foreach ($service in $services) {
        foreach ($propertyName in @("vm_id", "id", "component_id", "enabled", "primary_status_description")) {
            if ($null -eq $service.PSObject.Properties[$propertyName]) {
                throw "readiness_host_integration_service_refused"
            }
        }
        try {
            $serviceVmId = Normalize-Win11LabReadyGuid -Value ([string]$service.vm_id) -Name "integration-service VM ID"
            $componentId = Normalize-Win11LabReadyGuid -Value ([string]$service.component_id) -Name "integration-service component ID"
            $componentSuffix = Get-Win11LabReadyIntegrationComponentId -Value ([string]$service.id) -Name "integration-service Id"
        }
        catch {
            throw "readiness_host_integration_service_refused"
        }
        if ($serviceVmId -cne $vmId -or $componentSuffix -cne $componentId -or
            ($service.enabled -isnot [bool]) -or ([bool]$service.enabled -ne $true) -or
            [string]$service.primary_status_description -cne "OK") {
            throw "readiness_host_integration_service_refused"
        }
        $observedComponentIds.Add($componentId)
    }
    if (@($observedComponentIds | Select-Object -Unique).Count -ne $expectedComponentIds.Count) {
        throw "readiness_host_integration_service_refused"
    }
    foreach ($expectedComponentId in $expectedComponentIds) {
        if (@($observedComponentIds | Where-Object { $_ -ceq $expectedComponentId }).Count -ne 1) {
            throw "readiness_host_integration_service_refused"
        }
    }
    $networkAdapter = $networkAdapters[0]
    if ($null -eq $networkAdapter.PSObject.Properties["switch_name"] -or
        $null -eq $networkAdapter.PSObject.Properties["connected"] -or
        $null -eq $networkAdapter.PSObject.Properties["status"] -or
        [string]$networkAdapter.switch_name -cne $expectedSwitch -or
        ($networkAdapter.connected -isnot [bool]) -or ([bool]$networkAdapter.connected -ne $true) -or
        [string]$networkAdapter.status -cne "OK") {
        throw "readiness_host_network_adapter_refused"
    }
    $true
}

function Assert-Win11LabReadyGuestEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedComputerName,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedOsBuild,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedAdministratorIdentity,
        [Parameter(Mandatory = $true)]
        [datetime]$MinimumGuestBootUtc,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSignerSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSignerThumbprint,
        [Parameter(Mandatory = $true)]
        [string]$NetworkPolicy
    )

    foreach ($propertyName in @(
            "schema", "image_state", "system_setup_in_progress", "oobe_in_progress",
            "setup_phase", "setup_type", "computer_name", "os_build", "identity_name", "identity_sid",
            "is_administrator", "last_boot_utc", "provider_error_count", "provider_errors",
            "network_ipv4_provider_error_count", "routable_ipv4_count",
            "package_count", "service_count", "root_count", "ramshared_disk_count",
            "ramshared_pnp_disk_count", "verifier_query_exit_code", "verifier_target_count",
            "verifier_all_drivers", "testsigning_query_exit_code", "testsigning_enabled",
            "root_expected_thumbprint_count", "trusted_publisher_expected_thumbprint_count",
            "root_foreign_subject_count", "trusted_publisher_foreign_subject_count")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "readiness_guest_data_malformed"
        }
    }
    $expectedComputer = Normalize-Win11LabReadyIdentity $ExpectedComputerName "expected computer name"
    $expectedBuild = Normalize-Win11LabReadyIdentity $ExpectedOsBuild "expected OS build"
    $expectedAdmin = Normalize-Win11LabReadyIdentity $ExpectedAdministratorIdentity "expected administrator identity"
    $expectedSubject = Normalize-Win11LabReadyIdentity $ExpectedSignerSubject "expected signer subject"
    $expectedThumbprint = Normalize-Win11LabReadyThumbprint $ExpectedSignerThumbprint "expected signer thumbprint"
    $expectedNetworkPolicy = Normalize-Win11LabReadyNetworkPolicy $NetworkPolicy
    try {
        $guestBoot = [DateTimeOffset]::Parse([string]$Evidence.last_boot_utc,
            [Globalization.CultureInfo]::InvariantCulture,
            [Globalization.DateTimeStyles]::RoundtripKind).ToUniversalTime()
        $minimumBoot = ([DateTimeOffset]$MinimumGuestBootUtc).ToUniversalTime()
    }
    catch {
        throw "readiness_guest_identity_or_boot_failed"
    }
    $providerErrors = @($Evidence.provider_errors | Where-Object { $null -ne $_ })
    try {
        $schema = [int]$Evidence.schema
        $systemSetupInProgress = [int]$Evidence.system_setup_in_progress
        $oobeInProgress = [int]$Evidence.oobe_in_progress
        $setupPhase = [int]$Evidence.setup_phase
        $setupType = [int]$Evidence.setup_type
        $providerErrorCount = [int]$Evidence.provider_error_count
        $networkIpv4ProviderErrorCount = [int]$Evidence.network_ipv4_provider_error_count
        $routableIpv4Count = [int]$Evidence.routable_ipv4_count
        $packageCount = [int]$Evidence.package_count
        $serviceCount = [int]$Evidence.service_count
        $rootCount = [int]$Evidence.root_count
        $ramsharedDiskCount = [int]$Evidence.ramshared_disk_count
        $ramsharedPnpDiskCount = [int]$Evidence.ramshared_pnp_disk_count
        $verifierQueryExitCode = [int]$Evidence.verifier_query_exit_code
        $verifierTargetCount = [int]$Evidence.verifier_target_count
        $testSigningQueryExitCode = [int]$Evidence.testsigning_query_exit_code
        $rootExpectedThumbprintCount = [int]$Evidence.root_expected_thumbprint_count
        $trustedPublisherExpectedThumbprintCount = [int]$Evidence.trusted_publisher_expected_thumbprint_count
        $rootForeignSubjectCount = [int]$Evidence.root_foreign_subject_count
        $trustedPublisherForeignSubjectCount = [int]$Evidence.trusted_publisher_foreign_subject_count
    }
    catch {
        throw "readiness_guest_data_malformed"
    }
    if ($schema -ne 1 -or [string]$Evidence.image_state -cne "IMAGE_STATE_COMPLETE" -or
        $systemSetupInProgress -ne 0 -or $oobeInProgress -ne 0 -or
        $setupPhase -ne 0 -or $setupType -ne 0 -or
        [string]$Evidence.computer_name -cne $expectedComputer -or
        [string]$Evidence.os_build -cne $expectedBuild -or
        [string]$Evidence.identity_name -cne $expectedAdmin -or
        [string]$Evidence.identity_sid -notmatch '^S-1-' -or
        ($Evidence.is_administrator -isnot [bool]) -or ([bool]$Evidence.is_administrator -ne $true) -or
        $guestBoot -le $minimumBoot) {
        throw "readiness_guest_identity_or_boot_failed"
    }
    if ($providerErrorCount -ne 0 -or $providerErrors.Count -ne 0 -or
        $networkIpv4ProviderErrorCount -ne 0) {
        throw "readiness_provider_failure"
    }
    if ($packageCount -ne 0 -or $serviceCount -ne 0 -or $rootCount -ne 0 -or
        $ramsharedDiskCount -ne 0 -or $ramsharedPnpDiskCount -ne 0 -or
        $verifierQueryExitCode -ne 0 -or $verifierTargetCount -ne 0 -or
        ($Evidence.verifier_all_drivers -isnot [bool]) -or ([bool]$Evidence.verifier_all_drivers -ne $false) -or
        $testSigningQueryExitCode -ne 0 -or
        ($Evidence.testsigning_enabled -isnot [bool]) -or ([bool]$Evidence.testsigning_enabled -ne $false) -or
        $rootExpectedThumbprintCount -ne 0 -or
        $trustedPublisherExpectedThumbprintCount -ne 0 -or
        $rootForeignSubjectCount -ne 0 -or
        $trustedPublisherForeignSubjectCount -ne 0) {
        throw "readiness_guest_residue"
    }
    if ($expectedNetworkPolicy -ceq "RequireRoutableIPv4" -and $routableIpv4Count -lt 1) {
        throw "readiness_network_not_ready"
    }
    if ($expectedNetworkPolicy -ceq "SealedOffline" -and $routableIpv4Count -ne 0) {
        throw "readiness_guest_network_policy_violation"
    }
    if ($expectedSubject.Length -eq 0 -or $expectedThumbprint.Length -ne 40) {
        throw "readiness expected public signer identity is malformed"
    }
    $true
}

function Assert-Win11LabReadySuccessReceipt {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Receipt,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedVMName,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedComputerName,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedOsBuild,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedAdministratorIdentity,
        [Parameter(Mandatory = $true)]
        [datetime]$MinimumGuestBootUtc,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSignerSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSignerThumbprint,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSwitchName,
        [Parameter(Mandatory = $true)]
        [string]$NetworkPolicy
    )

    foreach ($propertyName in @(
            "schema", "status", "vm_name", "expected_switch_name", "network_policy",
            "attempt_count", "total_duration_ms",
            "before", "after", "attempts")) {
        if ($null -eq $Receipt.PSObject.Properties[$propertyName]) {
            throw "readiness receipt is missing $propertyName"
        }
    }
    $attempts = @($Receipt.attempts)
    if ([int]$Receipt.schema -ne 1 -or [string]$Receipt.status -cne "READY" -or
        [string]$Receipt.vm_name -cne $ExpectedVMName -or
        [string]$Receipt.expected_switch_name -cne $ExpectedSwitchName -or
        [string]$Receipt.network_policy -cne (Normalize-Win11LabReadyNetworkPolicy $NetworkPolicy) -or
        [int]$Receipt.attempt_count -lt 1 -or
        [int]$Receipt.total_duration_ms -lt 0 -or $attempts.Count -ne [int]$Receipt.attempt_count -or
        $null -eq $Receipt.before -or $null -eq $Receipt.after -or
        $null -eq $Receipt.after.PSObject.Properties["host"] -or
        $null -eq $Receipt.after.PSObject.Properties["guest"] -or
        $null -eq $Receipt.after.PSObject.Properties["provider_stages"]) {
        throw "readiness receipt is stale, malformed, or not ready"
    }
    foreach ($attempt in $attempts) {
        $allowedAttemptPropertyNames = @(
            "attempt", "started_utc", "completed_utc", "duration_ms", "stage_code",
            "outcome_code", "outer_timeout_seconds"
        )
        foreach ($propertyName in $allowedAttemptPropertyNames) {
            if ($null -eq $attempt.PSObject.Properties[$propertyName]) {
                throw "readiness attempt receipt is missing $propertyName"
            }
        }
        foreach ($property in @($attempt.PSObject.Properties)) {
            if ($allowedAttemptPropertyNames -cnotcontains [string]$property.Name) {
                throw "readiness attempt receipt contains an unsafe property"
            }
        }
        if ([int]$attempt.attempt -lt 1 -or [int]$attempt.duration_ms -lt 0 -or
            [int]$attempt.outer_timeout_seconds -lt 0 -or
            [string]$attempt.stage_code -cnotin @(
                "host_assert", "psdirect_connect_or_outer_timeout", "child_result", "guest_parse_assert") -or
            [string]::IsNullOrWhiteSpace([string]$attempt.outcome_code)) {
            throw "readiness attempt receipt is malformed"
        }
    }
    $lastAttempt = $attempts[$attempts.Count - 1]
    if ([string]$lastAttempt.outcome_code -cne "ready" -or
        [string]$lastAttempt.stage_code -cne "guest_parse_assert" -or
        [int]$lastAttempt.outer_timeout_seconds -le 0) {
        throw "readiness receipt did not end in an exact ready attempt"
    }
    $providerStages = @($Receipt.after.provider_stages | Where-Object { $null -ne $_ })
    if ($providerStages.Count -eq 0) {
        throw "readiness receipt has no provider stage receipts"
    }
    $attemptNumbers = @($attempts | ForEach-Object { [int]$_.attempt })
    $providerStageAttemptNumbers = @($providerStages | ForEach-Object { [int]$_.attempt } | Select-Object -Unique)
    foreach ($providerStageAttemptNumber in $providerStageAttemptNumbers) {
        if ($attemptNumbers -notcontains $providerStageAttemptNumber) {
            throw "readiness provider stage receipt has no matching logical attempt"
        }
        $stagesForAttempt = @($providerStages | Where-Object {
                [int]$_.attempt -eq $providerStageAttemptNumber
            })
        Assert-Win11LabReadyProviderStageReceipts -Receipts $stagesForAttempt `
            -ExpectedAttempt $providerStageAttemptNumber `
            -RequireComplete:($providerStageAttemptNumber -eq [int]$lastAttempt.attempt) | Out-Null
    }
    Assert-Win11LabReadyHostEvidence -Evidence $Receipt.after.host -ExpectedVMName $ExpectedVMName `
        -ExpectedSwitchName $ExpectedSwitchName | Out-Null
    Assert-Win11LabReadyGuestEvidence -Evidence $Receipt.after.guest `
        -ExpectedComputerName $ExpectedComputerName -ExpectedOsBuild $ExpectedOsBuild `
        -ExpectedAdministratorIdentity $ExpectedAdministratorIdentity -MinimumGuestBootUtc $MinimumGuestBootUtc `
        -ExpectedSignerSubject $ExpectedSignerSubject -ExpectedSignerThumbprint $ExpectedSignerThumbprint `
        -NetworkPolicy $NetworkPolicy | Out-Null
    $true
}

function ConvertFrom-Win11LabReadyProviderStageRows {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Rows,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedProviderCode
    )

    $nonEmptyRows = @($Rows | ForEach-Object { [string]$_ } | Where-Object {
            -not [string]::IsNullOrWhiteSpace($_)
        })
    if ($nonEmptyRows.Count -ne 1) {
        throw "readiness_guest_data_malformed"
    }
    try {
        $result = $nonEmptyRows[0] | ConvertFrom-Json -ErrorAction Stop
        $schema = [int]$result.schema
    }
    catch {
        throw "readiness_guest_data_malformed"
    }
    if ($null -eq $result -or $schema -ne 1 -or
        [string]$result.provider_code -cne $ExpectedProviderCode) {
        throw "readiness_guest_data_malformed"
    }
    $result
}

function Assert-Win11LabReadySetupState {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][object]$Evidence)

    foreach ($propertyName in @(
            "schema", "provider_code", "image_state", "system_setup_in_progress",
            "oobe_in_progress", "setup_phase", "setup_type")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "readiness_guest_setup_incomplete"
        }
    }
    try {
        $schema = [int]$Evidence.schema
        $systemSetupInProgress = [int]$Evidence.system_setup_in_progress
        $oobeInProgress = [int]$Evidence.oobe_in_progress
        $setupPhase = [int]$Evidence.setup_phase
        $setupType = [int]$Evidence.setup_type
    }
    catch {
        throw "readiness_guest_setup_incomplete"
    }
    if ($schema -ne 1 -or [string]$Evidence.provider_code -cne "setup_state" -or
        [string]$Evidence.image_state -cne "IMAGE_STATE_COMPLETE" -or
        $systemSetupInProgress -ne 0 -or $oobeInProgress -ne 0 -or
        $setupPhase -ne 0 -or $setupType -ne 0) {
        throw "readiness_guest_setup_incomplete"
    }
    $true
}

function Invoke-Win11LabReadyGuestProviderStage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$VMName,
        [Parameter(Mandatory = $true)]
        [string]$User,
        [Parameter(Mandatory = $true)]
        [string]$Password,
        [Parameter(Mandatory = $true)]
        [string]$ProviderCode,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint,
        [Parameter(Mandatory = $true)]
        [datetime]$DeadlineUtc,
        [Parameter(Mandatory = $true)]
        [int]$PerAttemptTimeoutSeconds,
        [Parameter(Mandatory = $true)]
        [int]$ConnectTimeoutSeconds,
        [Parameter(Mandatory = $true)]
        [int]$LogicalAttempt,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [System.Collections.Generic.List[object]]$StageReceipts
    )

    if (@(Get-Win11LabReadyProviderStageNames) -cnotcontains $ProviderCode) {
        throw "readiness_guest_data_malformed"
    }
    $stageStartedUtc = [DateTime]::UtcNow
    $stageOuterTimeoutSeconds = [int]0
    try {
        $remainingSeconds = [int][Math]::Floor(($DeadlineUtc - [DateTime]::UtcNow).TotalSeconds)
        $stageOuterTimeoutSeconds = Get-Win11LabReadyAttemptBudget -RemainingSeconds $remainingSeconds `
            -PerAttemptTimeoutSeconds $PerAttemptTimeoutSeconds -ConnectTimeoutSeconds $ConnectTimeoutSeconds
        $stageRows = Invoke-GuestPsDirectBounded -VMName $VMName -User $User -Password $Password `
            -Operation invoke -TimeoutSeconds $stageOuterTimeoutSeconds `
            -ConnectTimeoutSeconds $ConnectTimeoutSeconds -ScriptBlock {
            param($RemoteProviderCode, $RemoteExpectedSubject, $RemoteExpectedThumbprint)

            $ErrorActionPreference = "Stop"
            switch ($RemoteProviderCode) {
                "setup_state" {
                    $imageState = Get-ItemProperty -LiteralPath `
                        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup\State" `
                        -ErrorAction Stop
                    $systemSetup = Get-ItemProperty -LiteralPath "HKLM:\SYSTEM\Setup" `
                        -ErrorAction Stop
                    if ($null -eq $imageState.PSObject.Properties["ImageState"] -or
                        $imageState.ImageState -isnot [string]) {
                        throw "readiness setup image state is missing or malformed"
                    }
                    foreach ($propertyName in @(
                            "SystemSetupInProgress", "OOBEInProgress", "SetupPhase", "SetupType")) {
                        if ($null -eq $systemSetup.PSObject.Properties[$propertyName] -or
                            $systemSetup.$propertyName -isnot [int]) {
                            throw "readiness setup integer state is missing or malformed"
                        }
                    }
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        image_state = [string]$imageState.ImageState
                        system_setup_in_progress = [int]$systemSetup.SystemSetupInProgress
                        oobe_in_progress = [int]$systemSetup.OOBEInProgress
                        setup_phase = [int]$systemSetup.SetupPhase
                        setup_type = [int]$systemSetup.SetupType
                    }
                }
                "win32_operating_system" {
                    $rows = @(Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop | Where-Object { $null -ne $_ })
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        row_count = [int]$rows.Count
                        computer_name = if ($rows.Count -eq 1) { [string]$rows[0].CSName } else { "" }
                        os_build = if ($rows.Count -eq 1) { [string]$rows[0].BuildNumber } else { "" }
                        last_boot_utc = if ($rows.Count -eq 1) { $rows[0].LastBootUpTime.ToUniversalTime().ToString("o") } else { "" }
                    }
                }
                "windows_identity" {
                    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
                    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        name = [string]$identity.Name
                        sid = [string]$identity.User.Value
                        is_administrator = [bool]$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
                    }
                }
                "driver_store" {
                    $rows = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                            $null -ne $_ -and [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
                        })
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        package_count = [int]$rows.Count
                    }
                }
                "system_driver" {
                    $rows = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop | Where-Object { $null -ne $_ })
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        service_count = [int]$rows.Count
                    }
                }
                "pnp_root" {
                    $rows = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                            $null -ne $_ -and $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
                        })
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        root_count = [int]$rows.Count
                    }
                }
                "disk" {
                    $rows = @(Get-Disk -ErrorAction Stop | Where-Object {
                            $null -ne $_ -and ($_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                                $_.SerialNumber -match '(?i)ramshare|ramshared')
                        })
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        ramshared_disk_count = [int]$rows.Count
                    }
                }
                "pnp_disk" {
                    $rows = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                            $null -ne $_ -and ($_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                                $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK')
                        })
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        ramshared_pnp_disk_count = [int]$rows.Count
                    }
                }
                "network_ipv4" {
                    function Test-Win11LabReadyRoutableIpv4 {
                        param([AllowEmptyString()][string]$Value)

                        $match = [regex]::Match($Value,
                            '^(?<first>25[0-5]|2[0-4][0-9]|1?[0-9]{1,2})\.(?<second>25[0-5]|2[0-4][0-9]|1?[0-9]{1,2})\.(?<third>25[0-5]|2[0-4][0-9]|1?[0-9]{1,2})\.(?<fourth>25[0-5]|2[0-4][0-9]|1?[0-9]{1,2})$')
                        if (-not $match.Success) {
                            return $false
                        }
                        $first = [int]$match.Groups["first"].Value
                        $second = [int]$match.Groups["second"].Value
                        if ($first -eq 0 -or $first -eq 127 -or
                            ($first -eq 169 -and $second -eq 254) -or $first -ge 224) {
                            return $false
                        }
                        $true
                    }
                    $rows = @(Get-NetIPAddress -AddressFamily IPv4 -ErrorAction Stop | Where-Object { $null -ne $_ })
                    $routableRows = @($rows | Where-Object {
                            Test-Win11LabReadyRoutableIpv4 -Value ([string]$_.IPAddress)
                        })
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        routable_ipv4_count = [int]$routableRows.Count
                    }
                }
                "root_certificates" {
                    $certificates = @(Get-ChildItem -LiteralPath "Cert:\LocalMachine\Root" -ErrorAction Stop | Where-Object { $null -ne $_ })
                    $canonicalThumbprint = ($RemoteExpectedThumbprint -replace '\s', '').ToUpperInvariant()
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        expected_thumbprint_count = [int](@($certificates | Where-Object {
                                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $canonicalThumbprint
                            }).Count)
                        foreign_subject_count = [int](@($certificates | Where-Object {
                                [string]$_.Subject -ceq $RemoteExpectedSubject -and
                                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -cne $canonicalThumbprint
                            }).Count)
                    }
                }
                "trusted_publisher_certificates" {
                    $certificates = @(Get-ChildItem -LiteralPath "Cert:\LocalMachine\TrustedPublisher" -ErrorAction Stop | Where-Object { $null -ne $_ })
                    $canonicalThumbprint = ($RemoteExpectedThumbprint -replace '\s', '').ToUpperInvariant()
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        expected_thumbprint_count = [int](@($certificates | Where-Object {
                                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $canonicalThumbprint
                            }).Count)
                        foreign_subject_count = [int](@($certificates | Where-Object {
                                [string]$_.Subject -ceq $RemoteExpectedSubject -and
                                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -cne $canonicalThumbprint
                            }).Count)
                    }
                }
                "verifier_query" {
                    $output = & verifier /query 2>&1 | Out-String
                    $exitCode = [int]$LASTEXITCODE
                    $targets = @($output -split [Environment]::NewLine | Where-Object { $_ -match '(?i)\.sys\b' })
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        exit_code = $exitCode
                        target_count = [int]$targets.Count
                        all_drivers = [bool]($output -match '(?i)\ball drivers\b')
                    }
                }
                "testsigning_query" {
                    $output = & bcdedit.exe /enum "{current}" 2>&1 | Out-String
                    $exitCode = [int]$LASTEXITCODE
                    $stageResult = [pscustomobject]@{
                        schema = [int]1
                        provider_code = $RemoteProviderCode
                        exit_code = $exitCode
                        enabled = [bool]($output -match '(?im)^\s*testsigning\s+(yes|on|true|1)\s*$')
                    }
                }
                default {
                    throw "readiness guest provider code is invalid"
                }
            }
            $stageResult | ConvertTo-Json -Depth 6 -Compress
        } -ArgumentList @($ProviderCode, $ExpectedSubject, $ExpectedThumbprint)
        $stageResult = ConvertFrom-Win11LabReadyProviderStageRows -Rows $stageRows `
            -ExpectedProviderCode $ProviderCode
        if ($ProviderCode -ceq "setup_state") {
            Assert-Win11LabReadySetupState -Evidence $stageResult | Out-Null
        }
        $stageCompletedUtc = [DateTime]::UtcNow
        [void]$StageReceipts.Add([pscustomobject]@{
                attempt = [int]$LogicalAttempt
                provider_code = $ProviderCode
                stage_code = "guest_parse_assert"
                outcome_code = "completed"
                outer_timeout_seconds = [int]$stageOuterTimeoutSeconds
                started_utc = $stageStartedUtc.ToString("o")
                completed_utc = $stageCompletedUtc.ToString("o")
                duration_ms = [int][Math]::Max(0, [Math]::Round(($stageCompletedUtc - $stageStartedUtc).TotalMilliseconds))
            })
        return $stageResult
    }
    catch {
        $attemptFailureCode = Get-Win11LabReadyAttemptFailureCode -ExceptionMessage $_.Exception.Message -Password $Password
        $stageOutcomeCode = Get-Win11LabReadyProviderStageOutcomeCode -AttemptFailureCode $attemptFailureCode
        $stageCompletedUtc = [DateTime]::UtcNow
        [void]$StageReceipts.Add([pscustomobject]@{
                attempt = [int]$LogicalAttempt
                provider_code = $ProviderCode
                stage_code = Get-Win11LabReadyAttemptStageCode -OutcomeCode $stageOutcomeCode
                outcome_code = $stageOutcomeCode
                outer_timeout_seconds = [int]$stageOuterTimeoutSeconds
                started_utc = $stageStartedUtc.ToString("o")
                completed_utc = $stageCompletedUtc.ToString("o")
                duration_ms = [int][Math]::Max(0, [Math]::Round(($stageCompletedUtc - $stageStartedUtc).TotalMilliseconds))
            })
        throw ("readiness_provider_stage:{0}:{1}" -f $ProviderCode, $stageOutcomeCode)
    }
}

function ConvertTo-Win11LabReadyGuestEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$StageResults
    )

    $expectedProviderCodes = @(Get-Win11LabReadyProviderStageNames)
    $resultsByProvider = @{}
    foreach ($stageResult in @($StageResults | Where-Object { $null -ne $_ })) {
        try {
            $schema = [int]$stageResult.schema
            $providerCode = [string]$stageResult.provider_code
        }
        catch {
            throw "readiness_guest_data_malformed"
        }
        if ($schema -ne 1 -or $expectedProviderCodes -cnotcontains $providerCode -or
            $resultsByProvider.ContainsKey($providerCode)) {
            throw "readiness_guest_data_malformed"
        }
        $resultsByProvider[$providerCode] = $stageResult
    }
    if ($resultsByProvider.Count -ne $expectedProviderCodes.Count) {
        throw "readiness_guest_data_malformed"
    }
    foreach ($providerCode in $expectedProviderCodes) {
        if (-not $resultsByProvider.ContainsKey($providerCode)) {
            throw "readiness_guest_data_malformed"
        }
    }
    try {
        $setupState = $resultsByProvider["setup_state"]
        $operatingSystem = $resultsByProvider["win32_operating_system"]
        $identity = $resultsByProvider["windows_identity"]
        $driverStore = $resultsByProvider["driver_store"]
        $systemDriver = $resultsByProvider["system_driver"]
        $pnpRoot = $resultsByProvider["pnp_root"]
        $disk = $resultsByProvider["disk"]
        $pnpDisk = $resultsByProvider["pnp_disk"]
        $networkIpv4 = $resultsByProvider["network_ipv4"]
        $rootCertificates = $resultsByProvider["root_certificates"]
        $trustedPublisherCertificates = $resultsByProvider["trusted_publisher_certificates"]
        $verifier = $resultsByProvider["verifier_query"]
        $testSigning = $resultsByProvider["testsigning_query"]
        $providerErrors = [Collections.Generic.List[string]]::new()
        if ([int]$verifier.exit_code -ne 0) { $providerErrors.Add("verifier_query") }
        if ([int]$testSigning.exit_code -ne 0) { $providerErrors.Add("testsigning_query") }
        [pscustomobject]@{
            schema = [int]1
            image_state = [string]$setupState.image_state
            system_setup_in_progress = [int]$setupState.system_setup_in_progress
            oobe_in_progress = [int]$setupState.oobe_in_progress
            setup_phase = [int]$setupState.setup_phase
            setup_type = [int]$setupState.setup_type
            computer_name = if ([int]$operatingSystem.row_count -eq 1) { [string]$operatingSystem.computer_name } else { "" }
            os_build = if ([int]$operatingSystem.row_count -eq 1) { [string]$operatingSystem.os_build } else { "" }
            identity_name = [string]$identity.name
            identity_sid = [string]$identity.sid
            is_administrator = [bool]$identity.is_administrator
            last_boot_utc = if ([int]$operatingSystem.row_count -eq 1) { [string]$operatingSystem.last_boot_utc } else { "" }
            provider_error_count = [int]$providerErrors.Count
            provider_errors = @($providerErrors.ToArray())
            network_ipv4_provider_error_count = [int]0
            routable_ipv4_count = [int]$networkIpv4.routable_ipv4_count
            package_count = [int]$driverStore.package_count
            service_count = [int]$systemDriver.service_count
            root_count = [int]$pnpRoot.root_count
            ramshared_disk_count = [int]$disk.ramshared_disk_count
            ramshared_pnp_disk_count = [int]$pnpDisk.ramshared_pnp_disk_count
            verifier_query_exit_code = [int]$verifier.exit_code
            verifier_target_count = [int]$verifier.target_count
            verifier_all_drivers = [bool]$verifier.all_drivers
            testsigning_query_exit_code = [int]$testSigning.exit_code
            testsigning_enabled = [bool]$testSigning.enabled
            root_expected_thumbprint_count = [int]$rootCertificates.expected_thumbprint_count
            trusted_publisher_expected_thumbprint_count = [int]$trustedPublisherCertificates.expected_thumbprint_count
            root_foreign_subject_count = [int]$rootCertificates.foreign_subject_count
            trusted_publisher_foreign_subject_count = [int]$trustedPublisherCertificates.foreign_subject_count
        }
    }
    catch {
        throw "readiness_guest_data_malformed"
    }
}

function Invoke-Win11LabReadyGuestProviderStages {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$VMName,
        [Parameter(Mandatory = $true)]
        [string]$User,
        [Parameter(Mandatory = $true)]
        [string]$Password,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint,
        [Parameter(Mandatory = $true)]
        [datetime]$DeadlineUtc,
        [Parameter(Mandatory = $true)]
        [int]$PerAttemptTimeoutSeconds,
        [Parameter(Mandatory = $true)]
        [int]$ConnectTimeoutSeconds,
        [Parameter(Mandatory = $true)]
        [int]$LogicalAttempt,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [System.Collections.Generic.List[object]]$StageReceipts
    )

    $stageResults = [Collections.Generic.List[object]]::new()
    foreach ($providerCode in @(Get-Win11LabReadyProviderStageNames)) {
        [void]$stageResults.Add((Invoke-Win11LabReadyGuestProviderStage -VMName $VMName -User $User `
                -Password $Password -ProviderCode $providerCode -ExpectedSubject $ExpectedSubject `
                -ExpectedThumbprint $ExpectedThumbprint -DeadlineUtc $DeadlineUtc `
                -PerAttemptTimeoutSeconds $PerAttemptTimeoutSeconds -ConnectTimeoutSeconds $ConnectTimeoutSeconds `
                -LogicalAttempt $LogicalAttempt -StageReceipts $StageReceipts))
    }
    ConvertTo-Win11LabReadyGuestEvidence -StageResults @($stageResults)
}

if ($PerAttemptTimeoutSeconds -le $PsDirectConnectTimeoutSeconds) {
    throw "readiness per-attempt deadline must exceed its PowerShell Direct connect deadline"
}
if ($MinimumGuestBootUtc.Kind -ne [DateTimeKind]::Utc) {
    throw "MinimumGuestBootUtc must be an explicit UTC timestamp"
}

$expectedComputer = Normalize-Win11LabReadyIdentity $ExpectedComputerName "ExpectedComputerName"
$expectedBuild = Normalize-Win11LabReadyIdentity $ExpectedOsBuild "ExpectedOsBuild"
$expectedAdmin = Normalize-Win11LabReadyIdentity $ExpectedAdministratorIdentity "ExpectedAdministratorIdentity"
$expectedSubject = Normalize-Win11LabReadyIdentity $ExpectedSignerSubject "ExpectedSignerSubject"
$expectedThumbprint = Normalize-Win11LabReadyThumbprint $ExpectedSignerThumbprint "ExpectedSignerThumbprint"
$expectedSwitch = Normalize-Win11LabReadyIdentity $ExpectedSwitchName "ExpectedSwitchName"
$expectedNetworkPolicy = Normalize-Win11LabReadyNetworkPolicy $NetworkPolicy
$minimumBoot = ([DateTimeOffset]$MinimumGuestBootUtc).ToUniversalTime()
$startedUtc = [DateTime]::UtcNow
$deadlineUtc = $startedUtc.AddSeconds($TotalTimeoutSeconds)
$attempts = [Collections.Generic.List[object]]::new()
$providerStages = [Collections.Generic.List[object]]::new()
$before = $null
$after = $null
$status = "TIMEOUT"
$terminalCode = "total_deadline_exceeded"
$hostBeforeProviderFailed = $false

try {
    $before = Get-Win11LabReadyHostEvidence -VMName $VMName
}
catch {
    $before = [pscustomobject]@{
        schema = [int]1
        observation_utc = [DateTime]::UtcNow.ToString("o")
        vm_name = $VMName
        host_observation_available = $false
        failure_code = "host_preflight_refused"
    }
    $status = "REFUSED"
    $terminalCode = "host_preflight_refused"
    $hostBeforeProviderFailed = $true
}

$attemptNumber = 0
while (-not $hostBeforeProviderFailed -and [DateTime]::UtcNow -lt $deadlineUtc) {
    $attemptNumber++
    $attemptStartedUtc = [DateTime]::UtcNow
    $outcomeCode = "child_result"
    $stageCode = "host_assert"
    $attemptOuterTimeoutSeconds = [int]0
    $fatalFailure = $false
    $hostEvidence = $null
    $guestEvidence = $null
    try {
        try {
            $hostEvidence = Get-Win11LabReadyHostEvidence -VMName $VMName
        }
        catch {
            throw "readiness_host_provider_failure"
        }
        Assert-Win11LabReadyHostEvidence -Evidence $hostEvidence -ExpectedVMName $VMName `
            -ExpectedSwitchName $expectedSwitch | Out-Null
        $guestEvidence = Invoke-Win11LabReadyGuestProviderStages -VMName $VMName -User $User `
            -Password $Password -ExpectedSubject $expectedSubject -ExpectedThumbprint $expectedThumbprint `
            -DeadlineUtc $deadlineUtc -PerAttemptTimeoutSeconds $PerAttemptTimeoutSeconds `
            -ConnectTimeoutSeconds $PsDirectConnectTimeoutSeconds -LogicalAttempt $attemptNumber `
            -StageReceipts $providerStages
        Assert-Win11LabReadyGuestEvidence -Evidence $guestEvidence `
            -ExpectedComputerName $expectedComputer -ExpectedOsBuild $expectedBuild `
            -ExpectedAdministratorIdentity $expectedAdmin -MinimumGuestBootUtc $minimumBoot.UtcDateTime `
            -ExpectedSignerSubject $expectedSubject -ExpectedSignerThumbprint $expectedThumbprint `
            -NetworkPolicy $expectedNetworkPolicy | Out-Null
        $after = [pscustomobject]@{
            schema = [int]1
            ready = $true
            observation_utc = [DateTime]::UtcNow.ToString("o")
            host = $hostEvidence
            guest = $guestEvidence
            provider_stages = @($providerStages)
        }
        $status = "READY"
        $terminalCode = "ready"
        $outcomeCode = "ready"
        $stageCode = "guest_parse_assert"
    }
    catch {
        $outcomeCode = Get-Win11LabReadyAttemptFailureCode -ExceptionMessage $_.Exception.Message -Password $Password
        $stageCode = Get-Win11LabReadyAttemptStageCode -OutcomeCode $outcomeCode
        $providerFailure = [regex]::Match($_.Exception.Message,
            '^readiness_provider_stage:(?<provider>setup_state|win32_operating_system|windows_identity|driver_store|system_driver|pnp_root|disk|pnp_disk|network_ipv4|root_certificates|trusted_publisher_certificates|verifier_query|testsigning_query):')
        if ($providerFailure.Success) {
            $terminalCode = ("{0}_{1}" -f $outcomeCode, $providerFailure.Groups["provider"].Value)
        }
        else {
            $terminalCode = $outcomeCode
        }
        if (Test-Win11LabReadyTerminalAttemptOutcome -OutcomeCode $outcomeCode) {
            $status = "REFUSED"
            $fatalFailure = $true
        }
        $after = [pscustomobject]@{
            schema = [int]1
            ready = $false
            observation_utc = [DateTime]::UtcNow.ToString("o")
            host = $hostEvidence
            guest = $guestEvidence
            provider_stages = @($providerStages)
        }
    }
    $currentAttemptProviderStages = @($providerStages | Where-Object {
            [int]$_.attempt -eq $attemptNumber
        })
    if ($currentAttemptProviderStages.Count -gt 0) {
        $attemptOuterTimeoutSeconds = [int](($currentAttemptProviderStages |
                    Measure-Object -Property outer_timeout_seconds -Maximum).Maximum)
    }
    $attemptCompletedUtc = [DateTime]::UtcNow
    [void]$attempts.Add([pscustomobject]@{
            attempt = [int]$attemptNumber
            started_utc = $attemptStartedUtc.ToString("o")
            completed_utc = $attemptCompletedUtc.ToString("o")
            duration_ms = [int][Math]::Max(0, [Math]::Round(($attemptCompletedUtc - $attemptStartedUtc).TotalMilliseconds))
            stage_code = $stageCode
            outcome_code = $outcomeCode
            outer_timeout_seconds = [int]$attemptOuterTimeoutSeconds
        })
    if ($status -ceq "READY") {
        break
    }
    if ($fatalFailure) {
        break
    }
    $remainingAfterAttempt = [int][Math]::Floor(($deadlineUtc - [DateTime]::UtcNow).TotalSeconds)
    if ($remainingAfterAttempt -le 0) {
        break
    }
    Start-Sleep -Seconds ([Math]::Min($PollIntervalSeconds, $remainingAfterAttempt))
}

if ($null -eq $after) {
    $after = [pscustomobject]@{
        schema = [int]1
        ready = $false
        observation_utc = [DateTime]::UtcNow.ToString("o")
        host = $null
        guest = $null
        provider_stages = @($providerStages)
    }
}

$completedUtc = [DateTime]::UtcNow
$receipt = [pscustomobject]@{
    schema = [int]1
    status = $status
    terminal_code = $terminalCode
    vm_name = $VMName
    expected_switch_name = $expectedSwitch
    network_policy = $expectedNetworkPolicy
    started_utc = $startedUtc.ToString("o")
    completed_utc = $completedUtc.ToString("o")
    total_duration_ms = [int][Math]::Max(0, [Math]::Round(($completedUtc - $startedUtc).TotalMilliseconds))
    attempt_count = [int]$attempts.Count
    before = $before
    after = $after
    attempts = @($attempts)
}
Write-Output ($receipt | ConvertTo-Json -Depth 12 -Compress)

if ($status -cne "READY") {
    throw ("win11_lab_ready_" + $terminalCode)
}

Assert-Win11LabReadySuccessReceipt -Receipt $receipt -ExpectedVMName $VMName `
    -ExpectedComputerName $expectedComputer -ExpectedOsBuild $expectedBuild `
    -ExpectedAdministratorIdentity $expectedAdmin -MinimumGuestBootUtc $minimumBoot.UtcDateTime `
    -ExpectedSignerSubject $expectedSubject -ExpectedSignerThumbprint $expectedThumbprint `
    -ExpectedSwitchName $expectedSwitch -NetworkPolicy $expectedNetworkPolicy | Out-Null
Write-Output ("STATUS=READY vm={0} attempts={1}" -f $VMName, $attempts.Count)
