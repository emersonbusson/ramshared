#Requires -Version 5.1
# Exact, artifact-bound recovery for a failed disposable-VM verifier run.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourcePath,
    [Parameter(Mandatory = $true)][string]$HelperPath,
    [Parameter(Mandatory = $true)][string]$VMName,
    [Parameter(Mandatory = $true)][string]$ExpectedVMId,
    [Parameter(Mandatory = $true)][string]$User,
    [Parameter(Mandatory = $true)][string]$PasswordFile,
    [Parameter(Mandatory = $true)][string]$FailedRunArtifactDirectory,
    [string]$SealedPlanArtifactDirectory = "",
    [string]$PartialActionArtifactDirectory = "",
    [Parameter(Mandatory = $true)][string]$ArtifactRoot,
    [Parameter(Mandatory = $true)][string]$ExpectedDriverSha256,
    [Parameter(Mandatory = $true)][string]$ExpectedInfSha256,
    [Parameter(Mandatory = $true)][string]$ExpectedCatalogSha256,
    [Parameter(Mandatory = $true)][string]$ExpectedIoctlSha256,
    [Parameter(Mandatory = $true)][string]$ExpectedSignerCertSha256,
    [string]$ExpectedPartialPublishedInf = "",
    [ValidateRange(15, 30)][int]$GuestRestartDelaySeconds = 15,
    [switch]$PlanOnly,
    [switch]$ApproveExactRecovery
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $ApproveExactRecovery) {
    throw "exact recovery requires explicit approval"
}
foreach ($path in @($SourcePath, $HelperPath, $PasswordFile)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "required recovery input file is missing"
    }
}
. $HelperPath

$source = Get-Content -LiteralPath $SourcePath -Raw -ErrorAction Stop
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput(
    $source, [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0) {
    throw "production verifier harness does not parse"
}
function Import-ExactFunction([string]$Name) {
    $definition = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $Name
        }, $true)
    if ($null -eq $definition) {
        throw "required production function is missing: $Name"
    }
    $body = $definition.Body.Extent.Text
    Set-Item -Path ("Function:\script:{0}" -f $Name) -Value (
        [scriptblock]::Create($body.Substring(1, $body.Length - 2)))
}
foreach ($name in @(
        "Normalize-GuestVerifierSha256",
        "Normalize-GuestVerifierPublishedInf",
        "Get-GuestVerifierConnectTimeoutSeconds",
        "Invoke-GuestVerifierRemote",
        "ConvertFrom-GuestVerifierRows",
        "Assert-GuestVerifierRestartReceipt",
        "Assert-GuestVerifierBootChangeReceipt",
        "Get-GuestVerifierBootTime",
        "Request-GuestVerifierRestart",
        "Wait-GuestVerifierBootTimeChange",
        "Assert-GuestVerifierCurrentRunTeardownBinding",
        "Assert-GuestVerifierCurrentRunTeardownEvidence",
        "Get-GuestVerifierCurrentRunTeardownBinding",
        "Get-GuestVerifierPostPublishCleanupMode",
        "Get-GuestVerifierPostPublishCleanupState",
        "Assert-GuestVerifierRootRemovedState",
        "Remove-GuestVerifierCurrentRunRoot",
        "Remove-GuestVerifierRootRemovedArtifacts")) {
    Import-ExactFunction $name
}

$resolvedRoot = (Resolve-Path -LiteralPath $ArtifactRoot -ErrorAction Stop).Path.TrimEnd('\')
$resolvedFailed = (Resolve-Path -LiteralPath $FailedRunArtifactDirectory -ErrorAction Stop).Path
if ([string](Split-Path -Parent $resolvedFailed).TrimEnd('\') -cne $resolvedRoot -or
    ([IO.File]::GetAttributes($resolvedFailed) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "failed-run artifact directory is not one direct non-reparse child of ArtifactRoot"
}
$leaf = Split-Path -Leaf $resolvedFailed
if ($leaf -notmatch '^guest-verifier-(?<run>[0-9a-fA-F-]{36})$') {
    throw "failed-run artifact directory has no canonical run identity"
}
$runId = ([guid]$Matches["run"]).ToString("D")

function Read-ExactJson([string]$Name) {
    $path = Join-Path $resolvedFailed $Name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "required failed-run artifact is missing: $Name"
    }
    Get-Content -LiteralPath $path -Raw -ErrorAction Stop |
        ConvertFrom-Json -ErrorAction Stop
}

function Assert-GuestVerifierPartialInstallArtifacts {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$Summary,
        [Parameter(Mandatory = $true)][object]$InputBinding,
        [Parameter(Mandatory = $true)][object]$InitialPreflight,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$ExpectedPublishedInf,
        [Parameter(Mandatory = $true)][bool]$HasInstallReceipt,
        [Parameter(Mandatory = $true)][bool]$HasNormalIdentityReceipt,
        [Parameter(Mandatory = $true)][bool]$HasNormalPassReceipt,
        [Parameter(Mandatory = $true)][bool]$HasVerifierPassReceipt
    )

    $publishedInf = $ExpectedPublishedInf.Trim().ToLowerInvariant()
    if ($ExpectedPublishedInf -cne $ExpectedPublishedInf.Trim() -or
        $publishedInf -notmatch '^oem[0-9]+\.inf$' -or
        [int]$Summary.schema -ne 1 -or [string]$Summary.run_id -cne $RunId -or
        [string]$Summary.status -cne "FAIL" -or
        ($Summary.current_run_package_may_be_present -isnot [bool]) -or
        ([bool]$Summary.current_run_package_may_be_present -ne $true) -or
        [string]$Summary.error_code -cne "psdirect_outer_deadline" -or
        [string]$InputBinding.run_id -cne $RunId -or
        $HasInstallReceipt -or $HasNormalIdentityReceipt -or
        $HasNormalPassReceipt -or $HasVerifierPassReceipt -or
        [int]$InitialPreflight.package_count -ne 0 -or
        [int]$InitialPreflight.service_count -ne 0 -or
        [int]$InitialPreflight.root_count -ne 0 -or
        [int]$InitialPreflight.ramshared_disk_count -ne 0 -or
        [int]$InitialPreflight.ramshared_pnp_disk_count -ne 0) {
        throw "failed-run artifacts do not prove one exact partial install"
    }
    $true
}

function Assert-GuestVerifierRecoveryRootRemovedState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$State,
        [Parameter(Mandatory = $true)][object]$Binding
    )

    foreach ($propertyName in @(
            "schema", "package_count", "published_inf_count", "package_original_inf",
            "root_count", "service_count", "service_name", "service_state",
            "loaded_driver_sha256", "driver_store_inf_sha256", "driver_store_catalog_sha256",
            "ramshared_disk_count", "ramshared_pnp_disk_count",
            "ramshared_present_pnp_disk_count", "ramshared_retired_pnp_disk_count",
            "retired_pnp_instance_id")) {
        if ($null -eq $State.PSObject.Properties[$propertyName]) {
            throw "root-removed recovery state is missing $propertyName"
        }
    }
    $totalPnp = [int]$State.ramshared_pnp_disk_count
    $retiredPnp = [int]$State.ramshared_retired_pnp_disk_count
    $retiredInstance = [string]$State.retired_pnp_instance_id
    $retiredShapeIsExact = if ($totalPnp -eq 0) {
        [string]::IsNullOrEmpty($retiredInstance)
    }
    else {
        $retiredInstance -cmatch '^SCSI\\DISK&VEN_RAMSHARE&PROD_VRAMDISK\\[^\\]+$'
    }
    if ([int]$State.schema -ne 1 -or
        [int]$State.package_count -ne 1 -or [int]$State.published_inf_count -ne 1 -or
        [string]$State.package_original_inf -cne "ramshared.inf" -or
        [int]$State.root_count -ne 0 -or [int]$State.service_count -ne 1 -or
        [string]$State.service_name -cne "ramshared" -or
        [string]$State.service_state -cne "Stopped" -or
        [string]$State.loaded_driver_sha256 -cne [string]$Binding.loaded_driver_sha256 -or
        [string]$State.driver_store_inf_sha256 -cne [string]$Binding.driver_store_inf_sha256 -or
        [string]$State.driver_store_catalog_sha256 -cne [string]$Binding.driver_store_catalog_sha256 -or
        [int]$State.ramshared_disk_count -ne 0 -or
        [int]$State.ramshared_present_pnp_disk_count -ne 0 -or
        $totalPnp -lt 0 -or $totalPnp -gt 1 -or $retiredPnp -ne $totalPnp -or
        -not $retiredShapeIsExact) {
        throw "root-removed recovery state is zero, multiple, foreign, active, or hash-mismatched"
    }
    $State
}

