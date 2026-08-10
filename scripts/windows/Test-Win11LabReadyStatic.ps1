#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath = "",
    [string]$HelperPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path $PSScriptRoot "Wait-Win11LabReady.ps1"
}
if ([string]::IsNullOrWhiteSpace($HelperPath)) {
    $HelperPath = Join-Path $PSScriptRoot "Invoke-GuestPsDirectBounded.ps1"
}

function Get-ParsedAst {
    param([Parameter(Mandatory = $true)][string]$Path)

    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile(
        $Path, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "win11_lab_ready_parser_is_green failed: parser errors in $Path"
    }
    $ast
}

function Import-ProductionFunction {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Source
    )

    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput(
        $Source, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "win11_lab_ready_parser_is_green failed: parser errors in production source"
    }
    $definition = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $Name
        }, $true)
    if ($null -eq $definition) {
        throw "win11_lab_ready_static_contract_is_green failed: missing production function $Name"
    }
    $body = $definition.Body.Extent.Text.Trim()
    if ($body.Length -lt 2 -or $body[0] -ne "{" -or $body[$body.Length - 1] -ne "}") {
        throw "win11_lab_ready_parser_is_green failed: malformed function $Name"
    }
    Set-Item -Path ("Function:\script:{0}" -f $Name) -Value (
        [scriptblock]::Create($body.Substring(1, $body.Length - 2)))
}

function Assert-Throws {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [Parameter(Mandatory = $true)][string]$Name
    )

    try {
        & $Action
    }
    catch {
        return
    }
    throw "$Name failed: expected refusal was accepted"
}

if (-not (Test-Path -LiteralPath $HarnessPath -PathType Leaf)) {
    throw "win11_lab_ready_static_contract_is_green failed: readiness harness is missing"
}
if (-not (Test-Path -LiteralPath $HelperPath -PathType Leaf)) {
    throw "win11_lab_ready_static_contract_is_green failed: shared bounded PowerShell Direct helper is missing"
}

$text = Get-Content -LiteralPath $HarnessPath -Raw
$helperText = Get-Content -LiteralPath $HelperPath -Raw
$ast = Get-ParsedAst -Path $HarnessPath
$violations = [Collections.Generic.List[string]]::new()

$parameterNames = @($ast.ParamBlock.Parameters | ForEach-Object {
        $_.Name.VariablePath.UserPath
    })
foreach ($requiredParameter in @(
        "VMName", "User", "Password", "ExpectedComputerName", "ExpectedOsBuild",
        "ExpectedAdministratorIdentity", "MinimumGuestBootUtc", "ExpectedSignerSubject",
        "ExpectedSignerThumbprint", "ExpectedSwitchName", "NetworkPolicy",
        "TotalTimeoutSeconds", "PerAttemptTimeoutSeconds", "PsDirectConnectTimeoutSeconds")) {
    if ($parameterNames -notcontains $requiredParameter) {
        $violations.Add("missing parameter $requiredParameter")
    }
}

foreach ($functionName in @(
        "Normalize-Win11LabReadyThumbprint",
        "Normalize-Win11LabReadyIdentity",
        "Normalize-Win11LabReadyGuid",
        "Normalize-Win11LabReadyNetworkPolicy",
        "Get-Win11LabReadyIntegrationComponentId",
        "Get-Win11LabReadyExpectedIntegrationComponentIds",
        "Get-Win11LabReadyAttemptBudget",
        "Get-Win11LabReadyAttemptFailureCode",
        "Get-Win11LabReadyAttemptStageCode",
        "Test-Win11LabReadyTerminalAttemptOutcome",
        "Get-Win11LabReadyProviderStageNames",
        "Get-Win11LabReadyProviderStageOutcomeCode",
        "Assert-Win11LabReadySetupState",
        "Assert-Win11LabReadyProviderStageReceipts",
        "ConvertFrom-Win11LabReadyProviderStageRows",
        "ConvertTo-Win11LabReadyGuestEvidence",
        "Invoke-Win11LabReadyGuestProviderStage",
        "Invoke-Win11LabReadyGuestProviderStages",
        "Assert-Win11LabReadyHostEvidence",
        "Assert-Win11LabReadyGuestEvidence",
        "Assert-Win11LabReadySuccessReceipt",
        "Get-Win11LabReadyHostEvidence")) {
    if (-not $ast.Find({
                param($node)
                $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq $functionName
            }, $true)) {
        $violations.Add("missing function $functionName")
    }
}

foreach ($forbiddenPattern in @(
        '(?i)\bStart-VM\b',
        '(?i)\bStop-VM\b',
        '(?i)\bRestart-VM\b',
        '(?i)\bSet-VM\b',
        '(?i)\bNew-VM\b',
        '(?i)\bRemove-VM\b',
        '(?i)\bCheckpoint-VM\b',
        '(?i)\bEnable-VMIntegrationService\b',
        '(?i)\bNew-PSSession\b',
        '(?i)\bInvoke-Command\b',
        '(?i)\bRemove-PSSession\b',
        '(?i)\bbcdedit(?:\.exe)?\s+/set\b',
        '(?i)\bverifier\s+/reset\b',
        '(?i)\bpnputil(?:\.exe)?\b',
        '(?i)\bsc\.exe\b',
        '(?i)\bshutdown\.exe\b',
        '(?i)\bStart-Job\b',
        '(?i)\bStop-Job\b')) {
    if ($text -match $forbiddenPattern) {
        $violations.Add("read-only readiness harness contains forbidden mutation pattern $forbiddenPattern")
    }
}

