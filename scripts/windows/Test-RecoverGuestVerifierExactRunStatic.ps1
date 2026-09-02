#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path $PSScriptRoot "Recover-GuestVerifierExactRun.ps1"
}

function Assert-Throws([scriptblock]$Action, [string]$Name) {
    try {
        & $Action
    }
    catch {
        return
    }
    throw "$Name failed: expected refusal was accepted"
}

function Import-ProductionFunction([string]$Name, [string]$Source) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput(
        $Source, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "guest_verifier_recovery_parser_is_green failed"
    }
    $definition = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $Name
        }, $true)
    if ($null -eq $definition) {
        throw "guest_verifier_recovery_static_contract_is_green failed: missing $Name"
    }
    $body = $definition.Body.Extent.Text
    Set-Item -Path ("Function:\script:{0}" -f $Name) -Value (
        [scriptblock]::Create($body.Substring(1, $body.Length - 2)))
}

if (-not (Test-Path -LiteralPath $HarnessPath -PathType Leaf)) {
    throw "guest_verifier_recovery_parser_is_green failed: harness missing"
}
$source = Get-Content -LiteralPath $HarnessPath -Raw -ErrorAction Stop
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput(
    $source, [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0) {
    throw "guest_verifier_recovery_parser_is_green failed"
}
$parameters = @($ast.ParamBlock.Parameters | ForEach-Object { $_.Name.VariablePath.UserPath })
foreach ($required in @(
        "FailedRunArtifactDirectory", "SealedPlanArtifactDirectory", "PartialActionArtifactDirectory", "PlanOnly",
        "ApproveExactRecovery", "ExpectedVMId", "ExpectedDriverSha256",
        "ExpectedInfSha256", "ExpectedCatalogSha256", "ExpectedPartialPublishedInf",
        "GuestRestartDelaySeconds", "CrashDumpPath", "WinDbgPath")) {
    if ($parameters -notcontains $required) {
        throw "guest_verifier_recovery_static_contract_is_green failed: missing parameter $required"
    }
}
foreach ($name in @(
        "Assert-GuestVerifierPartialInstallArtifacts",
        "Assert-GuestVerifierRecoveryRootRemovedState",
        "Assert-GuestVerifierRecoveryRetiredOnlyState",
        "Assert-GuestVerifierRecoveryFinalState")) {
    Import-ProductionFunction $name $source
}

$partialSummary = [pscustomobject]@{
    schema = [int]1
    run_id = "manufactured-run"
    status = "FAIL"
    current_run_package_may_be_present = $true
    error_code = "psdirect_outer_deadline"
    error = "PowerShell Direct invoke outer deadline exceeded; process_tree_terminated=True"
}
$partialInput = [pscustomobject]@{ run_id = "manufactured-run" }
$initialZero = [pscustomobject]@{
    package_count = [int]0
    service_count = [int]0
    root_count = [int]0
    ramshared_disk_count = [int]0
    ramshared_pnp_disk_count = [int]0
}
Assert-GuestVerifierPartialInstallArtifacts -Summary $partialSummary -InputBinding $partialInput `
    -InitialPreflight $initialZero -RunId "manufactured-run" -ExpectedPublishedInf "oem2.inf" `
    -HasInstallReceipt $false -HasNormalIdentityReceipt $false -HasNormalPassReceipt $false `
    -HasVerifierPassReceipt $false | Out-Null
$notInitiallyClean = $initialZero.psobject.Copy()
$notInitiallyClean.package_count = [int]1
Assert-Throws {
    Assert-GuestVerifierPartialInstallArtifacts -Summary $partialSummary -InputBinding $partialInput `
        -InitialPreflight $notInitiallyClean -RunId "manufactured-run" -ExpectedPublishedInf "oem2.inf" `
        -HasInstallReceipt $false -HasNormalIdentityReceipt $false -HasNormalPassReceipt $false `
        -HasVerifierPassReceipt $false | Out-Null
} "guest_verifier_recovery_partial_install_is_exact"
Assert-Throws {
    Assert-GuestVerifierPartialInstallArtifacts -Summary $partialSummary -InputBinding $partialInput `
        -InitialPreflight $initialZero -RunId "manufactured-run" -ExpectedPublishedInf "oem2.inf" `
        -HasInstallReceipt $true -HasNormalIdentityReceipt $false -HasNormalPassReceipt $false `
        -HasVerifierPassReceipt $false | Out-Null
} "guest_verifier_recovery_partial_install_is_exact"
Write-Output "PASS guest_verifier_recovery_partial_install_is_exact"

$binding = [pscustomobject]@{
    published_inf = "oem2.inf"
    loaded_driver_sha256 = "A" * 64
    driver_store_inf_sha256 = "B" * 64
    driver_store_catalog_sha256 = "C" * 64
}
$valid = [pscustomobject]@{
    schema = [int]1
    package_count = [int]1
    published_inf_count = [int]1
    package_original_inf = "ramshared.inf"
    root_count = [int]0
    service_count = [int]1
    service_name = "ramshared"
    service_state = "Stopped"
    loaded_driver_sha256 = "A" * 64
    driver_store_inf_sha256 = "B" * 64
    driver_store_catalog_sha256 = "C" * 64
    ramshared_disk_count = [int]0
    ramshared_pnp_disk_count = [int]1
    ramshared_present_pnp_disk_count = [int]0
    ramshared_retired_pnp_disk_count = [int]1
    retired_pnp_instance_id = "SCSI\DISK&VEN_RAMSHARE&PROD_VRAMDISK\1&B23B977&0&000000"
}
Assert-GuestVerifierRecoveryRootRemovedState -State $valid -Binding $binding | Out-Null
$active = $valid.psobject.Copy()
$active.ramshared_present_pnp_disk_count = [int]1
$foreign = $valid.psobject.Copy()
$foreign.retired_pnp_instance_id = "SCSI\DISK&VEN_FOREIGN&PROD_DISK\1"
$ambiguous = $valid.psobject.Copy()
$ambiguous.ramshared_pnp_disk_count = [int]2
$ambiguous.ramshared_retired_pnp_disk_count = [int]2
foreach ($invalid in @($active, $foreign, $ambiguous)) {
    Assert-Throws {
        Assert-GuestVerifierRecoveryRootRemovedState -State $invalid -Binding $binding | Out-Null
    } "guest_verifier_recovery_root_removed_state_is_exact"
}
Write-Output "PASS guest_verifier_recovery_root_removed_state_is_exact"

$retiredOnly = $valid.psobject.Copy()
$retiredOnly.package_count = [int]0
$retiredOnly.published_inf_count = [int]0
$retiredOnly.package_original_inf = ""
$retiredOnly.service_count = [int]0
$retiredOnly.service_name = ""
$retiredOnly.service_state = ""
$retiredOnly.loaded_driver_sha256 = ""
$retiredOnly.driver_store_inf_sha256 = ""
$retiredOnly.driver_store_catalog_sha256 = ""
Assert-GuestVerifierRecoveryRetiredOnlyState -State $retiredOnly `
    -ExpectedRetiredInstanceId $retiredOnly.retired_pnp_instance_id | Out-Null
$wrongRetired = $retiredOnly.psobject.Copy()
$wrongRetired.retired_pnp_instance_id = "SCSI\DISK&VEN_RAMSHARE&PROD_VRAMDISK\FOREIGN"
Assert-Throws {
    Assert-GuestVerifierRecoveryRetiredOnlyState -State $wrongRetired `
        -ExpectedRetiredInstanceId $retiredOnly.retired_pnp_instance_id | Out-Null
} "guest_verifier_recovery_retired_only_state_is_exact"
Write-Output "PASS guest_verifier_recovery_retired_only_state_is_exact"

$final = [pscustomobject]@{
    schema = [int]1
    package_count = [int]0
    published_inf_count = [int]0
    root_count = [int]0
    service_count = [int]0
    ramshared_disk_count = [int]0
    ramshared_pnp_disk_count = [int]0
    ramshared_present_pnp_disk_count = [int]0
    ramshared_retired_pnp_disk_count = [int]0
}
Assert-GuestVerifierRecoveryFinalState -State $final | Out-Null
$notFinal = $final.psobject.Copy()
$notFinal.ramshared_retired_pnp_disk_count = [int]1
Assert-Throws {
    Assert-GuestVerifierRecoveryFinalState -State $notFinal | Out-Null
} "guest_verifier_recovery_action_is_exact"
Write-Output "PASS guest_verifier_recovery_action_is_exact"

foreach ($forbidden in @(
        '(?i)\bRestart-VM\b|\bStop-VM\b|\bStart-VM\b',
        '(?i)\bbcdedit\.exe\s+/(?:set|deletevalue)',
        '(?i)\bImport-Certificate\b|\bRemove-Item\s+Cert:',
        '(?i)\bpnputil\.exe\s+/add-driver',
        '(?i)\bpnputil\.exe\s+/remove-device\s+(?:ROOT\\|[^\$])',
        '(?i)\bpnputil\.exe[^\r\n]*/force',
        '(?i)\bsc\.exe\s+(?:create|start|stop)\b')) {
    if ($source -match $forbidden) {
        throw "guest_verifier_recovery_action_is_exact failed: forbidden mutation $forbidden"
    }
}
foreach ($required in @(
        'sc\.exe\s+delete\s+\$ExpectedServiceName',
        'pnputil\.exe\s+/delete-driver\s+\$ExpectedPublishedInf\s+/uninstall',
        'pnputil\.exe\s+/remove-device\s+\$ExpectedRetiredInstanceId',
        'Get-FileHash[^\r\n]+exact-binding\.json',
        'AddHours\(24\)',
        '"partial_install",\s*"pre_root",\s*"root_removed",\s*"package_removed_retired_only"',
        '\$recoveryPhase\s+-cin\s+@\("partial_install",\s*"pre_root"\)\s+-and\s+\$resumeRootRemoved',
        'recovery_phase\s*=\s*"root_removed"')) {
    if ($source -notmatch $required) {
        throw "guest_verifier_recovery_static_contract_is_green failed: missing $required"
    }
}
$splitRootIndex = $source.IndexOf(
    '$rootRemoval = Remove-GuestVerifierCurrentRunRoot', [StringComparison]::Ordinal)
$preRootObservationIndex = $source.IndexOf(
    '$preRootState = Get-GuestVerifierPostPublishCleanupState', [StringComparison]::Ordinal)
$preRootReceiptIndex = $source.IndexOf(
    'Write-RecoveryJson "pre-root-state.json"', [StringComparison]::Ordinal)
$preRootClassifierIndex = $source.IndexOf(
    '$preRootMode = Get-GuestVerifierPostPublishCleanupMode', [StringComparison]::Ordinal)
if ($preRootObservationIndex -lt 0 -or $preRootReceiptIndex -le $preRootObservationIndex -or
    $preRootClassifierIndex -le $preRootReceiptIndex -or $splitRootIndex -le $preRootClassifierIndex) {
    throw "guest_verifier_recovery_plan_persists_pre_root_state failed"
}
Write-Output "PASS guest_verifier_recovery_plan_persists_pre_root_state"
$splitRootReceiptIndex = $source.IndexOf(
    'Write-RecoveryJson "action-root-removal.json"', [StringComparison]::Ordinal)
$splitRestartIndex = $source.IndexOf(
    '$rootRemovalRestart = Request-GuestVerifierRestart', [StringComparison]::Ordinal)
$splitContinuationIndex = $source.IndexOf(
    '$rootRemovedActions = Remove-GuestVerifierRootRemovedArtifacts', [StringComparison]::Ordinal)
if ($splitRootIndex -lt 0 -or $splitRootReceiptIndex -le $splitRootIndex -or
    $splitRestartIndex -le $splitRootReceiptIndex -or
    $splitContinuationIndex -le $splitRestartIndex -or
    $source -notmatch 'Write-RecoveryJson\s+"root-removal-boot-change\.json"' -or
    $source -notmatch '(?s)Assert-GuestVerifierRootRemovedState.*?-RequireStopped') {
    throw "guest_verifier_recovery_pre_root_uses_split_teardown failed"
}
Write-Output "PASS guest_verifier_recovery_pre_root_uses_split_teardown"
$attemptIndex = $source.IndexOf('Write-RecoveryJson "attempt.json"', [StringComparison]::Ordinal)
$preconditionIndex = $source.IndexOf('$preconditionRows = Invoke-GuestVerifierRemote', [StringComparison]::Ordinal)
if ($attemptIndex -lt 0 -or $preconditionIndex -lt 0 -or $attemptIndex -ge $preconditionIndex -or
    $source -notmatch '\$preconditionRows\s*=\s*Invoke-GuestVerifierRemote\s+-Operation\s+invoke\s+-TimeoutSeconds\s+420') {
    throw "guest_verifier_recovery_plan_is_read_only failed: attempt receipt or 420-second precondition is missing"
}
Write-Output "PASS guest_verifier_recovery_plan_is_read_only"
Write-Output "PASS guest_verifier_recovery_requires_sealed_binding"
Write-Output "PASS guest_verifier_recovery_refuses_foreign_or_ambiguous_state"
Write-Output "PASS Test-RecoverGuestVerifierExactRunStatic"
