#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$BaseRoot,
    [Parameter(Mandatory = $true)][string]$ExpectedBaseVhdSha256,
    [Parameter(Mandatory = $true)][string]$VMName,
    [Parameter(Mandatory = $true)][string]$VMRoot,
    [Parameter(Mandatory = $true)][string]$ExpectedSwitchName,
    [Parameter(Mandatory = $true)][string]$ArtifactRoot,
    [ValidateRange(60, 1800)][int]$HashTimeoutSeconds = 600,
    [switch]$ApproveCreate
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Normalize-ReadyCloneSha256([string]$Value, [string]$Label) {
    $normalized = $Value.Trim().ToUpperInvariant()
    if ($Value -cne $Value.Trim() -or $normalized -notmatch '^[0-9A-F]{64}$') {
        throw "$Label is not one canonical SHA-256"
    }
    $normalized
}

function Get-ReadyCloneBoundedSha256 {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ResultPath,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds
    )

    $escapedPath = $Path.Replace("'", "''")
    $escapedResult = $ResultPath.Replace("'", "''")
    $worker = "& { (Get-FileHash -LiteralPath '$escapedPath' -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant() | Set-Content -LiteralPath '$escapedResult' -Encoding ASCII }"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($worker))
    $process = Start-Process -FilePath "powershell.exe" -ArgumentList @(
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", $encoded
    ) -WindowStyle Hidden -PassThru -ErrorAction Stop
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        $null = & taskkill.exe /PID $process.Id /T /F 2>&1 | Out-String
        $process.WaitForExit(30000) | Out-Null
        throw "base VHD SHA-256 worker exceeded its deadline"
    }
    if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $ResultPath -PathType Leaf)) {
        throw "base VHD SHA-256 worker failed"
    }
    Normalize-ReadyCloneSha256 ([IO.File]::ReadAllText($ResultPath).Trim()) "observed base VHD SHA-256"
}

function Assert-ReadyCloneOfflineIntegrationServices {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object[]]$Services,
        [Parameter(Mandatory = $true)][string]$ExpectedVMId,
        [Parameter(Mandatory = $true)][string[]]$ExpectedComponentIds
    )

    $vmId = ([guid]$ExpectedVMId).ToString("D").ToUpperInvariant()
    $expected = @($ExpectedComponentIds | ForEach-Object {
            ([guid]$_).ToString("D").ToUpperInvariant()
        })
    if ($expected.Count -eq 0 -or @($expected | Select-Object -Unique).Count -ne $expected.Count -or
        $Services.Count -ne $expected.Count) {
        throw "ready clone integration-service cardinality is incomplete or ambiguous"
    }
    $observed = [Collections.Generic.List[string]]::new()
    foreach ($service in $Services) {
        foreach ($propertyName in @("Id", "Enabled", "PrimaryStatusDescription")) {
            if ($null -eq $service.PSObject.Properties[$propertyName]) {
                throw "ready clone integration-service record is incomplete"
            }
        }
        $identity = [regex]::Match([string]$service.Id,
            '(?i)^Microsoft:(?<vm>[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\\(?<component>[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$')
        if (-not $identity.Success) {
            throw "ready clone integration-service ID is malformed"
        }
        $observedVmId = ([guid]$identity.Groups["vm"].Value).ToString("D").ToUpperInvariant()
        $componentId = ([guid]$identity.Groups["component"].Value).ToString("D").ToUpperInvariant()
        if ($observedVmId -cne $vmId -or $expected -cnotcontains $componentId -or
            $observed -ccontains $componentId -or ($service.Enabled -isnot [bool]) -or
            ([bool]$service.Enabled -ne $true) -or
            -not [string]::IsNullOrEmpty([string]$service.PrimaryStatusDescription)) {
            throw "ready clone offline integration-service state is foreign, disabled, or contact-bearing"
        }
        $observed.Add($componentId)
        [pscustomobject]@{
            vm_id = $observedVmId
            component_id = $componentId
            enabled = [bool]$service.Enabled
            contact_status = [string]$service.PrimaryStatusDescription
        }
    }
    if (@($expected | Where-Object { $observed -cnotcontains $_ }).Count -ne 0) {
        throw "ready clone integration-service component set is incomplete"
    }
}

