#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath = "",
    [string]$HelperPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path $PSScriptRoot "Run-GuestExhaustive.ps1"
}
if ([string]::IsNullOrWhiteSpace($HelperPath)) {
    $HelperPath = Join-Path $PSScriptRoot "Invoke-GuestPsDirectBounded.ps1"
}

function Get-ParsedAst([string]$Path) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile(
        $Path, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "guest_verifier_parser_is_green failed: parser errors in $Path"
    }
    $ast
}

function Import-ProductionFunction([string]$Name, [string]$Source) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput(
        $Source, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "guest_verifier_parser_is_green failed: parser errors in production source"
    }
    $definition = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $Name
        }, $true)
    if (-not $definition) {
        throw "guest_verifier_static_contract_is_green failed: missing production function $Name"
    }
    $bodyText = $definition.Body.Extent.Text.Trim()
    if ($bodyText.Length -lt 2 -or $bodyText[0] -ne "{" -or
        $bodyText[$bodyText.Length - 1] -ne "}") {
        throw "guest_verifier_parser_is_green failed: malformed production function $Name"
    }
    Set-Item -Path ("Function:\script:{0}" -f $Name) -Value (
        [scriptblock]::Create($bodyText.Substring(1, $bodyText.Length - 2)))
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

if (-not (Test-Path -LiteralPath $HarnessPath -PathType Leaf)) {
    throw "guest_verifier_parser_is_green failed: harness is missing"
}
if (-not (Test-Path -LiteralPath $HelperPath -PathType Leaf)) {
    throw "guest_verifier_uses_shared_bounded_psdirect failed: helper is missing"
}

$text = Get-Content -LiteralPath $HarnessPath -Raw
$ast = Get-ParsedAst $HarnessPath
$helperText = Get-Content -LiteralPath $HelperPath -Raw
$violations = [Collections.Generic.List[string]]::new()

$parameterNames = @($ast.ParamBlock.Parameters | ForEach-Object {
        $_.Name.VariablePath.UserPath
    })
foreach ($requiredParameter in @(
        "ExpectedVMId", "DriverPackage", "HostBinDir", "ExpectedDriverSha256",
        "ExpectedInfSha256", "ExpectedCatalogSha256",
        "ExpectedIoctlValidationSha256", "DriverSignerCert",
        "ExpectedDriverSignerCertSha256", "ExpectedDriverSignerSubject",
        "ExpectedDriverSignerThumbprint")) {
    if ($parameterNames -notcontains $requiredParameter) {
        $violations.Add("missing parameter $requiredParameter")
    }
}
if ($parameterNames -contains "SkipVerifier") {
    $violations.Add("Driver Verifier must not have a skip switch")
}
foreach ($forbiddenParameter in @("DriverSignerPfx", "DriverSignerP12", "DriverSignerPrivateKey", "DriverSignerCertPassword")) {
    if ($parameterNames -contains $forbiddenParameter) {
        $violations.Add("public certificate contract must not accept $forbiddenParameter")
    }
}

foreach ($functionName in @(
        "Normalize-GuestVerifierSha256",
        "Normalize-GuestVerifierSignerThumbprint",
        "Normalize-GuestVerifierSignerSubject",
        "Normalize-GuestVerifierPublishedInf",
        "Get-GuestVerifierPublicCertificateIdentity",
        "Assert-GuestVerifierSignerIdentity",
        "Assert-GuestVerifierSignatureRelationship",
        "Get-GuestVerifierInputBinding",
        "Assert-GuestVerifierInputBindingUnchanged",
        "Assert-GuestVerifierRestartReceipt",
        "Assert-GuestVerifierBootChangeReceipt",
        "Assert-GuestVerifierTestSigningReceipt",
        "Assert-GuestVerifierTestSigningState",
        "Assert-GuestVerifierSignerTrustReceipt",
        "Assert-GuestVerifierSignerTrustRemovalReceipt",
        "Assert-GuestVerifierGuestSignatureEvidence",
        "Get-GuestVerifierTestSigningState",
        "Set-GuestVerifierTestSigning",
        "Get-GuestVerifierStagedSignerCertificateEvidence",
        "Install-GuestVerifierSignerTrust",
        "Get-GuestVerifierSignerTrustEvidence",
        "Remove-GuestVerifierSignerTrust",
        "Get-GuestVerifierGuestSignatureEvidence",
        "Assert-GuestVerifierIoctlVerdict",
        "Assert-GuestVerifierDumpObservation",
        "Assert-GuestVerifierResidueWaitEvidence",
        "Assert-GuestVerifierPassEvidence",
        "Assert-GuestVerifierPreflight",
        "Assert-GuestVerifierCurrentIdentity",
        "Assert-GuestVerifierCurrentRunTeardownBinding",
        "Assert-GuestVerifierCurrentRunTeardownEvidence",
        "Get-GuestVerifierPostPublishCleanupMode",
        "Test-GuestVerifierPostRootRemovalState",
        "Assert-GuestVerifierPostRootRemovalState",
        "Get-GuestVerifierTargetLines",
        "Assert-GuestVerifierEnabled",
        "Assert-GuestVerifierReset",
        "Get-GuestVerifierCurrentRunTeardownBinding",
        "Get-GuestVerifierCurrentRunZeroResidueEvidence",
        "Assert-GuestVerifierRootRemovedState",
        "Remove-GuestVerifierCurrentRunRoot",
        "Remove-GuestVerifierRootRemovedArtifacts",
        "Remove-GuestVerifierCurrentRunArtifacts",
        "Wait-GuestVerifierResidueProviderZero",
        "Get-GuestVerifierConnectTimeoutSeconds",
        "Invoke-GuestVerifierPreflightStage",
        "Invoke-GuestVerifierRemote")) {
    if (-not $ast.Find({
                param($node)
                $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq $functionName
            }, $true)) {
        $violations.Add("missing function $functionName")
    }
}

foreach ($forbiddenPattern in @(
        '(?i)\bStart-Job\b',
        '(?i)\bStop-Job\b',
        '(?i)\bRestart-VM\b',
        '(?i)\bStop-VM\b',
        '(?i)\bStart-VM\b',
        '(?i)\bNew-PSSession\b',
        '(?i)\bInvoke-Command\b',
        '(?i)\bRemove-PSSession\b',
        '(?i)Import-PfxCertificate',
        '(?i)Export-PfxCertificate',
        '(?i)\bdiskpart\b')) {
    if ($text -match $forbiddenPattern) {
        $violations.Add("forbidden legacy control-flow pattern $forbiddenPattern")
    }
}

$boundedCalls = @($ast.FindAll({
            param($node)
            $node -is [Management.Automation.Language.CommandAst] -and
            $node.GetCommandName() -eq "Invoke-GuestPsDirectBounded"
        }, $true)).Count
$remoteCalls = @($ast.FindAll({
            param($node)
            $node -is [Management.Automation.Language.CommandAst] -and
            $node.GetCommandName() -eq "Invoke-GuestVerifierRemote"
        }, $true)).Count
if ($boundedCalls -ne 1 -or $remoteCalls -lt 8) {
    $violations.Add("every guest phase is not bounded helper_calls=$boundedCalls remote_calls=$remoteCalls")
}
foreach ($requiredNeedle in @(
        "Invoke-GuestPsDirectBounded.ps1",
        "Get-AuthenticodeSignature",
        "Win32_SystemDriver",
        "Get-WindowsDriver",
        "DriverStore\FileRepository",
        "Get-WinEvent",
        'ProviderName = "disk"',
        "C:\Windows\Minidump",
        "verifier /reset",
        "bcdedit.exe /set testsigning",
        "Cert:\LocalMachine\Root",
        "Cert:\LocalMachine\TrustedPublisher",
        "GetCertContentType",
        "HasPrivateKey",
        "shutdown.exe",
        "cleanup",
        "verifier_target_count",
        "verifier_all_drivers",
        "event153_error",
        "dump_observation_error",
        "verdict_error",
        "vpd_serial",
        "vpd_serial_observation_error")) {
    if ($text -notmatch [regex]::Escape($requiredNeedle)) {
        $violations.Add("missing evidence guard $requiredNeedle")
    }
}
if ($helperText -notmatch 'function\s+Invoke-GuestPsDirectBounded') {
    $violations.Add("bounded helper has no callable entrypoint")
}

$signerTrustDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Install-GuestVerifierSignerTrust"
    }, $true)
if ($null -eq $signerTrustDefinition) {
    throw "guest_verifier_signer_file_is_closed_before_store_import failed: trust function is missing"
}
$signerTrustText = $signerTrustDefinition.Extent.Text
$requiredDetachedTrust = @(
    '[IO.File]::ReadAllBytes',
    '[Security.Cryptography.X509Certificates.X509Certificate2]::new($certificateBytes)',
    '[Security.Cryptography.X509Certificates.X509Store]::new',
    '$store.Add($certificate)',
    '$certificate.Dispose()',
    '$store.Dispose()'
)
foreach ($needle in $requiredDetachedTrust) {
    if ($signerTrustText.IndexOf($needle, [StringComparison]::Ordinal) -lt 0) {
        throw "guest_verifier_signer_import_uses_detached_bytes failed: missing $needle"
    }
}
if ($signerTrustText -match '(?i)Import-Certificate' -or
    $signerTrustText -match 'X509Certificate2\]\:\:new\(\$certificatePath\)') {
    throw "guest_verifier_signer_import_uses_detached_bytes failed: file/cmdlet import remains"
}
Write-Output "PASS guest_verifier_signer_import_uses_detached_bytes"

foreach ($needle in @(
        'function Get-GuestVerifierFailureCode',
        'error_code = $failureCode',
        'error = $failureCode')) {
    if ($text.IndexOf($needle, [StringComparison]::Ordinal) -lt 0) {
        throw "guest_verifier_summary_error_is_sanitized failed: missing $needle"
    }
}
if ($text -match 'error\s*=\s*if\s*\(\$null -eq \$failure\).*Exception\.Message' -or
    $text -match '\$rollbackErrors\.Add\([^\r\n]*Exception\.Message') {
    throw "guest_verifier_summary_error_is_sanitized failed: raw exception persists"
}
Write-Output "PASS guest_verifier_summary_error_is_sanitized"

$rootResultDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "ConvertFrom-GuestVerifierRootCreationResult"
    }, $true)
if ($null -eq $rootResultDefinition) {
    throw "guest_verifier_root_failure_receipt_is_sanitized failed: classifier is missing"
}
Invoke-Expression $rootResultDefinition.Extent.Text
$rootOk = ConvertFrom-GuestVerifierRootCreationResult -Value "OK reboot=False"
$rootDenied = ConvertFrom-GuestVerifierRootCreationResult -Value "UpdateDriver err=5"
$rootMalformed = ConvertFrom-GuestVerifierRootCreationResult -Value "foreign raw exception"
if (-not [bool]$rootOk.root_creation_ok -or [string]$rootOk.root_creation_stage -cne "ok" -or
    [int]$rootOk.root_creation_win32_error -ne 0 -or [bool]$rootOk.root_creation_reboot_required -or
    [bool]$rootDenied.root_creation_ok -or [string]$rootDenied.root_creation_stage -cne "UpdateDriver" -or
    [int]$rootDenied.root_creation_win32_error -ne 5 -or
    [bool]$rootMalformed.root_creation_ok -or [string]$rootMalformed.root_creation_stage -cne "malformed" -or
    [int]$rootMalformed.root_creation_win32_error -ne -1) {
    throw "guest_verifier_root_failure_receipt_is_sanitized failed: classifier verdict is wrong"
}
foreach ($needle in @(
        'root_creation_ok',
        'root_creation_stage',
        'root_creation_win32_error',
        'worker_status',
        'worker_failure_stage',
        'worker_failure_hresult',
        'worker_failure_type',
        'failure_phase = $failurePhase')) {
    if ($text.IndexOf($needle, [StringComparison]::Ordinal) -lt 0) {
        throw "guest_verifier_root_failure_receipt_is_sanitized failed: missing $needle"
    }
}
if ($text -notmatch 'worker_failure_type\s*=\s*\$safeExceptionType' -or
    $text -match 'worker_failure_message' -or
    $text -match 'worker_failure[^\r\n]*Exception\.Message') {
    throw "guest_verifier_root_failure_receipt_is_sanitized failed: worker diagnostic is not bounded"
}
Write-Output "PASS guest_verifier_root_failure_receipt_is_sanitized"

foreach ($needle in @(
        '$workerStage = "package_file_visibility"',
        '$packageVisibilityDeadline',
        '.AddSeconds(60)',
        'Start-Sleep -Milliseconds 250',
        'package_visibility_elapsed_ms')) {
    if ($text.IndexOf($needle, [StringComparison]::Ordinal) -lt 0) {
        throw "guest_verifier_root_waits_for_driverstore_files failed: missing $needle"
    }
}
Write-Output "PASS guest_verifier_root_waits_for_driverstore_files"

foreach ($needle in @(
        '$driverStoreFileRepositoryRoot',
        'Join-Path $env:SystemRoot "System32\DriverStore\FileRepository"',
        '$packageInfCanonical.StartsWith(',
        '[IO.Path]::DirectorySeparatorChar',
        '[StringComparison]::OrdinalIgnoreCase')) {
    if ($text.IndexOf($needle, [StringComparison]::Ordinal) -lt 0) {
        throw "guest_verifier_driverstore_path_is_canonical failed: missing $needle"
    }
}
if ($text.IndexOf('"\\DriverStore\\FileRepository\\"', [StringComparison]::Ordinal) -ge 0) {
    throw "guest_verifier_driverstore_path_is_canonical failed: regex-style doubled separators remain"
}
Write-Output "PASS guest_verifier_driverstore_path_is_canonical"

$preflightDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Invoke-GuestVerifierPreflight"
    }, $true)
if ($null -eq $preflightDefinition) {
    throw "guest_verifier_preflight_providers_are_bounded_individually failed: preflight function is missing"
}
$preflightText = $preflightDefinition.Extent.Text
$preflightStageCalls = @($preflightDefinition.FindAll({
            param($node)
            $node -is [Management.Automation.Language.CommandAst] -and
            $node.GetCommandName() -eq "Invoke-GuestVerifierPreflightStage"
        }, $true))
if ($preflightStageCalls.Count -ne 8) {
    throw "guest_verifier_preflight_providers_are_bounded_individually failed: expected eight stage calls"
}
foreach ($providerCode in @(
        "driver_store", "system_driver", "pnp_root", "disk", "pnp_disk",
        "verifier_query", "certificate_stores", "testsigning_query")) {
    if ($preflightText -notmatch ('-ProviderCode\s+"' + [regex]::Escape($providerCode) + '"')) {
        throw "guest_verifier_preflight_providers_are_bounded_individually failed: missing $providerCode"
    }
}
if ($preflightText -notmatch '-ProviderCode\s+"driver_store"[\s\S]*?-TimeoutSeconds\s+420') {
    throw "guest_verifier_preflight_providers_are_bounded_individually failed: DriverStore deadline is not 420 seconds"
}
Write-Output "PASS guest_verifier_preflight_providers_are_bounded_individually"

$phaseBindingDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Get-GuestVerifierCurrentRunTeardownBinding"
    }, $true)
if ($null -eq $phaseBindingDefinition -or
    $phaseBindingDefinition.Extent.Text -notmatch '(?s)AllowEmptyString\(\).*?NormalVpdSerial' -or
    $phaseBindingDefinition.Extent.Text -notmatch '(?s)AllowEmptyString\(\).*?VerifierVpdSerial') {
    throw "guest_verifier_phase_bound_teardown_is_exact failed: future-pass empty serial cannot bind"
}

$ioctlDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Invoke-GuestVerifierIoctlPass"
    }, $true)
if ($null -eq $ioctlDefinition) {
    throw "guest_verifier_waits_for_async_lun_removal failed: IOCTL function is missing"
}
$ioctlText = $ioctlDefinition.Extent.Text
foreach ($providerCode in @("disk", "pnp_disk")) {
    if ($ioctlText -notmatch ('Wait-GuestVerifierResidueProviderZero[\s\S]*-ProviderCode\s+"' +
            [regex]::Escape($providerCode) + '"')) {
        throw "guest_verifier_waits_for_async_lun_removal failed: missing bounded $providerCode wait"
    }
}
$residueWaitDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Wait-GuestVerifierResidueProviderZero"
    }, $true)
if ($null -eq $residueWaitDefinition -or
    $residueWaitDefinition.Extent.Text -notmatch '\.Present' -or
    $residueWaitDefinition.Extent.Text -notmatch 'Problem' -or
    $residueWaitDefinition.Extent.Text -notmatch '45' -or
    $residueWaitDefinition.Extent.Text -notmatch 'retired_count') {
    throw "guest_verifier_waits_for_async_lun_removal failed: exact retired PnP classification is missing"
}
Write-Output "PASS guest_verifier_waits_for_async_lun_removal"

$publishDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Publish-GuestVerifierPackage"
    }, $true)
$rootDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Create-GuestVerifierRoot"
    }, $true)
$teardownDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Remove-GuestVerifierCurrentRunArtifacts"
    }, $true)
$publishedOnlyTeardownDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Remove-GuestVerifierPublishedPackageOnly"
}, $true)
$rootRemovalDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Remove-GuestVerifierCurrentRunRoot"
    }, $true)
$rootRemovedContinuationDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq "Remove-GuestVerifierRootRemovedArtifacts"
    }, $true)
