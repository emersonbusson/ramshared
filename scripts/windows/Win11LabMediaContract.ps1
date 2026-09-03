#Requires -Version 5.1
<#
.SYNOPSIS
  Sealed unattended-media contract helpers for disposable Windows 11 labs.

.DESCRIPTION
  This file is dot-sourced by the ISO builder and VM creator. When invoked with
  -WorkerMode it performs one bounded, read-only ISO operation in a disposable
  Windows PowerShell child and writes only a sanitized JSON receipt.
#>
[CmdletBinding()]
param(
    [Alias("WorkerMode")]
    [ValidateSet("", "Inspect", "Probe", "Stage", "Cleanup")]
    [string]$WorkerEntryMode = "",
    [Alias("IsoPath")]
    [string]$WorkerEntryIsoPath = "",
    [Alias("ResultPath")]
    [string]$WorkerEntryResultPath = "",
    [Alias("StagingRoot")]
    [string]$WorkerEntryStagingRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$script:Win11LabMediaContractPath = Join-Path $PSScriptRoot "Win11LabMediaContract.ps1"

function Normalize-Win11LabSha256 {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Sha256,
        [string]$FailureCode = "sha256_invalid"
    )

    $candidate = $Sha256.Trim()
    if ($candidate -notmatch '^[A-Fa-f0-9]{64}$') {
        throw ("win11_lab_media_contract_" + $FailureCode)
    }

    return $candidate.ToUpperInvariant()
}

function Get-Win11LabFileSha256 {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "win11_lab_media_contract_input_missing"
    }

    try {
        $hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash
    } catch {
        throw "win11_lab_media_contract_sha256_unavailable"
    }

    return Normalize-Win11LabSha256 -Sha256 $hash -FailureCode "sha256_unavailable"
}

function Get-Win11LabSingleXmlNode {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [System.Xml.XmlNode]$Context,
        [Parameter(Mandatory = $true)]
        [string]$XPath,
        [Parameter(Mandatory = $true)]
        [System.Xml.XmlNamespaceManager]$NamespaceManager,
        [Parameter(Mandatory = $true)]
        [string]$FailureCode
    )

    $nodes = @($Context.SelectNodes($XPath, $NamespaceManager))
    if ($nodes.Count -ne 1) {
        throw ("win11_lab_media_contract_" + $FailureCode)
    }

    return $nodes[0]
}

function New-Win11LabPersistentAutologonCommand {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateNotNullOrEmpty()]
        [string]$ComputerName,
        [Parameter(Mandatory = $true)]
        [ValidateNotNullOrEmpty()]
        [string]$LabUser,
        [Parameter(Mandatory = $true)]
        [ValidateNotNullOrEmpty()]
        [string]$Password
    )

    $escapedUser = $LabUser.Replace("'", "''")
    $escapedPassword = $Password.Replace("'", "''")
    $scriptText = @"