function Assert-GuestVerifierRecoveryFinalState {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][object]$State)

    foreach ($propertyName in @(
            "schema", "package_count", "published_inf_count", "root_count", "service_count",
            "ramshared_disk_count", "ramshared_pnp_disk_count",
            "ramshared_present_pnp_disk_count", "ramshared_retired_pnp_disk_count")) {
        if ($null -eq $State.PSObject.Properties[$propertyName]) {
            throw "final recovery state is missing $propertyName"
        }
    }
    if ([int]$State.schema -ne 1 -or [int]$State.package_count -ne 0 -or
        [int]$State.published_inf_count -ne 0 -or [int]$State.root_count -ne 0 -or
        [int]$State.service_count -ne 0 -or [int]$State.ramshared_disk_count -ne 0 -or
        [int]$State.ramshared_pnp_disk_count -ne 0 -or
        [int]$State.ramshared_present_pnp_disk_count -ne 0 -or
        [int]$State.ramshared_retired_pnp_disk_count -ne 0) {
        throw "final recovery state did not reach exact zero residue"
    }
    $State
}

function Assert-GuestVerifierRecoveryRetiredOnlyState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$State,
        [Parameter(Mandatory = $true)][string]$ExpectedRetiredInstanceId
    )

    $expected = $ExpectedRetiredInstanceId.Trim().ToUpperInvariant()
    if ($expected -cnotmatch '^SCSI\\DISK&VEN_RAMSHARE&PROD_VRAMDISK\\[^\\]+$' -or
        [int]$State.schema -ne 1 -or [int]$State.package_count -ne 0 -or
        [int]$State.published_inf_count -ne 0 -or [int]$State.root_count -ne 0 -or
        [int]$State.service_count -ne 0 -or [int]$State.ramshared_disk_count -ne 0 -or
        [int]$State.ramshared_pnp_disk_count -ne 1 -or
        [int]$State.ramshared_present_pnp_disk_count -ne 0 -or
        [int]$State.ramshared_retired_pnp_disk_count -ne 1 -or
        [string]$State.retired_pnp_instance_id -cne $expected) {
        throw "retired-only recovery state is absent, active, ambiguous, foreign, or not package-free"
    }
    $State
}

function Get-GuestVerifierRecoveryState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$Binding,
        [Parameter(Mandatory = $true)][string]$ExpectedPublishedInf
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        param($PublishedInf)
        $ErrorActionPreference = "Stop"

        function Resolve-DriverStoreImage([string]$RawPath) {
            if ([string]::IsNullOrWhiteSpace($RawPath)) {
                throw "system-driver path is empty"
            }
            $candidate = $RawPath.Trim()
            if ($candidate.StartsWith('"')) {
                $closing = $candidate.IndexOf('"', 1)
                if ($closing -lt 1) {
                    throw "system-driver path quote is malformed"
                }
                $candidate = $candidate.Substring(1, $closing - 1)
            }
            else {
                $candidate = ($candidate -split '\s+')[0]
            }
            if ($candidate -match '(?i)^\\SystemRoot\\') {
                $candidate = Join-Path $env:SystemRoot $candidate.Substring(12)
            }
            if ($candidate -match '(?i)^\\\?\?\\') {
                $candidate = $candidate.Substring(4)
            }
            $resolved = (Resolve-Path -LiteralPath $candidate -ErrorAction Stop).Path
            if ($resolved.IndexOf("\DriverStore\FileRepository\", [StringComparison]::OrdinalIgnoreCase) -lt 0 -or
                -not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
                throw "system-driver path is not an existing DriverStore image"
            }
            $resolved
        }

        $packages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
            })
        $publishedPackages = @($packages | Where-Object {
                ([IO.Path]::GetFileName([string]$_.Driver)).ToLowerInvariant() -ceq $PublishedInf
            })
        $roots = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
            })
        $services = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
        $disks = @(Get-Disk -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.SerialNumber -match '(?i)ramshare|ramshared'
            })
        $pnpDisks = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
            })
        $retired = @($pnpDisks | Where-Object {
                [string]$_.InstanceId -cmatch '^SCSI\\DISK&VEN_RAMSHARE&PROD_VRAMDISK\\[^\\]+$' -and
                ([bool]$_.Present -eq $false) -and [string]$_.Status -ceq "Unknown" -and
                [int]$_.Problem -eq 45
            })

        $servicePath = ""
        $sysHash = ""
        $infHash = ""
        $catHash = ""
        if ($services.Count -eq 1) {
            $servicePath = Resolve-DriverStoreImage ([string]$services[0].PathName)
            $serviceDirectory = Split-Path -Parent $servicePath
            $infPath = Join-Path $serviceDirectory "ramshared.inf"
            $catPath = Join-Path $serviceDirectory "ramshared.cat"
            foreach ($path in @($infPath, $catPath)) {
                if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                    throw "bound DriverStore package file is missing"
                }
            }
            $sysHash = (Get-FileHash -LiteralPath $servicePath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
            $infHash = (Get-FileHash -LiteralPath $infPath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
            $catHash = (Get-FileHash -LiteralPath $catPath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        }

        [pscustomobject]@{
            schema = [int]1
            package_count = [int]$packages.Count
            published_inf_count = [int]$publishedPackages.Count
            package_original_inf = if ($publishedPackages.Count -eq 1) {
                [IO.Path]::GetFileName([string]$publishedPackages[0].OriginalFileName).ToLowerInvariant()
            } else { "" }
            root_count = [int]$roots.Count
            service_count = [int]$services.Count
            service_name = if ($services.Count -eq 1) { [string]$services[0].Name } else { "" }
            service_state = if ($services.Count -eq 1) { [string]$services[0].State } else { "" }
            service_path = $servicePath
            loaded_driver_sha256 = $sysHash
            driver_store_inf_sha256 = $infHash
            driver_store_catalog_sha256 = $catHash
            ramshared_disk_count = [int]$disks.Count
            ramshared_pnp_disk_count = [int]$pnpDisks.Count
            ramshared_present_pnp_disk_count = [int]($pnpDisks.Count - $retired.Count)
            ramshared_retired_pnp_disk_count = [int]$retired.Count
            retired_pnp_instance_id = if ($retired.Count -eq 1) { [string]$retired[0].InstanceId } else { "" }
        } | ConvertTo-Json -Compress
    } -ArgumentList @($ExpectedPublishedInf)
    ConvertFrom-GuestVerifierRows $rows "root-removed recovery state"
}

