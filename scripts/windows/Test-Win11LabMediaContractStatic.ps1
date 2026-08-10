#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ContractPath,
    [string]$VmCreatorPath,
    [string]$IsoBuilderPath,
    [string]$ReadinessPath,
    [string]$AutounattendGeneratorPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ContractPath)) {
    $ContractPath = Join-Path $PSScriptRoot "Win11LabMediaContract.ps1"
}
if ([string]::IsNullOrWhiteSpace($VmCreatorPath)) {
    $VmCreatorPath = Join-Path $PSScriptRoot "New-Win11Wsl2LabVm.ps1"
}
if ([string]::IsNullOrWhiteSpace($IsoBuilderPath)) {
    $IsoBuilderPath = Join-Path $PSScriptRoot "New-WindowsNoPromptIso.ps1"
}
if ([string]::IsNullOrWhiteSpace($ReadinessPath)) {
    $ReadinessPath = Join-Path $PSScriptRoot "Wait-Win11LabReady.ps1"
}
if ([string]::IsNullOrWhiteSpace($AutounattendGeneratorPath)) {
    $AutounattendGeneratorPath = Join-Path $PSScriptRoot "New-Win11LabAutounattend.ps1"
}

if (-not (Test-Path -LiteralPath $ContractPath -PathType Leaf)) {
    throw "win11_lab_media_static: contract helper missing"
}

$mediaContractStagingSentinel = "manufactured-caller-staging-root"
$mediaContractModeSentinel = "manufactured-caller-mode"
$mediaContractIsoSentinel = "manufactured-caller-iso-path"
$mediaContractResultSentinel = "manufactured-caller-result-path"
$StagingRoot = $mediaContractStagingSentinel
$WorkerMode = $mediaContractModeSentinel
$IsoPath = $mediaContractIsoSentinel
$ResultPath = $mediaContractResultSentinel
. $ContractPath
if ($StagingRoot -cne $mediaContractStagingSentinel) {
    throw "media_contract_dot_source_preserves_caller_staging_root"
}
Write-Output "PASS media_contract_dot_source_preserves_caller_staging_root"
if ($WorkerMode -cne $mediaContractModeSentinel -or
    $IsoPath -cne $mediaContractIsoSentinel -or
    $ResultPath -cne $mediaContractResultSentinel) {
    throw "media_contract_dot_source_preserves_caller_worker_variables"
}
Write-Output "PASS media_contract_dot_source_preserves_caller_worker_variables"

foreach ($functionName in @(
    "Get-Win11LabAutounattendContract",
    "Assert-Win11LabSealedAutounattendContract",
    "Assert-Win11LabPrimaryIsoContract",
    "Invoke-Win11LabPrimaryIsoContract",
    "Invoke-Win11LabSourceIsoStage",
    "Invoke-Win11LabExternalProcessBounded",
    "Wait-Win11LabExactVhdGrowth"
)) {
    if (-not (Get-Command -Name $functionName -CommandType Function -ErrorAction SilentlyContinue)) {
        throw ("win11_lab_media_static: missing function " + $functionName)
    }
}

function New-ValidWin11LabAutounattendXml {
    param(
        [int]$ImageIndex = 6,
        [string]$PasswordMarker = "manufactured-password-must-not-appear"
    )

    return @"
<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
  <settings pass="windowsPE">
    <component name="Microsoft-Windows-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <ImageInstall>
        <OSImage>
          <InstallFrom>
            <MetaData wcm:action="add">
              <Key>/IMAGE/INDEX</Key>
              <Value>$ImageIndex</Value>
            </MetaData>
          </InstallFrom>
        </OSImage>
      </ImageInstall>
      <UserData>
        <ProductKey>
          <Key>MANUFACTURED-TEST-KEY</Key>
          <WillShowUI>Never</WillShowUI>
        </ProductKey>
      </UserData>
    </component>
  </settings>
  <settings pass="specialize">
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <ComputerName>MANUFACTURED-LAB</ComputerName>
    </component>
  </settings>
  <settings pass="oobeSystem">
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <AutoLogon>
        <Password>
          <Value>$PasswordMarker</Value>
          <PlainText>true</PlainText>
        </Password>
        <Enabled>true</Enabled>
        <LogonCount>1</LogonCount>
        <Domain>MANUFACTURED-LAB</Domain>
        <Username>manufactured-admin</Username>
      </AutoLogon>
      <FirstLogonCommands>
        <SynchronousCommand wcm:action="add">
          <Order>1</Order>
          <Description>Disable automatic logon after the first lab sign-in</Description>
          <CommandLine>reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon" /v AutoLogonCount /t REG_DWORD /d 0 /f</CommandLine>
        </SynchronousCommand>
      </FirstLogonCommands>
      <OOBE>
        <HideEULAPage>true</HideEULAPage>
        <HideLocalAccountScreen>true</HideLocalAccountScreen>
        <HideOEMRegistrationScreen>true</HideOEMRegistrationScreen>
        <HideOnlineAccountScreens>true</HideOnlineAccountScreens>
        <HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>
        <ProtectYourPC>3</ProtectYourPC>
      </OOBE>
      <UserAccounts>
        <LocalAccounts>
          <LocalAccount wcm:action="add">
            <Password>
              <Value>$PasswordMarker</Value>
              <PlainText>true</PlainText>
            </Password>
            <Group>Administrators</Group>
            <Name>manufactured-admin</Name>
          </LocalAccount>
        </LocalAccounts>
      </UserAccounts>
    </component>
  </settings>
</unattend>
"@
}