`$ErrorActionPreference = 'Stop'
`$winlogon = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
Remove-ItemProperty -LiteralPath `$winlogon -Name AutoLogonSID -ErrorAction SilentlyContinue
Remove-ItemProperty -LiteralPath `$winlogon -Name AutoLogonCount -ErrorAction SilentlyContinue
Set-ItemProperty -LiteralPath `$winlogon -Name AutoAdminLogon -Type String -Value '1'
Set-ItemProperty -LiteralPath `$winlogon -Name ForceAutoLogon -Type String -Value '1'
Set-ItemProperty -LiteralPath `$winlogon -Name DefaultUserName -Type String -Value '$escapedUser'
Set-ItemProperty -LiteralPath `$winlogon -Name DefaultDomainName -Type String -Value ''
Set-ItemProperty -LiteralPath `$winlogon -Name DefaultPassword -Type String -Value '$escapedPassword'
Set-ItemProperty -LiteralPath `$winlogon -Name DisableCAD -Type DWord -Value 1
Set-LocalUser -Name '$escapedUser' -PasswordNeverExpires `$true
`$personalization = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Personalization'
New-Item -Path `$personalization -Force | Out-Null
Set-ItemProperty -LiteralPath `$personalization -Name NoLockScreen -Type DWord -Value 1
`$desktop = 'HKCU:\Control Panel\Desktop'
New-Item -Path `$desktop -Force | Out-Null
Set-ItemProperty -LiteralPath `$desktop -Name ScreenSaveActive -Type String -Value '0'
Set-ItemProperty -LiteralPath `$desktop -Name ScreenSaverIsSecure -Type String -Value '0'
Set-ItemProperty -LiteralPath `$desktop -Name ScreenSaveTimeOut -Type String -Value '0'
Remove-ItemProperty -LiteralPath `$desktop -Name 'SCRNSAVE.EXE' -ErrorAction SilentlyContinue
`$userSystemPolicy = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\System'
New-Item -Path `$userSystemPolicy -Force | Out-Null
Set-ItemProperty -LiteralPath `$userSystemPolicy -Name DisableLockWorkstation -Type DWord -Value 1
`$machineSystemPolicy = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
Set-ItemProperty -LiteralPath `$machineSystemPolicy -Name InactivityTimeoutSecs -Type DWord -Value 0
foreach (`$arguments in @(@('/change','monitor-timeout-ac','0'),@('/change','monitor-timeout-dc','0'),@('/change','standby-timeout-ac','0'),@('/change','standby-timeout-dc','0'),@('/SETACVALUEINDEX','SCHEME_CURRENT','SUB_NONE','CONSOLELOCK','0'),@('/SETDCVALUEINDEX','SCHEME_CURRENT','SUB_NONE','CONSOLELOCK','0'),@('/SETACTIVE','SCHEME_CURRENT'))) {
    `$process = Start-Process -FilePath powercfg.exe -ArgumentList `$arguments -Wait -PassThru -WindowStyle Hidden
    if (`$process.ExitCode -ne 0) { throw 'persistent_autologon_power_policy_failed' }
}
"@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($scriptText))
    return "powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded"
}

function Get-Win11LabAutounattendContract {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "win11_lab_media_contract_autounattend_missing"
    }

    $document = New-Object System.Xml.XmlDocument
    $document.PreserveWhitespace = $true
    $document.XmlResolver = $null
    try {
        $document.Load($Path)
    } catch {
        throw "win11_lab_media_contract_autounattend_xml_invalid"
    }

    if ($null -eq $document.DocumentElement -or
        $document.DocumentElement.LocalName -ne "unattend" -or
        $document.DocumentElement.NamespaceURI -ne "urn:schemas-microsoft-com:unattend") {
        throw "win11_lab_media_contract_root_namespace_invalid"
    }

    $namespaceManager = New-Object System.Xml.XmlNamespaceManager($document.NameTable)
    $namespaceManager.AddNamespace("u", "urn:schemas-microsoft-com:unattend")

    $windowsPeSettings = @($document.SelectNodes('/u:unattend/u:settings[@pass="windowsPE"]', $namespaceManager))
    if ($windowsPeSettings.Count -ne 1) {
        throw "win11_lab_media_contract_windowspe_settings_count_invalid"
    }

    $windowsPeSetupComponents = @($windowsPeSettings[0].SelectNodes('u:component[@name="Microsoft-Windows-Setup"]', $namespaceManager))
    if ($windowsPeSetupComponents.Count -ne 1) {
        throw "win11_lab_media_contract_windowspe_setup_component_count_invalid"
    }
    $windowsPeSetup = $windowsPeSetupComponents[0]

    $productKeyNode = Get-Win11LabSingleXmlNode `
        -Context $windowsPeSetup `
        -XPath 'u:UserData/u:ProductKey/u:Key' `
        -NamespaceManager $namespaceManager `
        -FailureCode "product_key_count_invalid"
    if ([string]::IsNullOrWhiteSpace($productKeyNode.InnerText)) {
        throw "win11_lab_media_contract_product_key_empty"
    }

    $willShowUiNode = Get-Win11LabSingleXmlNode `
        -Context $windowsPeSetup `
        -XPath 'u:UserData/u:ProductKey/u:WillShowUI' `
        -NamespaceManager $namespaceManager `
        -FailureCode "product_key_willshowui_count_invalid"
    if ($willShowUiNode.InnerText.Trim() -cne "Never") {
        throw "win11_lab_media_contract_product_key_willshowui_invalid"
    }

    $imageMetadataNodes = @($windowsPeSetup.SelectNodes('u:ImageInstall/u:OSImage/u:InstallFrom/u:MetaData', $namespaceManager))
    $imageIndexMetadataNodes = @(
        $imageMetadataNodes | Where-Object {
            $metadataKeyNodes = @($_.SelectNodes('u:Key', $namespaceManager))
            $metadataKeyNodes.Count -eq 1 -and $metadataKeyNodes[0].InnerText.Trim() -ceq "/IMAGE/INDEX"
        }
    )
    if ($imageIndexMetadataNodes.Count -ne 1) {
        throw "win11_lab_media_contract_image_index_metadata_count_invalid"
    }

    $imageIndexValueNode = Get-Win11LabSingleXmlNode `
        -Context $imageIndexMetadataNodes[0] `
        -XPath 'u:Value' `
        -NamespaceManager $namespaceManager `
        -FailureCode "image_index_value_count_invalid"
    $imageIndex = 0
    if (-not [Int32]::TryParse($imageIndexValueNode.InnerText.Trim(), [ref]$imageIndex) -or $imageIndex -le 0) {
        throw "win11_lab_media_contract_image_index_invalid"
    }

    $oobeSettings = @($document.SelectNodes('/u:unattend/u:settings[@pass="oobeSystem"]', $namespaceManager))
    if ($oobeSettings.Count -ne 1) {
        throw "win11_lab_media_contract_oobe_system_settings_count_invalid"
    }

    $oobeShellSetupComponents = @($oobeSettings[0].SelectNodes('u:component[@name="Microsoft-Windows-Shell-Setup"]', $namespaceManager))
    if ($oobeShellSetupComponents.Count -ne 1) {
        throw "win11_lab_media_contract_oobe_shell_setup_component_count_invalid"
    }

    $specializeSettings = @($document.SelectNodes('/u:unattend/u:settings[@pass="specialize"]', $namespaceManager))
    if ($specializeSettings.Count -ne 1) {
        throw "win11_lab_media_contract_specialize_settings_count_invalid"
    }
    $specializeShellSetupComponents = @($specializeSettings[0].SelectNodes(
            'u:component[@name="Microsoft-Windows-Shell-Setup"]', $namespaceManager))
    if ($specializeShellSetupComponents.Count -ne 1) {
        throw "win11_lab_media_contract_specialize_shell_setup_component_count_invalid"
    }
    $computerNameNode = Get-Win11LabSingleXmlNode `
        -Context $specializeShellSetupComponents[0] `
        -XPath 'u:ComputerName' `
        -NamespaceManager $namespaceManager `
        -FailureCode "computer_name_count_invalid"
    $computerName = $computerNameNode.InnerText
    if ([string]::IsNullOrWhiteSpace($computerName) -or
        $computerName -cne $computerName.Trim()) {
        throw "win11_lab_media_contract_computer_name_invalid"
    }

    $oobeNode = Get-Win11LabSingleXmlNode `
        -Context $oobeShellSetupComponents[0] `
        -XPath 'u:OOBE' `
        -NamespaceManager $namespaceManager `
        -FailureCode "oobe_block_count_invalid"
    $requiredOobeValues = @(
        @{ Name = "HideEULAPage"; Value = "true" },
        @{ Name = "HideLocalAccountScreen"; Value = "true" },
        @{ Name = "HideOEMRegistrationScreen"; Value = "true" },
        @{ Name = "HideOnlineAccountScreens"; Value = "true" },
        @{ Name = "HideWirelessSetupInOOBE"; Value = "true" },
        @{ Name = "ProtectYourPC"; Value = "3" }
    )
    foreach ($requiredOobeValue in $requiredOobeValues) {
        $oobeValueNode = Get-Win11LabSingleXmlNode `
            -Context $oobeNode `
            -XPath ("u:" + $requiredOobeValue.Name) `
            -NamespaceManager $namespaceManager `
            -FailureCode ("oobe_" + $requiredOobeValue.Name.ToLowerInvariant() + "_invalid")
        if ($oobeValueNode.InnerText.Trim() -cne $requiredOobeValue.Value) {
            throw ("win11_lab_media_contract_oobe_" + $requiredOobeValue.Name.ToLowerInvariant() + "_invalid")
        }
    }

    $autoLogonNode = Get-Win11LabSingleXmlNode `
        -Context $oobeShellSetupComponents[0] `
        -XPath 'u:AutoLogon' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_count_invalid"
    $autoLogonEnabledNode = Get-Win11LabSingleXmlNode `
        -Context $autoLogonNode `
        -XPath 'u:Enabled' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_enabled_count_invalid"
    if ($autoLogonEnabledNode.InnerText.Trim() -cne "true") {
        throw "win11_lab_media_contract_autologon_enabled_invalid"
    }
    $autoLogonCountNode = Get-Win11LabSingleXmlNode `
        -Context $autoLogonNode `
        -XPath 'u:LogonCount' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_logoncount_count_invalid"
    if ($autoLogonCountNode.InnerText.Trim() -cne "1") {
        throw "win11_lab_media_contract_autologon_logoncount_invalid"
    }
    $autoLogonDomainNode = Get-Win11LabSingleXmlNode `
        -Context $autoLogonNode `
        -XPath 'u:Domain' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_domain_count_invalid"
    if ($autoLogonDomainNode.InnerText -cne $computerName) {
        throw "win11_lab_media_contract_autologon_domain_binding_invalid"
    }
    $autoLogonUsernameNode = Get-Win11LabSingleXmlNode `
        -Context $autoLogonNode `
        -XPath 'u:Username' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_username_count_invalid"
    $autoLogonUsername = $autoLogonUsernameNode.InnerText
    if ([string]::IsNullOrWhiteSpace($autoLogonUsername) -or
        $autoLogonUsername -cne $autoLogonUsername.Trim()) {
        throw "win11_lab_media_contract_autologon_username_invalid"
    }
    $autoLogonPasswordNode = Get-Win11LabSingleXmlNode `
        -Context $autoLogonNode `
        -XPath 'u:Password/u:Value' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_password_value_count_invalid"
    if ([string]::IsNullOrEmpty($autoLogonPasswordNode.InnerText)) {
        throw "win11_lab_media_contract_autologon_password_empty"
    }
    $autoLogonPlainTextNode = Get-Win11LabSingleXmlNode `
        -Context $autoLogonNode `
        -XPath 'u:Password/u:PlainText' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_password_plaintext_count_invalid"
    if ($autoLogonPlainTextNode.InnerText.Trim() -cne "true") {
        throw "win11_lab_media_contract_autologon_password_plaintext_invalid"
    }

    $localAccountNode = Get-Win11LabSingleXmlNode `
        -Context $oobeShellSetupComponents[0] `
        -XPath 'u:UserAccounts/u:LocalAccounts/u:LocalAccount' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_local_account_count_invalid"
    $localAccountNameNode = Get-Win11LabSingleXmlNode `
        -Context $localAccountNode `
        -XPath 'u:Name' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_local_account_name_count_invalid"
    $localAccountGroupNode = Get-Win11LabSingleXmlNode `
        -Context $localAccountNode `
        -XPath 'u:Group' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_local_account_group_count_invalid"
    $localAccountPasswordNode = Get-Win11LabSingleXmlNode `
        -Context $localAccountNode `
        -XPath 'u:Password/u:Value' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_local_account_password_value_count_invalid"
    $localAccountPlainTextNode = Get-Win11LabSingleXmlNode `
        -Context $localAccountNode `
        -XPath 'u:Password/u:PlainText' `
        -NamespaceManager $namespaceManager `
        -FailureCode "autologon_local_account_password_plaintext_count_invalid"
    if ($localAccountNameNode.InnerText -cne $autoLogonUsername -or
        $localAccountGroupNode.InnerText.Trim() -cne "Administrators" -or
        [string]::IsNullOrEmpty($localAccountPasswordNode.InnerText) -or
        $localAccountPasswordNode.InnerText -cne $autoLogonPasswordNode.InnerText -or
        $localAccountPlainTextNode.InnerText.Trim() -cne "true") {
        throw "win11_lab_media_contract_autologon_account_binding_invalid"
    }

    $firstLogonCommandNode = Get-Win11LabSingleXmlNode `
        -Context $oobeShellSetupComponents[0] `
        -XPath 'u:FirstLogonCommands/u:SynchronousCommand' `
        -NamespaceManager $namespaceManager `
        -FailureCode "post_oobe_autologon_command_count_invalid"
    $firstLogonOrderNode = Get-Win11LabSingleXmlNode `
        -Context $firstLogonCommandNode `
        -XPath 'u:Order' `
        -NamespaceManager $namespaceManager `
        -FailureCode "post_oobe_autologon_order_count_invalid"
    $firstLogonDescriptionNode = Get-Win11LabSingleXmlNode `
        -Context $firstLogonCommandNode `
        -XPath 'u:Description' `
        -NamespaceManager $namespaceManager `
        -FailureCode "post_oobe_autologon_description_count_invalid"
    $firstLogonCommandLineNode = Get-Win11LabSingleXmlNode `
        -Context $firstLogonCommandNode `
        -XPath 'u:CommandLine' `
        -NamespaceManager $namespaceManager `
        -FailureCode "post_oobe_autologon_commandline_count_invalid"
    $expectedPersistentAutologonCommand = New-Win11LabPersistentAutologonCommand `
        -ComputerName $computerName `
        -LabUser $autoLogonUsername `
        -Password $autoLogonPasswordNode.InnerText
    if ($firstLogonOrderNode.InnerText.Trim() -cne "1" -or
        $firstLogonDescriptionNode.InnerText -cne "Seal persistent disposable-lab autologon" -or
        $firstLogonCommandLineNode.InnerText -cne $expectedPersistentAutologonCommand) {
        throw "win11_lab_media_contract_post_oobe_autologon_invalid"
    }

    return [pscustomobject][ordered]@{
        sha256 = Get-Win11LabFileSha256 -Path $Path
        windows_pe_setup_component_count = [int]$windowsPeSetupComponents.Count
        product_key_present = $true
        image_index = [int]$imageIndex
        oobe_shell_setup_component_count = [int]$oobeShellSetupComponents.Count
        oobe_complete = $true
        autologon_complete = $true
        autologon_logon_count = [int]1
        autologon_domain_bound = $true
        post_oobe_autologon_bound = $true
        local_admin_account_count = [int]1
    }
}

function Get-Win11LabContractReceiptProperty {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Contract,
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [string]$Role
    )

    if ($null -eq $Contract) {
        throw ("win11_lab_media_contract_" + $Role + "_receipt_missing")
    }
    $property = $Contract.PSObject.Properties[$Name]
    if ($null -eq $property) {
        throw ("win11_lab_media_contract_" + $Role + "_receipt_incomplete")
    }

    return $property.Value
}