foreach ($requiredNeedle in @(
        "Invoke-GuestPsDirectBounded.ps1",
        "Invoke-GuestPsDirectBounded",
        "Get-VMIntegrationService",
        "Get-VMNetworkAdapter",
        "vm_id",
        "component_id",
        "6C09BB55-D683-4DA0-8931-C9BF705F6480",
        "84EAAE65-2F2E-45F5-9BB5-0E857DC8EB47",
        "2A34B1C2-FD73-4043-8A5B-DD2159BC743F",
        "9F8233AC-BE49-4C79-8EE3-E7E1985B2077",
        "2497F4DE-E9FA-4204-80E4-4B75C46419C0",
        "5CED1297-4598-4915-A5FC-AD21BB4D02A4",
        "WindowsIdentity",
        "Win32_OperatingSystem",
        "Get-WindowsDriver",
        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup\State",
        "HKLM:\SYSTEM\Setup",
        "IMAGE_STATE_COMPLETE",
        "Win32_SystemDriver",
        "Get-PnpDevice",
        "Get-Disk",
        "Cert:\LocalMachine\Root",
        "Cert:\LocalMachine\TrustedPublisher",
        "Get-NetIPAddress",
        "switch_name",
        "network_adapter_count",
        "routable_ipv4_count",
        "RequireRoutableIPv4",
        "SealedOffline",
        "verifier /query",
        "bcdedit.exe /enum",
        "provider_errors",
        "stage_code",
        "outer_timeout_seconds",
        "readiness_attempt_budget_exhausted",
        "PowerShell Direct invoke outer deadline exceeded",
        "PowerShell Direct invoke child failed exit=",
        "provider_stages",
        "win32_operating_system",
        "windows_identity",
        "driver_store",
        "system_driver",
        "pnp_root",
        "network_ipv4",
        "root_certificates",
        "trusted_publisher_certificates",
        "verifier_query",
        "testsigning_query",
        "before",
        "after",
        "attempts")) {
    if ($text -notmatch [regex]::Escape($requiredNeedle)) {
        $violations.Add("missing readiness evidence guard $requiredNeedle")
    }
}

$boundedCalls = @($ast.FindAll({
            param($node)
            $node -is [Management.Automation.Language.CommandAst] -and
            $node.GetCommandName() -eq "Invoke-GuestPsDirectBounded"
        }, $true)).Count
if ($boundedCalls -ne 1) {
    $violations.Add("readiness must use exactly one shared bounded PowerShell Direct call site, observed $boundedCalls")
}
if ($text -match '(?im)^.*(?:Write-(?:Output|Host|Verbose|Warning|Error)|ConvertTo-Json|Export-[A-Za-z]+).*\$Password') {
    $violations.Add("password is routed to an output, artifact, or diagnostic sink")
}
if ($helperText -match 'payload\.password|Arguments\s*=.*Password') {
    $violations.Add("shared bounded helper exposes passwords in worker payload or command arguments")
}

$orderedNeedles = @(
    '$before = Get-Win11LabReadyHostEvidence',
    '$hostEvidence = Get-Win11LabReadyHostEvidence',
    'Assert-Win11LabReadyHostEvidence -Evidence $hostEvidence',
    '$guestEvidence = Invoke-Win11LabReadyGuestProviderStages',
    '$after = [pscustomobject]@{',
    '$receipt = [pscustomobject]@{'
)
$lastIndex = -1
foreach ($needle in $orderedNeedles) {
    $needleIndex = $text.IndexOf($needle, [StringComparison]::Ordinal)
    if ($needleIndex -lt 0 -or $needleIndex -le $lastIndex) {
        $violations.Add("readiness before/guest/after receipt order is missing or unsafe at $needle")
        break
    }
    $lastIndex = $needleIndex
}

$terminalOutcomeNeedle = 'if (Test-Win11LabReadyTerminalAttemptOutcome -OutcomeCode $outcomeCode)'
if ($text.IndexOf($terminalOutcomeNeedle, [StringComparison]::Ordinal) -lt 0 -or
    $text.IndexOf('"host_preflight_refused"', [StringComparison]::Ordinal) -lt 0 -or
    $text.IndexOf('"attempt_budget_exhausted"', [StringComparison]::Ordinal) -lt 0) {
    $violations.Add("readiness host assertion or exhausted-attempt outcome is not terminal before a later PowerShell Direct retry")
}
if ($text -match '(?m)^\s*function\s+Invoke-Win11LabReadyProvider\s*\{') {
    $violations.Add("readiness retains a grouped guest-provider worker")
}
$providerStageCalls = @($ast.FindAll({
            param($node)
            $node -is [Management.Automation.Language.CommandAst] -and
            $node.GetCommandName() -eq "Invoke-Win11LabReadyGuestProviderStage"
        }, $true)).Count
if ($providerStageCalls -ne 1 -or
    $text.IndexOf('foreach ($providerCode in @(Get-Win11LabReadyProviderStageNames))', [StringComparison]::Ordinal) -lt 0) {
    $violations.Add("readiness does not invoke each fixed guest provider stage independently")
}

if ($violations.Count -ne 0) {
    throw ("win11_lab_ready_static_contract_is_green failed: " + ($violations -join "; "))
}
Write-Output "PASS win11_lab_ready_readonly_contract_is_exact"