function Set-ManufacturedXml {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Content
    )

    [System.IO.File]::WriteAllText(
        $Path,
        $Content,
        (New-Object System.Text.UTF8Encoding($false))
    )
}

function Assert-RefusalWithoutSecret {
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Action,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedCode,
        [string]$Secret = ""
    )

    try {
        & $Action
    } catch {
        $message = $_.Exception.Message
        if (-not [string]::IsNullOrWhiteSpace($Secret) -and $message.Contains($Secret)) {
            throw "win11_lab_media_static: refusal diagnostic echoed a manufactured password"
        }
        if ($message -notmatch [regex]::Escape($ExpectedCode)) {
            throw ("win11_lab_media_static: expected refusal " + $ExpectedCode + ", got " + $message)
        }
        return
    }

    throw ("win11_lab_media_static: expected refusal " + $ExpectedCode)
}

function Import-Win11LabVmCreatorFunction {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [string]$Source
    )

    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput(
        $Source, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) {
        throw "win11_lab_media_static: VM creator parser failed"
    }
    $definition = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $Name
        }, $true)
    if ($null -eq $definition) {
        throw ("win11_lab_media_static: VM creator function is missing " + $Name)
    }
    $body = $definition.Body.Extent.Text.Trim()
    if ($body.Length -lt 2 -or $body[0] -ne "{" -or $body[$body.Length - 1] -ne "}") {
        throw ("win11_lab_media_static: VM creator function is malformed " + $Name)
    }
    Set-Item -Path ("Function:\script:{0}" -f $Name) -Value (
        [scriptblock]::Create($body.Substring(1, $body.Length - 2)))
}

function Assert-Win11LabSwitchRefusal {
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Action,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedCode
    )

    try {
        & $Action
    }
    catch {
        if ($_.Exception.Message -notmatch [regex]::Escape($ExpectedCode)) {
            throw ("win11_lab_media_static: expected switch refusal " +
                $ExpectedCode + ", got " + $_.Exception.Message)
        }
        return
    }
    throw ("win11_lab_media_static: expected switch refusal " + $ExpectedCode)
}

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ramshared-win11-media-static-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tempRoot -ErrorAction Stop | Out-Null