if ($null -eq $rootRemovalDefinition -or $null -eq $rootRemovedContinuationDefinition) {
    $violations.Add("guest_verifier_root_removal_reboots_before_service_delete failed: split teardown functions are missing")
}
else {
    $rootRemovalText = $rootRemovalDefinition.Extent.Text
    $rootRemovedContinuationText = $rootRemovedContinuationDefinition.Extent.Text
    if ($rootRemovalText -notmatch [regex]::Escape('pnputil.exe /remove-device $ExpectedRootInstanceId') -or
        $rootRemovalText -match [regex]::Escape('sc.exe delete $ExpectedServiceName') -or
        $rootRemovedContinuationText -notmatch [regex]::Escape('sc.exe delete $ExpectedServiceName') -or
        $rootRemovedContinuationText -notmatch [regex]::Escape('pnputil.exe /delete-driver $ExpectedPublishedInf /uninstall')) {
        $violations.Add("guest_verifier_root_removal_reboots_before_service_delete failed: stage ownership is wrong")
    }
}
if ($null -eq $publishDefinition -or $null -eq $rootDefinition) {
    $violations.Add("signed package publish and ROOT creation are not separate functions")
}
else {
    $publishText = $publishDefinition.Extent.Text
    $rootText = $rootDefinition.Extent.Text
    foreach ($requiredPublishNeedle in @(
            "pnputil.exe /add-driver",
            "run_id",
            "published_inf",
            "oem[0-9]+\.inf",
            "Get-WindowsDriver")) {
        if ($publishText -notmatch [regex]::Escape($requiredPublishNeedle)) {
            $violations.Add("signed publish does not bind exact published INF $requiredPublishNeedle")
        }
    }
    if ($publishText -notmatch 'Invoke-GuestVerifierRemote\s+-Operation\s+invoke\s+-TimeoutSeconds\s+420' -or
        $publishText -match '(?i)pnputil\.exe[^\r\n]*/install') {
        $violations.Add("signed publish is not a 420-second add-only operation")
    }
    foreach ($requiredRootNeedle in @(
            "ExpectedPublishedInf", "ExpectedDriverHash", "ExpectedInfHash",
            "ExpectedCatalogHash", "Get-WindowsDriver", "RamSharedGuestRootEnum",
            "root_creation")) {
        if ($rootText -notmatch [regex]::Escape($requiredRootNeedle)) {
            $violations.Add("ROOT creation does not rebind the published package at $requiredRootNeedle")
        }
    }
    if ($rootText -notmatch 'Invoke-GuestVerifierRemote\s+-Operation\s+invoke\s+-TimeoutSeconds\s+420' -or
        $rootText -match '(?i)pnputil\.exe\s+/add-driver') {
        $violations.Add("ROOT creation is not one separate 420-second operation")
    }
}
if ($null -ne $teardownDefinition) {
    $teardownText = $teardownDefinition.Extent.Text
    foreach ($requiredTeardownNeedle in @(
            'pnputil.exe /remove-device $ExpectedRootInstanceId',
            'sc.exe delete $ExpectedServiceName',
            'pnputil.exe /delete-driver $ExpectedPublishedInf /uninstall',
            '$serviceStopAction = "deferred_to_root_removal"',
            '[string]$afterDeviceRemoval.service_state -cne "Stopped"',
            "Get-WindowsDriver",
            "Get-PnpDevice",
            "Win32_SystemDriver")) {
        if ($teardownText -notmatch [regex]::Escape($requiredTeardownNeedle)) {
            $violations.Add("current-run teardown is not exact at $requiredTeardownNeedle")
        }
    }
    if ($teardownText -match '(?i)(?:^|\s)/force(?:\s|$)' -or
        $teardownText -match '(?i)(?:\*|\?)[^\r\n]*pnputil|pnputil[^\r\n]*(?:\*|\?)') {
        $violations.Add("current-run teardown must not force or wildcard a package delete")
    }
    if ($teardownText -match [regex]::Escape('sc.exe stop $ExpectedServiceName')) {
        $violations.Add("current-run PnP miniport teardown must defer stop to exact ROOT removal")
    }
    if ([regex]::Matches($teardownText, '\bGet-WindowsDriver\b').Count -ne 1 -or
        $teardownText -notmatch 'Invoke-GuestVerifierRemote\s+-Operation\s+invoke\s+-TimeoutSeconds\s+900' -or
        $teardownText -notmatch 'Get-GuestVerifierCurrentRunState\s+-IncludeDriverStore\s+\$true' -or
        $teardownText -notmatch 'Get-GuestVerifierCurrentRunState\s+-IncludeDriverStore\s+\$false') {
        $violations.Add("phase-bound teardown repeats DriverStore observation or lacks the 900-second bound")
    }
    if ($teardownText -notmatch 'pnputil\.exe\s+/remove-device\s+\$retiredInstanceId' -or
        $teardownText -notmatch 'retired_node_delete_action' -or
        $teardownText -notmatch 'retired_node_delete_exit_code') {
        $violations.Add("phase-bound teardown does not remove only the exact post-OEM retired child")
    }
    $sourceOutsideTeardown = $text
    $allowedCleanupDefinitions = @(
        $teardownDefinition, $publishedOnlyTeardownDefinition,
        $rootRemovalDefinition, $rootRemovedContinuationDefinition) |
        Where-Object { $null -ne $_ } | Sort-Object { $_.Extent.StartOffset } -Descending
    foreach ($allowedCleanupDefinition in $allowedCleanupDefinitions) {
        $sourceOutsideTeardown = $sourceOutsideTeardown.Remove(
            $allowedCleanupDefinition.Extent.StartOffset,
            $allowedCleanupDefinition.Extent.EndOffset - $allowedCleanupDefinition.Extent.StartOffset)
    }
    foreach ($forbiddenOutsideTeardown in @(
            '(?i)\bsc\.exe\s+(?:stop|delete)\b',
            '(?i)\bpnputil\.exe\s+/(?:remove-device|delete-driver)\b')) {
        if ($sourceOutsideTeardown -match $forbiddenOutsideTeardown) {
            $violations.Add("destructive guest cleanup is outside the exact current-run teardown: $forbiddenOutsideTeardown")
        }
    }
}
if ($null -eq $publishedOnlyTeardownDefinition -or
    $publishedOnlyTeardownDefinition.Extent.Text -notmatch
        'pnputil\.exe\s+/delete-driver\s+\$PublishedInf\s+/uninstall' -or
    $publishedOnlyTeardownDefinition.Extent.Text -match '(?i)(?:^|\s)/force(?:\s|$)') {
    $violations.Add("published-only cleanup is missing its exact non-force OEM deletion")
}
$orderedMainNeedles = @(
    '$signerTrust = Install-GuestVerifierSignerTrust',
    '$testSigningEnable = Set-GuestVerifierTestSigning -Enabled $true',
    '$testSigningPostBoot = Get-GuestVerifierTestSigningState',
    '$guestSignature = Get-GuestVerifierGuestSignatureEvidence',
    '$publish = Publish-GuestVerifierPackage',
    'Write-GuestVerifierArtifact -Name "signed-package-publish.json"',
    '$rootCreation = Create-GuestVerifierRoot',
    'Write-GuestVerifierArtifact -Name "signed-package-root.json"',
    'Write-GuestVerifierArtifact -Name "signed-package-install.json"',
    '$reset = Reset-GuestVerifier',
    '$resetRestart = Request-GuestVerifierRestart',
    '$resetQuery = Get-GuestVerifierQuery',
    '$currentRunTeardownBinding = Get-GuestVerifierCurrentRunTeardownBinding',
    '$rootRemoval = Remove-GuestVerifierCurrentRunRoot',
    '$rootRemovedActions = Remove-GuestVerifierRootRemovedArtifacts',
    '$signerTrustRemoval = Remove-GuestVerifierSignerTrust',
    '$testSigningDisable = Set-GuestVerifierTestSigning -Enabled $false',
    '$testSigningDisableRestart = Request-GuestVerifierRestart',
    '$finalCurrentRunZeroResidue = Get-GuestVerifierCurrentRunZeroResidueEvidence',
    '$finalPreflight = Invoke-GuestVerifierPreflight'
)
$lastNeedleIndex = -1
foreach ($needle in $orderedMainNeedles) {
    $needleIndex = $text.IndexOf($needle, [StringComparison]::Ordinal)
    if ($needleIndex -lt 0 -or $needleIndex -le $lastNeedleIndex) {
        $violations.Add("TestSigning contract order is missing or unsafe at $needle")
        break
    }
    $lastNeedleIndex = $needleIndex
}

$publishReceiptIndex = $text.IndexOf(
    'Write-GuestVerifierArtifact -Name "signed-package-publish.json"',
    [StringComparison]::Ordinal)
$rootCallIndex = $text.IndexOf('$rootCreation = Create-GuestVerifierRoot', [StringComparison]::Ordinal)
if ($publishReceiptIndex -lt 0 -or $rootCallIndex -lt 0 -or
    $publishReceiptIndex -ge $rootCallIndex) {
    $violations.Add("guest_verifier_publish_receipt_precedes_root_creation failed")
}
$rootRemovalCallIndex = $text.IndexOf('$rootRemoval = Remove-GuestVerifierCurrentRunRoot', [StringComparison]::Ordinal)
$rootRemovalReceiptIndex = $text.IndexOf(
    'Write-GuestVerifierArtifact -Name "current-run-root-removal.json"',
    [StringComparison]::Ordinal)
$rootRemovalRestartIndex = $text.IndexOf('$rootRemovalRestart = Request-GuestVerifierRestart', [StringComparison]::Ordinal)
$rootRemovedContinuationIndex = $text.IndexOf(
    '$rootRemovedActions = Remove-GuestVerifierRootRemovedArtifacts',
    [StringComparison]::Ordinal)
if ($rootRemovalCallIndex -lt 0 -or $rootRemovalReceiptIndex -le $rootRemovalCallIndex -or
    $rootRemovalRestartIndex -le $rootRemovalReceiptIndex -or
    $rootRemovedContinuationIndex -le $rootRemovalRestartIndex) {
    $violations.Add("guest_verifier_root_removal_reboots_before_service_delete failed: receipt/reboot/continuation order is wrong")
}

foreach ($phase in @(
        "host_preflight", "guest_preflight", "signer_trust", "testsigning_enable",
        "package_publish", "root_creation", "normal_restart", "normal_io",
        "verifier_enable", "verifier_restart", "verifier_io", "verifier_reset",
        "current_run_teardown", "signer_trust_remove", "testsigning_disable",
        "final_zero_residue")) {
    $phaseNeedle = '$failurePhase = "' + $phase + '"'
    if ($text.IndexOf($phaseNeedle, [StringComparison]::Ordinal) -lt 0) {
        $violations.Add("guest_verifier_failure_phase_tracks_lifecycle failed: missing $phase")
    }
}
$resetQueryIndex = $text.IndexOf('$resetQuery = Get-GuestVerifierQuery', [StringComparison]::Ordinal)
$verifierDisarmedAfterResetIndex = if ($resetQueryIndex -ge 0) {
    $text.IndexOf('$verifierArmed = $false', $resetQueryIndex, [StringComparison]::Ordinal)
}
else { -1 }
if ($resetQueryIndex -lt 0 -or $verifierDisarmedAfterResetIndex -le $resetQueryIndex) {
    $violations.Add("guest_verifier_reset_pending_target_requires_reboot failed: verifier disarmed before post-boot query")
}

if ($violations.Count -ne 0) {
    throw ("guest_verifier_static_contract_is_green failed: " +
        ($violations -join "; "))
}
Write-Output "PASS guest_verifier_testsigning_order_is_exact"
Write-Output "PASS guest_verifier_failure_phase_tracks_lifecycle"
Write-Output "PASS guest_verifier_root_removal_reboots_before_service_delete"