if (-not $ApproveCreate) { throw "ready clone creation requires explicit approval" }
if ($VMName -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$') {
    throw "ready clone VM name is invalid"
}
$expectedHash = Normalize-ReadyCloneSha256 $ExpectedBaseVhdSha256 "expected base VHD SHA-256"
$baseRootPath = (Resolve-Path -LiteralPath $BaseRoot -ErrorAction Stop).Path.TrimEnd('\')
$artifactRootPath = (Resolve-Path -LiteralPath $ArtifactRoot -ErrorAction Stop).Path.TrimEnd('\')
if (([IO.File]::GetAttributes($baseRootPath) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "ready clone base root is reparse-backed"
}
$manifestPath = Join-Path $baseRootPath "base-manifest.json"
$baseVhdPath = Join-Path $baseRootPath "base.vhdx"
foreach ($path in @($manifestPath, $baseVhdPath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
        ([IO.File]::GetAttributes($path) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "ready clone base input is missing or reparse-backed"
    }
}
if (([IO.File]::GetAttributes($baseVhdPath) -band [IO.FileAttributes]::ReadOnly) -eq 0) {
    throw "ready clone base VHD is not read-only"
}
$manifest = Get-Content -LiteralPath $manifestPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
if ([int]$manifest.schema -ne 1 -or
    [string]$manifest.kind -cne "ramshared-isolated-win11-lab-base" -or
    [string]$manifest.base_usage -cne "isolated_lab_only" -or
    ([bool]$manifest.base_files_read_only -ne $true) -or
    [string]$manifest.network_policy -cne "SealedOffline" -or
    [string]$manifest.switch_name -cne $ExpectedSwitchName -or
    [int]$manifest.checkpoints -ne 0 -or
    ([bool]$manifest.tpm_enabled -ne $true) -or
    [int]$manifest.processor_count -lt 1 -or
    [int64]$manifest.memory_minimum_bytes -lt 1 -or
    [int64]$manifest.memory_minimum_bytes -gt [int64]$manifest.memory_startup_bytes -or
    [int64]$manifest.memory_startup_bytes -gt [int64]$manifest.memory_maximum_bytes -or
    [int64]$manifest.copied_vhd_bytes -ne (Get-Item -LiteralPath $baseVhdPath -Force).Length -or
    (Normalize-ReadyCloneSha256 ([string]$manifest.copied_vhd_sha256) "manifest base VHD SHA-256") -cne $expectedHash) {
    throw "ready clone base manifest is incomplete or mismatched"
}
$switches = @(Get-VMSwitch -ErrorAction Stop | Where-Object { [string]$_.Name -ceq $ExpectedSwitchName })
if ($switches.Count -ne 1 -or [string]$switches[0].SwitchType -cne "Private") {
    throw "ready clone sealed switch is absent, ambiguous, or not Private"
}
$existingVms = @(Get-VM -ErrorAction Stop | Where-Object { [string]$_.Name -ceq $VMName })
if ($existingVms.Count -ne 0) { throw "ready clone VM name already exists" }
$vmRootFull = [IO.Path]::GetFullPath($VMRoot).TrimEnd('\')
if (Test-Path -LiteralPath $vmRootFull) { throw "ready clone VM root already exists" }
$vmRootParent = Split-Path -Parent $vmRootFull
if (-not (Test-Path -LiteralPath $vmRootParent -PathType Container) -or
    ([IO.File]::GetAttributes($vmRootParent) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "ready clone VM root parent is absent or reparse-backed"
}

$cloneId = [guid]::NewGuid().ToString("D")
$artifactDirectory = Join-Path $artifactRootPath ("ready-clone-" + $cloneId)
New-Item -ItemType Directory -Path $artifactDirectory -ErrorAction Stop | Out-Null
$hashResultPath = Join-Path $artifactDirectory "base-vhd-sha256.txt"
$baseHashBefore = Get-ReadyCloneBoundedSha256 -Path $baseVhdPath -ResultPath $hashResultPath `
    -TimeoutSeconds $HashTimeoutSeconds
if ($baseHashBefore -cne $expectedHash) { throw "ready clone base VHD hash mismatched before creation" }

$vmCreated = $false
$rootCreated = $false
try {
    New-Item -ItemType Directory -Path $vmRootFull -ErrorAction Stop | Out-Null
    $rootCreated = $true
    $vhdDirectory = Join-Path $vmRootFull "Virtual Hard Disks"
    New-Item -ItemType Directory -Path $vhdDirectory -ErrorAction Stop | Out-Null
    $cloneVhdPath = Join-Path $vhdDirectory ($VMName + ".vhdx")
    New-VHD -Path $cloneVhdPath -Differencing -ParentPath $baseVhdPath -ErrorAction Stop | Out-Null
    $vm = New-VM -Name $VMName -Generation 2 -MemoryStartupBytes ([int64]$manifest.memory_startup_bytes) `
        -VHDPath $cloneVhdPath -Path $vmRootFull -SwitchName $ExpectedSwitchName -ErrorAction Stop
    $vmCreated = $true
    Set-VMMemory -VM $vm -DynamicMemoryEnabled ([bool]$manifest.dynamic_memory_enabled) `
        -MinimumBytes ([int64]$manifest.memory_minimum_bytes) `
        -StartupBytes ([int64]$manifest.memory_startup_bytes) `
        -MaximumBytes ([int64]$manifest.memory_maximum_bytes) -ErrorAction Stop
    Set-VMProcessor -VM $vm -Count ([int]$manifest.processor_count) -ErrorAction Stop
    Set-VM -VM $vm -CheckpointType Disabled -AutomaticStartAction Nothing `
        -AutomaticStopAction ShutDown -ErrorAction Stop
    $secureBootState = if ([bool]$manifest.secure_boot_enabled) { "On" } else { "Off" }
    Set-VMFirmware -VM $vm -EnableSecureBoot $secureBootState -ErrorAction Stop
    Set-VMKeyProtector -VM $vm -NewLocalKeyProtector -ErrorAction Stop
    Enable-VMTPM -VM $vm -ErrorAction Stop
    $expectedIntegrationComponentIds = @(
        "6C09BB55-D683-4DA0-8931-C9BF705F6480",
        "84EAAE65-2F2E-45F5-9BB5-0E857DC8EB47",
        "2A34B1C2-FD73-4043-8A5B-DD2159BC743F",
        "9F8233AC-BE49-4C79-8EE3-E7E1985B2077",
        "2497F4DE-E9FA-4204-80E4-4B75C46419C0",
        "5CED1297-4598-4915-A5FC-AD21BB4D02A4"
    )
    $integrationServices = @(Get-VMIntegrationService -VM $vm -ErrorAction Stop)
    $observedIntegrationIds = [Collections.Generic.List[string]]::new()
    foreach ($integrationService in $integrationServices) {
        $suffix = [regex]::Match([string]$integrationService.Id,
            '(?i)(?<component>[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$')
        if (-not $suffix.Success) { throw "ready clone integration service ID is malformed" }
        $componentId = ([guid]$suffix.Groups["component"].Value).ToString("D").ToUpperInvariant()
        if ($expectedIntegrationComponentIds -cnotcontains $componentId -or
            $observedIntegrationIds -ccontains $componentId) {
            throw "ready clone integration service ID is foreign or ambiguous"
        }
        $observedIntegrationIds.Add($componentId)
        Enable-VMIntegrationService -VMIntegrationService $integrationService -Confirm:$false -ErrorAction Stop
    }
    if ($observedIntegrationIds.Count -ne $expectedIntegrationComponentIds.Count) {
        throw "ready clone integration service set is incomplete"
    }

    $observedVm = Get-VM -Id $vm.Id -ErrorAction Stop
    $observedDrive = @(Get-VMHardDiskDrive -VM $observedVm -ErrorAction Stop)
    $observedAdapter = @(Get-VMNetworkAdapter -VM $observedVm -ErrorAction Stop)
    $observedVhd = Get-VHD -Path $cloneVhdPath -ErrorAction Stop
    $observedFirmware = Get-VMFirmware -VM $observedVm -ErrorAction Stop
    $observedSecurity = Get-VMSecurity -VM $observedVm -ErrorAction Stop
    $observedIntegrationServices = @(Get-VMIntegrationService -VM $observedVm -ErrorAction Stop)
    $checkpointCount = @(Get-VMSnapshot -VM $observedVm -ErrorAction Stop).Count
    $readback = [pscustomobject]@{
        schema = [int]1
        vm_name = [string]$observedVm.Name
        vm_id = ([guid]$observedVm.Id).ToString("D")
        vm_state = [string]$observedVm.State
        generation = [int]$observedVm.Generation
        drive_count = [int]$observedDrive.Count
        drive_path = if ($observedDrive.Count -eq 1) { [string]$observedDrive[0].Path } else { "" }
        adapter_count = [int]$observedAdapter.Count
        switch_name = if ($observedAdapter.Count -eq 1) { [string]$observedAdapter[0].SwitchName } else { "" }
        vhd_type = [string]$observedVhd.VhdType
        parent_path = [string]$observedVhd.ParentPath
        checkpoint_count = [int]$checkpointCount
        tpm_enabled = [bool]$observedSecurity.TpmEnabled
        secure_boot = [string]$observedFirmware.SecureBoot
        integration_services = @($observedIntegrationServices | ForEach-Object {
                [pscustomobject]@{
                    id = [string]$_.Id
                    enabled = [bool]$_.Enabled
                    enabled_type = if ($null -eq $_.Enabled) { "null" } else { $_.Enabled.GetType().FullName }
                    contact_status = [string]$_.PrimaryStatusDescription
                }
            })
    }
    $readback | ConvertTo-Json -Depth 8 |
        Set-Content -LiteralPath (Join-Path $artifactDirectory "readback.json") -Encoding UTF8
    $observedIntegrationEvidence = @(Assert-ReadyCloneOfflineIntegrationServices `
            -Services $observedIntegrationServices `
            -ExpectedVMId (([guid]$observedVm.Id).ToString("D")) `
            -ExpectedComponentIds $expectedIntegrationComponentIds)
    if ([string]$observedVm.State -cne "Off" -or [int]$observedVm.Generation -ne 2 -or
        ([guid]$observedVm.Id).ToString("D") -ieq ([guid]$manifest.source_vm_id).ToString("D") -or
        $observedDrive.Count -ne 1 -or [string]$observedDrive[0].Path -cne $cloneVhdPath -or
        $observedAdapter.Count -ne 1 -or [string]$observedAdapter[0].SwitchName -cne $ExpectedSwitchName -or
        [string]$observedVhd.VhdType -cne "Differencing" -or
        -not ([string]$observedVhd.ParentPath).Equals($baseVhdPath, [StringComparison]::OrdinalIgnoreCase) -or
        [int]$checkpointCount -ne 0 -or ([bool]$observedSecurity.TpmEnabled -ne $true) -or
        $observedIntegrationEvidence.Count -ne 6 -or
        [string]$observedFirmware.SecureBoot -cne $secureBootState) {
        throw "ready clone readback is incomplete or mismatched"
    }
    $baseHashAfter = Get-ReadyCloneBoundedSha256 -Path $baseVhdPath `
        -ResultPath (Join-Path $artifactDirectory "base-vhd-sha256-after.txt") `
        -TimeoutSeconds $HashTimeoutSeconds
    if ($baseHashAfter -cne $baseHashBefore) { throw "ready clone base VHD bytes changed" }
    $receipt = [pscustomobject]@{
        schema = [int]1
        status = "PASS"
        clone_id = $cloneId
        vm_name = $VMName
        vm_id = ([guid]$observedVm.Id).ToString("D")
        vm_state = [string]$observedVm.State
        switch_name = [string]$observedAdapter[0].SwitchName
        vhd_path = [string]$observedDrive[0].Path
        vhd_type = [string]$observedVhd.VhdType
        parent_path = [string]$observedVhd.ParentPath
        base_vhd_sha256 = $baseHashAfter
        processor_count = [int]$observedVm.ProcessorCount
        dynamic_memory_enabled = [bool]$observedVm.DynamicMemoryEnabled
        memory_startup_bytes = [int64]$observedVm.MemoryStartup
        secure_boot = [string]$observedFirmware.SecureBoot
        tpm_enabled = [bool]$observedSecurity.TpmEnabled
        integration_service_count = [int]$observedIntegrationServices.Count
        integration_services = $observedIntegrationEvidence
        checkpoint_count = [int]$checkpointCount
        artifact_directory = $artifactDirectory
    }
    $receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $artifactDirectory "summary.json") -Encoding UTF8
    Write-Output "STATUS=PASS"
    Write-Output ("CLONE_VM_ID={0}" -f $receipt.vm_id)
    Write-Output ("CLONE_ARTIFACT={0}" -f $artifactDirectory)
}
catch {
    if ($vmCreated) {
        $createdVm = @(Get-VM -ErrorAction Stop | Where-Object {
                ([guid]$_.Id).ToString("D") -ceq ([guid]$vm.Id).ToString("D") -and
                [string]$_.Name -ceq $VMName -and [string]$_.State -ceq "Off"
            })
        if ($createdVm.Count -eq 1) { Remove-VM -VM $createdVm[0] -Force -ErrorAction Stop }
    }
    if ($rootCreated -and (Test-Path -LiteralPath $vmRootFull -PathType Container)) {
        Remove-Item -LiteralPath $vmRootFull -Recurse -Force -ErrorAction Stop
    }
    throw
}