function Assert-Win11LabAutounattendContractReceipt {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Contract,
        [Parameter(Mandatory = $true)]
        [string]$Role
    )

    $sha256 = Normalize-Win11LabSha256 `
        -Sha256 ([string](Get-Win11LabContractReceiptProperty -Contract $Contract -Name "sha256" -Role $Role)) `
        -FailureCode ($Role + "_receipt_sha256_invalid")
    $windowsPeCount = Get-Win11LabContractReceiptProperty -Contract $Contract -Name "windows_pe_setup_component_count" -Role $Role
    $productKeyPresent = Get-Win11LabContractReceiptProperty -Contract $Contract -Name "product_key_present" -Role $Role
    $imageIndexValue = Get-Win11LabContractReceiptProperty -Contract $Contract -Name "image_index" -Role $Role
    $oobeShellCount = Get-Win11LabContractReceiptProperty -Contract $Contract -Name "oobe_shell_setup_component_count" -Role $Role
    $oobeComplete = Get-Win11LabContractReceiptProperty -Contract $Contract -Name "oobe_complete" -Role $Role
    $autoLogonComplete = Get-Win11LabContractReceiptProperty -Contract $Contract -Name "autologon_complete" -Role $Role
    $autoLogonCount = Get-Win11LabContractReceiptProperty -Contract $Contract -Name "autologon_logon_count" -Role $Role
    $autoLogonDomainBound = Get-Win11LabContractReceiptProperty -Contract $Contract -Name "autologon_domain_bound" -Role $Role
    $postOobeAutologonBound = Get-Win11LabContractReceiptProperty -Contract $Contract -Name "post_oobe_autologon_bound" -Role $Role
    $localAdminCount = Get-Win11LabContractReceiptProperty -Contract $Contract -Name "local_admin_account_count" -Role $Role

    if ([int]$windowsPeCount -ne 1 -or $productKeyPresent -isnot [bool] -or -not $productKeyPresent -or
        [int]$oobeShellCount -ne 1 -or $oobeComplete -isnot [bool] -or -not $oobeComplete -or
        $autoLogonComplete -isnot [bool] -or -not $autoLogonComplete -or
        [int]$autoLogonCount -ne 1 -or
        $autoLogonDomainBound -isnot [bool] -or -not $autoLogonDomainBound -or
        $postOobeAutologonBound -isnot [bool] -or -not $postOobeAutologonBound -or
        [int]$localAdminCount -ne 1) {
        throw ("win11_lab_media_contract_" + $Role + "_receipt_contract_invalid")
    }

    try {
        $imageIndex = [int]$imageIndexValue
    } catch {
        throw ("win11_lab_media_contract_" + $Role + "_receipt_image_index_invalid")
    }
    if ($imageIndex -le 0) {
        throw ("win11_lab_media_contract_" + $Role + "_receipt_image_index_invalid")
    }

    return $sha256
}