foreach ($functionName in @(
        "Normalize-GuestVerifierSha256",
        "Normalize-GuestVerifierSignerThumbprint",
        "Normalize-GuestVerifierSignerSubject",
        "Normalize-GuestVerifierPublishedInf",
        "Get-GuestVerifierPublicCertificateIdentity",
        "Assert-GuestVerifierSignerIdentity",
        "Assert-GuestVerifierSignatureRelationship",
        "Get-GuestVerifierInputBinding",
        "Assert-GuestVerifierInputBindingUnchanged",
        "Assert-GuestVerifierRestartReceipt",
        "Assert-GuestVerifierBootChangeReceipt",
        "Assert-GuestVerifierTestSigningReceipt",
        "Assert-GuestVerifierTestSigningState",
        "Assert-GuestVerifierSignerTrustReceipt",
        "Assert-GuestVerifierSignerTrustRemovalReceipt",
        "Assert-GuestVerifierGuestSignatureEvidence",
        "Assert-GuestVerifierIoctlVerdict",
        "Assert-GuestVerifierDumpObservation",
        "Assert-GuestVerifierResidueWaitEvidence",
        "Assert-GuestVerifierPassEvidence",
        "Assert-GuestVerifierPreflight",
        "Assert-GuestVerifierCurrentIdentity",
        "Assert-GuestVerifierCurrentRunTeardownBinding",
        "Assert-GuestVerifierCurrentRunTeardownEvidence",
        "Get-GuestVerifierPostPublishCleanupMode",
        "Test-GuestVerifierPostRootRemovalState",
        "Assert-GuestVerifierPostRootRemovalState",
        "Get-GuestVerifierTargetLines",
        "Assert-GuestVerifierEnabled",
        "Assert-GuestVerifierReset",
        "Get-GuestVerifierConnectTimeoutSeconds")) {
    Import-ProductionFunction $functionName $text
}

$shortConnectDeadline = Get-GuestVerifierConnectTimeoutSeconds `
    -OperationTimeoutSeconds 45 -ConfiguredConnectTimeoutSeconds 180
if ($shortConnectDeadline -ne 44) {
    throw "guest_verifier_connect_deadline_is_nested failed: short operation did not reserve outer headroom"
}
$longConnectDeadline = Get-GuestVerifierConnectTimeoutSeconds `
    -OperationTimeoutSeconds 420 -ConfiguredConnectTimeoutSeconds 180
if ($longConnectDeadline -ne 180) {
    throw "guest_verifier_connect_deadline_is_nested failed: configured ceiling was not preserved"
}
Write-Output "PASS guest_verifier_connect_deadline_is_nested"

$publishedOnlyState = [pscustomobject]@{
    schema = [int]1
    run_id = "manufactured-run"
    published_inf = "oem2.inf"
    package_count = [int]1
    published_inf_count = [int]1
    package_original_inf = "ramshared.inf"
    driver_store_sys_sha256 = "A" * 64
    driver_store_inf_sha256 = "B" * 64
    driver_store_catalog_sha256 = "C" * 64
    root_count = [int]0
    root_instance_id = ""
    hardware_id = ""
    service_count = [int]0
    service_name = ""
    service_state = ""
    service_sha256 = ""
    service_inf_sha256 = ""
    service_catalog_sha256 = ""
    ramshared_disk_count = [int]0
    ramshared_pnp_disk_count = [int]0
    ramshared_present_pnp_disk_count = [int]0
    ramshared_retired_pnp_disk_count = [int]0
    retired_pnp_instance_id = ""
}
$publishedOnlyMode = Get-GuestVerifierPostPublishCleanupMode -State $publishedOnlyState `
    -RunId "manufactured-run" -ExpectedPublishedInf "oem2.inf" `
    -ExpectedDriverHash ("A" * 64) -ExpectedInfHash ("B" * 64) `
    -ExpectedCatalogHash ("C" * 64)
if ($publishedOnlyMode -cne "published_only") {
    throw "guest_verifier_publish_only_cleanup_is_exact failed"
}
Write-Output "PASS guest_verifier_publish_only_cleanup_is_exact"

$rootBoundState = $publishedOnlyState.psobject.Copy()
$rootBoundState.root_count = [int]1
$rootBoundState.root_instance_id = "ROOT\RAMSHARED\0000"
$rootBoundState.hardware_id = "ROOT\RAMSHARED"
$rootBoundState.service_count = [int]1
$rootBoundState.service_name = "ramshared"
$rootBoundState.service_state = "Running"
$rootBoundState.service_sha256 = "A" * 64
$rootBoundState.service_inf_sha256 = "B" * 64
$rootBoundState.service_catalog_sha256 = "C" * 64
$rootBoundMode = Get-GuestVerifierPostPublishCleanupMode -State $rootBoundState `
    -RunId "manufactured-run" -ExpectedPublishedInf "oem2.inf" `
    -ExpectedDriverHash ("A" * 64) -ExpectedInfHash ("B" * 64) `
    -ExpectedCatalogHash ("C" * 64)
if ($rootBoundMode -cne "root_bound") {
    throw "guest_verifier_stage2_failure_branches_on_live_identity failed"
}
$rootBoundRetired = $rootBoundState.psobject.Copy()
$rootBoundRetired.ramshared_pnp_disk_count = [int]1
$rootBoundRetired.ramshared_present_pnp_disk_count = [int]0
$rootBoundRetired.ramshared_retired_pnp_disk_count = [int]1
$rootBoundRetired.retired_pnp_instance_id = "SCSI\DISK&VEN_RAMSHARE&PROD_VRAMDISK\1&TEST&0&000000"
$rootBoundRetiredMode = Get-GuestVerifierPostPublishCleanupMode -State $rootBoundRetired `
    -RunId "manufactured-run" -ExpectedPublishedInf "oem2.inf" `
    -ExpectedDriverHash ("A" * 64) -ExpectedInfHash ("B" * 64) `
    -ExpectedCatalogHash ("C" * 64)
if ($rootBoundRetiredMode -cne "root_bound") {
    throw "guest_verifier_pre_root_retired_state_is_exact failed: exact retired state was refused"
}
foreach ($invalidRetiredState in @(
        $(
            $presentRetired = $rootBoundRetired.psobject.Copy()
            $presentRetired.ramshared_present_pnp_disk_count = [int]1
            $presentRetired
        ),
        $(
            $multipleRetired = $rootBoundRetired.psobject.Copy()
            $multipleRetired.ramshared_pnp_disk_count = [int]2
            $multipleRetired.ramshared_retired_pnp_disk_count = [int]2
            $multipleRetired
        ),
        $(
            $foreignRetired = $rootBoundRetired.psobject.Copy()
            $foreignRetired.retired_pnp_instance_id = "SCSI\DISK&VEN_FOREIGN&PROD_DISK\1&TEST&0&000000"
            $foreignRetired
        ))) {
    Assert-Throws {
        Get-GuestVerifierPostPublishCleanupMode -State $invalidRetiredState `
            -RunId "manufactured-run" -ExpectedPublishedInf "oem2.inf" `
            -ExpectedDriverHash ("A" * 64) -ExpectedInfHash ("B" * 64) `
            -ExpectedCatalogHash ("C" * 64) | Out-Null
    } "guest_verifier_pre_root_retired_state_is_exact"
}
Write-Output "PASS guest_verifier_pre_root_retired_state_is_exact"
if ($text -match '\$hardwareIds\s*=\s*if\s*\(' -or
    $text -match '\[string\]\$hardwareIds\[0\]' -or
    ([regex]::Matches($text, '\[string\]\(\$hardwareIds\[0\]\)')).Count -lt 3) {
    throw "guest_verifier_hardware_id_preserves_complete_string failed"
}
Write-Output "PASS guest_verifier_hardware_id_preserves_complete_string"
$serviceWithoutRoot = $rootBoundState.psobject.Copy()
$serviceWithoutRoot.root_count = [int]0
$serviceWithoutRoot.root_instance_id = ""
$serviceWithoutRoot.hardware_id = ""
Assert-Throws {
    Get-GuestVerifierPostPublishCleanupMode -State $serviceWithoutRoot `
        -RunId "manufactured-run" -ExpectedPublishedInf "oem2.inf" `
        -ExpectedDriverHash ("A" * 64) -ExpectedInfHash ("B" * 64) `
        -ExpectedCatalogHash ("C" * 64) | Out-Null
} "guest_verifier_stage2_failure_branches_on_live_identity"
$diskPublished = $rootBoundState.psobject.Copy()
$diskPublished.ramshared_disk_count = [int]1
Assert-Throws {
    Get-GuestVerifierPostPublishCleanupMode -State $diskPublished `
        -RunId "manufactured-run" -ExpectedPublishedInf "oem2.inf" `
        -ExpectedDriverHash ("A" * 64) -ExpectedInfHash ("B" * 64) `
        -ExpectedCatalogHash ("C" * 64) | Out-Null
} "guest_verifier_stage2_failure_branches_on_live_identity"
Write-Output "PASS guest_verifier_stage2_failure_branches_on_live_identity"

$testRoot = Join-Path ([IO.Path]::GetTempPath()) (
    "ramshared-guest-verifier-static-" + [guid]::NewGuid().ToString("N"))