foreach ($functionName in @(
        "Normalize-Win11LabReadyThumbprint",
        "Normalize-Win11LabReadyIdentity",
        "Normalize-Win11LabReadyGuid",
        "Normalize-Win11LabReadyNetworkPolicy",
        "Get-Win11LabReadyIntegrationComponentId",
        "Get-Win11LabReadyExpectedIntegrationComponentIds",
        "Get-Win11LabReadyAttemptBudget",
        "Get-Win11LabReadyAttemptFailureCode",
        "Get-Win11LabReadyAttemptStageCode",
        "Test-Win11LabReadyTerminalAttemptOutcome",
        "Get-Win11LabReadyProviderStageNames",
        "Get-Win11LabReadyProviderStageOutcomeCode",
        "Assert-Win11LabReadyProviderStageReceipts",
        "ConvertFrom-Win11LabReadyProviderStageRows",
        "Assert-Win11LabReadySetupState",
        "ConvertTo-Win11LabReadyGuestEvidence",
        "Invoke-Win11LabReadyGuestProviderStage",
        "Invoke-Win11LabReadyGuestProviderStages",
        "Assert-Win11LabReadyHostEvidence",
        "Assert-Win11LabReadyGuestEvidence",
        "Assert-Win11LabReadySuccessReceipt")) {
    Import-ProductionFunction -Name $functionName -Source $text
}