function Invoke-GuestVerifierRecoveryDeleteService {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$Binding,
        [Parameter(Mandatory = $true)][string]$ExpectedServiceName
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 120 -ScriptBlock {
        param($ExpectedServiceName, $ExpectedPath, $ExpectedSysHash, $ExpectedInfHash, $ExpectedCatHash)
        $ErrorActionPreference = "Stop"
        $services = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
        if ($services.Count -ne 1 -or [string]$services[0].Name -cne $ExpectedServiceName -or
            [string]$services[0].State -cne "Stopped") {
            throw "exact stopped service is missing, foreign, or ambiguous"
        }
        $rawPath = [string]$services[0].PathName
        $candidate = $rawPath.Trim()
        if ($candidate.StartsWith('"')) {
            $closing = $candidate.IndexOf('"', 1)
            if ($closing -lt 1) { throw "system-driver path quote is malformed" }
            $candidate = $candidate.Substring(1, $closing - 1)
        }
        else { $candidate = ($candidate -split '\s+')[0] }
        if ($candidate -match '(?i)^\\SystemRoot\\') {
            $candidate = Join-Path $env:SystemRoot $candidate.Substring(12)
        }
        if ($candidate -match '(?i)^\\\?\?\\') { $candidate = $candidate.Substring(4) }
        $path = (Resolve-Path -LiteralPath $candidate -ErrorAction Stop).Path
        if ($path -ine $ExpectedPath) { throw "service path drifted from sealed binding" }
        $directory = Split-Path -Parent $path
        $infPath = Join-Path $directory "ramshared.inf"
        $catPath = Join-Path $directory "ramshared.cat"
        $sysHash = (Get-FileHash -LiteralPath $path -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        $infHash = (Get-FileHash -LiteralPath $infPath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        $catHash = (Get-FileHash -LiteralPath $catPath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        if ($sysHash -cne $ExpectedSysHash -or $infHash -cne $ExpectedInfHash -or
            $catHash -cne $ExpectedCatHash) {
            throw "service package hashes drifted from sealed binding"
        }
        $null = & sc.exe delete $ExpectedServiceName 2>&1 | Out-String
        $exitCode = [int]$LASTEXITCODE
        if ($exitCode -ne 0) { throw "exact service deletion failed exit=$exitCode" }
        $deadline = (Get-Date).ToUniversalTime().AddSeconds(60)
        do {
            $remaining = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
            if ($remaining.Count -eq 0) { break }
            if ($remaining.Count -ne 1 -or [string]$remaining[0].Name -cne $ExpectedServiceName) {
                throw "service deletion became ambiguous"
            }
            Start-Sleep -Seconds 2
        } while ((Get-Date).ToUniversalTime() -lt $deadline)
        if ($remaining.Count -ne 0) { throw "service deletion did not reach zero" }
        [pscustomobject]@{
            schema = [int]1
            action = "delete_exact_service"
            service_name = $ExpectedServiceName
            exit_code = $exitCode
            terminal_service_count = [int]$remaining.Count
        } | ConvertTo-Json -Compress
    } -ArgumentList @(
        $ExpectedServiceName, [string]$Binding.service_path,
        [string]$Binding.loaded_driver_sha256, [string]$Binding.driver_store_inf_sha256,
        [string]$Binding.driver_store_catalog_sha256)
    ConvertFrom-GuestVerifierRows $rows "exact service deletion"
}

function Invoke-GuestVerifierRecoveryDeleteDriver {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$ExpectedPublishedInf)

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        param($ExpectedPublishedInf)
        $ErrorActionPreference = "Stop"
        $packages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
            })
        $published = @($packages | Where-Object {
                ([IO.Path]::GetFileName([string]$_.Driver)).ToLowerInvariant() -ceq $ExpectedPublishedInf
            })
        if ($packages.Count -ne 1 -or $published.Count -ne 1 -or
            [IO.Path]::GetFileName([string]$published[0].OriginalFileName).ToLowerInvariant() -cne "ramshared.inf") {
            throw "exact DriverStore package is missing, foreign, or ambiguous"
        }
        $null = & pnputil.exe /delete-driver $ExpectedPublishedInf /uninstall 2>&1 | Out-String
        $exitCode = [int]$LASTEXITCODE
        if ($exitCode -ne 0) { throw "exact DriverStore deletion failed exit=$exitCode" }
        [pscustomobject]@{
            schema = [int]1
            action = "delete_exact_published_inf"
            published_inf = $ExpectedPublishedInf
            uninstall = $true
            force = $false
            exit_code = $exitCode
        } | ConvertTo-Json -Compress
    } -ArgumentList @($ExpectedPublishedInf)
    ConvertFrom-GuestVerifierRows $rows "exact DriverStore deletion"
}

function Invoke-GuestVerifierRecoveryDeleteRetiredNode {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$ExpectedRetiredInstanceId)

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 120 -ScriptBlock {
        param($ExpectedRetiredInstanceId)
        $ErrorActionPreference = "Stop"
        $productNodes = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
            })
        if ($productNodes.Count -ne 1 -or
            [string]$productNodes[0].InstanceId -cne $ExpectedRetiredInstanceId -or
            [string]$productNodes[0].InstanceId -cnotmatch '^SCSI\\DISK&VEN_RAMSHARE&PROD_VRAMDISK\\[^\\]+$' -or
            ([bool]$productNodes[0].Present -ne $false) -or
            [string]$productNodes[0].Status -cne "Unknown" -or [int]$productNodes[0].Problem -ne 45) {
            throw "exact retired node is missing, active, foreign, or ambiguous"
        }
        $null = & pnputil.exe /remove-device $ExpectedRetiredInstanceId 2>&1 | Out-String
        $exitCode = [int]$LASTEXITCODE
        if ($exitCode -ne 0) { throw "exact retired-node deletion failed exit=$exitCode" }
        $deadline = (Get-Date).ToUniversalTime().AddSeconds(60)
        do {
            $remaining = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                    $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                    $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
                })
            if ($remaining.Count -eq 0) { break }
            if ($remaining.Count -ne 1 -or [string]$remaining[0].InstanceId -cne $ExpectedRetiredInstanceId) {
                throw "retired-node deletion became ambiguous"
            }
            Start-Sleep -Seconds 2
        } while ((Get-Date).ToUniversalTime() -lt $deadline)
        if ($remaining.Count -ne 0) { throw "retired-node deletion did not reach zero" }
        [pscustomobject]@{
            schema = [int]1
            action = "delete_exact_retired_node"
            instance_id = $ExpectedRetiredInstanceId
            exit_code = $exitCode
            terminal_product_pnp_count = [int]$remaining.Count
        } | ConvertTo-Json -Compress
    } -ArgumentList @($ExpectedRetiredInstanceId)
    ConvertFrom-GuestVerifierRows $rows "exact retired-node deletion"
}