try {
    $sealedPath = Join-Path $tempRoot "sealed.xml"
    $embeddedPath = Join-Path $tempRoot "embedded.xml"
    Set-ManufacturedXml -Path $sealedPath -Content (New-ValidWin11LabAutounattendXml)
    Set-ManufacturedXml -Path $embeddedPath -Content (New-ValidWin11LabAutounattendXml)

    $sealedContract = Get-Win11LabAutounattendContract -Path $sealedPath
    $embeddedContract = Get-Win11LabAutounattendContract -Path $embeddedPath
    Assert-Win11LabSealedAutounattendContract -Contract $sealedContract -ExpectedSha256 $sealedContract.sha256
    Assert-Win11LabPrimaryIsoContract `
        -SealedContract $sealedContract `
        -EmbeddedContract $embeddedContract `
        -SealedAutounattendSha256 $sealedContract.sha256 `
        -EfiNoPromptPresent $true | Out-Null
    Write-Output "PASS sealed_unattend_contract_accepts_required_windowspe_and_oobe_structure"

    $missingProductKeyPath = Join-Path $tempRoot "missing-product-key.xml"
    Set-ManufacturedXml -Path $missingProductKeyPath -Content ((New-ValidWin11LabAutounattendXml) -replace '(?s)<ProductKey>.*?</ProductKey>', '')
    Assert-RefusalWithoutSecret -ExpectedCode "product_key" -Action {
        Get-Win11LabAutounattendContract -Path $missingProductKeyPath | Out-Null
    }
    Write-Output "PASS missing_windowspe_productkey_is_refused"

    $missingImageIndexPath = Join-Path $tempRoot "missing-image-index.xml"
    Set-ManufacturedXml -Path $missingImageIndexPath -Content ((New-ValidWin11LabAutounattendXml) -replace '(?s)<MetaData.*?</MetaData>', '')
    Assert-RefusalWithoutSecret -ExpectedCode "image_index" -Action {
        Get-Win11LabAutounattendContract -Path $missingImageIndexPath | Out-Null
    }
    Write-Output "PASS missing_image_index_is_refused"

    $missingOobePath = Join-Path $tempRoot "missing-oobe-shell.xml"
    Set-ManufacturedXml -Path $missingOobePath -Content ((New-ValidWin11LabAutounattendXml) -replace '(?s)<settings pass="oobeSystem">.*?</settings>', '<settings pass="oobeSystem"></settings>')
    Assert-RefusalWithoutSecret -ExpectedCode "oobe_shell_setup" -Action {
        Get-Win11LabAutounattendContract -Path $missingOobePath | Out-Null
    }
    Write-Output "PASS missing_oobe_shell_setup_is_refused"

    $missingLogonCountPath = Join-Path $tempRoot "missing-logon-count.xml"
    Set-ManufacturedXml -Path $missingLogonCountPath -Content ((New-ValidWin11LabAutounattendXml) -replace '<LogonCount>1</LogonCount>', '')
    Assert-RefusalWithoutSecret -ExpectedCode "autologon_logoncount" -Action {
        Get-Win11LabAutounattendContract -Path $missingLogonCountPath | Out-Null
    }
    Write-Output "PASS autologon_logoncount_is_required"

    $mismatchedAccountPath = Join-Path $tempRoot "mismatched-account.xml"
    Set-ManufacturedXml -Path $mismatchedAccountPath -Content ((New-ValidWin11LabAutounattendXml) -replace '<Name>manufactured-admin</Name>', '<Name>foreign-admin</Name>')
    Assert-RefusalWithoutSecret -ExpectedCode "autologon_account_binding" -Action {
        Get-Win11LabAutounattendContract -Path $mismatchedAccountPath | Out-Null
    }
    Write-Output "PASS autologon_account_binding_is_exact"

    $missingDomainPath = Join-Path $tempRoot "missing-autologon-domain.xml"
    Set-ManufacturedXml -Path $missingDomainPath -Content ((New-ValidWin11LabAutounattendXml) -replace '<Domain>MANUFACTURED-LAB</Domain>', '')
    Assert-RefusalWithoutSecret -ExpectedCode "autologon_domain" -Action {
        Get-Win11LabAutounattendContract -Path $missingDomainPath | Out-Null
    }
    Write-Output "PASS autologon_domain_binds_exact_local_computer"

    $missingAutoLogonWorkaroundPath = Join-Path $tempRoot "missing-autologon-workaround.xml"
    Set-ManufacturedXml -Path $missingAutoLogonWorkaroundPath -Content ((New-ValidWin11LabAutounattendXml) -replace '(?s)<FirstLogonCommands>.*?</FirstLogonCommands>', '')
    Assert-RefusalWithoutSecret -ExpectedCode "autologon_count_workaround" -Action {
        Get-Win11LabAutounattendContract -Path $missingAutoLogonWorkaroundPath | Out-Null
    }
    $changedAutoLogonWorkaroundPath = Join-Path $tempRoot "changed-autologon-workaround.xml"
    Set-ManufacturedXml -Path $changedAutoLogonWorkaroundPath -Content ((New-ValidWin11LabAutounattendXml) -replace 'AutoLogonCount /t REG_DWORD /d 0 /f', 'AutoLogonCount /t REG_DWORD /d 2 /f')
    Assert-RefusalWithoutSecret -ExpectedCode "autologon_count_workaround" -Action {
        Get-Win11LabAutounattendContract -Path $changedAutoLogonWorkaroundPath | Out-Null
    }
    Write-Output "PASS autologon_count_workaround_is_required"

    $generatedPath = Join-Path $tempRoot "generated-autounattend.xml"
    $generatorOutput = @(& $AutounattendGeneratorPath `
        -OutputXml $generatedPath `
        -ComputerName "MANUFACTURED-LAB" `
        -LabUser "manufactured-admin" `
        -Password "manufactured-password-must-not-appear" `
        -ImageIndex 6)
    if ($generatorOutput -join "`n" -match 'manufactured-password-must-not-appear') {
        throw "win11_lab_media_static: generator output echoed a manufactured password"
    }
    $generatedContract = Get-Win11LabAutounattendContract -Path $generatedPath
    if (-not [bool]$generatedContract.autologon_complete -or
        [int]$generatedContract.autologon_logon_count -ne 1 -or
        [int]$generatedContract.local_admin_account_count -ne 1 -or
        -not [bool]$generatedContract.autologon_domain_bound -or
        -not [bool]$generatedContract.autologon_count_workaround_exact) {
        throw "win11_lab_media_static: generator account binding receipt is incomplete"
    }
    Write-Output "PASS autounattend_generator_emits_exact_account_binding"

    $differentEmbeddedPath = Join-Path $tempRoot "different-embedded.xml"
    Set-ManufacturedXml -Path $differentEmbeddedPath -Content (New-ValidWin11LabAutounattendXml -ImageIndex 7)
    $differentEmbeddedContract = Get-Win11LabAutounattendContract -Path $differentEmbeddedPath
    Assert-RefusalWithoutSecret -ExpectedCode "embedded_sha256_mismatch" -Action {
        Assert-Win11LabPrimaryIsoContract `
            -SealedContract $sealedContract `
            -EmbeddedContract $differentEmbeddedContract `
            -SealedAutounattendSha256 $sealedContract.sha256 `
            -EfiNoPromptPresent $true | Out-Null
    }
    Write-Output "PASS embedded_unattend_hash_mismatch_is_refused"

    $passwordMarker = "manufactured-password-must-not-appear"
    $passwordFixturePath = Join-Path $tempRoot "password-fixture.xml"
    Set-ManufacturedXml -Path $passwordFixturePath -Content ((New-ValidWin11LabAutounattendXml -PasswordMarker $passwordMarker) -replace '(?s)<ProductKey>.*?</ProductKey>', '')
    Assert-RefusalWithoutSecret -ExpectedCode "product_key" -Secret $passwordMarker -Action {
        Get-Win11LabAutounattendContract -Path $passwordFixturePath | Out-Null
    }
    Write-Output "PASS unattend_diagnostics_never_echo_password"

    $growthStart = [DateTime]::UtcNow
    $script:manufacturedGrowthLengths = New-Object 'System.Collections.Generic.Queue[System.Int64]'
    $script:manufacturedGrowthLengths.Enqueue([int64]100)
    $script:manufacturedGrowthLengths.Enqueue([int64]180)
    $script:manufacturedGrowthTimes = New-Object 'System.Collections.Generic.Queue[System.DateTime]'
    $script:manufacturedGrowthTimes.Enqueue($growthStart)
    $script:manufacturedGrowthTimes.Enqueue($growthStart.AddSeconds(1))
    $growthReceipt = Wait-Win11LabExactVhdGrowth `
        -VhdPath "manufactured.vhdx" `
        -BaselineBytes 100 `
        -TimeoutSeconds 30 `
        -PollMilliseconds 250 `
        -ReadLength { param($ignoredPath) return $script:manufacturedGrowthLengths.Dequeue() } `
        -UtcNow { return $script:manufacturedGrowthTimes.Dequeue() } `
        -SleepAction { param($milliseconds) }
    if ($growthReceipt.observed_bytes -ne 180 -or $growthReceipt.baseline_bytes -ne 100) {
        throw "win11_lab_media_static: manufactured exact VHD growth receipt is invalid"
    }
    Write-Output "PASS exact_vhd_growth_receipt_is_manufactured"

    $script:manufacturedTimeoutLengths = New-Object 'System.Collections.Generic.Queue[System.Int64]'
    $script:manufacturedTimeoutLengths.Enqueue([int64]100)
    $script:manufacturedTimeoutTimes = New-Object 'System.Collections.Generic.Queue[System.DateTime]'
    $script:manufacturedTimeoutTimes.Enqueue($growthStart)
    $script:manufacturedTimeoutTimes.Enqueue($growthStart.AddSeconds(31))
    Assert-RefusalWithoutSecret -ExpectedCode "setup_vhd_growth_timeout" -Action {
        Wait-Win11LabExactVhdGrowth `
            -VhdPath "manufactured.vhdx" `
            -BaselineBytes 100 `
            -TimeoutSeconds 30 `
            -PollMilliseconds 250 `
            -ReadLength { param($ignoredPath) return $script:manufacturedTimeoutLengths.Dequeue() } `
            -UtcNow { return $script:manufacturedTimeoutTimes.Dequeue() } `
            -SleepAction { param($milliseconds) } | Out-Null
    }
    Write-Output "PASS exact_vhd_growth_timeout_is_refused"
} finally {
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}

