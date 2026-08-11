#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path $PSScriptRoot "New-Win11LabReadyClone.ps1"
}

if (-not (Test-Path -LiteralPath $HarnessPath -PathType Leaf)) {
    throw "ready clone harness is missing"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw -ErrorAction Stop
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput($text, [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0) {
    throw "ready clone harness does not parse"
}

function Import-ReadyCloneFunction([string]$Name) {
    $definition = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $Name
        }, $true)
    if ($null -eq $definition) {
        throw "ready clone static contract missing function $Name"
    }
    $body = $definition.Body.Extent.Text
    Set-Item -Path ("Function:\script:{0}" -f $Name) -Value (
        [scriptblock]::Create($body.Substring(1, $body.Length - 2)))
}
Import-ReadyCloneFunction "Assert-ReadyCloneOfflineIntegrationServices"

foreach ($needle in @(
        '[switch]$ApproveCreate',
        'base-manifest.json',
        'base.vhdx',
        'ramshared-isolated-win11-lab-base',
        'Get-FileHash',
        'FileAttributes]::ReadOnly',
        'Get-VMSwitch',
        'SwitchType -cne "Private"',
        'New-VHD',
        '-Differencing',
        '-ParentPath $baseVhdPath',
        'New-VM',
        '-Generation 2',
        '-VHDPath $cloneVhdPath',
        'Set-VMMemory',
        'Set-VMProcessor',
        'Set-VMKeyProtector',
        '-NewLocalKeyProtector',
        'Enable-VMTPM',
        'Get-VMSecurity',
        'Enable-VMIntegrationService -VMIntegrationService',
        '6C09BB55-D683-4DA0-8931-C9BF705F6480',
        '84EAAE65-2F2E-45F5-9BB5-0E857DC8EB47',
        '2A34B1C2-FD73-4043-8A5B-DD2159BC743F',
        '9F8233AC-BE49-4C79-8EE3-E7E1985B2077',
        '2497F4DE-E9FA-4204-80E4-4B75C46419C0',
        '5CED1297-4598-4915-A5FC-AD21BB4D02A4',
        'Set-VMFirmware',
        'Set-VM -VM $vm -CheckpointType Disabled',
        'Get-VHD',
        'VhdType',
        'ParentPath',
        'checkpoint_count',
        'base_vhd_sha256',
        'STATUS=PASS')) {
    if ($text.IndexOf($needle, [StringComparison]::Ordinal) -lt 0) {
        throw "ready clone static contract missing $needle"
    }
}
if ($text -match '(?i)\bStart-VM\b' -or $text -match '(?i)External' -or
    $text -match '(?i)Copy-Item[^\r\n]*base\.vhdx') {
    throw "ready clone must remain off/sealed and must not copy the base"
}
$keyIndex = $text.IndexOf('Set-VMKeyProtector', [StringComparison]::Ordinal)
$tpmIndex = $text.IndexOf('Enable-VMTPM', [StringComparison]::Ordinal)
if ($keyIndex -lt 0 -or $tpmIndex -le $keyIndex) {
    throw "ready clone vTPM ordering is unsafe"
}
foreach ($commandName in @("Remove-VM", "Remove-Item")) {
    if (-not $ast.Find({
                param($node)
                $node -is [Management.Automation.Language.CommandAst] -and
                $node.GetCommandName() -eq $commandName
            }, $true)) {
        throw "ready clone exact failure cleanup is missing $commandName"
    }
}
$expectedVmId = "11111111-2222-3333-4444-555555555555"
$expectedComponents = @(
    "6C09BB55-D683-4DA0-8931-C9BF705F6480",
    "84EAAE65-2F2E-45F5-9BB5-0E857DC8EB47",
    "2A34B1C2-FD73-4043-8A5B-DD2159BC743F",
    "9F8233AC-BE49-4C79-8EE3-E7E1985B2077",
    "2497F4DE-E9FA-4204-80E4-4B75C46419C0",
    "5CED1297-4598-4915-A5FC-AD21BB4D02A4"
)
$offlineServices = @($expectedComponents | ForEach-Object {
        [pscustomobject]@{
            Id = "Microsoft:$expectedVmId\$_"
            Enabled = $true
            PrimaryStatusDescription = $null
        }
    })
$offlineEvidence = @(Assert-ReadyCloneOfflineIntegrationServices -Services $offlineServices `
        -ExpectedVMId $expectedVmId -ExpectedComponentIds $expectedComponents)
if ($offlineEvidence.Count -ne 6 -or
    @($offlineEvidence | Where-Object {
            ([bool]$_.enabled -ne $true) -or
            -not [string]::IsNullOrEmpty([string]$_.contact_status)
        }).Count -ne 0) {
    throw "ready_clone_offline_integration_status_is_not_contact failed"
}
foreach ($invalidServices in @(
        @($offlineServices | ForEach-Object {
                [pscustomobject]@{
                    Id = $_.Id; Enabled = $_.Enabled
                    PrimaryStatusDescription = if ($_.Id -eq $offlineServices[0].Id) { "OK" } else { $null }
                }
            }),
        @($offlineServices | ForEach-Object {
                [pscustomobject]@{
                    Id = $_.Id
                    Enabled = if ($_.Id -eq $offlineServices[0].Id) { $false } else { $_.Enabled }
                    PrimaryStatusDescription = $null
                }
            }),
        @($offlineServices | ForEach-Object {
                [pscustomobject]@{
                    Id = if ($_.Id -eq $offlineServices[0].Id) {
                        $_.Id.Replace($expectedVmId, "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE")
                    } else { $_.Id }
                    Enabled = $_.Enabled
                    PrimaryStatusDescription = $null
                }
            }))) {
    try {
        Assert-ReadyCloneOfflineIntegrationServices -Services $invalidServices `
            -ExpectedVMId $expectedVmId -ExpectedComponentIds $expectedComponents | Out-Null
    }
    catch { continue }
    throw "ready_clone_offline_integration_status_is_not_contact failed: invalid state was accepted"
}
if ($text -notmatch 'readback\.json' -or
    $text -match 'PrimaryStatusDescription\s+-cne\s+"OK"') {
    throw "ready_clone_offline_integration_status_is_not_contact failed: production readback contract is stale"
}
Write-Output "PASS ready_clone_requires_immutable_base"
Write-Output "PASS ready_clone_uses_differencing_vhd"
Write-Output "PASS ready_clone_receives_new_vm_id_and_vtpm"
Write-Output "PASS ready_clone_stays_offline_on_sealed_switch"
Write-Output "PASS ready_clone_offline_integration_status_is_not_contact"
Write-Output "PASS ready_clone_failure_cleanup_is_exact"
Write-Output "PASS Test-Win11LabReadyCloneStatic"