function Assert-Win11LabSealedAutounattendContract {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Contract,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSha256
    )

    $contractSha256 = Assert-Win11LabAutounattendContractReceipt -Contract $Contract -Role "sealed"
    $expected = Normalize-Win11LabSha256 -Sha256 $ExpectedSha256 -FailureCode "sealed_sha256_invalid"
    if ($contractSha256 -cne $expected) {
        throw "win11_lab_media_contract_sealed_sha256_mismatch"
    }
}

function Assert-Win11LabPrimaryIsoContract {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$SealedContract,
        [Parameter(Mandatory = $true)]
        [object]$EmbeddedContract,
        [Parameter(Mandatory = $true)]
        [string]$SealedAutounattendSha256,
        [Parameter(Mandatory = $true)]
        [bool]$EfiNoPromptPresent
    )

    $sealedSha256 = Assert-Win11LabAutounattendContractReceipt -Contract $SealedContract -Role "sealed"
    $embeddedSha256 = Assert-Win11LabAutounattendContractReceipt -Contract $EmbeddedContract -Role "embedded"
    $expectedSha256 = Normalize-Win11LabSha256 -Sha256 $SealedAutounattendSha256 -FailureCode "sealed_sha256_invalid"

    if ($sealedSha256 -cne $expectedSha256) {
        throw "win11_lab_media_contract_sealed_sha256_mismatch"
    }
    if ($embeddedSha256 -cne $sealedSha256) {
        throw "win11_lab_media_contract_embedded_sha256_mismatch"
    }
    if (-not $EfiNoPromptPresent) {
        throw "win11_lab_media_contract_no_prompt_efi_missing"
    }

    return [pscustomobject][ordered]@{
        sealed_sha256 = $sealedSha256
        embedded_sha256 = $embeddedSha256
        efi_noprompt_present = $true
    }
}