$expectedVmName = "win11-verifier-clean"
$expectedComputerName = "WIN11-VERIFIER-CLEAN"
$expectedOsBuild = "26200"
$expectedAdministratorIdentity = "WIN11-VERIFIER-CLEAN\ramsharedlab"
$expectedSignerSubject = "CN=RamShared Guest Verifier Static Test"
$expectedSignerThumbprint = ("A" * 40)
$expectedSwitchName = "Manufactured Exact Switch"
$requiredNetworkPolicy = "RequireRoutableIPv4"
$sealedOfflineNetworkPolicy = "SealedOffline"
$minimumBootUtc = [datetime]"2026-08-09T12:00:00.0000000Z"
$expectedVmId = "11111111-2222-3333-4444-555555555555"
$expectedIntegrationComponentIds = @(
    "6C09BB55-D683-4DA0-8931-C9BF705F6480",
    "84EAAE65-2F2E-45F5-9BB5-0E857DC8EB47",
    "2A34B1C2-FD73-4043-8A5B-DD2159BC743F",
    "9F8233AC-BE49-4C79-8EE3-E7E1985B2077",
    "2497F4DE-E9FA-4204-80E4-4B75C46419C0",
    "5CED1297-4598-4915-A5FC-AD21BB4D02A4"
)
$ptBrIntegrationNames = @(
    "Interface de Serviço de Convidado",
    "Pulsação",
    "Troca de Pares Chave-Valor",
    "Desligamento",
    "Sincronização de Horário",
    "Serviço de Cópias de Sombra"
)
$validIntegrationServices = @(
    for ($index = 0; $index -lt $expectedIntegrationComponentIds.Count; $index++) {
        $componentId = $expectedIntegrationComponentIds[$index]
        [pscustomobject]@{
            vm_id = $expectedVmId
            id = ("Microsoft:{0}\\{1}" -f $expectedVmId, $componentId)
            component_id = $componentId
            name = $ptBrIntegrationNames[$index]
            enabled = $true
            primary_status_description = "OK"
        }
    }
)
$validHost = [pscustomobject]@{
    schema = [int]1
    vm_name = $expectedVmName
    vm_id = $expectedVmId
    vm_state = "Running"
    integration_service_count = [int]6
    integration_services = $validIntegrationServices
    network_adapter_count = [int]1
    network_adapters = @(
        [pscustomobject]@{
            switch_name = $expectedSwitchName
            connected = $true
            status = "OK"
        }
    )
}
$validGuest = [pscustomobject]@{
    schema = [int]1
    image_state = "IMAGE_STATE_COMPLETE"
    system_setup_in_progress = [int]0
    oobe_in_progress = [int]0
    setup_phase = [int]0
    setup_type = [int]0
    computer_name = $expectedComputerName
    os_build = $expectedOsBuild
    identity_name = $expectedAdministratorIdentity
    identity_sid = "S-1-5-21-1000"
    is_administrator = $true
    last_boot_utc = "2026-08-09T12:00:01.0000000Z"
    provider_error_count = [int]0
    provider_errors = @()
    network_ipv4_provider_error_count = [int]0
    routable_ipv4_count = [int]1
    package_count = [int]0
    service_count = [int]0
    root_count = [int]0
    ramshared_disk_count = [int]0
    ramshared_pnp_disk_count = [int]0
    verifier_query_exit_code = [int]0
    verifier_target_count = [int]0
    verifier_all_drivers = $false
    testsigning_query_exit_code = [int]0
    testsigning_enabled = $false
    root_expected_thumbprint_count = [int]0
    trusted_publisher_expected_thumbprint_count = [int]0
    root_foreign_subject_count = [int]0
    trusted_publisher_foreign_subject_count = [int]0
}
$expectedProviderStageNames = @(
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
$actualProviderStageNames = @(Get-Win11LabReadyProviderStageNames)
if (($actualProviderStageNames -join "|") -cne ($expectedProviderStageNames -join "|")) {
    throw "win11_lab_ready_provider_stages_are_exact failed: provider stages are missing, reordered, or foreign"
}
$validProviderStages = @(
    for ($index = 0; $index -lt $expectedProviderStageNames.Count; $index++) {
        [pscustomobject]@{
            attempt = [int]1
            provider_code = $expectedProviderStageNames[$index]
            stage_code = "guest_parse_assert"
            outcome_code = "completed"
            outer_timeout_seconds = [int]90
            started_utc = "2026-08-09T12:00:00.0000000Z"
            completed_utc = "2026-08-09T12:00:00.2500000Z"
            duration_ms = [int]250
        }
    }
)
Assert-Win11LabReadyProviderStageReceipts -Receipts $validProviderStages -ExpectedAttempt 1 -RequireComplete | Out-Null
Write-Output "PASS win11_lab_ready_provider_stages_are_exact"

Assert-Win11LabReadyHostEvidence -Evidence $validHost -ExpectedVMName $expectedVmName `
    -ExpectedSwitchName $expectedSwitchName | Out-Null
Write-Output "PASS win11_lab_ready_ptbr_integration_services_are_accepted_by_id"
Assert-Win11LabReadyGuestEvidence -Evidence $validGuest `
    -ExpectedComputerName $expectedComputerName -ExpectedOsBuild $expectedOsBuild `
    -ExpectedAdministratorIdentity $expectedAdministratorIdentity -MinimumGuestBootUtc $minimumBootUtc `
    -ExpectedSignerSubject $expectedSignerSubject -ExpectedSignerThumbprint $expectedSignerThumbprint `
    -NetworkPolicy $requiredNetworkPolicy | Out-Null
$validReceipt = [pscustomobject]@{
    schema = [int]1
    status = "READY"
    vm_name = $expectedVmName
    expected_switch_name = $expectedSwitchName
    network_policy = $requiredNetworkPolicy
    attempt_count = [int]1
    total_duration_ms = [int]250
    before = $validHost
    after = [pscustomobject]@{
        host = $validHost
        guest = $validGuest
        provider_stages = $validProviderStages
    }
    attempts = @(
        [pscustomobject]@{
            attempt = [int]1
            started_utc = "2026-08-09T12:00:00.0000000Z"
            completed_utc = "2026-08-09T12:00:00.2500000Z"
            duration_ms = [int]250
            stage_code = "guest_parse_assert"
            outcome_code = "ready"
            outer_timeout_seconds = [int]90
        }
    )
}
Assert-Win11LabReadySuccessReceipt -Receipt $validReceipt -ExpectedVMName $expectedVmName `
    -ExpectedComputerName $expectedComputerName -ExpectedOsBuild $expectedOsBuild `
    -ExpectedAdministratorIdentity $expectedAdministratorIdentity -MinimumGuestBootUtc $minimumBootUtc `
    -ExpectedSignerSubject $expectedSignerSubject -ExpectedSignerThumbprint $expectedSignerThumbprint `
    -ExpectedSwitchName $expectedSwitchName -NetworkPolicy $requiredNetworkPolicy | Out-Null
Write-Output "PASS win11_lab_ready_positive_receipt_is_exact"

$incompleteSetupGuest = $validGuest.psobject.Copy()
$incompleteSetupGuest.image_state = "IMAGE_STATE_SPECIALIZE_RESEAL_TO_OOBE"
Assert-Throws -Name "win11_lab_ready_setup_state_is_complete_before_other_providers" -Action {
    Assert-Win11LabReadyGuestEvidence -Evidence $incompleteSetupGuest `
        -ExpectedComputerName $expectedComputerName -ExpectedOsBuild $expectedOsBuild `
        -ExpectedAdministratorIdentity $expectedAdministratorIdentity -MinimumGuestBootUtc $minimumBootUtc `
        -ExpectedSignerSubject $expectedSignerSubject -ExpectedSignerThumbprint $expectedSignerThumbprint `
        -NetworkPolicy $requiredNetworkPolicy | Out-Null
}
Write-Output "PASS win11_lab_ready_setup_state_is_complete_before_other_providers"

Assert-Win11LabReadySetupState -Evidence ([pscustomobject]@{
        schema = [int]1
        provider_code = "setup_state"
        image_state = "IMAGE_STATE_COMPLETE"
        system_setup_in_progress = [int]0
        oobe_in_progress = [int]0
        setup_phase = [int]0
        setup_type = [int]0
    }) | Out-Null
Assert-Throws -Name "win11_lab_ready_setup_state_is_complete_before_other_providers" -Action {
    Assert-Win11LabReadySetupState -Evidence ([pscustomobject]@{
            schema = [int]1
            provider_code = "setup_state"
            image_state = "IMAGE_STATE_UNDEPLOYABLE"
            system_setup_in_progress = [int]0
            oobe_in_progress = [int]1
            setup_phase = [int]4
            setup_type = [int]2
        }) | Out-Null
}

function script:Invoke-GuestPsDirectBounded {
    [CmdletBinding()]
    param(
        [string]$VMName,
        [string]$User,
        [string]$Password,
        [string]$Operation,
        [int]$TimeoutSeconds,
        [int]$ConnectTimeoutSeconds,
        [scriptblock]$ScriptBlock,
        [object[]]$ArgumentList
    )

    $manufacturedProviderCode = [string]$ArgumentList[0]
    if ($manufacturedProviderCode -ceq "setup_state") {
        return @([pscustomobject]@{
                schema = [int]1
                provider_code = $manufacturedProviderCode
                image_state = "IMAGE_STATE_COMPLETE"
                system_setup_in_progress = [int]0
                oobe_in_progress = [int]0
                setup_phase = [int]0
                setup_type = [int]0
            } | ConvertTo-Json -Compress)
    }
    if ($manufacturedProviderCode -ceq "driver_store") {
        throw "PowerShell Direct invoke outer deadline exceeded; process_tree_terminated=True; stderr=opaque"
    }
    @([pscustomobject]@{
            schema = [int]1
            provider_code = $manufacturedProviderCode
        } | ConvertTo-Json -Compress)
}

$manufacturedStageReceipts = [Collections.Generic.List[object]]::new()
$manufacturedStageException = ""
try {
    Invoke-Win11LabReadyGuestProviderStages -VMName $expectedVmName -User "manufactured-user" `
        -Password "manufactured-ready-password" -ExpectedSubject $expectedSignerSubject `
        -ExpectedThumbprint $expectedSignerThumbprint -DeadlineUtc ([DateTime]::UtcNow.AddSeconds(120)) `
        -PerAttemptTimeoutSeconds 90 -ConnectTimeoutSeconds 60 -LogicalAttempt 1 `
        -StageReceipts $manufacturedStageReceipts | Out-Null
}
catch {
    $manufacturedStageException = $_.Exception.Message
}
if ([string]::IsNullOrWhiteSpace($manufacturedStageException)) {
    throw "win11_lab_ready_hanging_provider_is_terminal failed: manufactured provider timeout was accepted"
}
$partialProviderStages = @($manufacturedStageReceipts)
if ($partialProviderStages.Count -ne 4 -or
    $partialProviderStages[0].provider_code -cne "setup_state" -or
    $partialProviderStages[0].outcome_code -cne "completed" -or
    $partialProviderStages[1].provider_code -cne "win32_operating_system" -or
    $partialProviderStages[1].outcome_code -cne "completed" -or
    $partialProviderStages[2].provider_code -cne "windows_identity" -or
    $partialProviderStages[2].outcome_code -cne "completed" -or
    $partialProviderStages[3].provider_code -cne "driver_store" -or
    $partialProviderStages[3].stage_code -cne "psdirect_connect_or_outer_timeout" -or
    $partialProviderStages[3].outcome_code -cne "provider_timeout" -or
    -not (Test-Win11LabReadyTerminalAttemptOutcome -OutcomeCode $partialProviderStages[3].outcome_code)) {
    $observedManufacturedStages = @($partialProviderStages | ForEach-Object {
            ([string]$_.provider_code + "/" + [string]$_.stage_code + "/" + [string]$_.outcome_code)
        }) -join ","
    throw ("win11_lab_ready_hanging_provider_is_terminal failed: provider timeout was not exact and terminal; observed=" +
        $observedManufacturedStages)
}
Assert-Win11LabReadyProviderStageReceipts -Receipts $partialProviderStages -ExpectedAttempt 1 | Out-Null
$partialProviderStagesJson = $partialProviderStages | ConvertTo-Json -Depth 6 -Compress
if ($partialProviderStagesJson.Contains("PowerShell Direct invoke outer deadline exceeded") -or
    $partialProviderStagesJson.Contains("manufactured-ready-password")) {
    throw "win11_lab_ready_partial_provider_stage_receipt_never_echoes_raw_failure_or_password failed: raw helper data escaped"
}
Assert-Throws -Name "win11_lab_ready_hanging_provider_is_terminal" -Action {
    Assert-Win11LabReadyProviderStageReceipts -Receipts $partialProviderStages -ExpectedAttempt 1 -RequireComplete | Out-Null
}
$unsafePartialProviderStage = $partialProviderStages[3].psobject.Copy()
$unsafePartialProviderStage | Add-Member -NotePropertyName "exception_message" -NotePropertyValue "opaque"
Assert-Throws -Name "win11_lab_ready_partial_provider_stage_receipt_never_echoes_raw_failure_or_password" -Action {
    Assert-Win11LabReadyProviderStageReceipts -Receipts @($unsafePartialProviderStage) -ExpectedAttempt 1 | Out-Null
}
Write-Output "PASS win11_lab_ready_hanging_provider_is_terminal"
Write-Output "PASS win11_lab_ready_partial_provider_stage_receipt_never_echoes_raw_failure_or_password"

$completedRetryProviderStages = @($validProviderStages | ForEach-Object {
        $retryStage = $_.psobject.Copy()
        $retryStage.attempt = [int]2
        $retryStage
    })
$retryReceipt = [pscustomobject]@{
    schema = [int]1
    status = "READY"
    vm_name = $expectedVmName
    expected_switch_name = $expectedSwitchName
    network_policy = $requiredNetworkPolicy
    attempt_count = [int]2
    total_duration_ms = [int]90500
    before = $validHost
    after = [pscustomobject]@{
        host = $validHost
        guest = $validGuest
        provider_stages = @($partialProviderStages + $completedRetryProviderStages)
    }
    attempts = @(
        [pscustomobject]@{
            attempt = [int]1
            started_utc = "2026-08-09T12:00:00.0000000Z"
            completed_utc = "2026-08-09T12:01:30.0000000Z"
            duration_ms = [int]90000
            stage_code = "psdirect_connect_or_outer_timeout"
            outcome_code = "provider_timeout"
            outer_timeout_seconds = [int]90
        },
        [pscustomobject]@{
            attempt = [int]2
            started_utc = "2026-08-09T12:01:35.0000000Z"
            completed_utc = "2026-08-09T12:01:35.2500000Z"
            duration_ms = [int]250
            stage_code = "guest_parse_assert"
            outcome_code = "ready"
            outer_timeout_seconds = [int]90
        }
    )
}
Assert-Win11LabReadySuccessReceipt -Receipt $retryReceipt -ExpectedVMName $expectedVmName `
    -ExpectedComputerName $expectedComputerName -ExpectedOsBuild $expectedOsBuild `
    -ExpectedAdministratorIdentity $expectedAdministratorIdentity -MinimumGuestBootUtc $minimumBootUtc `
    -ExpectedSignerSubject $expectedSignerSubject -ExpectedSignerThumbprint $expectedSignerThumbprint `
    -ExpectedSwitchName $expectedSwitchName -NetworkPolicy $requiredNetworkPolicy | Out-Null
Write-Output "PASS win11_lab_ready_completed_provider_stages_persist_across_retry"

$wrongIdentity = $validGuest.psobject.Copy()
$wrongIdentity.computer_name = "FOREIGN-GUEST"
Assert-Throws -Name "win11_lab_ready_unexpected_identity_is_refused" -Action {
    Assert-Win11LabReadyGuestEvidence -Evidence $wrongIdentity `
        -ExpectedComputerName $expectedComputerName -ExpectedOsBuild $expectedOsBuild `
        -ExpectedAdministratorIdentity $expectedAdministratorIdentity -MinimumGuestBootUtc $minimumBootUtc `
        -ExpectedSignerSubject $expectedSignerSubject -ExpectedSignerThumbprint $expectedSignerThumbprint `
        -NetworkPolicy $requiredNetworkPolicy | Out-Null
}
Write-Output "PASS win11_lab_ready_unexpected_identity_is_refused"

$providerFailure = $validGuest.psobject.Copy()
$providerFailure.provider_errors = @("Get-Disk")
Assert-Throws -Name "win11_lab_ready_provider_failure_is_red" -Action {
    Assert-Win11LabReadyGuestEvidence -Evidence $providerFailure `
        -ExpectedComputerName $expectedComputerName -ExpectedOsBuild $expectedOsBuild `
        -ExpectedAdministratorIdentity $expectedAdministratorIdentity -MinimumGuestBootUtc $minimumBootUtc `
        -ExpectedSignerSubject $expectedSignerSubject -ExpectedSignerThumbprint $expectedSignerThumbprint `
        -NetworkPolicy $requiredNetworkPolicy | Out-Null
}
Write-Output "PASS win11_lab_ready_provider_failure_is_red"

$residueFailureCode = Get-Win11LabReadyAttemptFailureCode `
    -ExceptionMessage "readiness_guest_residue" -Password "manufactured-ready-password"
if ($residueFailureCode -cne "guest_preflight_refused") {
    throw "win11_lab_ready_nonzero_residue_is_terminal failed: residue could be retried into a later green result"
}
Write-Output "PASS win11_lab_ready_nonzero_residue_is_terminal"

$budget = Get-Win11LabReadyAttemptBudget -RemainingSeconds 120 -PerAttemptTimeoutSeconds 45 `
    -ConnectTimeoutSeconds 15
if ($budget -ne 45) {
    throw "win11_lab_ready_total_deadline_is_bounded failed: valid per-attempt budget was not preserved"
}
Assert-Throws -Name "win11_lab_ready_total_deadline_is_bounded" -Action {
    Get-Win11LabReadyAttemptBudget -RemainingSeconds 15 -PerAttemptTimeoutSeconds 45 `
        -ConnectTimeoutSeconds 15 | Out-Null
}
$timeoutReceipt = $validReceipt.psobject.Copy()
$timeoutReceipt.status = "TIMEOUT"
Assert-Throws -Name "win11_lab_ready_total_deadline_is_bounded" -Action {
    Assert-Win11LabReadySuccessReceipt -Receipt $timeoutReceipt -ExpectedVMName $expectedVmName `
        -ExpectedComputerName $expectedComputerName -ExpectedOsBuild $expectedOsBuild `
        -ExpectedAdministratorIdentity $expectedAdministratorIdentity -MinimumGuestBootUtc $minimumBootUtc `
        -ExpectedSignerSubject $expectedSignerSubject -ExpectedSignerThumbprint $expectedSignerThumbprint `
        -ExpectedSwitchName $expectedSwitchName -NetworkPolicy $requiredNetworkPolicy | Out-Null
}
Write-Output "PASS win11_lab_ready_total_deadline_is_bounded"

