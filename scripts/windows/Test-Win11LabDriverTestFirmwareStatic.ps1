#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath = "",
    [string]$VerifierPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path $PSScriptRoot "Set-Win11LabDriverTestFirmware.ps1"
}
if ([string]::IsNullOrWhiteSpace($VerifierPath)) {
    $VerifierPath = Join-Path $PSScriptRoot "Run-GuestExhaustive.ps1"
}

function Get-ParsedAst([string]$Path) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile(
        $Path, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "driver_test_firmware_parser_is_green failed: parser errors in $Path"
    }
    $ast
}

if (-not (Test-Path -LiteralPath $HarnessPath -PathType Leaf)) {
    throw "driver_test_firmware_static_contract_is_green failed: transition harness is missing"
}
if (-not (Test-Path -LiteralPath $VerifierPath -PathType Leaf)) {
    throw "driver_test_firmware_static_contract_is_green failed: verifier harness is missing"
}

$text = Get-Content -LiteralPath $HarnessPath -Raw
$ast = Get-ParsedAst $HarnessPath
$verifierText = Get-Content -LiteralPath $VerifierPath -Raw
$parameters = @($ast.ParamBlock.Parameters | ForEach-Object {
        $_.Name.VariablePath.UserPath
    })
foreach ($required in @(
        "VMName", "ExpectedVMId", "User", "Password", "DesiredSecureBoot",
        "ApproveGuestFirmwareTransition")) {
    if ($parameters -notcontains $required) {
        throw "driver_test_firmware_static_contract_is_green failed: missing parameter $required"
    }
}

foreach ($requiredNeedle in @(
        "Invoke-GuestPsDirectBounded.ps1",
        "Get-VMSnapshot",
        "Get-VMFirmware",
        "Get-VMIntegrationService",
        "Get-VMNetworkAdapter",
        "Wait-Win11DriverTestFirmwareHostContact",
        "shutdown.exe /s /t",
        'Set-VMFirmware -VMName $VMName -EnableSecureBoot',
        'Start-VM -Name $VMName',
        "prior_firmware_restored",
        "expected_vm_id")) {
    if ($text -notmatch [regex]::Escape($requiredNeedle)) {
        throw "driver_test_firmware_static_contract_is_green failed: missing $requiredNeedle"
    }
}
foreach ($forbidden in @(
        '(?i)Restart-Computer',
        '(?i)Stop-Computer',
        '(?i)Restart-VM',
        '(?i)Stop-VM',
        '(?i)checkpoint',
        '(?i)shutdown\.exe\s+/r')) {
    if ($text -match $forbidden) {
        throw "driver_test_firmware_never_touches_physical_host failed: forbidden $forbidden"
    }
}

$shutdownIndex = $text.IndexOf('shutdown.exe /s /t', [StringComparison]::Ordinal)
$offIndex = $text.IndexOf('$vm.State -cne "Off"', [StringComparison]::Ordinal)
$setIndex = $text.IndexOf('Set-VMFirmware -VMName $VMName -EnableSecureBoot', [StringComparison]::Ordinal)
$readbackIndex = $text.IndexOf('$firmwareReadback = Get-VMFirmware', [StringComparison]::Ordinal)
$startIndex = $text.IndexOf('Start-VM -Name $VMName', [StringComparison]::Ordinal)
$contactIndex = $text.IndexOf('$hostContact = Wait-Win11DriverTestFirmwareHostContact', [StringComparison]::Ordinal)
if ($shutdownIndex -lt 0 -or $offIndex -le $shutdownIndex -or $setIndex -le $offIndex) {
    throw "driver_test_firmware_requires_guest_shutdown_before_set failed"
}
if ($readbackIndex -le $setIndex -or $startIndex -le $readbackIndex) {
    throw "driver_test_firmware_transition_is_read_back failed"
}
if ($contactIndex -le $startIndex) {
    throw "driver_test_firmware_waits_for_host_contact failed"
}

$statusFunction = @($ast.FindAll({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -ceq "Test-Win11DriverTestFirmwareAdapterStatus"
        }, $true))
if ($statusFunction.Count -ne 1) {
    throw "driver_test_firmware_network_status_is_semantic failed: helper is missing or ambiguous"
}
Invoke-Expression $statusFunction[0].Extent.Text
if (-not (Test-Win11DriverTestFirmwareAdapterStatus -Status "Ok") -or
    -not (Test-Win11DriverTestFirmwareAdapterStatus -Status "OK") -or
    (Test-Win11DriverTestFirmwareAdapterStatus -Status " OK ") -or
    (Test-Win11DriverTestFirmwareAdapterStatus -Status "Degraded") -or
    (Test-Win11DriverTestFirmwareAdapterStatus -Status "")) {
    throw "driver_test_firmware_network_status_is_semantic failed: manufactured status verdict is wrong"
}
if ($text -notmatch '(?s)\$adapterValid\s*=.*?Test-Win11DriverTestFirmwareAdapterStatus') {
    throw "driver_test_firmware_network_status_is_semantic failed: live adapter guard does not use helper"
}

if ($verifierText -notmatch 'Get-VMFirmware' -or
    $verifierText -notmatch 'SecureBoot' -or
    $verifierText -notmatch 'guest verifier requires Secure Boot Off') {
    throw "guest_verifier_requires_secure_boot_off failed"
}

Write-Output "PASS driver_test_firmware_requires_exact_vm_id"
Write-Output "PASS driver_test_firmware_requires_guest_shutdown_before_set"
Write-Output "PASS driver_test_firmware_requires_off_state"
Write-Output "PASS driver_test_firmware_transition_is_read_back"
Write-Output "PASS driver_test_firmware_waits_for_host_contact"
Write-Output "PASS driver_test_firmware_restores_prior_state_on_prestart_failure"
Write-Output "PASS driver_test_firmware_network_status_is_semantic"
Write-Output "PASS driver_test_firmware_never_touches_physical_host"
Write-Output "PASS guest_verifier_requires_secure_boot_off"
Write-Output "PASS Test-Win11LabDriverTestFirmwareStatic"