function Wait-Win11LabExactVhdGrowth {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$VhdPath,
        [Parameter(Mandatory = $true)]
        [Int64]$BaselineBytes,
        [ValidateRange(1, 1800)]
        [int]$TimeoutSeconds = 900,
        [ValidateRange(100, 10000)]
        [int]$PollMilliseconds = 2000,
        [scriptblock]$ReadLength = {
            param([string]$Path)
            return [Int64](Get-Item -LiteralPath $Path -ErrorAction Stop).Length
        },
        [scriptblock]$UtcNow = {
            return [DateTime]::UtcNow
        },
        [scriptblock]$SleepAction = {
            param([int]$Milliseconds)
            Start-Sleep -Milliseconds $Milliseconds
        }
    )

    if ($BaselineBytes -lt 0) {
        throw "win11_lab_media_contract_setup_vhd_baseline_invalid"
    }
    try {
        $startedAt = & $UtcNow
    } catch {
        throw "win11_lab_media_contract_setup_vhd_clock_invalid"
    }
    if ($startedAt -isnot [DateTime]) {
        throw "win11_lab_media_contract_setup_vhd_clock_invalid"
    }
    $deadline = $startedAt.AddSeconds($TimeoutSeconds)

    while ($true) {
        try {
            $observedBytes = [Int64](& $ReadLength $VhdPath)
        } catch {
            throw "win11_lab_media_contract_setup_vhd_growth_read_failed"
        }
        if ($observedBytes -gt $BaselineBytes) {
            return [pscustomobject][ordered]@{
                baseline_bytes = $BaselineBytes
                observed_bytes = $observedBytes
                growth_bytes = $observedBytes - $BaselineBytes
            }
        }

        try {
            $now = & $UtcNow
        } catch {
            throw "win11_lab_media_contract_setup_vhd_clock_invalid"
        }
        if ($now -isnot [DateTime]) {
            throw "win11_lab_media_contract_setup_vhd_clock_invalid"
        }
        if ($now -ge $deadline) {
            throw "win11_lab_media_contract_setup_vhd_growth_timeout"
        }

        try {
            & $SleepAction $PollMilliseconds
        } catch {
            throw "win11_lab_media_contract_setup_vhd_sleep_failed"
        }
    }
}

function New-Win11LabMediaResultPath {
    [CmdletBinding()]
    param()

    return (Join-Path ([System.IO.Path]::GetTempPath()) ("ramshared-win11-lab-media-" + [Guid]::NewGuid().ToString("N") + ".json"))
}

function ConvertTo-Win11LabMediaCommandLineArgument {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value
    )

    if ($Value.Contains('"') -or $Value -match '[\r\n]') {
        throw "win11_lab_media_contract_argument_invalid"
    }

    return ('"' + $Value + '"')
}

function Stop-Win11LabMediaWorkerTree {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [int]$ProcessId,
        [ValidateRange(1, 30)]
        [int]$TimeoutSeconds = 5
    )

    $taskkillPath = Join-Path $env:SystemRoot "System32\\taskkill.exe"
    if (-not (Test-Path -LiteralPath $taskkillPath -PathType Leaf)) {
        return $false
    }

    $taskkillInfo = New-Object System.Diagnostics.ProcessStartInfo
    $taskkillInfo.FileName = $taskkillPath
    $taskkillInfo.Arguments = "/PID $ProcessId /T /F"
    $taskkillInfo.UseShellExecute = $false
    $taskkillInfo.CreateNoWindow = $true
    $taskkill = New-Object System.Diagnostics.Process
    $taskkill.StartInfo = $taskkillInfo
    try {
        if (-not $taskkill.Start()) {
            return $false
        }
        if (-not $taskkill.WaitForExit([int]($TimeoutSeconds * 1000))) {
            try {
                $taskkill.Kill()
            } catch {
            }
            return $false
        }
        return ($taskkill.ExitCode -eq 0)
    } finally {
        $taskkill.Dispose()
    }
}

function Invoke-Win11LabExternalProcessBounded {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [Parameter(Mandatory = $true)]
        [string[]]$ArgumentValues,
        [ValidateRange(1, 1800)]
        [int]$TimeoutSeconds
    )

    if (-not (Test-Path -LiteralPath $FilePath -PathType Leaf)) {
        throw "win11_lab_media_contract_external_process_missing"
    }

    $processInfo = New-Object System.Diagnostics.ProcessStartInfo
    $processInfo.FileName = $FilePath
    $processInfo.Arguments = (($ArgumentValues | ForEach-Object {
        ConvertTo-Win11LabMediaCommandLineArgument -Value $_
    }) -join " ")
    $processInfo.UseShellExecute = $false
    $processInfo.CreateNoWindow = $true

    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $processInfo
    try {
        if (-not $process.Start()) {
            throw "win11_lab_media_contract_external_process_start_failed"
        }
        if (-not $process.WaitForExit([int]($TimeoutSeconds * 1000))) {
            $terminated = Stop-Win11LabMediaWorkerTree -ProcessId $process.Id -TimeoutSeconds 5
            if (-not $terminated -and -not $process.HasExited) {
                throw "win11_lab_media_contract_external_process_tree_termination_failed"
            }
            if (-not $process.WaitForExit(5000)) {
                throw "win11_lab_media_contract_external_process_exit_after_termination_failed"
            }
            throw "win11_lab_media_contract_external_process_timeout"
        }
        $exitCode = [int]$process.ExitCode
    } finally {
        $process.Dispose()
    }

    if ($exitCode -ne 0) {
        throw "win11_lab_media_contract_external_process_failed"
    }
    [pscustomobject][ordered]@{
        exit_code = $exitCode
        completed = $true
    }
}