$slowGuestOuterBudget = Get-Win11LabReadyAttemptBudget -RemainingSeconds 120 -PerAttemptTimeoutSeconds 90 `
    -ConnectTimeoutSeconds 60
if ($slowGuestOuterBudget -ne 90) {
    throw "win11_lab_ready_slow_guest_query_uses_declared_outer_budget failed: slow guest query lost its declared outer budget"
}
Write-Output "PASS win11_lab_ready_slow_guest_query_uses_declared_outer_budget"

$remainingAfter65SecondAttempt = 54
$budgetExhaustionMessage = ""
try {
    Get-Win11LabReadyAttemptBudget -RemainingSeconds $remainingAfter65SecondAttempt -PerAttemptTimeoutSeconds 90 `
        -ConnectTimeoutSeconds 60 | Out-Null
}
catch {
    $budgetExhaustionMessage = $_.Exception.Message
}
if ($budgetExhaustionMessage -cne "readiness_attempt_budget_exhausted") {
    throw "win11_lab_ready_65s_like_budget_exhaustion_is_terminal_once failed: insufficient remaining budget was not classified exactly"
}
$budgetExhaustionCode = Get-Win11LabReadyAttemptFailureCode `
    -ExceptionMessage $budgetExhaustionMessage -Password "manufactured-ready-password"
$budgetExhaustionStage = Get-Win11LabReadyAttemptStageCode -OutcomeCode $budgetExhaustionCode
if ($budgetExhaustionCode -cne "attempt_budget_exhausted" -or
    $budgetExhaustionStage -cne "psdirect_connect_or_outer_timeout" -or
    -not (Test-Win11LabReadyTerminalAttemptOutcome -OutcomeCode $budgetExhaustionCode)) {
    throw "win11_lab_ready_65s_like_budget_exhaustion_is_terminal_once failed: budget exhaustion could retry"
}
Write-Output "PASS win11_lab_ready_65s_like_budget_exhaustion_is_terminal_once"

$outerTimeoutCode = Get-Win11LabReadyAttemptFailureCode `
    -ExceptionMessage "PowerShell Direct invoke outer deadline exceeded; process_tree_terminated=True; stderr=opaque" `
    -Password "manufactured-ready-password"
$connectTimeoutCode = Get-Win11LabReadyAttemptFailureCode `
    -ExceptionMessage "PowerShell Direct invoke child failed exit=1: PowerShell Direct worker failure: PowerShell Direct unavailable after 60 seconds: opaque" `
    -Password "manufactured-ready-password"
$prefixedConnectTimeoutCode = Get-Win11LabReadyAttemptFailureCode `
    -ExceptionMessage "PowerShell Direct invoke child failed exit=1: #< CLIXML`nPowerShell Direct worker failure: PowerShell Direct unavailable after 60 seconds: opaque" `
    -Password "manufactured-ready-password"
$childResultCode = Get-Win11LabReadyAttemptFailureCode `
    -ExceptionMessage "PowerShell Direct invoke child failed exit=1: opaque" -Password "manufactured-ready-password"