$contractText = Get-Content -LiteralPath $ContractPath -Raw
$vmCreatorText = Get-Content -LiteralPath $VmCreatorPath -Raw
$isoBuilderText = Get-Content -LiteralPath $IsoBuilderPath -Raw
if (-not (Test-Path -LiteralPath $ReadinessPath -PathType Leaf)) {
    throw "win11_lab_media_static: readiness harness missing"
}
$readinessText = Get-Content -LiteralPath $ReadinessPath -Raw

if ($isoBuilderText -notmatch '(?s)\[ValidateRange\(300,\s*1800\)\]\s*\[int\]\$MediaStageTimeoutSeconds\s*=\s*900' -or
    $isoBuilderText -notmatch 'Invoke-Win11LabSourceIsoStage[\s\S]*?-TimeoutSeconds\s+\$MediaStageTimeoutSeconds' -or
    $isoBuilderText -notmatch 'Invoke-Win11LabPrimaryIsoContract[\s\S]*?-TimeoutSeconds\s+\$MediaProbeTimeoutSeconds' -or
    $contractText -notmatch '(?s)function Invoke-Win11LabMediaWorker[\s\S]*?\[ValidateRange\(10,\s*1800\)\]') {
    throw "source_iso_stage_budget_is_independent_and_bounded"
}
Write-Output "PASS source_iso_stage_budget_is_independent_and_bounded"