function Invoke-Win11LabMediaWorker {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet("Inspect", "Probe", "Stage", "Cleanup")]
        [string]$Mode,
        [Parameter(Mandatory = $true)]
        [string]$IsoPath,
        [Parameter(Mandatory = $true)]
        [string]$ResultPath,
        [string]$StagingRoot = "",
        [ValidateRange(10, 1800)]
        [int]$TimeoutSeconds = 90
    )

    if (-not (Test-Path -LiteralPath $script:Win11LabMediaContractPath -PathType Leaf)) {
        throw "win11_lab_media_contract_worker_missing"
    }
    $powershellPath = Join-Path $PSHOME "powershell.exe"
    if (-not (Test-Path -LiteralPath $powershellPath -PathType Leaf)) {
        throw "win11_lab_media_contract_powershell51_missing"
    }

    $argumentValues = @(
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        $script:Win11LabMediaContractPath,
        "-WorkerMode",
        $Mode,
        "-IsoPath",
        $IsoPath,
        "-ResultPath",
        $ResultPath
    )
    if ($Mode -eq "Stage") {
        if ([string]::IsNullOrWhiteSpace($StagingRoot)) {
            throw "win11_lab_media_contract_staging_root_missing"
        }
        $argumentValues += @("-StagingRoot", $StagingRoot)
    }

    $workerInfo = New-Object System.Diagnostics.ProcessStartInfo
    $workerInfo.FileName = $powershellPath
    $workerInfo.Arguments = (($argumentValues | ForEach-Object {
        ConvertTo-Win11LabMediaCommandLineArgument -Value $_
    }) -join " ")
    $workerInfo.UseShellExecute = $false
    $workerInfo.CreateNoWindow = $true
    $worker = New-Object System.Diagnostics.Process
    $worker.StartInfo = $workerInfo
    try {
        if (-not $worker.Start()) {
            throw "win11_lab_media_contract_worker_start_failed"
        }
        if (-not $worker.WaitForExit([int]($TimeoutSeconds * 1000))) {
            if (-not (Stop-Win11LabMediaWorkerTree -ProcessId $worker.Id -TimeoutSeconds 5)) {
                throw "win11_lab_media_contract_worker_tree_termination_failed"
            }
            if (-not $worker.WaitForExit(5000)) {
                throw "win11_lab_media_contract_worker_exit_after_termination_failed"
            }
            throw "win11_lab_media_contract_worker_timeout"
        }
        if ($worker.ExitCode -ne 0) {
            throw "win11_lab_media_contract_worker_failed"
        }
    } finally {
        $worker.Dispose()
    }

    if (-not (Test-Path -LiteralPath $ResultPath -PathType Leaf)) {
        throw "win11_lab_media_contract_worker_result_missing"
    }
    try {
        $resultJson = [System.IO.File]::ReadAllText($ResultPath)
        if ([string]::IsNullOrWhiteSpace($resultJson)) {
            throw "win11_lab_media_contract_worker_result_empty"
        }
        return ($resultJson | ConvertFrom-Json -ErrorAction Stop)
    } catch {
        if ($_.Exception.Message -match '^win11_lab_media_contract_') {
            throw
        }
        throw "win11_lab_media_contract_worker_result_invalid"
    }
}

function Invoke-Win11LabMediaCleanup {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$IsoPath,
        [ValidateRange(10, 300)]
        [int]$TimeoutSeconds = 30
    )

    $cleanupResultPath = New-Win11LabMediaResultPath
    try {
        $cleanupResult = Invoke-Win11LabMediaWorker `
            -Mode "Cleanup" `
            -IsoPath $IsoPath `
            -ResultPath $cleanupResultPath `
            -TimeoutSeconds $TimeoutSeconds
        $cleanupComplete = Get-Win11LabContractReceiptProperty `
            -Contract $cleanupResult `
            -Name "cleanup_complete" `
            -Role "cleanup"
        if ($cleanupComplete -isnot [bool] -or -not $cleanupComplete) {
            throw "win11_lab_media_contract_cleanup_receipt_invalid"
        }
    } finally {
        if (Test-Path -LiteralPath $cleanupResultPath -PathType Leaf) {
            [System.IO.File]::Delete($cleanupResultPath)
        }
    }
}

function Assert-Win11LabIsoNotAttached {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$IsoPath,
        [ValidateRange(10, 300)]
        [int]$TimeoutSeconds = 30
    )

    $inspectionResultPath = New-Win11LabMediaResultPath
    try {
        $inspectionResult = Invoke-Win11LabMediaWorker `
            -Mode "Inspect" `
            -IsoPath $IsoPath `
            -ResultPath $inspectionResultPath `
            -TimeoutSeconds $TimeoutSeconds
        $attached = Get-Win11LabContractReceiptProperty `
            -Contract $inspectionResult `
            -Name "attached" `
            -Role "inspection"
        if ($attached -isnot [bool]) {
            throw "win11_lab_media_contract_inspection_receipt_invalid"
        }
        if ($attached) {
            throw "win11_lab_media_contract_iso_already_attached"
        }
    } finally {
        if (Test-Path -LiteralPath $inspectionResultPath -PathType Leaf) {
            [System.IO.File]::Delete($inspectionResultPath)
        }
    }
}