if ($outerTimeoutCode -cne "psdirect_connect_or_outer_timeout" -or
    $connectTimeoutCode -cne "psdirect_connect_or_outer_timeout" -or
    $childResultCode -cne "child_result" -or
    (Get-Win11LabReadyAttemptStageCode -OutcomeCode $childResultCode) -cne "child_result") {
    throw "win11_lab_ready_exact_helper_prefixes_are_sanitized failed: helper failure prefixes were not classified exactly"
}
Write-Output "PASS win11_lab_ready_exact_helper_prefixes_are_sanitized"
if ($prefixedConnectTimeoutCode -cne "psdirect_connect_or_outer_timeout" -or
    (Get-Win11LabReadyAttemptStageCode -OutcomeCode $prefixedConnectTimeoutCode) -cne
        "psdirect_connect_or_outer_timeout") {
    throw "win11_lab_ready_prefixed_psdirect_unavailable_is_timeout failed: prefixed helper timeout was misclassified"
}
Write-Output "PASS win11_lab_ready_prefixed_psdirect_unavailable_is_timeout"

$manufacturedPassword = "manufactured-ready-password-must-not-appear"
$credentialFailureCode = Get-Win11LabReadyAttemptFailureCode `
    -ExceptionMessage ("credential rejected: " + $manufacturedPassword) -Password $manufacturedPassword
if ([string]::IsNullOrWhiteSpace($credentialFailureCode) -or
    $credentialFailureCode.Contains($manufacturedPassword)) {
    throw "win11_lab_ready_invalid_credential_never_echoes_secret failed: password escaped into a diagnostic code"
}
Write-Output "PASS win11_lab_ready_invalid_credential_never_echoes_secret"

$rawHelperPassword = "manufactured-helper-password-must-not-appear"
$secretSuppressedCode = Get-Win11LabReadyAttemptFailureCode `
    -ExceptionMessage ("PowerShell Direct invoke child failed exit=1: " + $rawHelperPassword) `
    -Password $rawHelperPassword
$sanitizedAttempt = [pscustomobject]@{
    stage_code = Get-Win11LabReadyAttemptStageCode -OutcomeCode $secretSuppressedCode
    outcome_code = $secretSuppressedCode
    outer_timeout_seconds = [int]90
}
$sanitizedAttemptJson = $sanitizedAttempt | ConvertTo-Json -Compress
if ($sanitizedAttemptJson.Contains($rawHelperPassword) -or
    $sanitizedAttemptJson.Contains("PowerShell Direct invoke child failed")) {
    throw "win11_lab_ready_attempt_receipt_never_echoes_raw_exception_or_password failed: raw helper failure escaped"
}
Write-Output "PASS win11_lab_ready_attempt_receipt_never_echoes_raw_exception_or_password"

$missingIntegrationComponentId = $validHost.psobject.Copy()
$missingIntegrationComponentId.integration_service_count = [int]5
$missingIntegrationComponentId.integration_services = @($validHost.integration_services | Select-Object -Skip 1)
Assert-Throws -Name "win11_lab_ready_missing_integration_component_id_is_refused" -Action {
    Assert-Win11LabReadyHostEvidence -Evidence $missingIntegrationComponentId -ExpectedVMName $expectedVmName `
        -ExpectedSwitchName $expectedSwitchName | Out-Null
}
Write-Output "PASS win11_lab_ready_missing_integration_component_id_is_refused"