$summary = Read-ExactJson "summary.json"
$inputBinding = Read-ExactJson "input-binding.json"
$initialPreflight = Read-ExactJson "guest-preflight.json"
$installPath = Join-Path $resolvedFailed "signed-package-install.json"
$normalIdentityPath = Join-Path $resolvedFailed "normal-current-identity.json"
$normalPassPath = Join-Path $resolvedFailed "normal-ioctl-evidence.json"
$verifierPassPath = Join-Path $resolvedFailed "verifier-ioctl-evidence.json"
$hasInstallReceipt = Test-Path -LiteralPath $installPath -PathType Leaf
$hasNormalIdentityReceipt = Test-Path -LiteralPath $normalIdentityPath -PathType Leaf
$hasNormalPassReceipt = Test-Path -LiteralPath $normalPassPath -PathType Leaf
$hasVerifierPassReceipt = Test-Path -LiteralPath $verifierPassPath -PathType Leaf
$partialInstall = -not $hasInstallReceipt -and -not $hasNormalIdentityReceipt -and
    -not $hasNormalPassReceipt -and -not $hasVerifierPassReceipt
$install = $null
$normalIdentity = $null
$normalPass = $null
$publishedInf = ""
$normalSerial = ""
$verifierSerial = ""
$ioPassStarted = $false

if ($partialInstall) {
    Assert-GuestVerifierPartialInstallArtifacts -Summary $summary -InputBinding $inputBinding `
        -InitialPreflight $initialPreflight -RunId $runId `
        -ExpectedPublishedInf $ExpectedPartialPublishedInf `
        -HasInstallReceipt $hasInstallReceipt `
        -HasNormalIdentityReceipt $hasNormalIdentityReceipt `
        -HasNormalPassReceipt $hasNormalPassReceipt `
        -HasVerifierPassReceipt $hasVerifierPassReceipt | Out-Null
    $publishedInf = Normalize-GuestVerifierPublishedInf $ExpectedPartialPublishedInf `
        "expected partial-install published INF"
}
else {
    if (-not [string]::IsNullOrWhiteSpace($ExpectedPartialPublishedInf) -or
        -not $hasInstallReceipt -or -not $hasNormalIdentityReceipt -or
        -not $hasNormalPassReceipt) {
        throw "failed-run artifacts mix complete and partial-install recovery identities"
    }
    $install = Read-ExactJson "signed-package-install.json"
    $normalIdentity = Read-ExactJson "normal-current-identity.json"
    $normalPass = Read-ExactJson "normal-ioctl-evidence.json"
    if ([string]$summary.status -cne "FAIL" -or [string]$summary.run_id -cne $runId -or
        ([bool]$summary.current_run_package_may_be_present -ne $true) -or
        [string]$inputBinding.run_id -cne $runId -or [string]$install.run_id -cne $runId -or
        [string]$normalIdentity.run_id -cne $runId -or [string]$normalPass.run_id -cne $runId -or
        [string]$normalPass.status -cne "PASS" -or [int]$normalPass.exit_code -ne 0 -or
        [string]$normalPass.vpd_serial -cne "ABCDEF0123456789" -or
        [string]$normalIdentity.loaded_sha256 -cne $ExpectedDriverSha256 -or
        ([bool]$normalIdentity.binary_match -ne $true)) {
        throw "failed-run artifacts do not form one exact recoverable current-run identity"
    }
    $publishedInf = Normalize-GuestVerifierPublishedInf ([string]$install.published_inf) `
        "failed-run published INF"
    if ([int]$install.install_exit_code -ne 0 -or
        [int]$install.published_package_count -ne 1 -or
        [string]$install.package_original_inf -ine "ramshared.inf") {
        throw "failed-run install receipt is not exact"
    }
    $normalSerial = [string]$normalPass.vpd_serial
    $ioPassStarted = $true
    if ($hasVerifierPassReceipt) {
        $verifierPass = Read-ExactJson "verifier-ioctl-evidence.json"
        if ([string]$verifierPass.run_id -cne $runId -or
            [string]$verifierPass.status -cne "PASS" -or
            [int]$verifierPass.exit_code -ne 0 -or
            [string]$verifierPass.vpd_serial -cne "ABCDEF0123456789") {
            throw "failed-run Verifier pass evidence is stale or mismatched"
        }
        $verifierSerial = [string]$verifierPass.vpd_serial
    }
}