$testRsa = $null
$testCertificate = $null
$testPublicCertificate = $null
try {
    $driverPackage = Join-Path $testRoot "driver-package"
    $hostBin = Join-Path $testRoot "host-bin"
    New-Item -ItemType Directory -Force -Path $driverPackage, $hostBin | Out-Null
    [IO.File]::WriteAllText((Join-Path $driverPackage "ramshared.sys"), "driver")
    [IO.File]::WriteAllText((Join-Path $driverPackage "ramshared.inf"), @'
DriverVer = 08/09/2026,10.0.26200.8
'@)
    [IO.File]::WriteAllText((Join-Path $driverPackage "ramshared.cat"), "catalog")
    [IO.File]::WriteAllText((Join-Path $hostBin "Invoke-WinDriveIoctlValidation.ps1"),
        "Write-Output 'validator'")
    $certPath = Join-Path $testRoot "ramshared-signer.cer"
    $testRsa = [Security.Cryptography.RSA]::Create(2048)
    $certificateRequest = New-Object Security.Cryptography.X509Certificates.CertificateRequest(
        "CN=RamShared Guest Verifier Static Test", $testRsa,
        [Security.Cryptography.HashAlgorithmName]::SHA256,
        [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    $testCertificate = $certificateRequest.CreateSelfSigned(
        [datetimeoffset]::UtcNow.AddDays(-1), [datetimeoffset]::UtcNow.AddDays(1))
    [IO.File]::WriteAllBytes($certPath, $testCertificate.Export(
        [Security.Cryptography.X509Certificates.X509ContentType]::Cert))
    $testPublicCertificate = New-Object Security.Cryptography.X509Certificates.X509Certificate2($certPath)

    $expected = @{}
    foreach ($pair in @(
            @("Driver", (Join-Path $driverPackage "ramshared.sys")),
            @("Inf", (Join-Path $driverPackage "ramshared.inf")),
            @("Catalog", (Join-Path $driverPackage "ramshared.cat")),
            @("Ioctl", (Join-Path $hostBin "Invoke-WinDriveIoctlValidation.ps1")),
            @("SignerCert", $certPath))) {
        $expected[$pair[0]] = (Get-FileHash -LiteralPath $pair[1] -Algorithm SHA256).Hash
    }
    $expected.SignerSubject = [string]$testPublicCertificate.Subject
    $expected.SignerThumbprint = [string]$testPublicCertificate.Thumbprint
    $binding = Get-GuestVerifierInputBinding -DriverPackage $driverPackage `
        -HostBinDir $hostBin -ExpectedDriverSha256 $expected.Driver `
        -ExpectedInfSha256 $expected.Inf -ExpectedCatalogSha256 $expected.Catalog `
        -ExpectedIoctlValidationSha256 $expected.Ioctl -DriverSignerCert $certPath `
        -ExpectedDriverSignerCertSha256 $expected.SignerCert `
        -ExpectedDriverSignerSubject $expected.SignerSubject `
        -ExpectedDriverSignerThumbprint $expected.SignerThumbprint
    Assert-GuestVerifierInputBindingUnchanged -Binding $binding | Out-Null
    Write-Output "PASS guest_verifier_immutable_input_binding"

    Assert-Throws {
        Get-GuestVerifierInputBinding -DriverPackage $driverPackage -HostBinDir $hostBin `
            -ExpectedDriverSha256 $expected.Driver -ExpectedInfSha256 $expected.Inf `
            -ExpectedCatalogSha256 $expected.Catalog `
            -ExpectedIoctlValidationSha256 $expected.Ioctl -DriverSignerCert $certPath `
            -ExpectedDriverSignerCertSha256 ("0" * 64) `
            -ExpectedDriverSignerSubject $expected.SignerSubject `
            -ExpectedDriverSignerThumbprint $expected.SignerThumbprint | Out-Null
    } "guest_verifier_cert_hash_is_exact"
    Write-Output "PASS guest_verifier_cert_hash_is_exact"

    $privateKeyPath = Join-Path $testRoot "private-signer.pfx"
    [IO.File]::WriteAllText($privateKeyPath, "manufactured private-key input must be refused")
    Assert-Throws {
        Get-GuestVerifierInputBinding -DriverPackage $driverPackage -HostBinDir $hostBin `
            -ExpectedDriverSha256 $expected.Driver -ExpectedInfSha256 $expected.Inf `
            -ExpectedCatalogSha256 $expected.Catalog `
            -ExpectedIoctlValidationSha256 $expected.Ioctl -DriverSignerCert $privateKeyPath `
            -ExpectedDriverSignerCertSha256 $expected.SignerCert `
            -ExpectedDriverSignerSubject $expected.SignerSubject `
            -ExpectedDriverSignerThumbprint $expected.SignerThumbprint | Out-Null
    } "guest_verifier_private_key_input_is_refused"
    Write-Output "PASS guest_verifier_private_key_input_is_refused"

    $validSigner = [pscustomobject]@{
        Subject = $expected.SignerSubject
        Thumbprint = $expected.SignerThumbprint
        HasPrivateKey = $false
    }
    Assert-GuestVerifierSignerIdentity -Certificate $validSigner `
        -ExpectedSubject $expected.SignerSubject `
        -ExpectedThumbprint $expected.SignerThumbprint -Role "manufactured" | Out-Null
    $wrongSubjectSigner = $validSigner.psobject.Copy()
    $wrongSubjectSigner.Subject = "CN=Foreign signer"
    Assert-Throws {
        Assert-GuestVerifierSignerIdentity -Certificate $wrongSubjectSigner `
            -ExpectedSubject $expected.SignerSubject `
            -ExpectedThumbprint $expected.SignerThumbprint -Role "manufactured" | Out-Null
    } "guest_verifier_signer_subject_thumbprint_is_exact"
    $wrongThumbprintSigner = $validSigner.psobject.Copy()
    $wrongThumbprintSigner.Thumbprint = ("0" * 40)
    Assert-Throws {
        Assert-GuestVerifierSignerIdentity -Certificate $wrongThumbprintSigner `
            -ExpectedSubject $expected.SignerSubject `
            -ExpectedThumbprint $expected.SignerThumbprint -Role "manufactured" | Out-Null
    } "guest_verifier_signer_subject_thumbprint_is_exact"
    $validDriverSignature = [pscustomobject]@{
        Status = "Valid"
        SignerCertificate = $validSigner
    }
    $validCatalogSignature = [pscustomobject]@{
        Status = "Valid"
        SignerCertificate = $validSigner
    }
    Assert-GuestVerifierSignatureRelationship -DriverSignature $validDriverSignature `
        -CatalogSignature $validCatalogSignature -ExpectedSubject $expected.SignerSubject `
        -ExpectedThumbprint $expected.SignerThumbprint | Out-Null
    Write-Output "PASS guest_verifier_signer_subject_thumbprint_is_exact"

    [IO.File]::AppendAllText((Join-Path $driverPackage "ramshared.sys"), "changed")
    Assert-Throws {
        Assert-GuestVerifierInputBindingUnchanged -Binding $binding | Out-Null
    } "guest_verifier_hash_mutation_is_red"
    Write-Output "PASS guest_verifier_hash_mutation_is_red"

    Assert-Throws {
        Get-GuestVerifierInputBinding -DriverPackage $driverPackage -HostBinDir $hostBin `
            -ExpectedDriverSha256 ("0" * 64) -ExpectedInfSha256 $expected.Inf `
            -ExpectedCatalogSha256 $expected.Catalog `
            -ExpectedIoctlValidationSha256 $expected.Ioctl -DriverSignerCert $certPath `
            -ExpectedDriverSignerCertSha256 $expected.SignerCert `
            -ExpectedDriverSignerSubject $expected.SignerSubject `
            -ExpectedDriverSignerThumbprint $expected.SignerThumbprint | Out-Null
    } "guest_verifier_hash_input_is_exact"
    Write-Output "PASS guest_verifier_hash_input_is_exact"

    $validRestart = [pscustomobject]@{
        shutdown_scheduled = $true
        action = "restart"
        delay_seconds = [int]15
    }
    Assert-GuestVerifierRestartReceipt -Receipt $validRestart -ExpectedDelaySeconds 15 |
        Out-Null
    Assert-Throws {
        Assert-GuestVerifierRestartReceipt -Receipt ([pscustomobject]@{
                shutdown_scheduled = $true
                action = "shutdown"
                delay_seconds = [int]15
            }) -ExpectedDelaySeconds 15 | Out-Null
    } "guest_verifier_reset_reboot_is_required"
    Write-Output "PASS guest_verifier_reset_reboot_is_required"

    $validBootChange = [pscustomobject]@{
        before_boot_utc = "2026-08-09T00:00:00.0000000Z"
        after_boot_utc = "2026-08-09T00:01:00.0000000Z"
    }
    Assert-GuestVerifierBootChangeReceipt -Receipt $validBootChange | Out-Null
    $missingTestSigningReboot = $validBootChange.psobject.Copy()
    $missingTestSigningReboot.after_boot_utc = $missingTestSigningReboot.before_boot_utc
    Assert-Throws {
        Assert-GuestVerifierBootChangeReceipt -Receipt $missingTestSigningReboot | Out-Null
    } "guest_verifier_testsigning_reboot_is_required"
    Write-Output "PASS guest_verifier_testsigning_reboot_is_required"

    $validTestSigningEnable = [pscustomobject]@{
        schema = [int]1
        action = "enable"
        set_exit_code = [int]0
        query_exit_code = [int]0
        testsigning_enabled = $true
    }
    Assert-GuestVerifierTestSigningReceipt -Evidence $validTestSigningEnable -ExpectedEnabled $true | Out-Null
    $bcdeditFailure = $validTestSigningEnable.psobject.Copy()
    $bcdeditFailure.set_exit_code = [int]1
    Assert-Throws {
        Assert-GuestVerifierTestSigningReceipt -Evidence $bcdeditFailure -ExpectedEnabled $true | Out-Null
    } "guest_verifier_bcdedit_error_is_red"
    Write-Output "PASS guest_verifier_bcdedit_error_is_red"

    $validVerdict = [pscustomobject]@{
        DRIVER = "ramshared.sys"
        VERIFIER = $true
        PASS_VALID_QUEUE = 1
        REFUSE_FOREIGN_OWNER = 1
        REFUSE_RESERVED_REGISTER = 1
        REFUSE_BAD_RING = 1
        REFUSE_RING_INDEX_JUMP = 1
        REFUSE_RESERVED_CQE = 1
        REFUSE_UNKNOWN_IOCTL = 1
        REFUSE_RESERVED_DISK_PARAMS = 1
        COMPLETION_REENTRY_NO_SLOT_REUSE = 1
        RUNDOWN_UNMAP_AFTER_COPY = 1
        VPD_SERIAL_MATCH = 1
        EXACT_VIRTUAL_NONROTATING_IDENTITY = 1
        NO_NEW_DUMP = 1
    }
    Assert-GuestVerifierIoctlVerdict -Verdict $validVerdict -VerifierExpected $true |
        Out-Null
    $cleanPreflight = [pscustomobject]@{
        schema = [int]1
        package_count = [int]0
        service_count = [int]0
        root_count = [int]0
        ramshared_disk_count = [int]0
        ramshared_pnp_disk_count = [int]0
        verifier_query_exit_code = [int]0
        verifier_target_present = $false
        verifier_target_count = [int]0
        verifier_all_drivers = $false
        testsigning_query_exit_code = [int]0
        testsigning_enabled = $false
        root_expected_thumbprint_count = [int]0
        trusted_publisher_expected_thumbprint_count = [int]0
        root_foreign_subject_count = [int]0
        trusted_publisher_foreign_subject_count = [int]0
    }
    Assert-GuestVerifierPreflight -Evidence $cleanPreflight | Out-Null
    $foreignVerifierPreflight = $cleanPreflight.psobject.Copy()
    $foreignVerifierPreflight.verifier_target_count = [int]1
    Assert-Throws {
        Assert-GuestVerifierPreflight -Evidence $foreignVerifierPreflight | Out-Null
    } "guest_verifier_preexisting_state_is_refused"
    Write-Output "PASS guest_verifier_preexisting_state_is_refused"
    $ambiguousSubjectPreflight = $cleanPreflight.psobject.Copy()
    $ambiguousSubjectPreflight.root_foreign_subject_count = [int]1
    Assert-Throws {
        Assert-GuestVerifierPreflight -Evidence $ambiguousSubjectPreflight | Out-Null
    } "guest_verifier_preexisting_matching_subject_cert_is_refused"
    Write-Output "PASS guest_verifier_preexisting_matching_subject_cert_is_refused"

    $validTrustReceipt = [pscustomobject]@{
        schema = [int]1
        subject = $expected.SignerSubject
        thumbprint = $expected.SignerThumbprint
        has_private_key = $false
        root_expected_thumbprint_count = [int]1
        trusted_publisher_expected_thumbprint_count = [int]1
        root_foreign_subject_count = [int]0
        trusted_publisher_foreign_subject_count = [int]0
    }
    Assert-GuestVerifierSignerTrustReceipt -Evidence $validTrustReceipt `
        -ExpectedSubject $expected.SignerSubject -ExpectedThumbprint $expected.SignerThumbprint | Out-Null
    $validTrustRemoval = [pscustomobject]@{
        schema = [int]1
        subject = $expected.SignerSubject
        thumbprint = $expected.SignerThumbprint
        root_expected_thumbprint_count = [int]0
        trusted_publisher_expected_thumbprint_count = [int]0
        root_subject_count = [int]0
        trusted_publisher_subject_count = [int]0
    }
    Assert-GuestVerifierSignerTrustRemovalReceipt -Evidence $validTrustRemoval `
        -ExpectedSubject $expected.SignerSubject -ExpectedThumbprint $expected.SignerThumbprint | Out-Null
    $failedTrustRemoval = $validTrustRemoval.psobject.Copy()
    $failedTrustRemoval.trusted_publisher_expected_thumbprint_count = [int]1
    Assert-Throws {
        Assert-GuestVerifierSignerTrustRemovalReceipt -Evidence $failedTrustRemoval `
            -ExpectedSubject $expected.SignerSubject -ExpectedThumbprint $expected.SignerThumbprint | Out-Null
    } "guest_verifier_cert_cleanup_failure_is_red"
    Write-Output "PASS guest_verifier_cert_cleanup_failure_is_red"

    $validGuestSignature = [pscustomobject]@{
        schema = [int]1
        driver_status = "Valid"
        catalog_status = "Valid"
        driver_subject = $expected.SignerSubject
        catalog_subject = $expected.SignerSubject
        driver_thumbprint = $expected.SignerThumbprint
        catalog_thumbprint = $expected.SignerThumbprint
    }
    Assert-GuestVerifierGuestSignatureEvidence -Evidence $validGuestSignature `
        -ExpectedSubject $expected.SignerSubject -ExpectedThumbprint $expected.SignerThumbprint | Out-Null
    $untrustedGuestSignature = $validGuestSignature.psobject.Copy()
    $untrustedGuestSignature.driver_status = "UnknownError"
    Assert-Throws {
        Assert-GuestVerifierGuestSignatureEvidence -Evidence $untrustedGuestSignature `
            -ExpectedSubject $expected.SignerSubject -ExpectedThumbprint $expected.SignerThumbprint | Out-Null
    } "guest_verifier_signature_still_untrusted_is_red"
    Write-Output "PASS guest_verifier_signature_still_untrusted_is_red"

    $validIdentity = [pscustomobject]@{
        schema = [int]1
        run_id = "manufactured-run"
        root_count = [int]1
        scsi_count = [int]1
        running_service_count = [int]1
        root_instance_id = "ROOT\RAMSHARED\0000"
        scsi_instance_id = "ROOT\RAMSHARED\0000"
        loaded_path = "C:\Windows\System32\DriverStore\FileRepository\ramshared.inf_amd64\ramshared.sys"
        loaded_sha256 = ("A" * 64)
        staged_ioctl_sha256 = ("B" * 64)
        binary_match = $true
    }
    Assert-GuestVerifierCurrentIdentity -Identity $validIdentity -RunId "manufactured-run" -ExpectedDriverHash ("A" * 64) -ExpectedIoctlHash ("B" * 64) | Out-Null
    $ambiguousIdentity = $validIdentity.psobject.Copy()
    $ambiguousIdentity.root_count = [int]2
    Assert-Throws {
        Assert-GuestVerifierCurrentIdentity -Identity $ambiguousIdentity -RunId "manufactured-run" -ExpectedDriverHash ("A" * 64) -ExpectedIoctlHash ("B" * 64) | Out-Null
    } "guest_verifier_loaded_identity_is_exact"
    Write-Output "PASS guest_verifier_loaded_identity_is_exact"

    $expectedTeardownDriverHash = ("A" * 64)
    $expectedTeardownInfHash = ("B" * 64)
    $expectedTeardownCatalogHash = ("C" * 64)
    $expectedTeardownHardwareId = "ROOT\RAMSHARED"
    $expectedTeardownVpdSerial = "ABCDEF0123456789"
    $expectedTeardownServiceName = "ramshared"
    $validPublishedInf = Normalize-GuestVerifierPublishedInf -Value "oem42.inf" -Name "manufactured published INF"
    if ($validPublishedInf -cne "oem42.inf") {
        throw "guest_verifier_published_inf_is_exact failed: canonical OEM INF was changed"
    }
    Assert-Throws {
        Normalize-GuestVerifierPublishedInf -Value "ramshared.inf" -Name "manufactured published INF" | Out-Null
    } "guest_verifier_published_inf_is_exact"
    Assert-Throws {
        Normalize-GuestVerifierPublishedInf -Value "oem42.inf " -Name "manufactured published INF" | Out-Null
    } "guest_verifier_published_inf_is_exact"
    Write-Output "PASS guest_verifier_published_inf_is_exact"

    $validTeardownBinding = [pscustomobject]@{
        schema = [int]1
        run_id = "manufactured-run"
        published_inf = $validPublishedInf
        package_count = [int]1
        package_original_inf = "ramshared.inf"
        root_count = [int]1
        root_instance_id = "ROOT\RAMSHARED\0000"
        hardware_id = $expectedTeardownHardwareId
        service_count = [int]1
        service_name = $expectedTeardownServiceName
        service_state = "Running"
        service_path = "C:\Windows\System32\DriverStore\FileRepository\ramshared.inf_amd64\ramshared.sys"
        loaded_driver_sha256 = $expectedTeardownDriverHash
        driver_store_inf_sha256 = $expectedTeardownInfHash
        driver_store_catalog_sha256 = $expectedTeardownCatalogHash
        io_pass_started = $true
        normal_vpd_state = "validated"
        normal_vpd_serial = $expectedTeardownVpdSerial
        verifier_vpd_state = "validated"
        verifier_vpd_serial = $expectedTeardownVpdSerial
        binary_match = $true
    }
    Assert-GuestVerifierCurrentRunTeardownBinding -Binding $validTeardownBinding `
        -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf `
        -ExpectedDriverHash $expectedTeardownDriverHash -ExpectedInfHash $expectedTeardownInfHash `
        -ExpectedCatalogHash $expectedTeardownCatalogHash -ExpectedHardwareId $expectedTeardownHardwareId `
        -ExpectedVpdSerial $expectedTeardownVpdSerial -ExpectedServiceName $expectedTeardownServiceName | Out-Null
    $ambiguousTeardownBinding = $validTeardownBinding.psobject.Copy()
    $ambiguousTeardownBinding.package_count = [int]2
    Assert-Throws {
        Assert-GuestVerifierCurrentRunTeardownBinding -Binding $ambiguousTeardownBinding `
            -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf `
            -ExpectedDriverHash $expectedTeardownDriverHash -ExpectedInfHash $expectedTeardownInfHash `
            -ExpectedCatalogHash $expectedTeardownCatalogHash -ExpectedHardwareId $expectedTeardownHardwareId `
            -ExpectedVpdSerial $expectedTeardownVpdSerial -ExpectedServiceName $expectedTeardownServiceName | Out-Null
    } "guest_verifier_teardown_ambiguity_is_refused"
    Write-Output "PASS guest_verifier_teardown_ambiguity_is_refused"

    $foreignTeardownBinding = $validTeardownBinding.psobject.Copy()
    $foreignTeardownBinding.hardware_id = "ROOT\FOREIGN"
    Assert-Throws {
        Assert-GuestVerifierCurrentRunTeardownBinding -Binding $foreignTeardownBinding `
            -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf `
            -ExpectedDriverHash $expectedTeardownDriverHash -ExpectedInfHash $expectedTeardownInfHash `
            -ExpectedCatalogHash $expectedTeardownCatalogHash -ExpectedHardwareId $expectedTeardownHardwareId `
            -ExpectedVpdSerial $expectedTeardownVpdSerial -ExpectedServiceName $expectedTeardownServiceName | Out-Null
    } "guest_verifier_teardown_foreign_identity_is_refused"
    Write-Output "PASS guest_verifier_teardown_foreign_identity_is_refused"

    $wrongHashTeardownBinding = $validTeardownBinding.psobject.Copy()
    $wrongHashTeardownBinding.loaded_driver_sha256 = ("0" * 64)
    Assert-Throws {
        Assert-GuestVerifierCurrentRunTeardownBinding -Binding $wrongHashTeardownBinding `
            -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf `
            -ExpectedDriverHash $expectedTeardownDriverHash -ExpectedInfHash $expectedTeardownInfHash `
            -ExpectedCatalogHash $expectedTeardownCatalogHash -ExpectedHardwareId $expectedTeardownHardwareId `
            -ExpectedVpdSerial $expectedTeardownVpdSerial -ExpectedServiceName $expectedTeardownServiceName | Out-Null
    } "guest_verifier_teardown_hash_serial_service_is_exact"
    $wrongSerialTeardownBinding = $validTeardownBinding.psobject.Copy()
    $wrongSerialTeardownBinding.normal_vpd_serial = "FOREIGN000000000"
    Assert-Throws {
        Assert-GuestVerifierCurrentRunTeardownBinding -Binding $wrongSerialTeardownBinding `
            -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf `
            -ExpectedDriverHash $expectedTeardownDriverHash -ExpectedInfHash $expectedTeardownInfHash `
            -ExpectedCatalogHash $expectedTeardownCatalogHash -ExpectedHardwareId $expectedTeardownHardwareId `
            -ExpectedVpdSerial $expectedTeardownVpdSerial -ExpectedServiceName $expectedTeardownServiceName | Out-Null
    } "guest_verifier_teardown_hash_serial_service_is_exact"
    $wrongServiceTeardownBinding = $validTeardownBinding.psobject.Copy()
    $wrongServiceTeardownBinding.service_name = "foreignservice"
    Assert-Throws {
        Assert-GuestVerifierCurrentRunTeardownBinding -Binding $wrongServiceTeardownBinding `
            -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf `
            -ExpectedDriverHash $expectedTeardownDriverHash -ExpectedInfHash $expectedTeardownInfHash `
            -ExpectedCatalogHash $expectedTeardownCatalogHash -ExpectedHardwareId $expectedTeardownHardwareId `
            -ExpectedVpdSerial $expectedTeardownVpdSerial -ExpectedServiceName $expectedTeardownServiceName | Out-Null
    } "guest_verifier_teardown_hash_serial_service_is_exact"
    Write-Output "PASS guest_verifier_teardown_hash_serial_service_is_exact"

    $phaseBoundTeardown = $validTeardownBinding.psobject.Copy()
    $phaseBoundTeardown.service_state = "Stopped"
    $phaseBoundTeardown.verifier_vpd_state = "not_executed"
    $phaseBoundTeardown.verifier_vpd_serial = ""
    Assert-GuestVerifierCurrentRunTeardownBinding -Binding $phaseBoundTeardown `
        -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf `
        -ExpectedDriverHash $expectedTeardownDriverHash -ExpectedInfHash $expectedTeardownInfHash `
        -ExpectedCatalogHash $expectedTeardownCatalogHash -ExpectedHardwareId $expectedTeardownHardwareId `
        -ExpectedVpdSerial $expectedTeardownVpdSerial -ExpectedServiceName $expectedTeardownServiceName | Out-Null
    $noValidatedPass = $phaseBoundTeardown.psobject.Copy()
    $noValidatedPass.normal_vpd_state = "not_executed"
    $noValidatedPass.normal_vpd_serial = ""
    Assert-Throws {
        Assert-GuestVerifierCurrentRunTeardownBinding -Binding $noValidatedPass `
            -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf `
            -ExpectedDriverHash $expectedTeardownDriverHash -ExpectedInfHash $expectedTeardownInfHash `
            -ExpectedCatalogHash $expectedTeardownCatalogHash -ExpectedHardwareId $expectedTeardownHardwareId `
            -ExpectedVpdSerial $expectedTeardownVpdSerial -ExpectedServiceName $expectedTeardownServiceName | Out-Null
    } "guest_verifier_phase_bound_teardown_is_exact"
    $futureSerialLeak = $phaseBoundTeardown.psobject.Copy()
    $futureSerialLeak.verifier_vpd_serial = $expectedTeardownVpdSerial
    Assert-Throws {
        Assert-GuestVerifierCurrentRunTeardownBinding -Binding $futureSerialLeak `
            -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf `
            -ExpectedDriverHash $expectedTeardownDriverHash -ExpectedInfHash $expectedTeardownInfHash `
            -ExpectedCatalogHash $expectedTeardownCatalogHash -ExpectedHardwareId $expectedTeardownHardwareId `
            -ExpectedVpdSerial $expectedTeardownVpdSerial -ExpectedServiceName $expectedTeardownServiceName | Out-Null
    } "guest_verifier_phase_bound_teardown_is_exact"
    Write-Output "PASS guest_verifier_phase_bound_teardown_is_exact"
    $foreignServiceState = $validTeardownBinding.psobject.Copy()
    $foreignServiceState.service_state = "Paused"
    Assert-Throws {
        Assert-GuestVerifierCurrentRunTeardownBinding -Binding $foreignServiceState `
            -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf `
            -ExpectedDriverHash $expectedTeardownDriverHash -ExpectedInfHash $expectedTeardownInfHash `
            -ExpectedCatalogHash $expectedTeardownCatalogHash -ExpectedHardwareId $expectedTeardownHardwareId `
            -ExpectedVpdSerial $expectedTeardownVpdSerial -ExpectedServiceName $expectedTeardownServiceName | Out-Null
    } "guest_verifier_phase_bound_teardown_is_exact"

    $validTeardownEvidence = [pscustomobject]@{
        schema = [int]1
        run_id = "manufactured-run"
        published_inf = $validPublishedInf
        service_stop_exit_code = [int]0
        service_stop_action = "deferred_to_root_removal"
        device_remove_exit_code = [int]0
        service_delete_exit_code = [int]0
        service_delete_action = "deleted"
        driver_delete_exit_code = [int]0
        retired_node_delete_exit_code = [int]0
        retired_node_delete_action = "not_present"
        retired_node_instance_id = ""
        package_count = [int]0
        published_inf_count = [int]0
        root_count = [int]0
        service_count = [int]0
        ramshared_disk_count = [int]0
        ramshared_pnp_disk_count = [int]0
    }
    Assert-GuestVerifierCurrentRunTeardownEvidence -Evidence $validTeardownEvidence `
        -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf -RequireActionReceipts | Out-Null
    Write-Output "PASS guest_verifier_running_pnp_service_defers_stop_to_root_removal"
    $incompleteTeardownEvidence = $validTeardownEvidence.psobject.Copy()
    $incompleteTeardownEvidence.published_inf_count = [int]1
    Assert-Throws {
        Assert-GuestVerifierCurrentRunTeardownEvidence -Evidence $incompleteTeardownEvidence `
            -RunId "manufactured-run" -ExpectedPublishedInf $validPublishedInf -RequireActionReceipts | Out-Null
    } "guest_verifier_teardown_cleanup_incomplete_is_red"
    Write-Output "PASS guest_verifier_teardown_cleanup_incomplete_is_red"

    $validPostRootState = [pscustomobject]@{
        root_count = [int]0
        ramshared_disk_count = [int]0
        ramshared_pnp_disk_count = [int]1
        ramshared_present_pnp_disk_count = [int]0
        ramshared_retired_pnp_disk_count = [int]1
    }
    Assert-GuestVerifierPostRootRemovalState -State $validPostRootState | Out-Null
    foreach ($invalidPostRootState in @(
            [pscustomobject]@{
                root_count = [int]0; ramshared_disk_count = [int]0
                ramshared_pnp_disk_count = [int]1; ramshared_present_pnp_disk_count = [int]1
                ramshared_retired_pnp_disk_count = [int]0
            },
            [pscustomobject]@{
                root_count = [int]0; ramshared_disk_count = [int]0
                ramshared_pnp_disk_count = [int]2; ramshared_present_pnp_disk_count = [int]0
                ramshared_retired_pnp_disk_count = [int]2
            })) {
        Assert-Throws {
            Assert-GuestVerifierPostRootRemovalState -State $invalidPostRootState | Out-Null
        } "guest_verifier_post_root_removed_retired_state_is_exact"
    }
    Write-Output "PASS guest_verifier_post_root_removed_retired_state_is_exact"

    $preBootQuery = @"
No drivers are currently verified.
"@
    $configuredSettings = @"
Verified Drivers:

    ramshared.sys
"@
    $configuredTargets = @(Get-GuestVerifierTargetLines -Text $configuredSettings)
    $inactiveTargets = @(Get-GuestVerifierTargetLines -Text $preBootQuery)
    if ($configuredTargets.Count -ne 1 -or
        [string]$configuredTargets[0] -notmatch '(?i)^\s*ramshared\.sys\s*$' -or
        $inactiveTargets.Count -ne 0) {
        throw "guest_verifier_configured_target_uses_querysettings_only failed: manufactured phase evidence was misparsed"
    }
    if ($text -notmatch '(?s)function\s+Enable-GuestVerifier.*?Get-GuestVerifierTargetLines\s+-Text\s+\$settingsOutput') {
        throw "guest_verifier_configured_target_uses_querysettings_only failed: enable stage is not bound to querysettings"
    }
    Write-Output "PASS guest_verifier_configured_target_uses_querysettings_only"

    $validEnabled = [pscustomobject]@{
        schema = [int]1
        set_exit_code = [int]0
        boot_exit_code = [int]0
        query_exit_code = [int]0
        settings_exit_code = [int]0
        set_reboot_required = $false
        target_present = $true
        target_count = [int]1
        all_drivers = $false
        flags_exact = $true
    }
    Assert-GuestVerifierEnabled -Evidence $validEnabled | Out-Null
    $rebootNeededEnabled = $validEnabled.psobject.Copy()
    $rebootNeededEnabled.set_exit_code = [int]2
    $rebootNeededEnabled.set_reboot_required = $true
    Assert-GuestVerifierEnabled -Evidence $rebootNeededEnabled | Out-Null
    $errorEnabled = $validEnabled.psobject.Copy()
    $errorEnabled.set_exit_code = [int]1
    Assert-Throws {
        Assert-GuestVerifierEnabled -Evidence $errorEnabled | Out-Null
    } "guest_verifier_reboot_needed_exit_is_validated"
    $mismatchedRebootEnabled = $rebootNeededEnabled.psobject.Copy()
    $mismatchedRebootEnabled.set_reboot_required = $false
    Assert-Throws {
        Assert-GuestVerifierEnabled -Evidence $mismatchedRebootEnabled | Out-Null
    } "guest_verifier_reboot_needed_exit_is_validated"
    $foreignEnabled = $validEnabled.psobject.Copy()
    $foreignEnabled.all_drivers = $true
    Assert-Throws {
        Assert-GuestVerifierEnabled -Evidence $foreignEnabled | Out-Null
    } "guest_verifier_enable_query_is_exact"
    Write-Output "PASS guest_verifier_enable_query_is_exact"

    $validReset = [pscustomobject]@{
        schema = [int]1
        reset_exit_code = [int]0
        reset_reboot_required = $false
        query_exit_code = [int]0
        target_present = $false
        target_count = [int]0
        all_drivers = $false
    }
    Assert-GuestVerifierReset -Evidence $validReset | Out-Null
    $rebootNeededReset = $validReset.psobject.Copy()
    $rebootNeededReset.reset_exit_code = [int]2
    $rebootNeededReset.reset_reboot_required = $true
    Assert-GuestVerifierReset -Evidence $rebootNeededReset | Out-Null
    $pendingTargetReset = $rebootNeededReset.psobject.Copy()
    $pendingTargetReset.target_present = $true
    $pendingTargetReset.target_count = [int]1
    Assert-GuestVerifierReset -Evidence $pendingTargetReset | Out-Null
    $pendingWithoutReboot = $pendingTargetReset.psobject.Copy()
    $pendingWithoutReboot.reset_exit_code = [int]0
    $pendingWithoutReboot.reset_reboot_required = $false
    Assert-Throws {
        Assert-GuestVerifierReset -Evidence $pendingWithoutReboot | Out-Null
    } "guest_verifier_reset_pending_target_requires_reboot"
    $ambiguousPendingReset = $pendingTargetReset.psobject.Copy()
    $ambiguousPendingReset.target_count = [int]2
    Assert-Throws {
        Assert-GuestVerifierReset -Evidence $ambiguousPendingReset | Out-Null
    } "guest_verifier_reset_pending_target_requires_reboot"
    Write-Output "PASS guest_verifier_reset_pending_target_requires_reboot"
    $errorReset = $validReset.psobject.Copy()
    $errorReset.reset_exit_code = [int]1
    Assert-Throws {
        Assert-GuestVerifierReset -Evidence $errorReset | Out-Null
    } "guest_verifier_reboot_needed_exit_is_validated"
    Write-Output "PASS guest_verifier_reboot_needed_exit_is_validated"
    $staleReset = $validReset.psobject.Copy()
    $staleReset.target_count = [int]1
    Assert-Throws {
        Assert-GuestVerifierReset -Evidence $staleReset | Out-Null
    } "guest_verifier_reset_query_is_exact"
    Write-Output "PASS guest_verifier_reset_query_is_exact"

    $validEvidence = [pscustomobject]@{
        schema = [int]1
        run_id = "manufactured-run"
        pass_name = "manufactured"
        verifier_expected = $true
        status = "PASS"
        exit_code = [int]0
        event153_count = [int]0
        event153_error = ""
        new_dump_count = [int]0
        dump_observation_error = ""
        dump_before_state = "absent"
        dump_after_state = "absent"
        verdict_error = ""
        vpd_serial = $expectedTeardownVpdSerial
        vpd_serial_observation_error = ""
        cleanup = [pscustomobject]@{
            ramshared_disks = [int]0
            ramshared_pnp_disks = [int]0
            ramshared_retired_pnp_disks = [int]1
            disk_wait = [pscustomobject]@{
                schema = [int]1
                provider_code = "disk"
                status = "zero"
                terminal_count = [int]0
                observed_total_count = [int]0
                retired_count = [int]0
                attempts = [int]1
                duration_ms = [int64]1
            }
            pnp_disk_wait = [pscustomobject]@{
                schema = [int]1
                provider_code = "pnp_disk"
                status = "zero"
                terminal_count = [int]0
                observed_total_count = [int]1
                retired_count = [int]1
                attempts = [int]2
                duration_ms = [int64]1001
            }
        }
        verdict = $validVerdict
    }
    Assert-GuestVerifierPassEvidence -Evidence $validEvidence `
        -RunId "manufactured-run" -VerifierExpected $true `
        -ExpectedVpdSerial $expectedTeardownVpdSerial | Out-Null
    $wrongRunEvidence = $validEvidence.psobject.Copy()
    $wrongRunEvidence.run_id = "stale-run"
    Assert-Throws {
        Assert-GuestVerifierPassEvidence -Evidence $wrongRunEvidence `
            -RunId "manufactured-run" -VerifierExpected $true `
            -ExpectedVpdSerial $expectedTeardownVpdSerial | Out-Null
    } "guest_verifier_current_run_identity_is_exact"
    Write-Output "PASS guest_verifier_current_run_identity_is_exact"
    Assert-GuestVerifierDumpObservation -Evidence $validEvidence | Out-Null
    $dumpPathBecameInvalid = $validEvidence.psobject.Copy()
    $dumpPathBecameInvalid.dump_after_state = "non_directory"
    Assert-Throws {
        Assert-GuestVerifierDumpObservation -Evidence $dumpPathBecameInvalid | Out-Null
    } "guest_verifier_absent_minidump_directory_is_zero_dumps"
    Write-Output "PASS guest_verifier_absent_minidump_directory_is_zero_dumps"
    Assert-GuestVerifierResidueWaitEvidence -Evidence $validEvidence.cleanup.disk_wait `
        -ExpectedProviderCode "disk" | Out-Null
    Assert-GuestVerifierResidueWaitEvidence -Evidence $validEvidence.cleanup.pnp_disk_wait `
        -ExpectedProviderCode "pnp_disk" | Out-Null
    $timedOutResidue = $validEvidence.cleanup.pnp_disk_wait.psobject.Copy()
    $timedOutResidue.status = "timeout"
    $timedOutResidue.terminal_count = [int]1
    Assert-Throws {
        Assert-GuestVerifierResidueWaitEvidence -Evidence $timedOutResidue `
            -ExpectedProviderCode "pnp_disk" | Out-Null
    } "guest_verifier_waits_for_async_lun_removal"
    $ambiguousRetiredResidue = $validEvidence.cleanup.pnp_disk_wait.psobject.Copy()
    $ambiguousRetiredResidue.observed_total_count = [int]2
    $ambiguousRetiredResidue.retired_count = [int]2
    Assert-Throws {
        Assert-GuestVerifierResidueWaitEvidence -Evidence $ambiguousRetiredResidue `
            -ExpectedProviderCode "pnp_disk" | Out-Null
    } "guest_verifier_waits_for_async_lun_removal"
    $observationFailureEvidence = $validEvidence.psobject.Copy()
    $observationFailureEvidence.event153_error = "manufactured Event 153 query failure"
    Assert-Throws {
        Assert-GuestVerifierPassEvidence -Evidence $observationFailureEvidence -RunId "manufactured-run" -VerifierExpected $true `
            -ExpectedVpdSerial $expectedTeardownVpdSerial | Out-Null
    } "guest_verifier_observation_failure_is_red"
    Write-Output "PASS guest_verifier_observation_failure_is_red"
    $wrongVpdSerialEvidence = $validEvidence.psobject.Copy()
    $wrongVpdSerialEvidence.vpd_serial = "FOREIGN000000000"
    Assert-Throws {
        Assert-GuestVerifierPassEvidence -Evidence $wrongVpdSerialEvidence -RunId "manufactured-run" -VerifierExpected $true `
            -ExpectedVpdSerial $expectedTeardownVpdSerial | Out-Null
    } "guest_verifier_vpd_serial_observation_is_exact"
    Write-Output "PASS guest_verifier_vpd_serial_observation_is_exact"
    $invalidVerdict = $validVerdict.psobject.Copy()
    $invalidVerdict.REFUSE_FOREIGN_OWNER = 0
    Assert-Throws {
        Assert-GuestVerifierIoctlVerdict -Verdict $invalidVerdict `
            -VerifierExpected $true | Out-Null
    } "guest_verifier_dump_event153_refusal_cleanup_evidence_required"
    Write-Output "PASS guest_verifier_dump_event153_refusal_cleanup_evidence_required"
}
finally {
    if ($null -ne $testPublicCertificate) {
        $testPublicCertificate.Dispose()
    }
    if ($null -ne $testCertificate) {
        $testCertificate.Dispose()
    }
    if ($null -ne $testRsa) {
        $testRsa.Dispose()
    }
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Output "PASS guest_verifier_uses_shared_bounded_psdirect"
Write-Output "PASS Test-GuestExhaustiveStatic"