$duplicateIntegrationComponentId = $validHost.psobject.Copy()
$duplicateIntegrationServices = @($validHost.integration_services | ForEach-Object { $_.psobject.Copy() })
$duplicateIntegrationServices[5].id = $duplicateIntegrationServices[0].id
$duplicateIntegrationServices[5].component_id = $duplicateIntegrationServices[0].component_id
$duplicateIntegrationComponentId.integration_services = $duplicateIntegrationServices
Assert-Throws -Name "win11_lab_ready_duplicate_integration_component_id_is_refused" -Action {
    Assert-Win11LabReadyHostEvidence -Evidence $duplicateIntegrationComponentId -ExpectedVMName $expectedVmName `
        -ExpectedSwitchName $expectedSwitchName | Out-Null
}
Write-Output "PASS win11_lab_ready_duplicate_integration_component_id_is_refused"

$foreignIntegrationComponentId = $validHost.psobject.Copy()
$foreignIntegrationServices = @($validHost.integration_services | ForEach-Object { $_.psobject.Copy() })
$foreignComponentId = "00000000-0000-0000-0000-000000000001"
$foreignIntegrationServices[5].id = ("Microsoft:{0}\\{1}" -f $expectedVmId, $foreignComponentId)
$foreignIntegrationServices[5].component_id = $foreignComponentId
$foreignIntegrationComponentId.integration_services = $foreignIntegrationServices
Assert-Throws -Name "win11_lab_ready_foreign_integration_component_id_is_refused" -Action {
    Assert-Win11LabReadyHostEvidence -Evidence $foreignIntegrationComponentId -ExpectedVMName $expectedVmName `
        -ExpectedSwitchName $expectedSwitchName | Out-Null
}
Write-Output "PASS win11_lab_ready_foreign_integration_component_id_is_refused"