function Invoke-Win11LabBoundedIsoOperation {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet("Probe", "Stage")]
        [string]$Mode,
        [Parameter(Mandatory = $true)]
        [string]$IsoPath,
        [string]$StagingRoot = "",
        [ValidateRange(10, 1800)]
        [int]$TimeoutSeconds = 90
    )

    Assert-Win11LabIsoNotAttached -IsoPath $IsoPath -TimeoutSeconds 30

    $resultPath = New-Win11LabMediaResultPath
    $operationResult = $null
    $operationFailure = $null
    $cleanupFailure = $null
    try {
        $operationResult = Invoke-Win11LabMediaWorker `
            -Mode $Mode `
            -IsoPath $IsoPath `
            -ResultPath $resultPath `
            -StagingRoot $StagingRoot `
            -TimeoutSeconds $TimeoutSeconds
    } catch {
        $operationFailure = $_
    }

    try {
        Invoke-Win11LabMediaCleanup -IsoPath $IsoPath -TimeoutSeconds 30
    } catch {
        $cleanupFailure = $_
    } finally {
        if (Test-Path -LiteralPath $resultPath -PathType Leaf) {
            [System.IO.File]::Delete($resultPath)
        }
    }

    if ($null -ne $operationFailure -and $null -ne $cleanupFailure) {
        throw "win11_lab_media_contract_operation_and_cleanup_failed"
    }
    if ($null -ne $operationFailure) {
        throw $operationFailure
    }
    if ($null -ne $cleanupFailure) {
        throw $cleanupFailure
    }
    if ($null -eq $operationResult) {
        throw "win11_lab_media_contract_operation_result_missing"
    }

    return $operationResult
}

function Invoke-Win11LabPrimaryIsoContract {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$IsoPath,
        [ValidateRange(10, 300)]
        [int]$TimeoutSeconds = 90
    )

    $probeResult = Invoke-Win11LabBoundedIsoOperation `
        -Mode "Probe" `
        -IsoPath $IsoPath `
        -TimeoutSeconds $TimeoutSeconds
    $efiNoPromptPresent = Get-Win11LabContractReceiptProperty `
        -Contract $probeResult `
        -Name "efi_noprompt_present" `
        -Role "embedded"
    if ($efiNoPromptPresent -isnot [bool]) {
        throw "win11_lab_media_contract_embedded_receipt_incomplete"
    }
    Assert-Win11LabAutounattendContractReceipt -Contract $probeResult -Role "embedded" | Out-Null
    return $probeResult
}

function Invoke-Win11LabSourceIsoStage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$IsoPath,
        [Parameter(Mandatory = $true)]
        [string]$StagingRoot,
        [ValidateRange(300, 1800)]
        [int]$TimeoutSeconds = 900
    )

    $stageResult = Invoke-Win11LabBoundedIsoOperation `
        -Mode "Stage" `
        -IsoPath $IsoPath `
        -StagingRoot $StagingRoot `
        -TimeoutSeconds $TimeoutSeconds
    $sourceCopyComplete = Get-Win11LabContractReceiptProperty `
        -Contract $stageResult `
        -Name "source_copy_complete" `
        -Role "stage"
    $efiNoPromptPresent = Get-Win11LabContractReceiptProperty `
        -Contract $stageResult `
        -Name "efi_noprompt_present" `
        -Role "stage"
    $biosBootPresent = Get-Win11LabContractReceiptProperty `
        -Contract $stageResult `
        -Name "bios_boot_present" `
        -Role "stage"
    if ($sourceCopyComplete -isnot [bool] -or -not $sourceCopyComplete -or
        $efiNoPromptPresent -isnot [bool] -or -not $efiNoPromptPresent -or
        $biosBootPresent -isnot [bool] -or -not $biosBootPresent) {
        throw "win11_lab_media_contract_stage_receipt_invalid"
    }
    return $stageResult
}

function Assert-Win11LabWorkerResultPath {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\\')
    $candidate = [System.IO.Path]::GetFullPath($Path)
    if ([System.IO.Path]::GetDirectoryName($candidate) -cne $tempRoot -or
        [System.IO.File]::Exists($candidate)) {
        throw "win11_lab_media_contract_worker_result_path_invalid"
    }
}

function Write-Win11LabWorkerResult {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Result,
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $json = $Result | ConvertTo-Json -Depth 6 -Compress
    [System.IO.File]::WriteAllText($Path, $json, (New-Object System.Text.UTF8Encoding($false)))
}

function Assert-Win11LabWorkerIsoNotAttached {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $diskImage = Get-DiskImage -ImagePath $Path -ErrorAction Stop
    if ([bool]$diskImage.Attached) {
        throw "win11_lab_media_contract_iso_already_attached"
    }
}

function Assert-Win11LabArtifactChecksum {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ManifestPath,
        [Parameter(Mandatory = $true)]
        [string]$ArtifactPath,
        [Parameter(Mandatory = $true)]
        [string]$Role
    )

    if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
        throw "win11_lab_media_contract_manifest_missing"
    }
    if (-not (Test-Path -LiteralPath $ArtifactPath -PathType Leaf)) {
        throw "win11_lab_media_contract_artifact_missing"
    }

    $manifest = $null
    try {
        $manifestJson = [System.IO.File]::ReadAllText($ManifestPath)
        $manifest = $manifestJson | ConvertFrom-Json -ErrorAction Stop
    } catch {
        throw "win11_lab_media_contract_manifest_invalid"
    }

    if ($null -eq $manifest -or $null -eq $manifest.artifacts -or $manifest.artifacts -isnot [array]) {
        throw "win11_lab_media_contract_manifest_artifacts_missing"
    }

    $expectedSha256 = $null
    foreach ($artifact in $manifest.artifacts) {
        if ($null -ne $artifact.role -and [string]$artifact.role -ceq $Role) {
            $expectedSha256 = [string]$artifact.sha256
            break
        }
    }

    if ([string]::IsNullOrWhiteSpace($expectedSha256)) {
        throw "win11_lab_media_contract_manifest_artifact_role_missing"
    }

    $computedSha256 = Get-Win11LabFileSha256 -Path $ArtifactPath
    $normalizedExpected = Normalize-Win11LabSha256 -Sha256 $expectedSha256 -FailureCode "manifest_artifact_sha256_invalid"

    if ($computedSha256 -cne $normalizedExpected) {
        throw "win11_lab_media_contract_artifact_hash_mismatch"
    }
}