$expectedHashes = @{
    driver_sys = Normalize-GuestVerifierSha256 $ExpectedDriverSha256 "expected driver hash"
    driver_inf = Normalize-GuestVerifierSha256 $ExpectedInfSha256 "expected INF hash"
    driver_cat = Normalize-GuestVerifierSha256 $ExpectedCatalogSha256 "expected catalog hash"
    ioctl_validation = Normalize-GuestVerifierSha256 $ExpectedIoctlSha256 "expected IOCTL hash"
    driver_signer_cert = Normalize-GuestVerifierSha256 $ExpectedSignerCertSha256 "expected signer certificate hash"
}
foreach ($role in @($expectedHashes.Keys)) {
    $rows = @($inputBinding.binding.artifacts | Where-Object { [string]$_.role -ceq $role })
    if ($rows.Count -ne 1 -or
        (Normalize-GuestVerifierSha256 ([string]$rows[0].sha256) "$role bound hash") -cne $expectedHashes[$role]) {
        throw "failed-run immutable input hash is missing or mismatched: $role"
    }
    $artifactPath = [string]$rows[0].path
    if (-not (Test-Path -LiteralPath $artifactPath -PathType Leaf)) {
        throw "failed-run immutable input is missing: $role"
    }
    $artifact = Get-Item -LiteralPath $artifactPath -Force -ErrorAction Stop
    $currentHash = (Get-FileHash -LiteralPath $artifact.FullName -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
    if ($currentHash -cne $expectedHashes[$role] -or [int64]$artifact.Length -ne [int64]$rows[0].byte_length) {
        throw "failed-run immutable input bytes drifted: $role"
    }
}
$vm = Get-VM -Name $VMName -ErrorAction Stop
if (([guid]$vm.Id).ToString("D") -ine ([guid]$ExpectedVMId).ToString("D") -or
    [int]$vm.Generation -ne 2 -or [string]$vm.State -cne "Running" -or
    [string](Get-VMFirmware -VM $vm -ErrorAction Stop).SecureBoot -cne "Off") {
    throw "exact recovery VM identity, generation, state, or firmware is wrong"
}

$resumeRootRemoved = -not [string]::IsNullOrWhiteSpace($SealedPlanArtifactDirectory)
$partialActionRequested = -not [string]::IsNullOrWhiteSpace($PartialActionArtifactDirectory)
$recoveryPhase = if ($resumeRootRemoved) {
    "root_removed"
}
elseif ($partialInstall) {
    "partial_install"
}
else {
    "pre_root"
}
$sealedPlanSummary = $null
$sealedBinding = $null
$resolvedPlan = ""
$resolvedPartialAction = ""
$expectedRetiredInstanceId = ""
if ($resumeRootRemoved) {
    $resolvedPlan = (Resolve-Path -LiteralPath $SealedPlanArtifactDirectory -ErrorAction Stop).Path
    if ([string](Split-Path -Parent $resolvedPlan).TrimEnd('\') -cne $resolvedRoot -or
        ([IO.File]::GetAttributes($resolvedPlan) -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
        (Split-Path -Leaf $resolvedPlan) -notmatch '^guest-verifier-recovery-(?<plan>[0-9a-fA-F-]{36})$') {
        throw "sealed recovery plan is not one canonical direct non-reparse child of ArtifactRoot"
    }
    $planSummaryPath = Join-Path $resolvedPlan "summary.json"
    $planBindingPath = Join-Path $resolvedPlan "exact-binding.json"
    foreach ($path in @($planSummaryPath, $planBindingPath)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            ([IO.File]::GetAttributes($path) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "sealed recovery plan file is missing or reparse-backed"
        }
    }
    $sealedPlanSummary = Get-Content -LiteralPath $planSummaryPath -Raw -ErrorAction Stop |
        ConvertFrom-Json -ErrorAction Stop
    $sealedBinding = Get-Content -LiteralPath $planBindingPath -Raw -ErrorAction Stop |
        ConvertFrom-Json -ErrorAction Stop
    if ([int]$sealedPlanSummary.schema -ne 1 -or [string]$sealedPlanSummary.status -cne "READY" -or
        [string]$sealedPlanSummary.source_run_id -cne $runId -or
        [string]$sealedPlanSummary.source_artifact_directory -ine $resolvedFailed -or
        [string]$sealedPlanSummary.artifact_directory -ine $resolvedPlan -or
        [string]$sealedPlanSummary.recovery_id -ine ([guid]$Matches["plan"]).ToString("D")) {
        throw "sealed recovery plan summary is stale, foreign, or non-READY"
    }
    Assert-GuestVerifierCurrentRunTeardownBinding -Binding $sealedBinding -RunId $runId `
        -ExpectedPublishedInf $publishedInf -ExpectedDriverHash $ExpectedDriverSha256 `
        -ExpectedInfHash $ExpectedInfSha256 -ExpectedCatalogHash $ExpectedCatalogSha256 `
        -ExpectedHardwareId "ROOT\RAMSHARED" -ExpectedVpdSerial "ABCDEF0123456789" `
        -ExpectedServiceName "ramshared" | Out-Null

    if (-not $PlanOnly -or $partialActionRequested) {
        if ($null -eq $sealedPlanSummary.PSObject.Properties["recovery_phase"] -or
            [string]$sealedPlanSummary.recovery_phase -cnotin @("partial_install", "pre_root", "root_removed", "package_removed_retired_only") -or
            $null -eq $sealedPlanSummary.PSObject.Properties["planned_at_utc"] -or
            $null -eq $sealedPlanSummary.PSObject.Properties["exact_binding_sha256"]) {
            throw "continuation requires a phase-bound READY plan"
        }
        $plannedAt = [datetimeoffset]::ParseExact(
            [string]$sealedPlanSummary.planned_at_utc, "o",
            [Globalization.CultureInfo]::InvariantCulture,
            [Globalization.DateTimeStyles]::RoundtripKind)
        $now = [datetimeoffset]::UtcNow
        if ($plannedAt -gt $now.AddMinutes(5) -or $plannedAt.AddHours(24) -lt $now) {
            throw "recovery plan is stale or future-dated"
        }
        $planBindingHash = (Get-FileHash -LiteralPath (Join-Path $resolvedPlan "exact-binding.json") -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        if ($planBindingHash -cne (Normalize-GuestVerifierSha256 ([string]$sealedPlanSummary.exact_binding_sha256) "sealed binding hash")) {
            throw "sealed recovery binding bytes drifted"
        }
        $recoveryPhase = [string]$sealedPlanSummary.recovery_phase
    }
}

function Get-ValidatedPartialAction([string]$Directory, [string]$ExpectedPlan,
    [object]$ExpectedBinding, [string]$ExpectedRetiredId) {
    $resolved = (Resolve-Path -LiteralPath $Directory -ErrorAction Stop).Path
    if ([string](Split-Path -Parent $resolved).TrimEnd('\') -cne $resolvedRoot -or
        ([IO.File]::GetAttributes($resolved) -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
        (Split-Path -Leaf $resolved) -notmatch '^guest-verifier-recovery-[0-9a-fA-F-]{36}$') {
        throw "partial-action artifact is not one canonical direct non-reparse child"
    }
    $documents = @{}
    foreach ($name in @("attempt.json", "exact-binding.json", "action-service-delete.json",
            "action-driver-delete.json", "after-zero-state.json")) {
        $path = Join-Path $resolved $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            ([IO.File]::GetAttributes($path) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "partial-action artifact is missing or reparse-backed: $name"
        }
        $documents[$name] = Get-Content -LiteralPath $path -Raw -ErrorAction Stop |
            ConvertFrom-Json -ErrorAction Stop
    }
    $attempt = $documents["attempt.json"]
    $service = $documents["action-service-delete.json"]
    $driver = $documents["action-driver-delete.json"]
    $after = $documents["after-zero-state.json"]
    $partialBindingPath = Join-Path $resolved "exact-binding.json"
    $partialBindingHash = (Get-FileHash -LiteralPath $partialBindingPath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
    $expectedBindingHash = (Get-FileHash -LiteralPath (Join-Path $ExpectedPlan "exact-binding.json") -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
    if ($partialBindingHash -cne $expectedBindingHash -or
        [string]$attempt.source_run_id -cne $runId -or [string]$attempt.mode -cne "action" -or
        [string]$attempt.recovery_phase -cne "root_removed" -or
        [string]$attempt.sealed_plan_artifact_directory -ine $ExpectedPlan -or
        [string]$service.action -cne "delete_exact_service" -or
        [string]$service.service_name -cne "ramshared" -or [int]$service.exit_code -ne 0 -or
        [int]$service.terminal_service_count -ne 0 -or
        [string]$driver.action -cne "delete_exact_published_inf" -or
        [string]$driver.published_inf -cne [string]$ExpectedBinding.published_inf -or
        ([bool]$driver.uninstall -ne $true) -or ([bool]$driver.force -ne $false) -or
        [int]$driver.exit_code -ne 0) {
        throw "partial-action receipts are stale, foreign, incomplete, or hash-mismatched"
    }
    $retiredId = [string]$after.retired_pnp_instance_id
    if (-not [string]::IsNullOrEmpty($ExpectedRetiredId) -and $retiredId -cne $ExpectedRetiredId) {
        throw "partial-action retired node drifted from the continuation plan"
    }
    Assert-GuestVerifierRecoveryRetiredOnlyState -State $after `
        -ExpectedRetiredInstanceId $retiredId | Out-Null
    [pscustomobject]@{
        directory = $resolved
        retired_instance_id = $retiredId
        exact_binding_sha256 = $partialBindingHash
    }
}

if ($partialActionRequested) {
    if (-not $PlanOnly -or $recoveryPhase -cne "root_removed") {
        throw "PartialActionArtifactDirectory is accepted only by a root_removed plan invocation"
    }
    $partial = Get-ValidatedPartialAction -Directory $PartialActionArtifactDirectory `
        -ExpectedPlan $resolvedPlan -ExpectedBinding $sealedBinding -ExpectedRetiredId ""
    $resolvedPartialAction = [string]$partial.directory
    $expectedRetiredInstanceId = [string]$partial.retired_instance_id
    $recoveryPhase = "package_removed_retired_only"
}
elseif ($recoveryPhase -ceq "package_removed_retired_only") {
    if ($PlanOnly -or $null -eq $sealedPlanSummary.PSObject.Properties["partial_action_artifact_directory"] -or
        $null -eq $sealedPlanSummary.PSObject.Properties["expected_retired_instance_id"]) {
        throw "retired-only action requires its sealed continuation plan"
    }
    $partial = Get-ValidatedPartialAction `
        -Directory ([string]$sealedPlanSummary.partial_action_artifact_directory) `
        -ExpectedPlan ([string]$sealedPlanSummary.sealed_plan_artifact_directory) `
        -ExpectedBinding $sealedBinding `
        -ExpectedRetiredId ([string]$sealedPlanSummary.expected_retired_instance_id)
    $resolvedPartialAction = [string]$partial.directory
    $expectedRetiredInstanceId = [string]$partial.retired_instance_id
}

$recoveryId = [guid]::NewGuid().ToString("D")
$recoveryDirectory = Join-Path $resolvedRoot ("guest-verifier-recovery-" + $recoveryId)
New-Item -ItemType Directory -Path $recoveryDirectory -ErrorAction Stop | Out-Null
function Write-RecoveryJson([string]$Name, [object]$Value) {
    $Value | ConvertTo-Json -Depth 16 |
        Set-Content -LiteralPath (Join-Path $recoveryDirectory $Name) -Encoding UTF8
}

Write-RecoveryJson "attempt.json" ([pscustomobject]@{
        schema = 1
        recovery_id = $recoveryId
        source_run_id = $runId
        recovery_phase = $recoveryPhase
        mode = if ($PlanOnly) { "plan" } else { "action" }
        started_at_utc = (Get-Date).ToUniversalTime().ToString("o")
        source_artifact_directory = $resolvedFailed
        sealed_plan_artifact_directory = $resolvedPlan
        artifact_directory = $recoveryDirectory
    })

$script:GuestVerifierVmName = $VMName
$script:GuestVerifierUser = $User
$script:GuestVerifierPassword = [IO.File]::ReadAllText($PasswordFile)
$script:GuestVerifierConnectTimeoutSeconds = 180
try {
    $preconditionRows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $verifierOutput = & verifier /query 2>&1 | Out-String
        $verifierExit = [int]$LASTEXITCODE
        $bcdOutput = & bcdedit.exe /enum "{current}" 2>&1 | Out-String
        $bcdExit = [int]$LASTEXITCODE
        $rootCerts = @(Get-ChildItem Cert:\LocalMachine\Root -ErrorAction Stop | Where-Object {
                $_.Thumbprint -eq "1D862ECA5F76B1D064B47222016FF20F92BF690F"
            })
        $publisherCerts = @(Get-ChildItem Cert:\LocalMachine\TrustedPublisher -ErrorAction Stop | Where-Object {
                $_.Thumbprint -eq "1D862ECA5F76B1D064B47222016FF20F92BF690F"
            })
        [pscustomobject]@{
            schema = 1
            verifier_exit_code = $verifierExit
            verifier_target_present = [bool]($verifierOutput -match '(?i)\bramshared\.sys\b')
            verifier_all_drivers = [bool]($verifierOutput -match '(?i)\ball drivers\b')
            testsigning_query_exit_code = $bcdExit
            testsigning_enabled = [bool]($bcdOutput -match '(?im)^\s*testsigning\s+(yes|on|true|1)\s*$')
            root_expected_thumbprint_count = [int]$rootCerts.Count
            trusted_publisher_expected_thumbprint_count = [int]$publisherCerts.Count
        } | ConvertTo-Json -Compress
    }
    $precondition = ConvertFrom-GuestVerifierRows $preconditionRows "exact recovery precondition"
    Write-RecoveryJson "before-safety-state.json" $precondition
    if ([int]$precondition.schema -ne 1 -or [int]$precondition.verifier_exit_code -ne 0 -or
        [bool]$precondition.verifier_target_present -or [bool]$precondition.verifier_all_drivers -or
        [int]$precondition.testsigning_query_exit_code -ne 0 -or [bool]$precondition.testsigning_enabled -or
        [int]$precondition.root_expected_thumbprint_count -ne 0 -or
        [int]$precondition.trusted_publisher_expected_thumbprint_count -ne 0) {
        throw "exact recovery precondition is not zero"
    }

    if ($recoveryPhase -ceq "package_removed_retired_only") {
        $retiredOnlyState = Get-GuestVerifierRecoveryState -Binding $sealedBinding `
            -ExpectedPublishedInf $publishedInf
        Write-RecoveryJson "before-retired-only-state.json" $retiredOnlyState
        Assert-GuestVerifierRecoveryRetiredOnlyState -State $retiredOnlyState `
            -ExpectedRetiredInstanceId $expectedRetiredInstanceId | Out-Null
        Write-RecoveryJson "exact-binding.json" $sealedBinding
        $recoveryBindingHash = (Get-FileHash -LiteralPath (Join-Path $recoveryDirectory "exact-binding.json") -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()

        if ($PlanOnly) {
            Write-RecoveryJson "summary.json" ([pscustomobject]@{
                    schema = 1
                    status = "READY"
                    recovery_phase = "package_removed_retired_only"
                    planned_at_utc = (Get-Date).ToUniversalTime().ToString("o")
                    exact_binding_sha256 = $recoveryBindingHash
                    expected_retired_instance_id = $expectedRetiredInstanceId
                    recovery_id = $recoveryId
                    source_run_id = $runId
                    source_artifact_directory = $resolvedFailed
                    sealed_plan_artifact_directory = $resolvedPlan
                    partial_action_artifact_directory = $resolvedPartialAction
                    artifact_directory = $recoveryDirectory
                })
            Write-Output "STATUS=READY"
            Write-Output "RECOVERY_ARTIFACT=$recoveryDirectory"
            return
        }

        $retiredDeletion = Invoke-GuestVerifierRecoveryDeleteRetiredNode `
            -ExpectedRetiredInstanceId $expectedRetiredInstanceId
        Write-RecoveryJson "action-retired-node-delete.json" $retiredDeletion
        if ([int]$retiredDeletion.schema -ne 1 -or
            [string]$retiredDeletion.action -cne "delete_exact_retired_node" -or
            [string]$retiredDeletion.instance_id -cne $expectedRetiredInstanceId -or
            [int]$retiredDeletion.exit_code -ne 0 -or
            [int]$retiredDeletion.terminal_product_pnp_count -ne 0) {
            throw "exact retired-node deletion receipt is invalid"
        }
        $finalState = Get-GuestVerifierRecoveryState -Binding $sealedBinding `
            -ExpectedPublishedInf $publishedInf
        Write-RecoveryJson "after-zero-state.json" $finalState
        Assert-GuestVerifierRecoveryFinalState -State $finalState | Out-Null
        Write-RecoveryJson "summary.json" ([pscustomobject]@{
                schema = 1
                status = "PASS"
                recovery_phase = "package_removed_retired_only"
                planned_at_utc = [string]$sealedPlanSummary.planned_at_utc
                exact_binding_sha256 = $recoveryBindingHash
                expected_retired_instance_id = $expectedRetiredInstanceId
                recovery_id = $recoveryId
                source_run_id = $runId
                source_artifact_directory = $resolvedFailed
                sealed_plan_artifact_directory = $resolvedPlan
                partial_action_artifact_directory = $resolvedPartialAction
                artifact_directory = $recoveryDirectory
            })
        Write-Output "STATUS=PASS"
        Write-Output "RECOVERY_ARTIFACT=$recoveryDirectory"
        return
    }

    if ($recoveryPhase -ceq "root_removed") {
        $rootRemovedState = Get-GuestVerifierRecoveryState -Binding $sealedBinding `
            -ExpectedPublishedInf $publishedInf
        Write-RecoveryJson "before-root-removed-state.json" $rootRemovedState
        Assert-GuestVerifierRecoveryRootRemovedState -State $rootRemovedState `
            -Binding $sealedBinding | Out-Null

        Write-RecoveryJson "exact-binding.json" $sealedBinding
        $recoveryBindingHash = (Get-FileHash -LiteralPath (Join-Path $recoveryDirectory "exact-binding.json") -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        if ($PlanOnly) {
            Write-RecoveryJson "summary.json" ([pscustomobject]@{
                    schema = 1
                    status = "READY"
                    recovery_phase = "root_removed"
                    planned_at_utc = (Get-Date).ToUniversalTime().ToString("o")
                    exact_binding_sha256 = $recoveryBindingHash
                    recovery_id = $recoveryId
                    source_run_id = $runId
                    source_artifact_directory = $resolvedFailed
                    sealed_plan_artifact_directory = $resolvedPlan
                    artifact_directory = $recoveryDirectory
                })
            Write-Output "STATUS=READY"
            Write-Output "RECOVERY_ARTIFACT=$recoveryDirectory"
            return
        }

        $serviceDeletion = Invoke-GuestVerifierRecoveryDeleteService -Binding $sealedBinding `
            -ExpectedServiceName "ramshared"
        Write-RecoveryJson "action-service-delete.json" $serviceDeletion
        if ([int]$serviceDeletion.schema -ne 1 -or [string]$serviceDeletion.action -cne "delete_exact_service" -or
            [string]$serviceDeletion.service_name -cne "ramshared" -or
            [int]$serviceDeletion.exit_code -ne 0 -or [int]$serviceDeletion.terminal_service_count -ne 0) {
            throw "exact service deletion receipt is invalid"
        }

        $driverDeletion = Invoke-GuestVerifierRecoveryDeleteDriver -ExpectedPublishedInf $publishedInf
        Write-RecoveryJson "action-driver-delete.json" $driverDeletion
        if ([int]$driverDeletion.schema -ne 1 -or
            [string]$driverDeletion.action -cne "delete_exact_published_inf" -or
            [string]$driverDeletion.published_inf -cne $publishedInf -or
            ([bool]$driverDeletion.uninstall -ne $true) -or ([bool]$driverDeletion.force -ne $false) -or
            [int]$driverDeletion.exit_code -ne 0) {
            throw "exact DriverStore deletion receipt is invalid"
        }

        $finalState = Get-GuestVerifierRecoveryState -Binding $sealedBinding `
            -ExpectedPublishedInf $publishedInf
        Write-RecoveryJson "after-zero-state.json" $finalState
        Assert-GuestVerifierRecoveryFinalState -State $finalState | Out-Null
        Write-RecoveryJson "summary.json" ([pscustomobject]@{
                schema = 1
                status = "PASS"
                recovery_phase = "root_removed"
                planned_at_utc = [string]$sealedPlanSummary.planned_at_utc
                exact_binding_sha256 = $recoveryBindingHash
                recovery_id = $recoveryId
                source_run_id = $runId
                source_artifact_directory = $resolvedFailed
                sealed_plan_artifact_directory = $resolvedPlan
                artifact_directory = $recoveryDirectory
            })
        Write-Output "STATUS=PASS"
        Write-Output "RECOVERY_ARTIFACT=$recoveryDirectory"
        return
    }

    $binding = Get-GuestVerifierCurrentRunTeardownBinding -RunId $runId `
        -PublishedInf $publishedInf -ExpectedDriverHash $ExpectedDriverSha256 `
        -ExpectedInfHash $ExpectedInfSha256 -ExpectedCatalogHash $ExpectedCatalogSha256 `
        -ExpectedHardwareId "ROOT\RAMSHARED" -ExpectedVpdSerial "ABCDEF0123456789" `
        -ExpectedServiceName "ramshared" -IoPassStarted $ioPassStarted `
        -NormalVpdSerial $normalSerial -VerifierVpdSerial $verifierSerial
    Write-RecoveryJson "exact-binding.json" $binding
    Assert-GuestVerifierCurrentRunTeardownBinding -Binding $binding -RunId $runId `
        -ExpectedPublishedInf $publishedInf -ExpectedDriverHash $ExpectedDriverSha256 `
        -ExpectedInfHash $ExpectedInfSha256 -ExpectedCatalogHash $ExpectedCatalogSha256 `
        -ExpectedHardwareId "ROOT\RAMSHARED" -ExpectedVpdSerial "ABCDEF0123456789" `
        -ExpectedServiceName "ramshared" | Out-Null

    if ($recoveryPhase -cin @("partial_install", "pre_root") -and $resumeRootRemoved) {
        foreach ($propertyName in @(
                "published_inf", "root_instance_id", "hardware_id", "service_name", "service_state",
                "service_path", "loaded_driver_sha256", "driver_store_inf_sha256",
                "driver_store_catalog_sha256", "normal_vpd_state", "normal_vpd_serial",
                "verifier_vpd_state", "verifier_vpd_serial")) {
            if ([string]$binding.$propertyName -cne [string]$sealedBinding.$propertyName) {
                throw "pre_root current binding drifted from sealed plan at $propertyName"
            }
        }
    }

    $preRootState = Get-GuestVerifierPostPublishCleanupState -RunId $runId `
        -ExpectedPublishedInf $publishedInf
    Write-RecoveryJson "pre-root-state.json" $preRootState
    $preRootMode = Get-GuestVerifierPostPublishCleanupMode -State $preRootState -RunId $runId `
        -ExpectedPublishedInf $publishedInf -ExpectedDriverHash $ExpectedDriverSha256 `
        -ExpectedInfHash $ExpectedInfSha256 -ExpectedCatalogHash $ExpectedCatalogSha256
    if ($preRootMode -cne "root_bound") {
        throw "pre-ROOT recovery state is not the exact sealed ROOT-bound identity"
    }

    if ($PlanOnly) {
        $preRootBindingHash = (Get-FileHash -LiteralPath (Join-Path $recoveryDirectory "exact-binding.json") -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        Write-RecoveryJson "summary.json" ([pscustomobject]@{
                schema = 1
                status = "READY"
                recovery_phase = $recoveryPhase
                planned_at_utc = (Get-Date).ToUniversalTime().ToString("o")
                exact_binding_sha256 = $preRootBindingHash
                recovery_id = $recoveryId
                source_run_id = $runId
                source_artifact_directory = $resolvedFailed
                artifact_directory = $recoveryDirectory
            })
        Write-Output "STATUS=READY"
        Write-Output "RECOVERY_ARTIFACT=$recoveryDirectory"
        return
    }

    $rootRemoval = Remove-GuestVerifierCurrentRunRoot -Binding $binding -RunId $runId `
        -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedInfHash $ExpectedInfSha256 `
        -ExpectedCatalogHash $ExpectedCatalogSha256 -ExpectedHardwareId "ROOT\RAMSHARED" `
        -ExpectedVpdSerial "ABCDEF0123456789" -ExpectedServiceName "ramshared"
    Write-RecoveryJson "action-root-removal.json" $rootRemoval

    $rootRemovedState = Get-GuestVerifierPostPublishCleanupState -RunId $runId `
        -ExpectedPublishedInf $publishedInf
    Assert-GuestVerifierRootRemovedState -State $rootRemovedState -Binding $binding -RunId $runId `
        -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedInfHash $ExpectedInfSha256 `
        -ExpectedCatalogHash $ExpectedCatalogSha256 | Out-Null
    Write-RecoveryJson "root-removed-state.json" $rootRemovedState
    $rootRemovalRebooted = $false
    if ([string]$rootRemovedState.service_state -ceq "Running") {
        $rootRemovalBootBefore = Get-GuestVerifierBootTime
        $rootRemovalRestart = Request-GuestVerifierRestart -DelaySeconds $GuestRestartDelaySeconds
        Write-RecoveryJson "root-removal-restart-receipt.json" $rootRemovalRestart
        $rootRemovalBootChange = Wait-GuestVerifierBootTimeChange -Before $rootRemovalBootBefore
        Write-RecoveryJson "root-removal-boot-change.json" $rootRemovalBootChange
        Assert-GuestVerifierBootChangeReceipt -Receipt $rootRemovalBootChange | Out-Null
        $rootRemovalRebooted = $true
        $rootRemovedState = Get-GuestVerifierPostPublishCleanupState -RunId $runId `
            -ExpectedPublishedInf $publishedInf
    }
    Assert-GuestVerifierRootRemovedState -State $rootRemovedState -Binding $binding -RunId $runId `
        -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedInfHash $ExpectedInfSha256 `
        -ExpectedCatalogHash $ExpectedCatalogSha256 -RequireStopped | Out-Null
    Write-RecoveryJson "root-removed-ready.json" $rootRemovedState

    $rootRemovedActions = Remove-GuestVerifierRootRemovedArtifacts -Binding $binding `
        -RootRemovedState $rootRemovedState -RunId $runId `
        -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedInfHash $ExpectedInfSha256 `
        -ExpectedCatalogHash $ExpectedCatalogSha256
    Write-RecoveryJson "root-removed-actions.json" $rootRemovedActions
    $finalState = Get-GuestVerifierPostPublishCleanupState -RunId $runId `
        -ExpectedPublishedInf $publishedInf
    Write-RecoveryJson "after-zero-state.json" $finalState
    $teardown = [pscustomobject]@{
        schema = [int]1
        run_id = $runId
        published_inf = $publishedInf
        service_stop_exit_code = [int]0
        service_stop_action = if ($rootRemovalRebooted) { "stopped_after_root_reboot" } else { "stopped_by_root_removal" }
        device_remove_exit_code = [int]$rootRemoval.action.device_remove_exit_code
        service_delete_exit_code = [int]$rootRemovedActions.service_delete_exit_code
        service_delete_action = [string]$rootRemovedActions.service_delete_action
        driver_delete_exit_code = [int]$rootRemovedActions.driver_delete_exit_code
        retired_node_delete_exit_code = [int]$rootRemovedActions.retired_node_delete_exit_code
        retired_node_delete_action = [string]$rootRemovedActions.retired_node_delete_action
        retired_node_instance_id = [string]$rootRemovedActions.retired_node_instance_id
        package_count = [int]$finalState.package_count
        published_inf_count = [int]$finalState.published_inf_count
        root_count = [int]$finalState.root_count
        service_count = [int]$finalState.service_count
        ramshared_disk_count = [int]$finalState.ramshared_disk_count
        ramshared_pnp_disk_count = [int]$finalState.ramshared_pnp_disk_count
    }
    Write-RecoveryJson "action-and-after.json" $teardown
    Assert-GuestVerifierCurrentRunTeardownEvidence -Evidence $teardown -RunId $runId `
        -ExpectedPublishedInf $publishedInf -RequireActionReceipts | Out-Null
    Write-RecoveryJson "summary.json" ([pscustomobject]@{
            schema = 1
            status = "PASS"
            recovery_phase = $recoveryPhase
            planned_at_utc = [string]$sealedPlanSummary.planned_at_utc
            exact_binding_sha256 = (Get-FileHash -LiteralPath (Join-Path $recoveryDirectory "exact-binding.json") -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
            recovery_id = $recoveryId
            source_run_id = $runId
            source_artifact_directory = $resolvedFailed
            artifact_directory = $recoveryDirectory
        })
    Write-Output "STATUS=PASS"
    Write-Output "RECOVERY_ARTIFACT=$recoveryDirectory"
}
finally {
    $script:GuestVerifierPassword = $null
}