if ($isoBuilderText -notmatch '(?s)\[ValidateRange\(300,\s*1800\)\]\s*\[int\]\$OscdimgTimeoutSeconds\s*=\s*900' -or
    $isoBuilderText -notmatch 'Invoke-Win11LabExternalProcessBounded' -or
    $isoBuilderText -match '&\s*\$oscdimgPath') {
    throw "oscdimg_deadline_terminates_process_tree"
}
$timeoutStarted = [DateTime]::UtcNow
Assert-RefusalWithoutSecret -ExpectedCode "external_process_timeout" -Action {
    Invoke-Win11LabExternalProcessBounded `
        -FilePath (Join-Path $PSHOME "powershell.exe") `
        -ArgumentValues @("-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 5") `
        -TimeoutSeconds 1 | Out-Null
}
if (([DateTime]::UtcNow - $timeoutStarted).TotalSeconds -ge 5) {
    throw "win11_lab_media_static: bounded external process exceeded deadline"
}
Write-Output "PASS oscdimg_deadline_terminates_process_tree"

$switchNameParameterPattern = '(?s)\[Parameter\(Mandatory\s*=\s*\$true\)\]\s*\[ValidateNotNullOrEmpty\(\)\]\s*\[string\]\$SwitchName(?:,|\s*\))'
if ($vmCreatorText -notmatch $switchNameParameterPattern -or
    $vmCreatorText -match '(?i)Default Switch' -or
    $vmCreatorText -match '(?i)\bExternal\b') {
    throw "win11_lab_media_static: VM creator does not require one explicit non-host-specific switch"
}
foreach ($needle in @(
    'Resolve-Win11LabExactSwitch',
    'Get-VMSwitch -Name $RequestedSwitchName -ErrorAction Stop',
    '$exactSwitch = Resolve-Win11LabExactSwitch -RequestedSwitchName $SwitchName',
    '-SwitchName $exactSwitch.name',
    'expected_switch_name = [string]$exactSwitch.name',
    'network_policy = $NetworkPolicy',
    'readiness_required = $false',
    '$metadata["readiness_required"] = $true',
    'RequireRoutableIPv4',
    'SealedOffline'
)) {
    if ($vmCreatorText -notmatch [regex]::Escape($needle)) {
        throw ("win11_lab_media_static: VM creator missing switch/readiness handoff " + $needle)
    }
}
if ($vmCreatorText -match '(?i)Wait-Win11LabReady|\bPassword\b') {
    throw "win11_lab_media_static: VM creator must not wait for readiness or receive a credential"
}
$switchPreflightIndex = $vmCreatorText.IndexOf('$exactSwitch = Resolve-Win11LabExactSwitch -RequestedSwitchName $SwitchName', [System.StringComparison]::Ordinal)
$rootCreateIndex = $vmCreatorText.IndexOf('New-Item -ItemType Directory -Force -Path $Root', [System.StringComparison]::Ordinal)
$newVhdIndexForSwitch = $vmCreatorText.IndexOf('New-VHD -Path $vhdPath', [System.StringComparison]::Ordinal)
$newVmIndexForSwitch = $vmCreatorText.IndexOf('New-VM -Name $VMName', [System.StringComparison]::Ordinal)
if ($switchPreflightIndex -lt 0 -or $rootCreateIndex -lt 0 -or
    $switchPreflightIndex -gt $rootCreateIndex -or
    $switchPreflightIndex -gt $newVhdIndexForSwitch -or
    $switchPreflightIndex -gt $newVmIndexForSwitch) {
    throw "win11_lab_media_static: exact switch preflight does not precede root/VHD/VM creation"
}
foreach ($needle in @('ExpectedSwitchName', 'NetworkPolicy', 'RequireRoutableIPv4', 'SealedOffline')) {
    if ($readinessText -notmatch [regex]::Escape($needle)) {
        throw ("win11_lab_media_static: DT-57 readiness contract missing " + $needle)
    }
}
$networkPolicyParameterPattern = '(?s)\[ValidateSet\("RequireRoutableIPv4",\s*"SealedOffline"\)\]\s*\[string\]\$NetworkPolicy'
if ($vmCreatorText -notmatch $networkPolicyParameterPattern -or
    $readinessText -notmatch $networkPolicyParameterPattern) {
    throw "win11_lab_media_static: creator and DT-57 do not share one exact network-policy domain"
}
if ($vmCreatorText -notmatch '(?s)\[ValidateRange\(4,\s*64\)\]\s*\[int\]\$StartupMemoryGB\s*=\s*4' -or
    $vmCreatorText -notmatch 'MinMemoryGB\s*-gt\s*\$StartupMemoryGB' -or
    $vmCreatorText -notmatch 'StartupMemoryGB\s*-gt\s*\$MaxMemoryGB') {
    throw "win11_lab_creator_refuses_setup_below_four_gib"
}
Write-Output "PASS win11_lab_creator_refuses_setup_below_four_gib"

Import-Win11LabVmCreatorFunction -Name 'Resolve-Win11LabExactSwitch' -Source $vmCreatorText
$manufacturedSwitchName = 'Manufactured exact switch'
$exactSwitchReceipt = Resolve-Win11LabExactSwitch -RequestedSwitchName $manufacturedSwitchName -GetSwitch {
    param($RequestedSwitchName)
    [pscustomobject]@{
        schema = [int]1
        name = [string]$RequestedSwitchName
        SwitchType = 'Private'
    }
}
if ([int]$exactSwitchReceipt.schema -ne 1 -or
    [string]$exactSwitchReceipt.name -cne $manufacturedSwitchName -or
    [string]$exactSwitchReceipt.switch_type -cne 'Private') {
    throw "win11_lab_media_static: exact manufactured switch receipt is invalid"
}
Write-Output "PASS win11_lab_switch_exact_receipt_is_typed"

Assert-Win11LabSwitchRefusal -ExpectedCode 'win11_lab_switch_name_missing' -Action {
    Resolve-Win11LabExactSwitch -RequestedSwitchName ' ' -GetSwitch {
        param($RequestedSwitchName)
        [pscustomobject]@{ Name = $RequestedSwitchName }
    } | Out-Null
}
Write-Output "PASS win11_lab_switch_missing_is_refused"

Assert-Win11LabSwitchRefusal -ExpectedCode 'win11_lab_switch_ambiguous' -Action {
    Resolve-Win11LabExactSwitch -RequestedSwitchName $manufacturedSwitchName -GetSwitch {
        param($RequestedSwitchName)
        @(
            [pscustomobject]@{ Name = $RequestedSwitchName },
            [pscustomobject]@{ Name = $RequestedSwitchName }
        )
    } | Out-Null
}
Write-Output "PASS win11_lab_switch_ambiguous_is_refused"

Assert-Win11LabSwitchRefusal -ExpectedCode 'win11_lab_switch_unavailable' -Action {
    Resolve-Win11LabExactSwitch -RequestedSwitchName $manufacturedSwitchName -GetSwitch {
        param($RequestedSwitchName)
        @()
    } | Out-Null
}
Assert-Win11LabSwitchRefusal -ExpectedCode 'win11_lab_switch_provider_failed' -Action {
    Resolve-Win11LabExactSwitch -RequestedSwitchName $manufacturedSwitchName -GetSwitch {
        param($RequestedSwitchName)
        throw 'manufactured Get-VMSwitch provider failure'
    } | Out-Null
}
Write-Output "PASS win11_lab_switch_unavailable_is_refused"

Assert-Win11LabSwitchRefusal -ExpectedCode 'win11_lab_switch_identity_mismatch' -Action {
    Resolve-Win11LabExactSwitch -RequestedSwitchName $manufacturedSwitchName -GetSwitch {
        param($RequestedSwitchName)
        [pscustomobject]@{ Name = 'foreign manufactured switch' }
    } | Out-Null
}
Write-Output "PASS win11_lab_creator_dt57_switch_policy_parity_is_exact"

foreach ($needle in @(
    "WorkerMode",
    "Mount-DiskImage",
    "Dismount-DiskImage",
    "taskkill.exe",
    "WaitForExit",
    "Invoke-Win11LabMediaCleanup",
    "Invoke-Win11LabPrimaryIsoContract",
    "Invoke-Win11LabSourceIsoStage"
)) {
    if ($contractText -notmatch [regex]::Escape($needle)) {
        throw ("win11_lab_media_static: contract helper missing bounded-media seam " + $needle)
    }
}

foreach ($receiptBinding in @(
    'autologon_domain_bound = $embeddedContract.autologon_domain_bound',
    'autologon_count_workaround_exact = $embeddedContract.autologon_count_workaround_exact'
)) {
    if ($contractText -notmatch [regex]::Escape($receiptBinding)) {
        throw ("embedded_probe_receipt_preserves_autologon_contract failed: missing " + $receiptBinding)
    }
}
Write-Output "PASS embedded_probe_receipt_preserves_autologon_contract"

foreach ($needle in @(
    'AutounattendXml',
    'SealedAutounattendSha256',
    '$requiredTpmCommands',
    'Set-VMKeyProtector -VMName $VMName -NewLocalKeyProtector',
    'Enable-VMTPM -VMName $VMName',
    'Start-VM -Name $VMName',
    '$setupStartVhdBytesBefore',
    'Wait-Win11LabExactVhdGrowth -VhdPath $vhdPath',
    'Get-VMHardDiskDrive',
    'Set-VMFirmware -VMName $VMName -FirstBootDevice $vhdBootDrive',
    '$nextBootFirmware = Get-VMFirmware -VMName $VMName',
    'Stop-Win11LabVmFailSafe',
    'Invoke-Win11LabPrimaryIsoContract -IsoPath $WindowsIso',
    'Assert-Win11LabPrimaryIsoContract',
    'New-VHD -Path $vhdPath',
    'New-VM -Name $VMName'
)) {
    if ($vmCreatorText -notmatch [regex]::Escape($needle)) {
        throw ("win11_lab_media_static: VM creator missing " + $needle)
    }
}

$preflightIndex = $vmCreatorText.IndexOf('Invoke-Win11LabPrimaryIsoContract -IsoPath $WindowsIso', [System.StringComparison]::Ordinal)
$newVhdIndex = $vmCreatorText.IndexOf('New-VHD -Path $vhdPath', [System.StringComparison]::Ordinal)
$newVmIndex = $vmCreatorText.IndexOf('New-VM -Name $VMName', [System.StringComparison]::Ordinal)
if ($preflightIndex -lt 0 -or $preflightIndex -gt $newVhdIndex -or $preflightIndex -gt $newVmIndex) {
    throw "win11_lab_media_static: primary ISO contract does not precede VM/VHD creation"
}
if ($vmCreatorText -match "AutounattendIso|Add-VMDvdDrive.*Autounattend") {
    throw "win11_lab_media_static: second unattended-answer DVD is forbidden"
}
Write-Output "PASS primary_iso_contract_precedes_vm_vhd_creation"
Write-Output "PASS second_unattend_dvd_is_forbidden"

$tpmCapabilityIndex = $vmCreatorText.IndexOf('$requiredTpmCommands', [System.StringComparison]::Ordinal)
$keyProtectorIndex = $vmCreatorText.IndexOf('Set-VMKeyProtector -VMName $VMName -NewLocalKeyProtector', [System.StringComparison]::Ordinal)
$enableTpmIndex = $vmCreatorText.IndexOf('Enable-VMTPM -VMName $VMName', [System.StringComparison]::Ordinal)
$startVmIndex = $vmCreatorText.IndexOf('Start-VM -Name $VMName', [System.StringComparison]::Ordinal)
if ($tpmCapabilityIndex -lt 0 -or $tpmCapabilityIndex -gt $newVhdIndex) {
    throw "win11_lab_media_static: native vTPM capability does not precede VHD creation"
}
if ($keyProtectorIndex -lt 0 -or $enableTpmIndex -lt 0 -or $startVmIndex -lt 0 -or
    $keyProtectorIndex -gt $startVmIndex -or $enableTpmIndex -gt $startVmIndex) {
    throw "win11_lab_media_static: native local-key-protector/vTPM does not precede first start"
}
if ($vmCreatorText -match "BypassTPMCheck|LabConfig|AllowUpgradesWithUnsupportedTPMOrCPU|reg\.exe|New-ItemProperty.*TPM") {
    throw "win11_lab_media_static: setup-requirement bypass is forbidden"
}
Write-Output "PASS native_vtpm_precedes_first_start"
Write-Output "PASS setup_requirement_bypass_is_forbidden"

$startVmIndex = $vmCreatorText.IndexOf('Start-VM -Name $VMName', [System.StringComparison]::Ordinal)
$vhdGrowthIndex = $vmCreatorText.IndexOf('Wait-Win11LabExactVhdGrowth -VhdPath $vhdPath', [System.StringComparison]::Ordinal)
$vhdFirstBootIndex = $vmCreatorText.IndexOf('Set-VMFirmware -VMName $VMName -FirstBootDevice $vhdBootDrive', [System.StringComparison]::Ordinal)
$firstBootReadbackIndex = $vmCreatorText.IndexOf('$nextBootFirmware = Get-VMFirmware -VMName $VMName', [System.StringComparison]::Ordinal)
if ($startVmIndex -lt 0 -or $vhdGrowthIndex -lt 0 -or $vhdFirstBootIndex -lt 0 -or $firstBootReadbackIndex -lt 0 -or
    $startVmIndex -gt $vhdGrowthIndex -or $vhdGrowthIndex -gt $vhdFirstBootIndex -or $vhdFirstBootIndex -gt $firstBootReadbackIndex) {
    throw "win11_lab_media_static: exact VHD growth and next-boot ordering is invalid"
}
if ($vmCreatorText -notmatch [regex]::Escape('Stop-Win11LabVmFailSafe -Name $VMName') -or
    $vmCreatorText -notmatch [regex]::Escape('Stop-VM -Name $Name -TurnOff')) {
    throw "win11_lab_media_static: unattended ISO first-boot failure does not stop the new VM"
}
Write-Output "PASS setup_vhd_growth_precedes_next_boot_vhd"
Write-Output "PASS unattended_iso_first_boot_failure_stops_new_vm"

foreach ($stageCode in @("start_vm", "vhd_growth", "boot_order_set", "boot_order_readback")) {
    if ($vmCreatorText -notmatch ('\$setupStartStage\s*=\s*"' + [regex]::Escape($stageCode) + '"')) {
        throw "win11_lab_start_failure_stage_is_exact failed: missing $stageCode"
    }
}
if ($vmCreatorText -match 'win11_lab_vm_unattended_start_boot_order_failed' -or
    $vmCreatorText -notmatch 'win11_lab_vm_start_stage_failed:' -or
    $vmCreatorText -notmatch 'Get-Win11LabSetupStartFailureCode') {
    throw "win11_lab_start_failure_stage_is_exact failed: diagnostic is generic or unsanitized"
}
Write-Output "PASS win11_lab_start_failure_stage_is_exact"

if ($vmCreatorText -match '\$nextBootFirmware\.FirstBootDevice' -or
    $vmCreatorText -notmatch '\$nextBootFirmware\.BootOrder' -or
    $vmCreatorText -notmatch '\$nextBootOrder\[0\]' -or
    $vmCreatorText -notmatch '\$nextBootOrder\[0\]\.Device' -or
    $vmCreatorText -notmatch 'Microsoft\.HyperV\.PowerShell\.VMBootSource' -or
    $vmCreatorText -notmatch 'Microsoft\.HyperV\.PowerShell\.HardDiskDrive') {
    throw "win11_lab_boot_order_readback_uses_bootorder_head failed"
}
Write-Output "PASS win11_lab_boot_order_readback_uses_bootorder_head"

foreach ($needle in @(
    'SealedAutounattendSha256',
    'Get-Win11LabAutounattendContract -Path $AutounattendXml',
    'Assert-Win11LabSealedAutounattendContract',
    'Invoke-Win11LabSourceIsoStage',
    'Invoke-Win11LabPrimaryIsoContract -IsoPath $OutputIso',
    'Assert-Win11LabPrimaryIsoContract'
)) {
    if ($isoBuilderText -notmatch [regex]::Escape($needle)) {
        throw ("win11_lab_media_static: ISO builder missing " + $needle)
    }
}
Write-Output "PASS bounded_iso_probe_and_cleanup_are_required"
Write-Output "PASS Test-Win11LabMediaContractStatic"