$foreignIntegrationVmId = $validHost.psobject.Copy()
$foreignVmIntegrationServices = @($validHost.integration_services | ForEach-Object { $_.psobject.Copy() })
$foreignVmIntegrationServices[0].vm_id = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"
$foreignIntegrationVmId.integration_services = $foreignVmIntegrationServices
Assert-Throws -Name "win11_lab_ready_foreign_integration_vm_id_is_refused" -Action {
    Assert-Win11LabReadyHostEvidence -Evidence $foreignIntegrationVmId -ExpectedVMName $expectedVmName `
        -ExpectedSwitchName $expectedSwitchName | Out-Null
}
Write-Output "PASS win11_lab_ready_foreign_integration_vm_id_is_refused"

$hostAssertionFailureCode = Get-Win11LabReadyAttemptFailureCode `
    -ExceptionMessage "readiness_host_integration_service_refused" -Password "manufactured-ready-password"
if ($hostAssertionFailureCode -cne "host_preflight_refused") {
    throw "win11_lab_ready_host_assertion_failure_is_terminal failed: host assertion could retry as bounded PowerShell Direct"
}
Write-Output "PASS win11_lab_ready_host_assertion_failure_is_terminal"

$wrongSwitch = $validHost.psobject.Copy()
$wrongSwitch.network_adapters = @(
    [pscustomobject]@{
        switch_name = "Foreign Manufactured Switch"
        connected = $true
        status = "OK"
    }
)
Assert-Throws -Name "win11_lab_ready_exact_switch_is_required" -Action {
    Assert-Win11LabReadyHostEvidence -Evidence $wrongSwitch -ExpectedVMName $expectedVmName `
        -ExpectedSwitchName $expectedSwitchName | Out-Null
}
Write-Output "PASS win11_lab_ready_exact_switch_is_required"

$noRoutableIpv4 = $validGuest.psobject.Copy()
$noRoutableIpv4.routable_ipv4_count = [int]0
Assert-Throws -Name "win11_lab_ready_no_routable_ipv4_is_red" -Action {
    Assert-Win11LabReadyGuestEvidence -Evidence $noRoutableIpv4 `
        -ExpectedComputerName $expectedComputerName -ExpectedOsBuild $expectedOsBuild `
        -ExpectedAdministratorIdentity $expectedAdministratorIdentity -MinimumGuestBootUtc $minimumBootUtc `
        -ExpectedSignerSubject $expectedSignerSubject -ExpectedSignerThumbprint $expectedSignerThumbprint `
        -NetworkPolicy $requiredNetworkPolicy | Out-Null
}
Assert-Win11LabReadyGuestEvidence -Evidence $noRoutableIpv4 `
    -ExpectedComputerName $expectedComputerName -ExpectedOsBuild $expectedOsBuild `
    -ExpectedAdministratorIdentity $expectedAdministratorIdentity -MinimumGuestBootUtc $minimumBootUtc `
    -ExpectedSignerSubject $expectedSignerSubject -ExpectedSignerThumbprint $expectedSignerThumbprint `
    -NetworkPolicy $sealedOfflineNetworkPolicy | Out-Null
Write-Output "PASS win11_lab_ready_no_routable_ipv4_is_red"

$networkProviderFailure = $validGuest.psobject.Copy()
$networkProviderFailure.provider_error_count = [int]1
$networkProviderFailure.provider_errors = @("network_ipv4")
$networkProviderFailure.network_ipv4_provider_error_count = [int]1
Assert-Throws -Name "win11_lab_ready_network_provider_failure_is_red" -Action {
    Assert-Win11LabReadyGuestEvidence -Evidence $networkProviderFailure `
        -ExpectedComputerName $expectedComputerName -ExpectedOsBuild $expectedOsBuild `
        -ExpectedAdministratorIdentity $expectedAdministratorIdentity -MinimumGuestBootUtc $minimumBootUtc `
        -ExpectedSignerSubject $expectedSignerSubject -ExpectedSignerThumbprint $expectedSignerThumbprint `
        -NetworkPolicy $requiredNetworkPolicy | Out-Null
}
Write-Output "PASS win11_lab_ready_network_provider_failure_is_red"

Write-Output "PASS Test-Win11LabReadyStatic"
