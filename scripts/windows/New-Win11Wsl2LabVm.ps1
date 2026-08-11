#Requires -Version 5.1
<#
.SYNOPSIS
  Create a disposable Windows 11 Hyper-V VM for isolated WSL2 freeze campaigns.

.DESCRIPTION
  This script creates a new VM with a new dynamic VHD under a dedicated lab
  directory. It never modifies win11-drill, never formats disks, and refuses to
  run when the target VM or target VHD already exists. The goal is to provide a
  clean WSL2-capable guest surface without reimaging an existing lab disk. The
  primary installer ISO must already embed the sealed unattended answer file;
  no second unattended-answer DVD is accepted.
  The default VHD root is on C: because the HDD-backed lab path is too slow for
  Windows setup, Windows Update, and WSL package registration.
#>
[CmdletBinding()]
param(
    [string]$VMName = "win11-wsl2-lab",
    [string]$Root = "C:\ramshared-hyperv\win11-wsl2-lab",
    [string]$WindowsIso = "E:\Hyper-V\iso\Win11_25H2_English_x64_v2_noprompt_unattend.iso",
    [string]$AutounattendXml = "E:\Hyper-V\iso\unattend-staging\Autounattend.xml",
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Fa-f0-9]{64}$')]
    [string]$SealedAutounattendSha256,
    [ValidateRange(10, 300)]
    [int]$MediaProbeTimeoutSeconds = 90,
    [ValidateRange(30, 1800)]
    [int]$SetupStartTimeoutSeconds = 900,
    [int]$VhdSizeGB = 80,
    [ValidateRange(4, 64)]
    [int]$StartupMemoryGB = 4,
    [ValidateRange(1, 64)]
    [int]$MinMemoryGB = 2,
    [ValidateRange(4, 64)]
    [int]$MaxMemoryGB = 8,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$SwitchName,
    [ValidateSet("RequireRoutableIPv4", "SealedOffline")]
    [string]$NetworkPolicy = "RequireRoutableIPv4",
    [switch]$Start
)

$ErrorActionPreference = "Stop"

$mediaContractPath = Join-Path $PSScriptRoot "Win11LabMediaContract.ps1"
if (-not (Test-Path -LiteralPath $mediaContractPath -PathType Leaf)) {
    Write-Error "Win11 lab media contract helper is missing"
    exit 2
}
. $mediaContractPath

function Fail([string]$Message) {
    Write-Error $Message
    exit 2
}

if ($MinMemoryGB -gt $StartupMemoryGB -or
    $StartupMemoryGB -gt $MaxMemoryGB) {
    Fail "VM memory ordering must satisfy MinMemoryGB <= StartupMemoryGB <= MaxMemoryGB"
}

function Resolve-Win11LabExactSwitch {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$RequestedSwitchName,
        [scriptblock]$GetSwitch = {
            param($RequestedSwitchName)
            @(Get-VMSwitch -Name $RequestedSwitchName -ErrorAction Stop)
        }
    )

    if ([string]::IsNullOrWhiteSpace($RequestedSwitchName) -or
        $RequestedSwitchName -cne $RequestedSwitchName.Trim()) {
        throw "win11_lab_switch_name_missing"
    }
    try {
        $switches = @(& $GetSwitch $RequestedSwitchName | Where-Object { $null -ne $_ })
    }
    catch {
        throw "win11_lab_switch_provider_failed"
    }
    if ($switches.Count -eq 0) {
        throw "win11_lab_switch_unavailable"
    }
    if ($switches.Count -ne 1) {
        throw "win11_lab_switch_ambiguous"
    }
    $returnedName = [string]$switches[0].Name
    if ([string]::IsNullOrWhiteSpace($returnedName) -or
        $returnedName -cne $RequestedSwitchName) {
        throw "win11_lab_switch_identity_mismatch"
    }
    [pscustomobject]@{
        schema = [int]1
        name = $returnedName
        switch_type = [string]$switches[0].SwitchType
    }
}

function Stop-Win11LabVmFailSafe {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [ValidateRange(5, 120)]
        [int]$TimeoutSeconds = 30
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $vm = Get-VM -Name $Name -ErrorAction Stop
    if ([string]$vm.State -eq "Off") {
        return
    }

    try {
        Stop-VM -Name $Name -TurnOff -ErrorAction Stop | Out-Null
    } catch {
        throw "win11_lab_vm_failsafe_stop_invoke_failed"
    }

    while ([DateTime]::UtcNow -lt $deadline) {
        $vm = Get-VM -Name $Name -ErrorAction Stop
        if ([string]$vm.State -eq "Off") {
            return
        }
        Start-Sleep -Milliseconds 500
    }

    throw "win11_lab_vm_failsafe_stop_timeout"
}