function Invoke-Win11LabMediaWorkerMode {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet("Inspect", "Probe", "Stage", "Cleanup")]
        [string]$Mode,
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$OutputPath,
        [string]$WorkerStagingRoot = ""
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "win11_lab_media_contract_iso_missing"
    }
    Assert-Win11LabWorkerResultPath -Path $OutputPath

    switch ($Mode) {
        "Inspect" {
            $diskImage = Get-DiskImage -ImagePath $Path -ErrorAction Stop
            Write-Win11LabWorkerResult -Path $OutputPath -Result ([ordered]@{
                attached = [bool]$diskImage.Attached
            })
            return
        }
        "Cleanup" {
            $diskImage = Get-DiskImage -ImagePath $Path -ErrorAction Stop
            $wasAttached = [bool]$diskImage.Attached
            if ($wasAttached) {
                Dismount-DiskImage -ImagePath $Path -ErrorAction Stop | Out-Null
            }
            Write-Win11LabWorkerResult -Path $OutputPath -Result ([ordered]@{
                cleanup_complete = $true
                was_attached = $wasAttached
            })
            return
        }
        "Probe" {
            Assert-Win11LabWorkerIsoNotAttached -Path $Path
            $mounted = $null
            try {
                $mounted = Mount-DiskImage -ImagePath $Path -PassThru -ErrorAction Stop
                $volumes = @($mounted | Get-Volume -ErrorAction Stop)
                if ($volumes.Count -ne 1 -or [string]::IsNullOrWhiteSpace([string]$volumes[0].DriveLetter)) {
                    throw "win11_lab_media_contract_mounted_iso_drive_letter_invalid"
                }
                $sourceRoot = ("$($volumes[0].DriveLetter):\\")
                $embeddedAutounattendPath = Join-Path $sourceRoot "Autounattend.xml"
                $efiNoPromptPath = Join-Path $sourceRoot "efi\\microsoft\\boot\\efisys_noprompt.bin"
                $embeddedContract = Get-Win11LabAutounattendContract -Path $embeddedAutounattendPath
                Write-Win11LabWorkerResult -Path $OutputPath -Result ([ordered]@{
                    sha256 = $embeddedContract.sha256
                    windows_pe_setup_component_count = $embeddedContract.windows_pe_setup_component_count
                    product_key_present = $embeddedContract.product_key_present
                    image_index = $embeddedContract.image_index
                    oobe_shell_setup_component_count = $embeddedContract.oobe_shell_setup_component_count
                    oobe_complete = $embeddedContract.oobe_complete
                    autologon_complete = $embeddedContract.autologon_complete
                    autologon_logon_count = $embeddedContract.autologon_logon_count
                    autologon_domain_bound = $embeddedContract.autologon_domain_bound
                    post_oobe_autologon_bound = $embeddedContract.post_oobe_autologon_bound
                    local_admin_account_count = $embeddedContract.local_admin_account_count
                    efi_noprompt_present = [bool](Test-Path -LiteralPath $efiNoPromptPath -PathType Leaf)
                })
            } finally {
                if ($null -ne $mounted) {
                    Dismount-DiskImage -ImagePath $Path -ErrorAction Stop | Out-Null
                }
            }
            return
        }
        "Stage" {
            if ([string]::IsNullOrWhiteSpace($WorkerStagingRoot) -or
                -not (Test-Path -LiteralPath $WorkerStagingRoot -PathType Container)) {
                throw "win11_lab_media_contract_staging_root_invalid"
            }
            Assert-Win11LabWorkerIsoNotAttached -Path $Path
            $mounted = $null
            try {
                $mounted = Mount-DiskImage -ImagePath $Path -PassThru -ErrorAction Stop
                $volumes = @($mounted | Get-Volume -ErrorAction Stop)
                if ($volumes.Count -ne 1 -or [string]::IsNullOrWhiteSpace([string]$volumes[0].DriveLetter)) {
                    throw "win11_lab_media_contract_mounted_iso_drive_letter_invalid"
                }
                $sourceRoot = ("$($volumes[0].DriveLetter):\\")
                $efiNoPromptPath = Join-Path $sourceRoot "efi\\microsoft\\boot\\efisys_noprompt.bin"
                $biosBootPath = Join-Path $sourceRoot "boot\\etfsboot.com"
                if (-not (Test-Path -LiteralPath $efiNoPromptPath -PathType Leaf)) {
                    throw "win11_lab_media_contract_no_prompt_efi_missing"
                }
                if (-not (Test-Path -LiteralPath $biosBootPath -PathType Leaf)) {
                    throw "win11_lab_media_contract_bios_boot_missing"
                }
                robocopy $sourceRoot $WorkerStagingRoot /MIR /NFL /NDL /NJH /NJS /NP | Out-Null
                if ($LASTEXITCODE -gt 7) {
                    throw "win11_lab_media_contract_source_copy_failed"
                }
                Write-Win11LabWorkerResult -Path $OutputPath -Result ([ordered]@{
                    source_copy_complete = $true
                    efi_noprompt_present = $true
                    bios_boot_present = $true
                })
            } finally {
                if ($null -ne $mounted) {
                    Dismount-DiskImage -ImagePath $Path -ErrorAction Stop | Out-Null
                }
            }
            return
        }
    }
}

if (-not [string]::IsNullOrWhiteSpace($WorkerEntryMode)) {
    Invoke-Win11LabMediaWorkerMode `
        -Mode $WorkerEntryMode `
        -Path $WorkerEntryIsoPath `
        -OutputPath $WorkerEntryResultPath `
        -WorkerStagingRoot $WorkerEntryStagingRoot
}