function Get-Win11LabSetupStartFailureCode {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet("start_vm", "vhd_growth", "boot_order_set", "boot_order_readback")]
        [string]$Stage,
        [Parameter(Mandatory = $true)]
        [string]$Message
    )

    if ($Stage -ceq "vhd_growth" -and
        $Message -cmatch '^win11_lab_media_contract_setup_vhd_(?:baseline_invalid|clock_invalid|growth_read_failed|growth_timeout|sleep_failed)$') {
        return $Message
    }
    if ($Stage -ceq "boot_order_readback" -and
        $Message -ceq "win11_lab_vm_next_boot_vhd_readback_failed") {
        return $Message
    }
    switch ($Stage) {
        "start_vm" { return "win11_lab_vm_start_provider_failed" }
        "vhd_growth" { return "win11_lab_vm_vhd_growth_provider_failed" }
        "boot_order_set" { return "win11_lab_vm_boot_order_set_provider_failed" }
        "boot_order_readback" { return "win11_lab_vm_boot_order_readback_provider_failed" }
    }
}

if (Get-VM -Name $VMName -ErrorAction SilentlyContinue) {
    Fail "VM already exists: $VMName"
}
if (-not (Test-Path -LiteralPath $WindowsIso)) {
    Fail "Windows ISO not found: $WindowsIso"
}
if (-not (Test-Path -LiteralPath $AutounattendXml)) {
    Fail "Autounattend.xml not found: $AutounattendXml"
}

$requiredTpmCommands = @("Set-VMKeyProtector", "Enable-VMTPM")
foreach ($requiredTpmCommand in $requiredTpmCommands) {
    if ($null -eq (Get-Command -Name $requiredTpmCommand -CommandType Cmdlet -ErrorAction SilentlyContinue)) {
        Fail "Native Hyper-V TPM command unavailable: $requiredTpmCommand"
    }
}
if ($Start) {
    $requiredStartCommands = @("Get-VMHardDiskDrive", "Get-VMFirmware", "Stop-VM")
    foreach ($requiredStartCommand in $requiredStartCommands) {
        if ($null -eq (Get-Command -Name $requiredStartCommand -CommandType Cmdlet -ErrorAction SilentlyContinue)) {
            Fail "Required Hyper-V setup-start command unavailable: $requiredStartCommand"
        }
    }
}

try {
    $sealedAutounattendContract = Get-Win11LabAutounattendContract -Path $AutounattendXml
    Assert-Win11LabSealedAutounattendContract `
        -Contract $sealedAutounattendContract `
        -ExpectedSha256 $SealedAutounattendSha256
    $embeddedAutounattendContract = Invoke-Win11LabPrimaryIsoContract -IsoPath $WindowsIso -TimeoutSeconds $MediaProbeTimeoutSeconds
    $mediaContractReceipt = Assert-Win11LabPrimaryIsoContract `
        -SealedContract $sealedAutounattendContract `
        -EmbeddedContract $embeddedAutounattendContract `
        -SealedAutounattendSha256 $SealedAutounattendSha256 `
        -EfiNoPromptPresent ([bool]$embeddedAutounattendContract.efi_noprompt_present)
} catch {
    Fail $_.Exception.Message
}
try {
    $exactSwitch = Resolve-Win11LabExactSwitch -RequestedSwitchName $SwitchName
} catch {
    Fail $_.Exception.Message
}
if (Test-Path -LiteralPath $Root) {
    $children = @(Get-ChildItem -LiteralPath $Root -Force -ErrorAction SilentlyContinue)
    if ($children.Count -gt 0) {
        Fail "Target root exists and is not empty: $Root"
    }
} else {
    New-Item -ItemType Directory -Force -Path $Root | Out-Null
}

$vhdDir = Join-Path $Root "Virtual Hard Disks"
$vmConfigDir = Join-Path $Root "Virtual Machines"
New-Item -ItemType Directory -Force -Path $vhdDir | Out-Null
New-Item -ItemType Directory -Force -Path $vmConfigDir | Out-Null

$vhdPath = Join-Path $vhdDir "$VMName.vhdx"
if (Test-Path -LiteralPath $vhdPath) {
    Fail "Target VHD already exists: $vhdPath"
}

New-VHD -Path $vhdPath -SizeBytes ([int64]$VhdSizeGB * 1GB) -Dynamic | Out-Null
New-VM -Name $VMName `
    -Generation 2 `
    -MemoryStartupBytes ([int64]$StartupMemoryGB * 1GB) `
    -VHDPath $vhdPath `
    -Path $vmConfigDir `
    -SwitchName $exactSwitch.name | Out-Null

Set-VMKeyProtector -VMName $VMName -NewLocalKeyProtector
Enable-VMTPM -VMName $VMName

Set-VM -Name $VMName `
    -CheckpointType Disabled `
    -AutomaticCheckpointsEnabled $false `
    -AutomaticStartAction Nothing `
    -AutomaticStopAction ShutDown `
    -DynamicMemory `
    -MemoryMinimumBytes ([int64]$MinMemoryGB * 1GB) `
    -MemoryMaximumBytes ([int64]$MaxMemoryGB * 1GB)

Set-VMProcessor -VMName $VMName -Count 4 -ExposeVirtualizationExtensions $true
Set-VMFirmware -VMName $VMName -EnableSecureBoot On -SecureBootTemplate "MicrosoftWindows"
Get-VMIntegrationService -VMName $VMName |
    Enable-VMIntegrationService -ErrorAction SilentlyContinue

$windowsDvd = Add-VMDvdDrive -VMName $VMName -Path $WindowsIso -Passthru
Set-VMFirmware -VMName $VMName -FirstBootDevice $windowsDvd

$vhdBootDrives = @(
    Get-VMHardDiskDrive -VMName $VMName | Where-Object {
        -not [string]::IsNullOrWhiteSpace([string]$_.Path) -and
        ([string]$_.Path -ieq [string]$vhdPath)
    }
)
if ($vhdBootDrives.Count -ne 1) {
    Fail "New VM does not expose exactly one target VHD boot device"
}
$vhdBootDrive = $vhdBootDrives[0]

$metadata = [ordered]@{
    vm = $VMName
    root = $Root
    vhd = $vhdPath
    windows_iso = $WindowsIso
    autounattend_xml = $AutounattendXml
    sealed_autounattend_sha256 = $mediaContractReceipt.sealed_sha256
    embedded_autounattend_sha256 = $mediaContractReceipt.embedded_sha256
    primary_iso_efi_noprompt = $mediaContractReceipt.efi_noprompt_present
    vhd_size_gb = $VhdSizeGB
    nested_virtualization = $true
    virtual_tpm = "enabled_local_key_protector"
    integration_services = "enabled_by_pipeline"
    expected_switch_name = [string]$exactSwitch.name
    network_policy = $NetworkPolicy
    readiness_required = $false
    disk_mutation = "new_vhd_only"
    existing_lab_disks_modified = $false
    setup_start_proven = $false
    setup_vhd_bytes_before = $null
    setup_vhd_bytes_after = $null
    next_boot_device = "installer_iso_before_first_start"
}

if ($Start) {
    $setupStartStage = "start_vm"
    try {
        $setupStartVhdBytesBefore = [Int64](Get-Item -LiteralPath $vhdPath -ErrorAction Stop).Length
        Start-VM -Name $VMName
        $setupStartStage = "vhd_growth"
        $setupStartReceipt = Wait-Win11LabExactVhdGrowth -VhdPath $vhdPath `
            -BaselineBytes $setupStartVhdBytesBefore `
            -TimeoutSeconds $SetupStartTimeoutSeconds
        $setupStartStage = "boot_order_set"
        Set-VMFirmware -VMName $VMName -FirstBootDevice $vhdBootDrive
        $setupStartStage = "boot_order_readback"
        $nextBootFirmware = Get-VMFirmware -VMName $VMName -ErrorAction Stop
        $nextBootOrder = @($nextBootFirmware.BootOrder)
        $nextBootDevice = if ($nextBootOrder.Count -lt 1 -or $null -eq $nextBootOrder[0]) {
            $null
        }
        else {
            $nextBootOrder[0].Device
        }
        if ($nextBootOrder.Count -lt 1 -or $null -eq $nextBootOrder[0] -or
            [string]($nextBootOrder[0].GetType().FullName) -cne "Microsoft.HyperV.PowerShell.VMBootSource" -or
            $null -eq $nextBootDevice -or
            [string]($nextBootDevice.GetType().FullName) -cne "Microsoft.HyperV.PowerShell.HardDiskDrive" -or
            [string]::IsNullOrWhiteSpace([string]$nextBootDevice.Path) -or
            [string]$nextBootDevice.Path -ine [string]$vhdPath) {
            throw "win11_lab_vm_next_boot_vhd_readback_failed"
        }
        $metadata["setup_start_proven"] = $true
        $metadata["setup_vhd_bytes_before"] = $setupStartReceipt.baseline_bytes
        $metadata["setup_vhd_bytes_after"] = $setupStartReceipt.observed_bytes
        $metadata["next_boot_device"] = "exact_new_vhd"
        $metadata["readiness_required"] = $true
    } catch {
        $setupStartFailureCode = Get-Win11LabSetupStartFailureCode `
            -Stage $setupStartStage -Message $_.Exception.Message
        try {
            Stop-Win11LabVmFailSafe -Name $VMName
        } catch {
            Fail "win11_lab_vm_start_stage_failed:failsafe_stop:win11_lab_vm_failsafe_stop_failed"
        }
        Fail ("win11_lab_vm_start_stage_failed:{0}:{1}" -f $setupStartStage, $setupStartFailureCode)
    }
}

$metadata | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $Root "ramshared-wsl2-lab.json")

$metadata | ConvertTo-Json -Depth 4
