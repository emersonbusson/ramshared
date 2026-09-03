#Requires -Version 5.1
<#
.SYNOPSIS
  Disposable-VM-only signed .8 Driver Verifier drill.

.DESCRIPTION
  This harness refuses residue instead of trying to repair it.  It binds every
  input by canonical path and SHA-256 before crossing the guest boundary, uses
  the shared bounded PowerShell Direct helper for every guest operation, and
  records only evidence from its fresh GUID run directory.
#>
[CmdletBinding()]
param(
    [ValidateSet("win11-drill")]
    [string]$VMName = "win11-drill",
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedVMId,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$User,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$Password,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$DriverPackage,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$HostBinDir,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedDriverSha256,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedInfSha256,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedCatalogSha256,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedIoctlValidationSha256,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$DriverSignerCert,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedDriverSignerCertSha256,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedDriverSignerSubject,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ExpectedDriverSignerThumbprint,
    [ValidateRange(15, 30)]
    [int]$GuestRestartDelaySeconds = 15,
    [ValidateRange(2, 3600)]
    [int]$GuestOperationTimeoutSeconds = 420,
    [ValidateRange(1, 180)]
    [int]$PsDirectConnectTimeoutSeconds = 180,
    [ValidateNotNullOrEmpty()]
    [string]$ArtifactRoot = "C:\ramshared\artifacts"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not [bool](Get-Command "Get-VM" -ErrorAction SilentlyContinue) -or -not (Get-Module -ListAvailable -Name Hyper-V)) {
    Write-Error -Message "Hyper-V module is not available" -ErrorId "HyperVModuleMissing"
    throw [System.Exception]::new("Hyper-V module is missing")
}
if (-not (Get-VM -Name $VMName -ErrorAction SilentlyContinue)) {
    Write-Error -Message "VM not found: $VMName" -ErrorId "VMNotFound"
    throw [System.Exception]::new("VM not found: $VMName")
}

. (Join-Path $PSScriptRoot "Invoke-GuestPsDirectBounded.ps1")

function Normalize-GuestVerifierSha256 {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    $normalized = $Value.Trim().ToUpperInvariant()
    if ($normalized -notmatch '^[0-9A-F]{64}$') {
        throw "$Name must be one exact SHA-256 value"
    }
    $normalized
}

function Normalize-GuestVerifierSignerThumbprint {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    $normalized = $Value.Trim().ToUpperInvariant()
    if ($Value -cne $Value.Trim() -or $normalized -notmatch '^[0-9A-F]{40}$') {
        throw "$Name must be one exact SHA-1 certificate thumbprint"
    }
    $normalized
}

function Normalize-GuestVerifierSignerSubject {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if ([string]::IsNullOrWhiteSpace($Value) -or $Value -cne $Value.Trim()) {
        throw "$Name must be one exact non-empty certificate subject"
    }
    $Value
}

function Get-GuestVerifierFailureCode {
    [CmdletBinding()]
    param([AllowNull()][object]$Failure)

    if ($null -eq $Failure) { return "" }
    $message = if ($null -ne $Failure.PSObject.Properties["Exception"] -and
        $null -ne $Failure.Exception) {
        [string]$Failure.Exception.Message
    }
    else {
        [string]$Failure
    }
    if ($message.StartsWith("PowerShell Direct invoke outer deadline exceeded",
            [StringComparison]::Ordinal)) {
        return "psdirect_outer_deadline"
    }
    if ($message.StartsWith("PowerShell Direct invoke child failed exit=",
            [StringComparison]::Ordinal)) {
        return "psdirect_child_failed"
    }
    "guest_verifier_refused"
}

function Normalize-GuestVerifierPublishedInf {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    $normalized = $Value.Trim().ToLowerInvariant()
    if ($Value -cne $Value.Trim() -or $normalized -notmatch '^oem[0-9]+\.inf$') {
        throw "$Name must be one exact published OEM INF name"
    }
    $normalized
}

function Get-GuestVerifierPublicCertificateIdentity {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "DriverSignerCert is not an existing file: $Path"
    }
    $canonical = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
    if ([IO.Path]::GetExtension($canonical) -cne ".cer") {
        throw "DriverSignerCert must be an exact public .cer file"
    }
    try {
        $contentType = [Security.Cryptography.X509Certificates.X509Certificate2]::GetCertContentType($canonical)
    }
    catch {
        throw "DriverSignerCert does not have X.509 Cert content type"
    }
    if ($contentType -ne [Security.Cryptography.X509Certificates.X509ContentType]::Cert) {
        throw "DriverSignerCert must be X.509 Cert content, not a private-key container"
    }

    $certificate = $null
    try {
        $certificate = New-Object Security.Cryptography.X509Certificates.X509Certificate2($canonical)
        if ([bool]$certificate.HasPrivateKey) {
            throw "DriverSignerCert must not contain a private key"
        }
        [pscustomobject]@{
            path = $canonical
            subject = Normalize-GuestVerifierSignerSubject ([string]$certificate.Subject) "DriverSignerCert subject"
            thumbprint = Normalize-GuestVerifierSignerThumbprint ([string]$certificate.Thumbprint) "DriverSignerCert thumbprint"
            has_private_key = [bool]$certificate.HasPrivateKey
        }
    }
    finally {
        if ($null -ne $certificate) {
            $certificate.Dispose()
        }
    }
}

function Assert-GuestVerifierSignerIdentity {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Certificate,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint,
        [Parameter(Mandatory = $true)]
        [string]$Role,
        [switch]$AllowPrivateKey
    )

    $aliases = @{
        Subject = @("Subject")
        Thumbprint = @("Thumbprint")
        HasPrivateKey = @("HasPrivateKey", "has_private_key")
    }
    $values = @{}
    foreach ($propertyName in @("Subject", "Thumbprint", "HasPrivateKey")) {
        $matches = @($Certificate.PSObject.Properties | Where-Object {
                $aliases[$propertyName] -contains $_.Name
            })
        if ($matches.Count -ne 1) {
            throw "$Role signer certificate is missing $propertyName"
        }
        $values[$propertyName] = $matches[0].Value
    }
    if ([bool]$values["HasPrivateKey"] -and -not $AllowPrivateKey) {
        throw "$Role signer certificate must not contain a private key"
    }
    $expectedCanonicalSubject = Normalize-GuestVerifierSignerSubject $ExpectedSubject "ExpectedDriverSignerSubject"
    $expectedCanonicalThumbprint = Normalize-GuestVerifierSignerThumbprint $ExpectedThumbprint "ExpectedDriverSignerThumbprint"
    $actualSubject = Normalize-GuestVerifierSignerSubject ([string]$values["Subject"]) "$Role signer subject"
    $actualThumbprint = Normalize-GuestVerifierSignerThumbprint ([string]$values["Thumbprint"]) "$Role signer thumbprint"
    if ($actualSubject -cne $expectedCanonicalSubject -or $actualThumbprint -cne $expectedCanonicalThumbprint) {
        throw "$Role signer certificate subject or thumbprint does not match the sealed public certificate"
    }
    $true
}

function Assert-GuestVerifierSignatureRelationship {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$DriverSignature,
        [Parameter(Mandatory = $true)]
        [object]$CatalogSignature,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint
    )

    foreach ($item in @(
            [pscustomobject]@{ role = "driver"; signature = $DriverSignature },
            [pscustomobject]@{ role = "catalog"; signature = $CatalogSignature })) {
        if ($null -eq $item.signature.PSObject.Properties["Status"] -or
            [string]$item.signature.Status -cne "Valid") {
            throw "$($item.role) Authenticode status is not Valid"
        }
        if ($null -eq $item.signature.PSObject.Properties["SignerCertificate"] -or
            $null -eq $item.signature.SignerCertificate) {
            throw "$($item.role) Authenticode signer certificate is missing"
        }
        Assert-GuestVerifierSignerIdentity -Certificate $item.signature.SignerCertificate `
            -ExpectedSubject $ExpectedSubject -ExpectedThumbprint $ExpectedThumbprint `
            -Role $item.role -AllowPrivateKey | Out-Null
    }
    $true
}

function Get-GuestVerifierInputBinding {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$DriverPackage,
        [Parameter(Mandatory = $true)]
        [string]$HostBinDir,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedDriverSha256,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedInfSha256,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedCatalogSha256,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedIoctlValidationSha256,
        [Parameter(Mandatory = $true)]
        [string]$DriverSignerCert,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedDriverSignerCertSha256,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedDriverSignerSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedDriverSignerThumbprint
    )

    if (-not (Test-Path -LiteralPath $DriverPackage -PathType Container)) {
        throw "DriverPackage is not an existing directory: $DriverPackage"
    }
    if (-not (Test-Path -LiteralPath $HostBinDir -PathType Container)) {
        throw "HostBinDir is not an existing directory: $HostBinDir"
    }

    $packageRoot = (Resolve-Path -LiteralPath $DriverPackage -ErrorAction Stop).Path
    $binRoot = (Resolve-Path -LiteralPath $HostBinDir -ErrorAction Stop).Path
    $certificateIdentity = Get-GuestVerifierPublicCertificateIdentity -Path $DriverSignerCert
    $certificateRoot = Split-Path -Parent ([string]$certificateIdentity.path)
    $certificateLeaf = Split-Path -Leaf ([string]$certificateIdentity.path)
    $expected = @{
        driver_sys = (Normalize-GuestVerifierSha256 $ExpectedDriverSha256 "ExpectedDriverSha256")
        driver_inf = (Normalize-GuestVerifierSha256 $ExpectedInfSha256 "ExpectedInfSha256")
        driver_cat = (Normalize-GuestVerifierSha256 $ExpectedCatalogSha256 "ExpectedCatalogSha256")
        ioctl_validation = (Normalize-GuestVerifierSha256 $ExpectedIoctlValidationSha256 "ExpectedIoctlValidationSha256")
        driver_signer_cert = (Normalize-GuestVerifierSha256 $ExpectedDriverSignerCertSha256 "ExpectedDriverSignerCertSha256")
    }
    Assert-GuestVerifierSignerIdentity -Certificate $certificateIdentity `
        -ExpectedSubject $ExpectedDriverSignerSubject `
        -ExpectedThumbprint $ExpectedDriverSignerThumbprint -Role "sealed public" | Out-Null
    $sources = @(
        [pscustomobject]@{ role = "driver_sys"; root = $packageRoot; leaf = "ramshared.sys"; expected = $expected.driver_sys },
        [pscustomobject]@{ role = "driver_inf"; root = $packageRoot; leaf = "ramshared.inf"; expected = $expected.driver_inf },
        [pscustomobject]@{ role = "driver_cat"; root = $packageRoot; leaf = "ramshared.cat"; expected = $expected.driver_cat },
        [pscustomobject]@{ role = "ioctl_validation"; root = $binRoot; leaf = "Invoke-WinDriveIoctlValidation.ps1"; expected = $expected.ioctl_validation },
        [pscustomobject]@{ role = "driver_signer_cert"; root = $certificateRoot; leaf = $certificateLeaf; expected = $expected.driver_signer_cert }
    )

    $artifacts = @()
    foreach ($source in $sources) {
        $candidate = Join-Path $source.root $source.leaf
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            throw "sealed input is missing: $($source.role) at $candidate"
        }
        $canonical = (Resolve-Path -LiteralPath $candidate -ErrorAction Stop).Path
        $actual = Normalize-GuestVerifierSha256 (
            (Get-FileHash -LiteralPath $canonical -Algorithm SHA256 -ErrorAction Stop).Hash
        ) "$($source.role) actual SHA-256"
        if ($actual -cne $source.expected) {
            throw "sealed input SHA-256 mismatch for $($source.role): expected=$($source.expected) actual=$actual"
        }
        $artifacts += [pscustomobject]@{
            role = $source.role
            path = $canonical
            leaf = $source.leaf
            sha256 = $actual
            byte_length = [int64](Get-Item -LiteralPath $canonical -ErrorAction Stop).Length
        }
    }

    $inf = Get-Content -LiteralPath (Join-Path $packageRoot "ramshared.inf") -Raw -ErrorAction Stop
    if ($inf -notmatch '(?im)^\s*DriverVer\s*=\s*08/09/2026,10\.0\.26200\.8\s*$') {
        throw "ramshared.inf does not bind DriverVer 08/09/2026,10.0.26200.8"
    }

    [pscustomobject]@{
        schema = [int]1
        driver_ver = "08/09/2026,10.0.26200.8"
        driver_package = $packageRoot
        host_bin_dir = $binRoot
        signer = [pscustomobject]@{
            certificate_path = [string]$certificateIdentity.path
            certificate_sha256 = $expected.driver_signer_cert
            subject = Normalize-GuestVerifierSignerSubject $ExpectedDriverSignerSubject "ExpectedDriverSignerSubject"
            thumbprint = Normalize-GuestVerifierSignerThumbprint $ExpectedDriverSignerThumbprint "ExpectedDriverSignerThumbprint"
            has_private_key = [bool]$certificateIdentity.has_private_key
        }
        artifacts = @($artifacts)
    }
}

function Assert-GuestVerifierInputBindingUnchanged {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Binding
    )

    if ([int]$Binding.schema -ne 1 -or
        [string]$Binding.driver_ver -cne "08/09/2026,10.0.26200.8") {
        throw "input binding schema or DriverVer is invalid"
    }
    $expectedRoles = @("driver_sys", "driver_inf", "driver_cat", "ioctl_validation", "driver_signer_cert")
    $artifacts = @($Binding.artifacts)
    if ($artifacts.Count -ne $expectedRoles.Count) {
        throw "input binding must contain exactly five sealed artifacts"
    }
    foreach ($role in $expectedRoles) {
        $matches = @($artifacts | Where-Object { [string]$_.role -ceq $role })
        if ($matches.Count -ne 1) {
            throw "input binding role is missing or ambiguous: $role"
        }
        $artifact = $matches[0]
        if ([string]::IsNullOrWhiteSpace([string]$artifact.path) -or
            -not (Test-Path -LiteralPath ([string]$artifact.path) -PathType Leaf)) {
            throw "sealed source disappeared: $role"
        }
        $canonical = (Resolve-Path -LiteralPath ([string]$artifact.path) -ErrorAction Stop).Path
        if ($canonical -cne [string]$artifact.path) {
            throw "sealed source path changed: $role"
        }
        $actual = Normalize-GuestVerifierSha256 (
            (Get-FileHash -LiteralPath $canonical -Algorithm SHA256 -ErrorAction Stop).Hash
        ) "$role actual SHA-256"
        $bound = Normalize-GuestVerifierSha256 ([string]$artifact.sha256) "$role bound SHA-256"
        $length = [int64](Get-Item -LiteralPath $canonical -ErrorAction Stop).Length
        if ($actual -cne $bound -or $length -ne [int64]$artifact.byte_length) {
            throw "sealed source mutated: $role expected=$bound actual=$actual expected_bytes=$($artifact.byte_length) actual_bytes=$length"
        }
    }
    if ($null -eq $Binding.PSObject.Properties["signer"] -or $null -eq $Binding.signer) {
        throw "input binding is missing sealed signer certificate identity"
    }
    $signer = $Binding.signer
    foreach ($propertyName in @("certificate_path", "certificate_sha256", "subject", "thumbprint", "has_private_key")) {
        if ($null -eq $signer.PSObject.Properties[$propertyName]) {
            throw "input binding signer is missing $propertyName"
        }
    }
    $certificateArtifact = @($artifacts | Where-Object { [string]$_.role -ceq "driver_signer_cert" })[0]
    if ([string]$signer.certificate_path -cne [string]$certificateArtifact.path -or
        (Normalize-GuestVerifierSha256 ([string]$signer.certificate_sha256) "signer certificate SHA-256") -cne
        (Normalize-GuestVerifierSha256 ([string]$certificateArtifact.sha256) "bound signer certificate SHA-256")) {
        throw "input binding signer certificate is not the fifth sealed artifact"
    }
    $certificateIdentity = Get-GuestVerifierPublicCertificateIdentity -Path ([string]$signer.certificate_path)
    Assert-GuestVerifierSignerIdentity -Certificate $certificateIdentity -ExpectedSubject ([string]$signer.subject) `
        -ExpectedThumbprint ([string]$signer.thumbprint) -Role "sealed public" | Out-Null
    if ([bool]$signer.has_private_key) {
        throw "input binding signer certificate must not contain a private key"
    }
    $true
}

function Get-GuestVerifierHostAuthenticodeEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Binding
    )

    Assert-GuestVerifierInputBindingUnchanged -Binding $Binding | Out-Null
    if ($null -eq $Binding.signer) {
        throw "sealed input binding does not identify a signer certificate"
    }
    $sys = @($Binding.artifacts | Where-Object { [string]$_.role -ceq "driver_sys" })
    $cat = @($Binding.artifacts | Where-Object { [string]$_.role -ceq "driver_cat" })
    if ($sys.Count -ne 1 -or $cat.Count -ne 1) {
        throw "sealed input binding does not identify exactly one driver and catalog"
    }
    $sysSignature = Get-AuthenticodeSignature -FilePath ([string]$sys[0].path) -ErrorAction Stop
    $catSignature = Get-AuthenticodeSignature -FilePath ([string]$cat[0].path) -ErrorAction Stop
    Assert-GuestVerifierSignatureRelationship -DriverSignature $sysSignature -CatalogSignature $catSignature `
        -ExpectedSubject ([string]$Binding.signer.subject) -ExpectedThumbprint ([string]$Binding.signer.thumbprint) | Out-Null
    [pscustomobject]@{
        schema = [int]1
        driver_status = [string]$sysSignature.Status
        catalog_status = [string]$catSignature.Status
        driver_subject = [string]$sysSignature.SignerCertificate.Subject
        catalog_subject = [string]$catSignature.SignerCertificate.Subject
        driver_thumbprint = [string]$sysSignature.SignerCertificate.Thumbprint
        catalog_thumbprint = [string]$catSignature.SignerCertificate.Thumbprint
    }
}

function Assert-GuestVerifierRestartReceipt {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Receipt,
        [Parameter(Mandatory = $true)]
        [ValidateRange(15, 30)]
        [int]$ExpectedDelaySeconds
    )

    foreach ($propertyName in @("shutdown_scheduled", "action", "delay_seconds")) {
        if ($null -eq $Receipt.PSObject.Properties[$propertyName]) {
            throw "guest restart receipt is missing $propertyName"
        }
    }
    if (($Receipt.shutdown_scheduled -isnot [bool]) -or -not [bool]$Receipt.shutdown_scheduled) {
        throw "guest restart receipt did not confirm a scheduled restart"
    }
    if ([string]$Receipt.action -cne "restart") {
        throw "guest restart receipt action is not restart"
    }
    if (($Receipt.delay_seconds -isnot [int]) -and
        ($Receipt.delay_seconds -isnot [long])) {
        throw "guest restart receipt delay is not an integer"
    }
    if ([int]$Receipt.delay_seconds -ne $ExpectedDelaySeconds) {
        throw "guest restart receipt delay does not match the requested delay"
    }
    $true
}

function Assert-GuestVerifierBootChangeReceipt {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Receipt
    )

    foreach ($propertyName in @("before_boot_utc", "after_boot_utc")) {
        if ($null -eq $Receipt.PSObject.Properties[$propertyName] -or
            [string]::IsNullOrWhiteSpace([string]$Receipt.$propertyName)) {
            throw "guest boot-change receipt is missing $propertyName"
        }
    }
    try {
        $before = [datetime]::Parse([string]$Receipt.before_boot_utc,
            [Globalization.CultureInfo]::InvariantCulture,
            [Globalization.DateTimeStyles]::RoundtripKind).ToUniversalTime()
        $after = [datetime]::Parse([string]$Receipt.after_boot_utc,
            [Globalization.CultureInfo]::InvariantCulture,
            [Globalization.DateTimeStyles]::RoundtripKind).ToUniversalTime()
    }
    catch {
        throw "guest boot-change receipt has malformed timestamps"
    }
    if ($after -le $before) {
        throw "guest reboot did not produce a strictly newer boot time"
    }
    $true
}

function Assert-GuestVerifierTestSigningReceipt {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [bool]$ExpectedEnabled
    )

    foreach ($propertyName in @("schema", "action", "set_exit_code", "query_exit_code", "testsigning_enabled")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "TestSigning receipt is missing $propertyName"
        }
    }
    $expectedAction = if ($ExpectedEnabled) { "enable" } else { "disable" }
    if ([int]$Evidence.schema -ne 1 -or [string]$Evidence.action -cne $expectedAction -or
        [int]$Evidence.set_exit_code -ne 0 -or [int]$Evidence.query_exit_code -ne 0 -or
        ($Evidence.testsigning_enabled -isnot [bool]) -or
        ([bool]$Evidence.testsigning_enabled -ne $ExpectedEnabled)) {
        throw "TestSigning set/query receipt is missing, nonzero, or inconsistent"
    }
    $true
}

function Assert-GuestVerifierTestSigningState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [bool]$ExpectedEnabled
    )

    foreach ($propertyName in @("schema", "query_exit_code", "testsigning_enabled")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "TestSigning state receipt is missing $propertyName"
        }
    }
    if ([int]$Evidence.schema -ne 1 -or [int]$Evidence.query_exit_code -ne 0 -or
        ($Evidence.testsigning_enabled -isnot [bool]) -or
        ([bool]$Evidence.testsigning_enabled -ne $ExpectedEnabled)) {
        throw "TestSigning state is missing, nonzero, or inconsistent"
    }
    $true
}

function Assert-GuestVerifierSignerTrustReceipt {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint
    )

    foreach ($propertyName in @(
            "schema", "subject", "thumbprint", "has_private_key",
            "root_expected_thumbprint_count", "trusted_publisher_expected_thumbprint_count",
            "root_foreign_subject_count", "trusted_publisher_foreign_subject_count")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "signer trust receipt is missing $propertyName"
        }
    }
    $expectedCanonicalSubject = Normalize-GuestVerifierSignerSubject $ExpectedSubject "ExpectedDriverSignerSubject"
    $expectedCanonicalThumbprint = Normalize-GuestVerifierSignerThumbprint $ExpectedThumbprint "ExpectedDriverSignerThumbprint"
    if ([int]$Evidence.schema -ne 1 -or [string]$Evidence.subject -cne $expectedCanonicalSubject -or
        (Normalize-GuestVerifierSignerThumbprint ([string]$Evidence.thumbprint) "signer trust thumbprint") -cne $expectedCanonicalThumbprint -or
        ([bool]$Evidence.has_private_key) -or
        [int]$Evidence.root_expected_thumbprint_count -ne 1 -or
        [int]$Evidence.trusted_publisher_expected_thumbprint_count -ne 1 -or
        [int]$Evidence.root_foreign_subject_count -ne 0 -or
        [int]$Evidence.trusted_publisher_foreign_subject_count -ne 0) {
        throw "signer trust receipt is incomplete, ambiguous, or not exact"
    }
    $true
}

function Assert-GuestVerifierSignerTrustRemovalReceipt {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint
    )

    foreach ($propertyName in @(
            "schema", "subject", "thumbprint", "root_expected_thumbprint_count",
            "trusted_publisher_expected_thumbprint_count", "root_subject_count",
            "trusted_publisher_subject_count")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "signer trust-removal receipt is missing $propertyName"
        }
    }
    $expectedCanonicalSubject = Normalize-GuestVerifierSignerSubject $ExpectedSubject "ExpectedDriverSignerSubject"
    $expectedCanonicalThumbprint = Normalize-GuestVerifierSignerThumbprint $ExpectedThumbprint "ExpectedDriverSignerThumbprint"
    if ([int]$Evidence.schema -ne 1 -or [string]$Evidence.subject -cne $expectedCanonicalSubject -or
        (Normalize-GuestVerifierSignerThumbprint ([string]$Evidence.thumbprint) "signer removal thumbprint") -cne $expectedCanonicalThumbprint -or
        [int]$Evidence.root_expected_thumbprint_count -ne 0 -or
        [int]$Evidence.trusted_publisher_expected_thumbprint_count -ne 0 -or
        [int]$Evidence.root_subject_count -ne 0 -or
        [int]$Evidence.trusted_publisher_subject_count -ne 0) {
        throw "signer trust-removal receipt is incomplete, ambiguous, or still present"
    }
    $true
}

function Assert-GuestVerifierGuestSignatureEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint
    )

    foreach ($propertyName in @(
            "schema", "driver_status", "catalog_status", "driver_subject", "catalog_subject",
            "driver_thumbprint", "catalog_thumbprint")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "guest Authenticode evidence is missing $propertyName"
        }
    }
    $expectedCanonicalSubject = Normalize-GuestVerifierSignerSubject $ExpectedSubject "ExpectedDriverSignerSubject"
    $expectedCanonicalThumbprint = Normalize-GuestVerifierSignerThumbprint $ExpectedThumbprint "ExpectedDriverSignerThumbprint"
    if ([int]$Evidence.schema -ne 1 -or [string]$Evidence.driver_status -cne "Valid" -or
        [string]$Evidence.catalog_status -cne "Valid" -or
        [string]$Evidence.driver_subject -cne $expectedCanonicalSubject -or
        [string]$Evidence.catalog_subject -cne $expectedCanonicalSubject -or
        (Normalize-GuestVerifierSignerThumbprint ([string]$Evidence.driver_thumbprint) "guest driver signer thumbprint") -cne $expectedCanonicalThumbprint -or
        (Normalize-GuestVerifierSignerThumbprint ([string]$Evidence.catalog_thumbprint) "guest catalog signer thumbprint") -cne $expectedCanonicalThumbprint) {
        throw "guest Authenticode evidence is untrusted, stale, or has the wrong signer"
    }
    $true
}

function Assert-GuestVerifierIoctlVerdict {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Verdict,
        [Parameter(Mandatory = $true)]
        [bool]$VerifierExpected
    )

    foreach ($propertyName in @("DRIVER", "VERIFIER")) {
        if ($null -eq $Verdict.PSObject.Properties[$propertyName]) {
            throw "IOCTL verdict is missing $propertyName"
        }
    }
    if ([string]$Verdict.DRIVER -ine "ramshared.sys") {
        throw "IOCTL verdict names a different driver"
    }
    if (($Verdict.VERIFIER -isnot [bool]) -or
        ([bool]$Verdict.VERIFIER -ne $VerifierExpected)) {
        throw "IOCTL verdict Verifier state is not current-run exact"
    }
    $required = @(
        "PASS_VALID_QUEUE",
        "REFUSE_FOREIGN_OWNER",
        "REFUSE_RESERVED_REGISTER",
        "REFUSE_BAD_RING",
        "REFUSE_RING_INDEX_JUMP",
        "REFUSE_RESERVED_CQE",
        "REFUSE_UNKNOWN_IOCTL",
        "REFUSE_RESERVED_DISK_PARAMS",
        "COMPLETION_REENTRY_NO_SLOT_REUSE",
        "RUNDOWN_UNMAP_AFTER_COPY",
        "VPD_SERIAL_MATCH",
        "EXACT_VIRTUAL_NONROTATING_IDENTITY",
        "NO_NEW_DUMP"
    )
    foreach ($name in $required) {
        $property = $Verdict.PSObject.Properties[$name]
        if ($null -eq $property) {
            throw "IOCTL verdict is missing $name"
        }
        if (($property.Value -is [bool]) -or ([string]$property.Value -cne "1")) {
            throw "IOCTL verdict is not a complete legitimate/refusal pass: $name"
        }
    }
    $true
}

function Assert-GuestVerifierDumpObservation {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence
    )

    foreach ($propertyName in @(
            "dump_before_state", "dump_after_state", "dump_observation_error",
            "new_dump_count")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "dump observation evidence is missing $propertyName"
        }
    }
    $beforeState = [string]$Evidence.dump_before_state
    $afterState = [string]$Evidence.dump_after_state
    if ($beforeState -cnotin @("absent", "present") -or
        $afterState -cnotin @("absent", "present") -or
        -not [string]::IsNullOrEmpty([string]$Evidence.dump_observation_error) -or
        ($beforeState -ceq "present" -and $afterState -ceq "absent") -or
        [int]$Evidence.new_dump_count -ne 0) {
        throw "current-interval dump observation is unavailable, inconsistent, or nonzero"
    }
    $true
}

function Assert-GuestVerifierResidueWaitEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [ValidateSet("disk", "pnp_disk")]
        [string]$ExpectedProviderCode
    )

    foreach ($propertyName in @(
            "schema", "provider_code", "status", "terminal_count",
            "observed_total_count", "retired_count", "attempts", "duration_ms")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "residue wait evidence is missing $propertyName"
        }
    }
    $totalCount = [int]$Evidence.observed_total_count
    $retiredCount = [int]$Evidence.retired_count
    if ([int]$Evidence.schema -ne 1 -or
        [string]$Evidence.provider_code -cne $ExpectedProviderCode -or
        [string]$Evidence.status -cne "zero" -or
        [int]$Evidence.terminal_count -ne 0 -or
        $totalCount -lt 0 -or $retiredCount -lt 0 -or
        ($ExpectedProviderCode -ceq "disk" -and ($totalCount -ne 0 -or $retiredCount -ne 0)) -or
        ($ExpectedProviderCode -ceq "pnp_disk" -and
            ($totalCount -gt 1 -or $retiredCount -ne $totalCount)) -or
        [int]$Evidence.attempts -lt 1 -or
        [int64]$Evidence.duration_ms -lt 0 -or
        [int64]$Evidence.duration_ms -gt 60000) {
        throw "residue wait evidence is stale, timed out, or nonzero"
    }
    $true
}

function Assert-GuestVerifierPassEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [ValidateNotNullOrEmpty()]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [bool]$VerifierExpected,
        [Parameter(Mandatory = $true)]
        [ValidateNotNullOrEmpty()]
        [string]$ExpectedVpdSerial
    )

    foreach ($propertyName in @(
            "schema", "run_id", "pass_name", "verifier_expected", "status",
            "exit_code", "event153_count", "event153_error", "new_dump_count",
            "dump_observation_error", "dump_before_state", "dump_after_state",
            "verdict_error", "vpd_serial",
            "vpd_serial_observation_error", "cleanup", "verdict")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "pass evidence is missing $propertyName"
        }
    }
    if ([int]$Evidence.schema -ne 1 -or [string]$Evidence.run_id -cne $RunId) {
        throw "pass evidence is stale or has the wrong run identity"
    }
    if ([string]::IsNullOrWhiteSpace([string]$Evidence.pass_name) -or
        ($Evidence.verifier_expected -isnot [bool]) -or
        ([bool]$Evidence.verifier_expected -ne $VerifierExpected)) {
        throw "IOCTL evidence is not bound to the current pass and Verifier state"
    }
    if ([string]$Evidence.status -cne "PASS" -or [int]$Evidence.exit_code -ne 0) {
        throw "IOCTL process did not report PASS with exit zero"
    }
    if (-not [string]::IsNullOrEmpty([string]$Evidence.verdict_error) -or
        -not [string]::IsNullOrEmpty([string]$Evidence.event153_error) -or
        -not [string]::IsNullOrEmpty([string]$Evidence.vpd_serial_observation_error)) {
        throw "current-interval verdict, Event 153, dump, or VPD serial observation failed"
    }
    if ([int]$Evidence.event153_count -ne 0 -or [int]$Evidence.new_dump_count -ne 0) {
        throw "current interval has Event 153 or a new dump"
    }
    Assert-GuestVerifierDumpObservation -Evidence $Evidence | Out-Null
    if ([string]$Evidence.vpd_serial -cne $ExpectedVpdSerial) {
        throw "current interval VPD serial is not the sealed current-run serial"
    }
    $cleanup = $Evidence.cleanup
    if ($null -eq $cleanup -or
        $null -eq $cleanup.PSObject.Properties["ramshared_disks"] -or
        $null -eq $cleanup.PSObject.Properties["ramshared_pnp_disks"] -or
        $null -eq $cleanup.PSObject.Properties["ramshared_retired_pnp_disks"] -or
        $null -eq $cleanup.PSObject.Properties["disk_wait"] -or
        $null -eq $cleanup.PSObject.Properties["pnp_disk_wait"] -or
        [int]$cleanup.ramshared_disks -ne 0 -or
        [int]$cleanup.ramshared_pnp_disks -ne 0) {
        throw "current pass did not leave exact zero RamShared LUN residue"
    }
    Assert-GuestVerifierResidueWaitEvidence -Evidence $cleanup.disk_wait `
        -ExpectedProviderCode "disk" | Out-Null
    Assert-GuestVerifierResidueWaitEvidence -Evidence $cleanup.pnp_disk_wait `
        -ExpectedProviderCode "pnp_disk" | Out-Null
    Assert-GuestVerifierIoctlVerdict -Verdict $Evidence.verdict -VerifierExpected $VerifierExpected | Out-Null
    $true
}

function Get-GuestVerifierConnectTimeoutSeconds {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateRange(2, 3600)]
        [int]$OperationTimeoutSeconds,
        [Parameter(Mandatory = $true)]
        [ValidateRange(1, 180)]
        [int]$ConfiguredConnectTimeoutSeconds
    )

    $effective = [Math]::Min($ConfiguredConnectTimeoutSeconds,
        $OperationTimeoutSeconds - 1)
    if ($effective -lt 1 -or $effective -ge $OperationTimeoutSeconds) {
        throw "guest PowerShell Direct connect deadline is not nested inside its outer deadline"
    }
    [int]$effective
}

function Invoke-GuestVerifierRemote {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet("invoke", "copy_to", "copy_from")]
        [string]$Operation,
        [scriptblock]$ScriptBlock = $null,
        [object[]]$ArgumentList = @(),
        [string]$SourcePath = "",
        [string]$DestinationPath = "",
        [switch]$Recurse,
        [ValidateRange(2, 3600)]
        [int]$TimeoutSeconds = 210
    )

    $call = @{
        VMName = $script:GuestVerifierVmName
        User = $script:GuestVerifierUser
        Password = $script:GuestVerifierPassword
        Operation = $Operation
        TimeoutSeconds = $TimeoutSeconds
        ConnectTimeoutSeconds = Get-GuestVerifierConnectTimeoutSeconds `
            -OperationTimeoutSeconds $TimeoutSeconds `
            -ConfiguredConnectTimeoutSeconds $script:GuestVerifierConnectTimeoutSeconds
    }
    if ($Operation -eq "invoke") {
        if ($null -eq $ScriptBlock) {
            throw "guest invoke requires a script block"
        }
        $call.ScriptBlock = $ScriptBlock
        $call.ArgumentList = @($ArgumentList)
    }
    else {
        if ([string]::IsNullOrWhiteSpace($SourcePath) -or
            [string]::IsNullOrWhiteSpace($DestinationPath)) {
            throw "guest copy requires source and destination paths"
        }
        $call.SourcePath = $SourcePath
        $call.DestinationPath = $DestinationPath
        $call.Recurse = [bool]$Recurse
    }
    Invoke-GuestPsDirectBounded @call
}

function Write-GuestVerifierArtifact {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateNotNullOrEmpty()]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [object]$Value
    )

    $path = Join-Path $script:GuestVerifierArtifactDirectory $Name
    $Value | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath $path -Encoding UTF8 -ErrorAction Stop
    $path
}

function ConvertFrom-GuestVerifierRows {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Rows,
        [Parameter(Mandatory = $true)]
        [string]$Operation
    )

    $json = (@($Rows) | ForEach-Object { [string]$_ }) -join [Environment]::NewLine
    if ([string]::IsNullOrWhiteSpace($json)) {
        throw "$Operation emitted no typed result"
    }
    try {
        $json | ConvertFrom-Json -ErrorAction Stop
    }
    catch {
        throw "$Operation emitted invalid typed JSON: $($_.Exception.Message)"
    }
}

function Get-GuestVerifierBootTime {
    [CmdletBinding()]
    param()

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 45 -ScriptBlock {
        $boot = (Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop).LastBootUpTime
        [pscustomobject]@{
            schema = [int]1
            boot_time_utc = $boot.ToUniversalTime().ToString("o")
        } | ConvertTo-Json -Compress
    }
    $receipt = ConvertFrom-GuestVerifierRows -Rows $rows -Operation "guest boot-time query"
    if ([int]$receipt.schema -ne 1 -or [string]::IsNullOrWhiteSpace([string]$receipt.boot_time_utc)) {
        throw "guest boot-time query is incomplete"
    }
    try {
        [datetime]::Parse([string]$receipt.boot_time_utc, [Globalization.CultureInfo]::InvariantCulture,
            [Globalization.DateTimeStyles]::RoundtripKind).ToUniversalTime()
    }
    catch {
        throw "guest boot-time query is malformed"
    }
}

function Request-GuestVerifierRestart {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateRange(15, 30)]
        [int]$DelaySeconds
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 45 -ScriptBlock {
        param($RequestedDelay)
        $output = & shutdown.exe /r /t $RequestedDelay /d p:4:1 /c "RamShared Driver Verifier guest boundary" 2>&1 | Out-String
        $exitCode = [int]$LASTEXITCODE
        if ($exitCode -ne 0) {
            throw "guest shutdown.exe restart schedule failed exit=$($exitCode): $output"
        }
        [pscustomobject]@{
            schema = [int]1
            shutdown_scheduled = $true
            action = "restart"
            delay_seconds = [int]$RequestedDelay
            command_exit_code = $exitCode
        } | ConvertTo-Json -Compress
    } -ArgumentList @($DelaySeconds)
    $receipt = ConvertFrom-GuestVerifierRows -Rows $rows -Operation "guest restart request"
    Assert-GuestVerifierRestartReceipt -Receipt $receipt -ExpectedDelaySeconds $DelaySeconds | Out-Null
    $receipt
}

function Wait-GuestVerifierBootTimeChange {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [datetime]$Before,
        [ValidateRange(60, 900)]
        [int]$TimeoutSeconds = 600
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $lastFailure = ""
    while ((Get-Date) -lt $deadline) {
        $vm = Get-VM -Name $script:GuestVerifierVmName -ErrorAction Stop
        if ($vm.State -eq "Running") {
            try {
                $after = Get-GuestVerifierBootTime
                if ($after -gt $Before) {
                    return [pscustomobject]@{
                        before_boot_utc = $Before.ToUniversalTime().ToString("o")
                        after_boot_utc = $after.ToUniversalTime().ToString("o")
                    }
                }
                $lastFailure = "guest boot time did not advance"
            }
            catch {
                $lastFailure = $_.Exception.Message
            }
        }
        else {
            $lastFailure = "VM state is $($vm.State)"
        }
        Start-Sleep -Seconds 5
    }
    throw "guest reboot did not produce a newer boot time within $TimeoutSeconds seconds: $lastFailure"
}

function Get-GuestVerifierTestSigningState {
    [CmdletBinding()]
    param()

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 45 -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $queryOutput = & bcdedit.exe /enum "{current}" 2>&1 | Out-String
        $queryExit = [int]$LASTEXITCODE
        [pscustomobject]@{
            schema = [int]1
            query_exit_code = $queryExit
            testsigning_enabled = [bool]($queryOutput -match '(?im)^\s*testsigning\s+(yes|on|true|1)\s*$')
        } | ConvertTo-Json -Compress
    }
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "TestSigning state query"
}

function Set-GuestVerifierTestSigning {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [bool]$Enabled
    )

    $requestedState = if ($Enabled) { "on" } else { "off" }
    $requestedAction = if ($Enabled) { "enable" } else { "disable" }
    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 60 -ScriptBlock {
        param($RequestedState, $RequestedAction)
        $ErrorActionPreference = "Stop"
        $setOutput = & bcdedit.exe /set testsigning $RequestedState 2>&1 | Out-String
        $setExit = [int]$LASTEXITCODE
        $queryOutput = & bcdedit.exe /enum "{current}" 2>&1 | Out-String
        $queryExit = [int]$LASTEXITCODE
        [pscustomobject]@{
            schema = [int]1
            action = $RequestedAction
            set_exit_code = $setExit
            query_exit_code = $queryExit
            testsigning_enabled = [bool]($queryOutput -match '(?im)^\s*testsigning\s+(yes|on|true|1)\s*$')
        } | ConvertTo-Json -Compress
    } -ArgumentList @($requestedState, $requestedAction)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "TestSigning $requestedAction set/query"
}

function Get-GuestVerifierStagedSignerCertificateEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$GuestStage,
        [Parameter(Mandatory = $true)]
        [string]$CertificateLeaf,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 60 -ScriptBlock {
        param($Stage, $Leaf, $ExpectedSignerSubject, $ExpectedSignerThumbprint)
        $ErrorActionPreference = "Stop"
        $certificatePath = Join-Path $Stage $Leaf
        if (-not (Test-Path -LiteralPath $certificatePath -PathType Leaf) -or
            [IO.Path]::GetExtension($certificatePath) -cne ".cer") {
            throw "staged public signer certificate is missing or not .cer"
        }
        $contentType = [Security.Cryptography.X509Certificates.X509Certificate2]::GetCertContentType($certificatePath)
        if ($contentType -ne [Security.Cryptography.X509Certificates.X509ContentType]::Cert) {
            throw "staged signer certificate is not public X.509 Cert content"
        }
        $certificate = $null
        try {
            $certificate = New-Object Security.Cryptography.X509Certificates.X509Certificate2($certificatePath)
            if ([bool]$certificate.HasPrivateKey -or [string]$certificate.Subject -cne $ExpectedSignerSubject -or
                (([string]$certificate.Thumbprint -replace '\s', '').ToUpperInvariant() -cne
                ($ExpectedSignerThumbprint -replace '\s', '').ToUpperInvariant())) {
                throw "staged signer certificate identity is not the sealed public certificate"
            }
            [pscustomobject]@{
                schema = [int]1
                subject = [string]$certificate.Subject
                thumbprint = [string]$certificate.Thumbprint
                has_private_key = [bool]$certificate.HasPrivateKey
            } | ConvertTo-Json -Compress
        }
        finally {
            if ($null -ne $certificate) {
                $certificate.Dispose()
            }
        }
    } -ArgumentList @($GuestStage, $CertificateLeaf, $ExpectedSubject, $ExpectedThumbprint)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "staged public signer certificate identity"
}

function Install-GuestVerifierSignerTrust {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$GuestStage,
        [Parameter(Mandatory = $true)]
        [string]$CertificateLeaf,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 90 -ScriptBlock {
        param($Stage, $Leaf, $ExpectedSignerSubject, $ExpectedSignerThumbprint)
        $ErrorActionPreference = "Stop"
        $certificatePath = Join-Path $Stage $Leaf
        if (-not (Test-Path -LiteralPath $certificatePath -PathType Leaf) -or
            [IO.Path]::GetExtension($certificatePath) -cne ".cer") {
            throw "staged public signer certificate is missing or not .cer"
        }
        $contentType = [Security.Cryptography.X509Certificates.X509Certificate2]::GetCertContentType($certificatePath)
        if ($contentType -ne [Security.Cryptography.X509Certificates.X509ContentType]::Cert) {
            throw "staged signer certificate is not public X.509 Cert content"
        }
        $expectedThumbprintCanonical = ($ExpectedSignerThumbprint -replace '\s', '').ToUpperInvariant()
        $certificateBytes = [IO.File]::ReadAllBytes($certificatePath)
        if ($certificateBytes.Length -eq 0) {
            throw "staged signer certificate is empty"
        }
        $certificate = $null
        $certificateSubject = ""
        $certificateThumbprint = ""
        $certificateHasPrivateKey = $true
        try {
            $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($certificateBytes)
            if ([bool]$certificate.HasPrivateKey -or [string]$certificate.Subject -cne $ExpectedSignerSubject -or
                (([string]$certificate.Thumbprint -replace '\s', '').ToUpperInvariant() -cne $expectedThumbprintCanonical)) {
                throw "staged signer certificate identity is not the sealed public certificate"
            }
            $certificateSubject = [string]$certificate.Subject
            $certificateThumbprint = [string]$certificate.Thumbprint
            $certificateHasPrivateKey = [bool]$certificate.HasPrivateKey
            $providerStores = @("Cert:\LocalMachine\Root", "Cert:\LocalMachine\TrustedPublisher")
            foreach ($providerStore in $providerStores) {
                $existing = @(Get-ChildItem -LiteralPath $providerStore -ErrorAction Stop)
                $exact = @($existing | Where-Object {
                        (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $expectedThumbprintCanonical
                    })
                $foreignSubject = @($existing | Where-Object {
                        [string]$_.Subject -ceq $ExpectedSignerSubject -and
                        (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -cne $expectedThumbprintCanonical
                    })
                if ($exact.Count -ne 0 -or $foreignSubject.Count -ne 0) {
                    throw "guest signer store is not clean for the sealed public certificate"
                }
            }

            foreach ($storeName in @(
                    [Security.Cryptography.X509Certificates.StoreName]::Root,
                    [Security.Cryptography.X509Certificates.StoreName]::TrustedPublisher)) {
                $store = $null
                try {
                    $store = [Security.Cryptography.X509Certificates.X509Store]::new(
                        $storeName,
                        [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine)
                    $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
                    $store.Add($certificate)
                }
                finally {
                    if ($null -ne $store) {
                        $store.Dispose()
                    }
                }
            }
        }
        finally {
            if ($null -ne $certificate) {
                $certificate.Dispose()
            }
        }
        $rootCertificates = @(Get-ChildItem -LiteralPath "Cert:\LocalMachine\Root" -ErrorAction Stop)
        $trustedPublisherCertificates = @(Get-ChildItem -LiteralPath "Cert:\LocalMachine\TrustedPublisher" -ErrorAction Stop)
        $rootExpected = @($rootCertificates | Where-Object {
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $expectedThumbprintCanonical
            })
        $trustedPublisherExpected = @($trustedPublisherCertificates | Where-Object {
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $expectedThumbprintCanonical
            })
        $rootForeignSubject = @($rootCertificates | Where-Object {
                [string]$_.Subject -ceq $ExpectedSignerSubject -and
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -cne $expectedThumbprintCanonical
            })
        $trustedPublisherForeignSubject = @($trustedPublisherCertificates | Where-Object {
                [string]$_.Subject -ceq $ExpectedSignerSubject -and
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -cne $expectedThumbprintCanonical
            })
        [pscustomobject]@{
            schema = [int]1
            subject = $certificateSubject
            thumbprint = $certificateThumbprint
            has_private_key = $certificateHasPrivateKey
            root_expected_thumbprint_count = [int]$rootExpected.Count
            trusted_publisher_expected_thumbprint_count = [int]$trustedPublisherExpected.Count
            root_foreign_subject_count = [int]$rootForeignSubject.Count
            trusted_publisher_foreign_subject_count = [int]$trustedPublisherForeignSubject.Count
        } | ConvertTo-Json -Compress
    } -ArgumentList @($GuestStage, $CertificateLeaf, $ExpectedSubject, $ExpectedThumbprint)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "exact public signer trust install"
}

function Get-GuestVerifierSignerTrustEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 60 -ScriptBlock {
        param($ExpectedSignerSubject, $ExpectedSignerThumbprint)
        $ErrorActionPreference = "Stop"
        $expectedThumbprintCanonical = ($ExpectedSignerThumbprint -replace '\s', '').ToUpperInvariant()
        $rootCertificates = @(Get-ChildItem -LiteralPath "Cert:\LocalMachine\Root" -ErrorAction Stop)
        $trustedPublisherCertificates = @(Get-ChildItem -LiteralPath "Cert:\LocalMachine\TrustedPublisher" -ErrorAction Stop)
        $rootExpected = @($rootCertificates | Where-Object {
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $expectedThumbprintCanonical
            })
        $trustedPublisherExpected = @($trustedPublisherCertificates | Where-Object {
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $expectedThumbprintCanonical
            })
        $rootForeignSubject = @($rootCertificates | Where-Object {
                [string]$_.Subject -ceq $ExpectedSignerSubject -and
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -cne $expectedThumbprintCanonical
            })
        $trustedPublisherForeignSubject = @($trustedPublisherCertificates | Where-Object {
                [string]$_.Subject -ceq $ExpectedSignerSubject -and
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -cne $expectedThumbprintCanonical
            })
        $allExpected = @($rootExpected) + @($trustedPublisherExpected)
        [pscustomobject]@{
            schema = [int]1
            subject = $ExpectedSignerSubject
            thumbprint = $expectedThumbprintCanonical
            has_private_key = [bool](@($allExpected | Where-Object { [bool]$_.HasPrivateKey }).Count -ne 0)
            root_expected_thumbprint_count = [int]$rootExpected.Count
            trusted_publisher_expected_thumbprint_count = [int]$trustedPublisherExpected.Count
            root_foreign_subject_count = [int]$rootForeignSubject.Count
            trusted_publisher_foreign_subject_count = [int]$trustedPublisherForeignSubject.Count
        } | ConvertTo-Json -Compress
    } -ArgumentList @($ExpectedSubject, $ExpectedThumbprint)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "exact public signer trust query"
}

function Remove-GuestVerifierSignerTrust {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedThumbprint
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 90 -ScriptBlock {
        param($ExpectedSignerSubject, $ExpectedSignerThumbprint)
        $ErrorActionPreference = "Stop"
        $expectedThumbprintCanonical = ($ExpectedSignerThumbprint -replace '\s', '').ToUpperInvariant()
        $stores = @("Cert:\LocalMachine\Root", "Cert:\LocalMachine\TrustedPublisher")
        foreach ($store in $stores) {
            $existing = @(Get-ChildItem -LiteralPath $store -ErrorAction Stop)
            $foreignSubject = @($existing | Where-Object {
                    [string]$_.Subject -ceq $ExpectedSignerSubject -and
                    (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -cne $expectedThumbprintCanonical
                })
            if ($foreignSubject.Count -ne 0) {
                throw "guest signer store has a foreign matching-subject certificate"
            }
            $exact = @($existing | Where-Object {
                    (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $expectedThumbprintCanonical
                })
            if ($exact.Count -gt 1) {
                throw "guest signer store has an ambiguous exact-thumbprint certificate"
            }
            if ($exact.Count -eq 1) {
                if ([string]$exact[0].Subject -cne $ExpectedSignerSubject) {
                    throw "guest signer store exact-thumbprint certificate has the wrong subject"
                }
                Remove-Item -LiteralPath $exact[0].PSPath -Force -ErrorAction Stop
            }
        }
        $rootCertificates = @(Get-ChildItem -LiteralPath "Cert:\LocalMachine\Root" -ErrorAction Stop)
        $trustedPublisherCertificates = @(Get-ChildItem -LiteralPath "Cert:\LocalMachine\TrustedPublisher" -ErrorAction Stop)
        $rootExpected = @($rootCertificates | Where-Object {
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $expectedThumbprintCanonical
            })
        $trustedPublisherExpected = @($trustedPublisherCertificates | Where-Object {
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $expectedThumbprintCanonical
            })
        $rootSubject = @($rootCertificates | Where-Object { [string]$_.Subject -ceq $ExpectedSignerSubject })
        $trustedPublisherSubject = @($trustedPublisherCertificates | Where-Object { [string]$_.Subject -ceq $ExpectedSignerSubject })
        [pscustomobject]@{
            schema = [int]1
            subject = $ExpectedSignerSubject
            thumbprint = $expectedThumbprintCanonical
            root_expected_thumbprint_count = [int]$rootExpected.Count
            trusted_publisher_expected_thumbprint_count = [int]$trustedPublisherExpected.Count
            root_subject_count = [int]$rootSubject.Count
            trusted_publisher_subject_count = [int]$trustedPublisherSubject.Count
        } | ConvertTo-Json -Compress
    } -ArgumentList @($ExpectedSubject, $ExpectedThumbprint)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "exact public signer trust removal"
}

function Get-GuestVerifierGuestSignatureEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$GuestStage
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 60 -ScriptBlock {
        param($Stage)
        $ErrorActionPreference = "Stop"
        $sys = Join-Path $Stage "ramshared.sys"
        $cat = Join-Path $Stage "ramshared.cat"
        foreach ($path in @($sys, $cat)) {
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                throw "staged signed package input is missing: $path"
            }
        }
        $sysSignature = Get-AuthenticodeSignature -FilePath $sys -ErrorAction Stop
        $catSignature = Get-AuthenticodeSignature -FilePath $cat -ErrorAction Stop
        [pscustomobject]@{
            schema = [int]1
            driver_status = [string]$sysSignature.Status
            catalog_status = [string]$catSignature.Status
            driver_subject = if ($null -eq $sysSignature.SignerCertificate) { "" } else { [string]$sysSignature.SignerCertificate.Subject }
            catalog_subject = if ($null -eq $catSignature.SignerCertificate) { "" } else { [string]$catSignature.SignerCertificate.Subject }
            driver_thumbprint = if ($null -eq $sysSignature.SignerCertificate) { "" } else { [string]$sysSignature.SignerCertificate.Thumbprint }
            catalog_thumbprint = if ($null -eq $catSignature.SignerCertificate) { "" } else { [string]$catSignature.SignerCertificate.Thumbprint }
        } | ConvertTo-Json -Compress
    } -ArgumentList @($GuestStage)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "guest staged Authenticode query"
}

function Assert-GuestVerifierCurrentIdentity {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Identity,
        [Parameter(Mandatory = $true)]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedIoctlHash
    )

    if ([int]$Identity.schema -ne 1 -or [string]$Identity.run_id -cne $RunId) {
        throw "loaded identity is stale or malformed"
    }
    if ([int]$Identity.root_count -ne 1 -or [int]$Identity.scsi_count -ne 1 -or
        [int]$Identity.running_service_count -ne 1) {
        throw "loaded identity is not an exact ROOT/SCSI/service singleton"
    }
    if ([string]::IsNullOrWhiteSpace([string]$Identity.root_instance_id) -or
        [string]::IsNullOrWhiteSpace([string]$Identity.scsi_instance_id) -or
        [string]::IsNullOrWhiteSpace([string]$Identity.loaded_path)) {
        throw "loaded identity lacks an exact current path or device identity"
    }
    if ([bool]$Identity.binary_match -ne $true) {
        throw "loaded identity did not prove BINARY_MATCH"
    }
    $loadedHash = Normalize-GuestVerifierSha256 ([string]$Identity.loaded_sha256) "loaded driver SHA-256"
    $driverHash = Normalize-GuestVerifierSha256 $ExpectedDriverHash "expected driver SHA-256"
    $scriptHash = Normalize-GuestVerifierSha256 ([string]$Identity.staged_ioctl_sha256) "staged IOCTL SHA-256"
    $expectedScriptHash = Normalize-GuestVerifierSha256 $ExpectedIoctlHash "expected IOCTL SHA-256"
    if ($loadedHash -cne $driverHash -or $scriptHash -cne $expectedScriptHash) {
        throw "loaded identity does not match the sealed current run"
    }
    $true
}

function Get-GuestVerifierCurrentIdentity {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [string]$GuestStage,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedIoctlHash
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        param($CurrentRunId, $Stage, $ExpectedSysHash, $ExpectedScriptHash)
        $ErrorActionPreference = "Stop"
        $allRoots = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
            })
        $roots = @($allRoots | Where-Object {
                $_.Status -eq "OK" -and [int]$_.Problem -eq 0
            })
        $allScsi = @(Get-PnpDevice -Class SCSIAdapter -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\' -or
                $_.FriendlyName -match '(?i)^RamShared'
            })
        $scsi = @($allScsi | Where-Object {
                $_.Status -eq "OK" -and [int]$_.Problem -eq 0
            })
        $services = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop |
            Where-Object { $_.State -eq "Running" })
        if ($allRoots.Count -ne 1 -or $roots.Count -ne 1 -or
            $allScsi.Count -ne 1 -or $scsi.Count -ne 1 -or $services.Count -ne 1) {
            throw "current RamShared identity is ambiguous or not healthy"
        }
        $rawPath = [string]$services[0].PathName
        if ([string]::IsNullOrWhiteSpace($rawPath)) {
            throw "Win32_SystemDriver.PathName is empty"
        }
        $candidate = $rawPath.Trim()
        if ($candidate.StartsWith('"')) {
            $closing = $candidate.IndexOf('"', 1)
            if ($closing -lt 1) {
                throw "Win32_SystemDriver.PathName quote is malformed"
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
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            throw "Win32_SystemDriver.PathName does not name an existing file: $candidate"
        }
        $loadedPath = (Resolve-Path -LiteralPath $candidate -ErrorAction Stop).Path
        if ($loadedPath.IndexOf("\DriverStore\FileRepository\", [StringComparison]::OrdinalIgnoreCase) -lt 0) {
            throw "Win32_SystemDriver.PathName is not the DriverStore FileRepository image"
        }
        $loadedHash = (Get-FileHash -LiteralPath $loadedPath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        $stagedScript = Join-Path $Stage "Invoke-WinDriveIoctlValidation.ps1"
        if (-not (Test-Path -LiteralPath $stagedScript -PathType Leaf)) {
            throw "current-run staged IOCTL script is missing"
        }
        $stagedScriptHash = (Get-FileHash -LiteralPath $stagedScript -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        [pscustomobject]@{
            schema = [int]1
            run_id = $CurrentRunId
            root_count = [int]$roots.Count
            scsi_count = [int]$scsi.Count
            running_service_count = [int]$services.Count
            root_instance_id = [string]$roots[0].InstanceId
            scsi_instance_id = [string]$scsi[0].InstanceId
            loaded_path = $loadedPath
            loaded_sha256 = $loadedHash
            staged_ioctl_sha256 = $stagedScriptHash
            binary_match = [bool](($loadedHash -ceq $ExpectedSysHash.ToUpperInvariant()) -and
                ($stagedScriptHash -ceq $ExpectedScriptHash.ToUpperInvariant()))
        } | ConvertTo-Json -Compress
    } -ArgumentList @($RunId, $GuestStage, $ExpectedDriverHash, $ExpectedIoctlHash)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "current loaded identity query"
}

function Assert-GuestVerifierCurrentRunTeardownBinding {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Binding,
        [Parameter(Mandatory = $true)]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedPublishedInf,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedInfHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedCatalogHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedHardwareId,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedVpdSerial,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedServiceName
    )

    foreach ($propertyName in @(
            "schema", "run_id", "published_inf", "package_count", "package_original_inf",
            "root_count", "root_instance_id", "hardware_id", "service_count", "service_name",
            "service_state", "service_path", "loaded_driver_sha256", "driver_store_inf_sha256",
            "driver_store_catalog_sha256", "io_pass_started", "normal_vpd_state",
            "normal_vpd_serial", "verifier_vpd_state", "verifier_vpd_serial", "binary_match")) {
        if ($null -eq $Binding.PSObject.Properties[$propertyName]) {
            throw "current-run teardown binding is missing $propertyName"
        }
    }
    $publishedInf = Normalize-GuestVerifierPublishedInf ([string]$Binding.published_inf) "current-run published INF"
    $expectedPublishedInfCanonical = Normalize-GuestVerifierPublishedInf $ExpectedPublishedInf "expected published INF"
    $validatedPassCount = [int]0
    foreach ($passName in @("normal", "verifier")) {
        $state = [string]$Binding.("${passName}_vpd_state")
        $serial = [string]$Binding.("${passName}_vpd_serial")
        if ($state -ceq "validated") {
            if ($serial -cne $ExpectedVpdSerial) {
                throw "current-run $passName VPD serial is not the sealed serial"
            }
            $validatedPassCount++
        }
        elseif ($state -ceq "not_executed") {
            if (-not [string]::IsNullOrEmpty($serial)) {
                throw "current-run $passName future-pass serial is not empty"
            }
        }
        else {
            throw "current-run $passName VPD state is invalid"
        }
    }
    if (($Binding.io_pass_started -isnot [bool]) -or
        ([bool]$Binding.io_pass_started -and $validatedPassCount -lt 1) -or
        (-not [bool]$Binding.io_pass_started -and $validatedPassCount -ne 0)) {
        throw "current-run phase-bound VPD evidence is inconsistent"
    }
    if ([int]$Binding.schema -ne 1 -or [string]$Binding.run_id -cne $RunId -or
        $publishedInf -cne $expectedPublishedInfCanonical -or
        [int]$Binding.package_count -ne 1 -or
        [string]$Binding.package_original_inf -ine "ramshared.inf" -or
        [int]$Binding.root_count -ne 1 -or
        [string]::IsNullOrWhiteSpace([string]$Binding.root_instance_id) -or
        [string]$Binding.hardware_id -cne $ExpectedHardwareId -or
        [int]$Binding.service_count -ne 1 -or
        [string]$Binding.service_name -cne $ExpectedServiceName -or
        [string]$Binding.service_state -cnotin @("Running", "Stopped") -or
        [string]::IsNullOrWhiteSpace([string]$Binding.service_path) -or
        ([string]$Binding.service_path).IndexOf("\DriverStore\FileRepository\", [StringComparison]::OrdinalIgnoreCase) -lt 0 -or
        ($Binding.binary_match -isnot [bool]) -or
        ([bool]$Binding.binary_match -ne $true)) {
        throw "current-run teardown binding is stale, foreign, ambiguous, or incomplete"
    }
    $actualDriverHash = Normalize-GuestVerifierSha256 ([string]$Binding.loaded_driver_sha256) "current-run loaded SYS SHA-256"
    $actualInfHash = Normalize-GuestVerifierSha256 ([string]$Binding.driver_store_inf_sha256) "current-run DriverStore INF SHA-256"
    $actualCatalogHash = Normalize-GuestVerifierSha256 ([string]$Binding.driver_store_catalog_sha256) "current-run DriverStore CAT SHA-256"
    if ($actualDriverHash -cne (Normalize-GuestVerifierSha256 $ExpectedDriverHash "expected SYS SHA-256") -or
        $actualInfHash -cne (Normalize-GuestVerifierSha256 $ExpectedInfHash "expected INF SHA-256") -or
        $actualCatalogHash -cne (Normalize-GuestVerifierSha256 $ExpectedCatalogHash "expected CAT SHA-256")) {
        throw "current-run teardown binding DriverStore hashes do not match sealed inputs"
    }
    $true
}

function Assert-GuestVerifierCurrentRunTeardownEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedPublishedInf,
        [switch]$RequireActionReceipts
    )

    foreach ($propertyName in @(
            "schema", "run_id", "published_inf", "package_count", "published_inf_count",
            "root_count", "service_count", "ramshared_disk_count", "ramshared_pnp_disk_count")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "current-run teardown evidence is missing $propertyName"
        }
    }
    $publishedInf = Normalize-GuestVerifierPublishedInf ([string]$Evidence.published_inf) "teardown evidence published INF"
    $expectedPublishedInfCanonical = Normalize-GuestVerifierPublishedInf $ExpectedPublishedInf "expected published INF"
    if ([int]$Evidence.schema -ne 1 -or [string]$Evidence.run_id -cne $RunId -or
        $publishedInf -cne $expectedPublishedInfCanonical -or
        [int]$Evidence.package_count -ne 0 -or
        [int]$Evidence.published_inf_count -ne 0 -or
        [int]$Evidence.root_count -ne 0 -or
        [int]$Evidence.service_count -ne 0 -or
        [int]$Evidence.ramshared_disk_count -ne 0 -or
        [int]$Evidence.ramshared_pnp_disk_count -ne 0) {
        throw "current-run teardown is incomplete, ambiguous, or left residue"
    }
    if ($RequireActionReceipts) {
        foreach ($propertyName in @(
                "service_stop_exit_code", "service_stop_action", "device_remove_exit_code", "service_delete_exit_code",
                "driver_delete_exit_code", "service_delete_action", "retired_node_delete_exit_code",
                "retired_node_delete_action", "retired_node_instance_id")) {
            if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
                throw "current-run teardown action evidence is missing $propertyName"
            }
        }
        if ([int]$Evidence.service_stop_exit_code -ne 0 -or
            [string]$Evidence.service_stop_action -cnotin @(
                "deferred_to_root_removal", "already_stopped",
                "stopped_after_root_reboot", "stopped_by_root_removal") -or
            [int]$Evidence.device_remove_exit_code -ne 0 -or
            [int]$Evidence.service_delete_exit_code -ne 0 -or
            [int]$Evidence.driver_delete_exit_code -ne 0 -or
            [int]$Evidence.retired_node_delete_exit_code -ne 0 -or
            [string]$Evidence.retired_node_delete_action -cnotin @("deleted", "not_present") -or
            ([string]$Evidence.retired_node_delete_action -ceq "deleted" -and
                [string]$Evidence.retired_node_instance_id -cnotmatch '^SCSI\\DISK&VEN_RAMSHARE&PROD_VRAMDISK\\[^\\]+$') -or
            ([string]$Evidence.retired_node_delete_action -ceq "not_present" -and
                -not [string]::IsNullOrEmpty([string]$Evidence.retired_node_instance_id)) -or
            @("deleted", "already_removed_by_device") -notcontains [string]$Evidence.service_delete_action) {
            throw "current-run teardown action receipt is nonzero or unsafe"
        }
    }
    $true
}

function Get-GuestVerifierCurrentRunTeardownBinding {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [string]$PublishedInf,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedInfHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedCatalogHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedHardwareId,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedVpdSerial,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedServiceName,
        [Parameter(Mandatory = $true)]
        [bool]$IoPassStarted,
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$NormalVpdSerial,
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$VerifierVpdSerial
    )

    $expectedPublishedInfCanonical = Normalize-GuestVerifierPublishedInf $PublishedInf "published INF from pnputil install receipt"
    $expectedDriverHashCanonical = Normalize-GuestVerifierSha256 $ExpectedDriverHash "expected SYS SHA-256"
    $expectedInfHashCanonical = Normalize-GuestVerifierSha256 $ExpectedInfHash "expected INF SHA-256"
    $expectedCatalogHashCanonical = Normalize-GuestVerifierSha256 $ExpectedCatalogHash "expected CAT SHA-256"
    $normalVpdState = if ([string]::IsNullOrEmpty($NormalVpdSerial)) { "not_executed" } else { "validated" }
    $verifierVpdState = if ([string]::IsNullOrEmpty($VerifierVpdSerial)) { "not_executed" } else { "validated" }
    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        param($CurrentRunId, $ExpectedPublishedInf, $ExpectedSysHash, $ExpectedInfHash, $ExpectedCatHash,
            $ExpectedHardware, $ExpectedSerial, $ExpectedService, $PassStarted,
            $NormalState, $NormalSerial, $VerifierState, $VerifierSerial)
        $ErrorActionPreference = "Stop"

        function Resolve-GuestVerifierDriverStoreImage {
            param([string]$RawPath)
            if ([string]::IsNullOrWhiteSpace($RawPath)) {
                throw "Win32_SystemDriver.PathName is empty"
            }
            $candidate = $RawPath.Trim()
            if ($candidate.StartsWith('"')) {
                $closing = $candidate.IndexOf('"', 1)
                if ($closing -lt 1) {
                    throw "Win32_SystemDriver.PathName quote is malformed"
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
            if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
                throw "Win32_SystemDriver.PathName does not name an existing file"
            }
            $resolved = (Resolve-Path -LiteralPath $candidate -ErrorAction Stop).Path
            if ($resolved.IndexOf("\DriverStore\FileRepository\", [StringComparison]::OrdinalIgnoreCase) -lt 0) {
                throw "Win32_SystemDriver.PathName is not the DriverStore FileRepository image"
            }
            $resolved
        }

        $packages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
            })
        $publishedPackages = @($packages | Where-Object {
                ([IO.Path]::GetFileName([string]$_.Driver)).ToLowerInvariant() -ceq $ExpectedPublishedInf
            })
        if ($packages.Count -ne 1 -or $publishedPackages.Count -ne 1) {
            throw "DriverStore package binding is zero, multiple, or foreign"
        }
        $packageOriginalInf = [IO.Path]::GetFileName([string]$publishedPackages[0].OriginalFileName).ToLowerInvariant()
        if ($packageOriginalInf -cne "ramshared.inf") {
            throw "published DriverStore package has a foreign original INF"
        }

        $roots = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
            })
        if ($roots.Count -ne 1) {
            throw "ROOT RamShared device binding is zero or ambiguous"
        }
        $root = $roots[0]
        $hardwareProperties = @(Get-PnpDeviceProperty -InstanceId ([string]$root.InstanceId) `
                -KeyName "DEVPKEY_Device_HardwareIds" -ErrorAction Stop)
        if ($hardwareProperties.Count -ne 1) {
            throw "ROOT RamShared hardware ID provider result is ambiguous"
        }
        $hardwareIds = @($hardwareProperties[0].Data | ForEach-Object {
                ([string]$_).Trim().ToUpperInvariant()
            })
        if ($hardwareIds.Count -ne 1 -or $hardwareIds[0] -cne $ExpectedHardware.ToUpperInvariant()) {
            throw "ROOT RamShared hardware ID is foreign or ambiguous"
        }

        $services = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
        if ($services.Count -ne 1 -or [string]$services[0].Name -cne $ExpectedService -or
            [string]$services[0].State -cnotin @("Running", "Stopped")) {
            throw "RamShared service binding is zero, foreign, ambiguous, or not in an exact teardown state"
        }
        $loadedPath = Resolve-GuestVerifierDriverStoreImage -RawPath ([string]$services[0].PathName)
        $loadedHash = (Get-FileHash -LiteralPath $loadedPath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        $driverStoreDirectory = Split-Path -Parent $loadedPath
        $driverStoreInf = Join-Path $driverStoreDirectory "ramshared.inf"
        $driverStoreCatalog = Join-Path $driverStoreDirectory "ramshared.cat"
        foreach ($requiredPath in @($driverStoreInf, $driverStoreCatalog)) {
            if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
                throw "bound DriverStore package is missing an immutable package file"
            }
        }
        $driverStoreInfHash = (Get-FileHash -LiteralPath $driverStoreInf -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        $driverStoreCatalogHash = (Get-FileHash -LiteralPath $driverStoreCatalog -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        if ($loadedHash -cne $ExpectedSysHash -or $driverStoreInfHash -cne $ExpectedInfHash -or
            $driverStoreCatalogHash -cne $ExpectedCatHash -or
            ($NormalState -ceq "validated" -and $NormalSerial -cne $ExpectedSerial) -or
            ($VerifierState -ceq "validated" -and $VerifierSerial -cne $ExpectedSerial) -or
            ($PassStarted -and $NormalState -cne "validated" -and $VerifierState -cne "validated")) {
            throw "current-run package/service/serial binding does not match sealed identity"
        }
        [pscustomobject]@{
            schema = [int]1
            run_id = $CurrentRunId
            published_inf = $ExpectedPublishedInf
            package_count = [int]$packages.Count
            package_original_inf = $packageOriginalInf
            root_count = [int]$roots.Count
            root_instance_id = [string]$root.InstanceId
            hardware_id = [string]($hardwareIds[0])
            service_count = [int]$services.Count
            service_name = [string]$services[0].Name
            service_state = [string]$services[0].State
            service_path = $loadedPath
            loaded_driver_sha256 = $loadedHash
            driver_store_inf_sha256 = $driverStoreInfHash
            driver_store_catalog_sha256 = $driverStoreCatalogHash
            io_pass_started = [bool]$PassStarted
            normal_vpd_state = $NormalState
            normal_vpd_serial = $NormalSerial
            verifier_vpd_state = $VerifierState
            verifier_vpd_serial = $VerifierSerial
            binary_match = [bool](($loadedHash -ceq $ExpectedSysHash) -and
                ($driverStoreInfHash -ceq $ExpectedInfHash) -and
                ($driverStoreCatalogHash -ceq $ExpectedCatHash))
        } | ConvertTo-Json -Compress
    } -ArgumentList @($RunId, $expectedPublishedInfCanonical, $expectedDriverHashCanonical, $expectedInfHashCanonical,
        $expectedCatalogHashCanonical, $ExpectedHardwareId, $ExpectedVpdSerial, $ExpectedServiceName,
        $IoPassStarted, $normalVpdState, $NormalVpdSerial, $verifierVpdState, $VerifierVpdSerial)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "current-run teardown binding query"
}

function Test-GuestVerifierPostRootRemovalState {
    param([Parameter(Mandatory = $true)][object]$State)

    foreach ($propertyName in @(
            "root_count", "ramshared_disk_count", "ramshared_pnp_disk_count",
            "ramshared_present_pnp_disk_count", "ramshared_retired_pnp_disk_count")) {
        if ($null -eq $State.PSObject.Properties[$propertyName]) {
            return $false
        }
    }
    $totalPnp = [int]$State.ramshared_pnp_disk_count
    $retiredPnp = [int]$State.ramshared_retired_pnp_disk_count
    return [bool](
        [int]$State.root_count -eq 0 -and
        [int]$State.ramshared_disk_count -eq 0 -and
        [int]$State.ramshared_present_pnp_disk_count -eq 0 -and
        $totalPnp -ge 0 -and $totalPnp -le 1 -and
        $retiredPnp -eq $totalPnp)
}

function Assert-GuestVerifierPostRootRemovalState {
    param([Parameter(Mandatory = $true)][object]$State)

    if (-not (Test-GuestVerifierPostRootRemovalState -State $State)) {
        throw "post-ROOT-removal state has ROOT, active disk, or ambiguous retired PnP residue"
    }
    $State
}

function Assert-GuestVerifierRootRemovedState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$State,
        [Parameter(Mandatory = $true)][object]$Binding,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)][string]$ExpectedInfHash,
        [Parameter(Mandatory = $true)][string]$ExpectedCatalogHash,
        [switch]$RequireStopped
    )

    foreach ($propertyName in @(
            "schema", "run_id", "published_inf", "package_count", "published_inf_count",
            "package_original_inf", "driver_store_sys_sha256", "driver_store_inf_sha256",
            "driver_store_catalog_sha256", "root_count", "root_instance_id", "hardware_id",
            "service_count", "service_name", "service_state", "service_path", "service_sha256",
            "service_inf_sha256", "service_catalog_sha256", "ramshared_disk_count",
            "ramshared_pnp_disk_count", "ramshared_present_pnp_disk_count",
            "ramshared_retired_pnp_disk_count", "retired_pnp_instance_id")) {
        if ($null -eq $State.PSObject.Properties[$propertyName]) {
            throw "root-removed state is missing $propertyName"
        }
    }
    $serviceStateAllowed = if ($RequireStopped) {
        [string]$State.service_state -ceq "Stopped"
    }
    else {
        [string]$State.service_state -cin @("Running", "Stopped")
    }
    $pnpCount = [int]$State.ramshared_pnp_disk_count
    $retiredCount = [int]$State.ramshared_retired_pnp_disk_count
    $retiredId = [string]$State.retired_pnp_instance_id
    $retiredShapeExact = if ($pnpCount -eq 0) {
        [string]::IsNullOrEmpty($retiredId)
    }
    else {
        $retiredId -cmatch '^SCSI\\DISK&VEN_RAMSHARE&PROD_VRAMDISK\\[^\\]+$'
    }
    if ([int]$State.schema -ne 1 -or [string]$State.run_id -cne $RunId -or
        [string]$State.published_inf -cne [string]$Binding.published_inf -or
        [int]$State.package_count -ne 1 -or [int]$State.published_inf_count -ne 1 -or
        [string]$State.package_original_inf -cne "ramshared.inf" -or
        [string]$State.driver_store_sys_sha256 -cne $ExpectedDriverHash -or
        [string]$State.driver_store_inf_sha256 -cne $ExpectedInfHash -or
        [string]$State.driver_store_catalog_sha256 -cne $ExpectedCatalogHash -or
        [int]$State.root_count -ne 0 -or
        -not [string]::IsNullOrEmpty([string]$State.root_instance_id) -or
        -not [string]::IsNullOrEmpty([string]$State.hardware_id) -or
        [int]$State.service_count -ne 1 -or [string]$State.service_name -cne "ramshared" -or
        -not $serviceStateAllowed -or
        [string]$State.service_path -cne [string]$Binding.service_path -or
        [string]$State.service_sha256 -cne $ExpectedDriverHash -or
        [string]$State.service_inf_sha256 -cne $ExpectedInfHash -or
        [string]$State.service_catalog_sha256 -cne $ExpectedCatalogHash -or
        [int]$State.ramshared_disk_count -ne 0 -or
        [int]$State.ramshared_present_pnp_disk_count -ne 0 -or
        $pnpCount -lt 0 -or $pnpCount -gt 1 -or $retiredCount -ne $pnpCount -or
        -not $retiredShapeExact) {
        throw "root-removed state is foreign, active, ambiguous, or hash-mismatched"
    }
    $State
}

function Remove-GuestVerifierCurrentRunRoot {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$Binding,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)][string]$ExpectedInfHash,
        [Parameter(Mandatory = $true)][string]$ExpectedCatalogHash,
        [Parameter(Mandatory = $true)][string]$ExpectedHardwareId,
        [Parameter(Mandatory = $true)][string]$ExpectedVpdSerial,
        [Parameter(Mandatory = $true)][string]$ExpectedServiceName
    )

    $publishedInf = Normalize-GuestVerifierPublishedInf ([string]$Binding.published_inf) "root-removal published INF"
    $rootInstanceId = [string]$Binding.root_instance_id
    Assert-GuestVerifierCurrentRunTeardownBinding -Binding $Binding -RunId $RunId `
        -ExpectedPublishedInf $publishedInf -ExpectedDriverHash $ExpectedDriverHash `
        -ExpectedInfHash $ExpectedInfHash -ExpectedCatalogHash $ExpectedCatalogHash `
        -ExpectedHardwareId $ExpectedHardwareId -ExpectedVpdSerial $ExpectedVpdSerial `
        -ExpectedServiceName $ExpectedServiceName | Out-Null
    $before = Get-GuestVerifierPostPublishCleanupState -RunId $RunId -ExpectedPublishedInf $publishedInf
    if ((Get-GuestVerifierPostPublishCleanupMode -State $before -RunId $RunId `
            -ExpectedPublishedInf $publishedInf -ExpectedDriverHash $ExpectedDriverHash `
            -ExpectedInfHash $ExpectedInfHash -ExpectedCatalogHash $ExpectedCatalogHash) -cne "root_bound" -or
        [string]$before.root_instance_id -cne $rootInstanceId -or
        [string]$before.service_path -cne [string]$Binding.service_path) {
        throw "root-removal live identity drifted from the sealed binding"
    }
    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 120 -ScriptBlock {
        param($ExpectedRootInstanceId)
        $ErrorActionPreference = "Stop"
        $roots = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
            })
        if ($roots.Count -ne 1 -or [string]$roots[0].InstanceId -cne $ExpectedRootInstanceId) {
            throw "exact current-run ROOT identity drifted before removal"
        }
        $removeOutput = & pnputil.exe /remove-device $ExpectedRootInstanceId 2>&1 | Out-String
        $removeExit = [int]$LASTEXITCODE
        if ($removeExit -ne 0) {
            throw "exact current-run ROOT removal failed exit=$removeExit"
        }
        [pscustomobject]@{
            schema = [int]1
            action = "remove_exact_current_run_root"
            root_instance_id = $ExpectedRootInstanceId
            device_remove_exit_code = $removeExit
        } | ConvertTo-Json -Compress
    } -ArgumentList @($rootInstanceId)
    [pscustomobject]@{
        schema = [int]1
        run_id = $RunId
        published_inf = $publishedInf
        before = $before
        action = ConvertFrom-GuestVerifierRows -Rows $rows -Operation "exact current-run ROOT removal"
    }
}

function Remove-GuestVerifierRootRemovedArtifacts {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$Binding,
        [Parameter(Mandatory = $true)][object]$RootRemovedState,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)][string]$ExpectedInfHash,
        [Parameter(Mandatory = $true)][string]$ExpectedCatalogHash
    )

    $publishedInf = Normalize-GuestVerifierPublishedInf ([string]$Binding.published_inf) "root-removed published INF"
    Assert-GuestVerifierRootRemovedState -State $RootRemovedState -Binding $Binding -RunId $RunId `
        -ExpectedDriverHash $ExpectedDriverHash -ExpectedInfHash $ExpectedInfHash `
        -ExpectedCatalogHash $ExpectedCatalogHash -RequireStopped | Out-Null
    $retiredId = [string]$RootRemovedState.retired_pnp_instance_id
    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        param($ExpectedPublishedInf, $ExpectedServiceName, $ExpectedServicePath,
            $ExpectedSysHash, $ExpectedInfFileHash, $ExpectedCatHash, $ExpectedRetiredInstanceId)
        $ErrorActionPreference = "Stop"
        $packages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
            })
        $publishedPackages = @($packages | Where-Object {
                ([IO.Path]::GetFileName([string]$_.Driver)).ToLowerInvariant() -ceq $ExpectedPublishedInf
            })
        $roots = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
            })
        $services = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
        if ($packages.Count -ne 1 -or $publishedPackages.Count -ne 1 -or $roots.Count -ne 0 -or
            $services.Count -ne 1 -or [string]$services[0].Name -cne $ExpectedServiceName -or
            [string]$services[0].State -cne "Stopped") {
            throw "root-removed continuation identity is absent, active, or ambiguous"
        }
        $servicePath = ([string]$services[0].PathName).Trim().Trim('"')
        if ($servicePath -match '(?i)^\\SystemRoot\\') {
            $servicePath = Join-Path $env:SystemRoot $servicePath.Substring(12)
        }
        if ($servicePath -match '(?i)^\\\?\?\\') { $servicePath = $servicePath.Substring(4) }
        $servicePath = (Resolve-Path -LiteralPath $servicePath -ErrorAction Stop).Path
        $packageInf = [string]$publishedPackages[0].OriginalFileName
        $packageDirectory = Split-Path -Parent $packageInf
        $packageSys = Join-Path $packageDirectory "ramshared.sys"
        $packageCat = Join-Path $packageDirectory "ramshared.cat"
        if (-not $servicePath.Equals($ExpectedServicePath, [StringComparison]::OrdinalIgnoreCase) -or
            (Get-FileHash -LiteralPath $packageSys -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant() -cne $ExpectedSysHash -or
            (Get-FileHash -LiteralPath $packageInf -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant() -cne $ExpectedInfFileHash -or
            (Get-FileHash -LiteralPath $packageCat -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant() -cne $ExpectedCatHash) {
            throw "root-removed continuation hashes or service path drifted"
        }
        $pnpDisks = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
            })
        if ([string]::IsNullOrEmpty($ExpectedRetiredInstanceId)) {
            if ($pnpDisks.Count -ne 0) { throw "unexpected retired node appeared before continuation" }
        }
        elseif ($pnpDisks.Count -ne 1 -or
            [string]$pnpDisks[0].InstanceId -cne $ExpectedRetiredInstanceId -or
            [bool]$pnpDisks[0].Present -or [string]$pnpDisks[0].Status -cne "Unknown" -or
            [int]$pnpDisks[0].Problem -ne 45) {
            throw "retired node drifted before continuation"
        }
        $null = & sc.exe delete $ExpectedServiceName 2>&1 | Out-String
        $serviceDeleteExit = [int]$LASTEXITCODE
        if ($serviceDeleteExit -ne 0) { throw "exact service deletion failed exit=$serviceDeleteExit" }
        $serviceDeadline = [DateTime]::UtcNow.AddSeconds(60)
        do {
            $remainingServices = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
            if ($remainingServices.Count -eq 0) { break }
            Start-Sleep -Seconds 2
        } while ([DateTime]::UtcNow -lt $serviceDeadline)
        if ($remainingServices.Count -ne 0) { throw "exact service deletion did not reach zero" }
        $null = & pnputil.exe /delete-driver $ExpectedPublishedInf /uninstall 2>&1 | Out-String
        $driverDeleteExit = [int]$LASTEXITCODE
        if ($driverDeleteExit -ne 0) { throw "exact package deletion failed exit=$driverDeleteExit" }
        $retiredDeleteExit = [int]0
        $retiredDeleteAction = "not_present"
        if (-not [string]::IsNullOrEmpty($ExpectedRetiredInstanceId)) {
            $null = & pnputil.exe /remove-device $ExpectedRetiredInstanceId 2>&1 | Out-String
            $retiredDeleteExit = [int]$LASTEXITCODE
            if ($retiredDeleteExit -ne 0) { throw "exact retired-node deletion failed exit=$retiredDeleteExit" }
            $retiredDeleteAction = "deleted"
        }
        [pscustomobject]@{
            schema = [int]1
            service_delete_exit_code = $serviceDeleteExit
            service_delete_action = "deleted"
            driver_delete_exit_code = $driverDeleteExit
            retired_node_delete_exit_code = $retiredDeleteExit
            retired_node_delete_action = $retiredDeleteAction
            retired_node_instance_id = $ExpectedRetiredInstanceId
        } | ConvertTo-Json -Compress
    } -ArgumentList @($publishedInf, "ramshared", [string]$Binding.service_path,
        $ExpectedDriverHash, $ExpectedInfHash, $ExpectedCatalogHash, $retiredId)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "root-removed current-run teardown"
}

function Remove-GuestVerifierCurrentRunArtifacts {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Binding,
        [Parameter(Mandatory = $true)]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedInfHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedCatalogHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedHardwareId,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedVpdSerial,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedServiceName
    )

    $ExpectedPublishedInf = Normalize-GuestVerifierPublishedInf ([string]$Binding.published_inf) "current-run published INF"
    $ExpectedRootInstanceId = [string]$Binding.root_instance_id
    Assert-GuestVerifierCurrentRunTeardownBinding -Binding $Binding -RunId $RunId `
        -ExpectedPublishedInf $ExpectedPublishedInf -ExpectedDriverHash $ExpectedDriverHash `
        -ExpectedInfHash $ExpectedInfHash -ExpectedCatalogHash $ExpectedCatalogHash `
        -ExpectedHardwareId $ExpectedHardwareId -ExpectedVpdSerial $ExpectedVpdSerial `
        -ExpectedServiceName $ExpectedServiceName | Out-Null

    $expectedDriverHashCanonical = Normalize-GuestVerifierSha256 $ExpectedDriverHash "expected SYS SHA-256"
    $expectedInfHashCanonical = Normalize-GuestVerifierSha256 $ExpectedInfHash "expected INF SHA-256"
    $expectedCatalogHashCanonical = Normalize-GuestVerifierSha256 $ExpectedCatalogHash "expected CAT SHA-256"
    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 900 -ScriptBlock {
        param($CurrentRunId, $ExpectedPublishedInf, $ExpectedRootInstanceId, $ExpectedServiceName,
            $ExpectedSysHash, $ExpectedInfHash, $ExpectedCatHash, $ExpectedHardwareId)
        $ErrorActionPreference = "Stop"

        function Resolve-GuestVerifierDriverStoreImage {
            param([string]$RawPath)
            if ([string]::IsNullOrWhiteSpace($RawPath)) {
                throw "Win32_SystemDriver.PathName is empty"
            }
            $candidate = $RawPath.Trim()
            if ($candidate.StartsWith('"')) {
                $closing = $candidate.IndexOf('"', 1)
                if ($closing -lt 1) {
                    throw "Win32_SystemDriver.PathName quote is malformed"
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
            if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
                throw "Win32_SystemDriver.PathName does not name an existing file"
            }
            $resolved = (Resolve-Path -LiteralPath $candidate -ErrorAction Stop).Path
            if ($resolved.IndexOf("\DriverStore\FileRepository\", [StringComparison]::OrdinalIgnoreCase) -lt 0) {
                throw "Win32_SystemDriver.PathName is not the DriverStore FileRepository image"
            }
            $resolved
        }

        function Get-GuestVerifierCurrentRunState {
            param([bool]$IncludeDriverStore)
            $packages = @()
            $publishedPackages = @()
            if ($IncludeDriverStore) {
                $packages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                        [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
                    })
                $publishedPackages = @($packages | Where-Object {
                        ([IO.Path]::GetFileName([string]$_.Driver)).ToLowerInvariant() -ceq $ExpectedPublishedInf
                    })
            }
            $roots = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                    $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
                })
            $services = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
            $ramsharedDisks = @(Get-Disk -ErrorAction Stop | Where-Object {
                    $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                    $_.SerialNumber -match '(?i)ramshare|ramshared'
                })
            $ramsharedPnpDisks = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                    $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                    $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
                })
            $retiredRamsharedPnpDisks = @($ramsharedPnpDisks | Where-Object {
                    ([bool]$_.Present -eq $false) -and
                    [string]$_.Status -ceq "Unknown" -and [int]$_.Problem -eq 45
                })
            $hardwareId = ""
            if ($roots.Count -eq 1) {
                $hardwareProperties = @(Get-PnpDeviceProperty -InstanceId ([string]$roots[0].InstanceId) `
                        -KeyName "DEVPKEY_Device_HardwareIds" -ErrorAction Stop)
                if ($hardwareProperties.Count -ne 1) {
                    throw "ROOT RamShared hardware ID provider result is ambiguous"
                }
                $hardwareIds = @($hardwareProperties[0].Data | ForEach-Object {
                        ([string]$_).Trim().ToUpperInvariant()
                    })
                if ($hardwareIds.Count -ne 1) {
                    throw "ROOT RamShared hardware ID is ambiguous"
                }
                $hardwareId = [string]($hardwareIds[0])
            }
            $servicePath = ""
            $serviceHash = ""
            $serviceInfHash = ""
            $serviceCatalogHash = ""
            if ($services.Count -eq 1) {
                $servicePath = Resolve-GuestVerifierDriverStoreImage -RawPath ([string]$services[0].PathName)
                $serviceHash = (Get-FileHash -LiteralPath $servicePath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
                $serviceDirectory = Split-Path -Parent $servicePath
                $serviceInf = Join-Path $serviceDirectory "ramshared.inf"
                $serviceCatalog = Join-Path $serviceDirectory "ramshared.cat"
                foreach ($requiredPath in @($serviceInf, $serviceCatalog)) {
                    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
                        throw "bound DriverStore package is missing an immutable package file"
                    }
                }
                $serviceInfHash = (Get-FileHash -LiteralPath $serviceInf -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
                $serviceCatalogHash = (Get-FileHash -LiteralPath $serviceCatalog -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
            }
            [pscustomobject]@{
                driver_store_observed = [bool]$IncludeDriverStore
                package_count = if ($IncludeDriverStore) { [int]$packages.Count } else { [int]-1 }
                published_inf_count = if ($IncludeDriverStore) { [int]$publishedPackages.Count } else { [int]-1 }
                root_count = [int]$roots.Count
                root_instance_id = if ($roots.Count -eq 1) { [string]$roots[0].InstanceId } else { "" }
                hardware_id = $hardwareId
                service_count = [int]$services.Count
                service_name = if ($services.Count -eq 1) { [string]$services[0].Name } else { "" }
                service_state = if ($services.Count -eq 1) { [string]$services[0].State } else { "" }
                service_path = $servicePath
                service_sha256 = $serviceHash
                service_inf_sha256 = $serviceInfHash
                service_catalog_sha256 = $serviceCatalogHash
                ramshared_disk_count = [int]$ramsharedDisks.Count
                ramshared_pnp_disk_count = [int]$ramsharedPnpDisks.Count
                ramshared_present_pnp_disk_count = [int]($ramsharedPnpDisks.Count - $retiredRamsharedPnpDisks.Count)
                ramshared_retired_pnp_disk_count = [int]$retiredRamsharedPnpDisks.Count
                retired_pnp_instance_id = if ($retiredRamsharedPnpDisks.Count -eq 1) {
                    [string]$retiredRamsharedPnpDisks[0].InstanceId
                } else { "" }
            }
        }

        function Test-GuestVerifierPostRootRemovalState {
            param([object]$State)
            $totalPnp = [int]$State.ramshared_pnp_disk_count
            $retiredPnp = [int]$State.ramshared_retired_pnp_disk_count
            [bool]([int]$State.root_count -eq 0 -and
                [int]$State.ramshared_disk_count -eq 0 -and
                [int]$State.ramshared_present_pnp_disk_count -eq 0 -and
                $totalPnp -ge 0 -and $totalPnp -le 1 -and $retiredPnp -eq $totalPnp)
        }

        function Assert-GuestVerifierPostRootRemovalState {
            param([object]$State)
            if (-not (Test-GuestVerifierPostRootRemovalState -State $State)) {
                throw "post-ROOT-removal state has ROOT, active disk, or ambiguous retired PnP residue"
            }
        }

        function Assert-GuestVerifierExactCurrentRunState {
            param([object]$State)
            if (($State.driver_store_observed -isnot [bool]) -or
                ([bool]$State.driver_store_observed -ne $true) -or
                [int]$State.package_count -ne 1 -or [int]$State.published_inf_count -ne 1 -or
                [int]$State.root_count -ne 1 -or [string]$State.root_instance_id -cne $ExpectedRootInstanceId -or
                [string]$State.hardware_id -cne $ExpectedHardwareId -or [int]$State.service_count -ne 1 -or
                [string]$State.service_name -cne $ExpectedServiceName -or
                [string]$State.service_state -cnotin @("Running", "Stopped") -or
                [string]$State.service_sha256 -cne $ExpectedSysHash -or
                [string]$State.service_inf_sha256 -cne $ExpectedInfHash -or
                [string]$State.service_catalog_sha256 -cne $ExpectedCatHash -or
                [int]$State.ramshared_disk_count -ne 0 -or
                [int]$State.ramshared_present_pnp_disk_count -ne 0 -or
                [int]$State.ramshared_pnp_disk_count -gt 1 -or
                [int]$State.ramshared_retired_pnp_disk_count -ne [int]$State.ramshared_pnp_disk_count) {
                throw "current-run teardown precondition is zero, multiple, foreign, or has LUN residue"
            }
        }

        $before = Get-GuestVerifierCurrentRunState -IncludeDriverStore $true
        Assert-GuestVerifierExactCurrentRunState -State $before
        $serviceStopExit = [int]0
        if ([string]$before.service_state -ceq "Running") {
            $serviceStopAction = "deferred_to_root_removal"
        }
        else {
            $serviceStopAction = "already_stopped"
        }

        $removeDeviceOutput = & pnputil.exe /remove-device $ExpectedRootInstanceId 2>&1 | Out-String
        $deviceRemoveExit = [int]$LASTEXITCODE
        if ($deviceRemoveExit -ne 0) {
            throw "exact current-run PnP removal failed exit=$deviceRemoveExit"
        }
        $deviceRemovalDeadline = (Get-Date).ToUniversalTime().AddSeconds(60)
        do {
            $afterDeviceRemoval = Get-GuestVerifierCurrentRunState -IncludeDriverStore $false
            if (Test-GuestVerifierPostRootRemovalState -State $afterDeviceRemoval) {
                break
            }
            Start-Sleep -Seconds 2
        } while ((Get-Date).ToUniversalTime() -lt $deviceRemovalDeadline)
        Assert-GuestVerifierPostRootRemovalState -State $afterDeviceRemoval | Out-Null

        $serviceDeleteAction = ""
        $serviceDeleteExit = [int]0
        if ([int]$afterDeviceRemoval.service_count -eq 1) {
            if ([string]$afterDeviceRemoval.service_name -cne $ExpectedServiceName -or
                [string]$afterDeviceRemoval.service_state -cne "Stopped" -or
                [string]$afterDeviceRemoval.service_sha256 -cne $ExpectedSysHash -or
                [string]$afterDeviceRemoval.service_inf_sha256 -cne $ExpectedInfHash -or
                [string]$afterDeviceRemoval.service_catalog_sha256 -cne $ExpectedCatHash) {
                throw "remaining current-run service is foreign or does not match sealed hashes"
            }
            $deleteServiceOutput = & sc.exe delete $ExpectedServiceName 2>&1 | Out-String
            $serviceDeleteExit = [int]$LASTEXITCODE
            if ($serviceDeleteExit -ne 0) {
                throw "exact current-run service deletion failed exit=$serviceDeleteExit"
            }
            $serviceDeleteAction = "deleted"
        }
        elseif ([int]$afterDeviceRemoval.service_count -eq 0) {
            $serviceDeleteAction = "already_removed_by_device"
        }
        else {
            throw "remaining current-run service is ambiguous"
        }

        $serviceDeletionDeadline = (Get-Date).ToUniversalTime().AddSeconds(60)
        do {
            $remainingServices = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
            if ($remainingServices.Count -eq 0) {
                break
            }
            if ($remainingServices.Count -ne 1 -or [string]$remainingServices[0].Name -cne $ExpectedServiceName) {
                throw "current-run service deletion became ambiguous"
            }
            Start-Sleep -Seconds 2
        } while ((Get-Date).ToUniversalTime() -lt $serviceDeletionDeadline)
        if ($remainingServices.Count -ne 0) {
            throw "exact current-run service deletion did not reach zero before bounded deadline"
        }

        $deleteDriverOutput = & pnputil.exe /delete-driver $ExpectedPublishedInf /uninstall 2>&1 | Out-String
        $driverDeleteExit = [int]$LASTEXITCODE
        if ($driverDeleteExit -ne 0) {
            throw "exact current-run DriverStore deletion failed exit=$driverDeleteExit"
        }
        $finalState = Get-GuestVerifierCurrentRunState -IncludeDriverStore $true
        $retiredNodeDeleteExit = [int]0
        $retiredNodeDeleteAction = "not_present"
        $retiredNodeInstanceId = ""
        $baseStateIsZero = [bool]([int]$finalState.package_count -eq 0 -and
            [int]$finalState.published_inf_count -eq 0 -and [int]$finalState.root_count -eq 0 -and
            [int]$finalState.service_count -eq 0 -and [int]$finalState.ramshared_disk_count -eq 0 -and
            [int]$finalState.ramshared_present_pnp_disk_count -eq 0)
        if ($baseStateIsZero -and [int]$finalState.ramshared_pnp_disk_count -eq 1 -and
            [int]$finalState.ramshared_retired_pnp_disk_count -eq 1 -and
            [string]$finalState.retired_pnp_instance_id -cmatch '^SCSI\\DISK&VEN_RAMSHARE&PROD_VRAMDISK\\[^\\]+$') {
            $retiredInstanceId = [string]$finalState.retired_pnp_instance_id
            $null = & pnputil.exe /remove-device $retiredInstanceId 2>&1 | Out-String
            $retiredNodeDeleteExit = [int]$LASTEXITCODE
            if ($retiredNodeDeleteExit -ne 0) {
                throw "exact retired-node deletion failed exit=$retiredNodeDeleteExit"
            }
            $retiredNodeDeadline = (Get-Date).ToUniversalTime().AddSeconds(60)
            do {
                $remainingPnpDisks = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                        $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                        $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
                    })
                if ($remainingPnpDisks.Count -eq 0) { break }
                if ($remainingPnpDisks.Count -ne 1 -or
                    [string]$remainingPnpDisks[0].InstanceId -cne $retiredInstanceId) {
                    throw "retired-node deletion became ambiguous"
                }
                Start-Sleep -Seconds 2
            } while ((Get-Date).ToUniversalTime() -lt $retiredNodeDeadline)
            if ($remainingPnpDisks.Count -ne 0) {
                throw "exact retired-node deletion did not reach zero"
            }
            $retiredNodeDeleteAction = "deleted"
            $retiredNodeInstanceId = $retiredInstanceId
            $finalState.ramshared_pnp_disk_count = [int]0
            $finalState.ramshared_present_pnp_disk_count = [int]0
            $finalState.ramshared_retired_pnp_disk_count = [int]0
            $finalState.retired_pnp_instance_id = ""
        }
        if ([int]$finalState.package_count -ne 0 -or [int]$finalState.published_inf_count -ne 0 -or
            [int]$finalState.root_count -ne 0 -or [int]$finalState.service_count -ne 0 -or
            [int]$finalState.ramshared_disk_count -ne 0 -or [int]$finalState.ramshared_pnp_disk_count -ne 0) {
            throw "exact current-run teardown did not prove zero residue before bounded deadline"
        }
        [pscustomobject]@{
            schema = [int]1
            run_id = $CurrentRunId
            published_inf = $ExpectedPublishedInf
            service_stop_exit_code = $serviceStopExit
            service_stop_action = $serviceStopAction
            device_remove_exit_code = $deviceRemoveExit
            service_delete_exit_code = $serviceDeleteExit
            service_delete_action = $serviceDeleteAction
            driver_delete_exit_code = $driverDeleteExit
            retired_node_delete_exit_code = $retiredNodeDeleteExit
            retired_node_delete_action = $retiredNodeDeleteAction
            retired_node_instance_id = $retiredNodeInstanceId
            package_count = [int]$finalState.package_count
            published_inf_count = [int]$finalState.published_inf_count
            root_count = [int]$finalState.root_count
            service_count = [int]$finalState.service_count
            ramshared_disk_count = [int]$finalState.ramshared_disk_count
            ramshared_pnp_disk_count = [int]$finalState.ramshared_pnp_disk_count
        } | ConvertTo-Json -Compress
    } -ArgumentList @($RunId, $ExpectedPublishedInf, $ExpectedRootInstanceId, $ExpectedServiceName,
        $expectedDriverHashCanonical, $expectedInfHashCanonical, $expectedCatalogHashCanonical, $ExpectedHardwareId)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "exact current-run guest teardown"
}

function Get-GuestVerifierCurrentRunZeroResidueEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [string]$PublishedInf
    )

    $expectedPublishedInfCanonical = Normalize-GuestVerifierPublishedInf $PublishedInf "published INF for final zero-residue query"
    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        param($CurrentRunId, $ExpectedPublishedInf)
        $ErrorActionPreference = "Stop"
        $packages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
            })
        $publishedPackages = @($packages | Where-Object {
                ([IO.Path]::GetFileName([string]$_.Driver)).ToLowerInvariant() -ceq $ExpectedPublishedInf
            })
        $roots = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
            })
        $services = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
        $ramsharedDisks = @(Get-Disk -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.SerialNumber -match '(?i)ramshare|ramshared'
            })
        $ramsharedPnpDisks = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
            })
        [pscustomobject]@{
            schema = [int]1
            run_id = $CurrentRunId
            published_inf = $ExpectedPublishedInf
            package_count = [int]$packages.Count
            published_inf_count = [int]$publishedPackages.Count
            root_count = [int]$roots.Count
            service_count = [int]$services.Count
            ramshared_disk_count = [int]$ramsharedDisks.Count
            ramshared_pnp_disk_count = [int]$ramsharedPnpDisks.Count
        } | ConvertTo-Json -Compress
    } -ArgumentList @($RunId, $expectedPublishedInfCanonical)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "final current-run zero-residue query"
}

function Get-GuestVerifierCleanupEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunId
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 60 -ScriptBlock {
        param($CurrentRunId)
        $ramsharedDisks = @(Get-Disk -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.SerialNumber -match '(?i)ramshare|ramshared'
            })
        $ramsharedPnpDisks = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
            })
        [pscustomobject]@{
            schema = [int]1
            run_id = $CurrentRunId
            cleanup = [pscustomobject]@{
                ramshared_disks = [int]$ramsharedDisks.Count
                ramshared_pnp_disks = [int]$ramsharedPnpDisks.Count
            }
        } | ConvertTo-Json -Compress
    } -ArgumentList @($RunId)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "zero-LUN cleanup query"
}

function Assert-GuestVerifierCleanupEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence,
        [Parameter(Mandatory = $true)]
        [string]$RunId
    )

    if ([int]$Evidence.schema -ne 1 -or [string]$Evidence.run_id -cne $RunId -or
        $null -eq $Evidence.cleanup -or
        [int]$Evidence.cleanup.ramshared_disks -ne 0 -or
        [int]$Evidence.cleanup.ramshared_pnp_disks -ne 0) {
        throw "exact zero-LUN cleanup evidence is missing or nonzero"
    }
    $true
}

function Wait-GuestVerifierResidueProviderZero {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet("disk", "pnp_disk")]
        [string]$ProviderCode
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 240 -ScriptBlock {
        param($CurrentProviderCode)
        $ErrorActionPreference = "Stop"
        $startedUtc = (Get-Date).ToUniversalTime()
        $deadlineUtc = $startedUtc.AddSeconds(60)
        $attempts = [int]0
        $count = [int]-1
        $totalCount = [int]-1
        $retiredCount = [int]0
        do {
            $attempts++
            if ($CurrentProviderCode -ceq "disk") {
                $items = @(Get-Disk -ErrorAction Stop | Where-Object {
                        $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                        $_.SerialNumber -match '(?i)ramshare|ramshared'
                    })
                $totalCount = [int]$items.Count
                $retiredCount = [int]0
                $count = $totalCount
            }
            elseif ($CurrentProviderCode -ceq "pnp_disk") {
                $items = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                        $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                        $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
                    })
                $retired = @($items | Where-Object {
                        ([bool]$_.Present -eq $false) -and
                        [string]$_.Status -ceq "Unknown" -and [int]$_.Problem -eq 45
                    })
                $totalCount = [int]$items.Count
                $retiredCount = [int]$retired.Count
                $count = [int]($totalCount - $retiredCount)
            }
            else {
                throw "residue provider code is invalid"
            }
            if ($count -eq 0) { break }
            if ((Get-Date).ToUniversalTime() -ge $deadlineUtc) { break }
            Start-Sleep -Seconds 1
        } while ($true)
        $completedUtc = (Get-Date).ToUniversalTime()
        [pscustomobject]@{
            schema = [int]1
            provider_code = $CurrentProviderCode
            status = if ($count -eq 0) { "zero" } else { "timeout" }
            terminal_count = $count
            observed_total_count = $totalCount
            retired_count = $retiredCount
            attempts = $attempts
            duration_ms = [int64][Math]::Round(($completedUtc - $startedUtc).TotalMilliseconds)
        } | ConvertTo-Json -Compress
    } -ArgumentList @($ProviderCode)
    $evidence = ConvertFrom-GuestVerifierRows -Rows $rows -Operation "$ProviderCode zero-residue wait"
    $evidence
}

function Invoke-GuestVerifierIoctlPass {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [string]$GuestStage,
        [Parameter(Mandatory = $true)]
        [string]$PassName,
        [Parameter(Mandatory = $true)]
        [bool]$VerifierExpected,
        [Parameter(Mandatory = $true)]
        [ValidateNotNullOrEmpty()]
        [string]$ExpectedVpdSerial
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 600 -ScriptBlock {
        param($CurrentRunId, $Stage, $CurrentPassName, $VerifierOn, $ExpectedSerial)
        $ErrorActionPreference = "Stop"
        $passDirectory = Join-Path $Stage ("ioctl-" + $CurrentPassName)
        if (Test-Path -LiteralPath $passDirectory) {
            throw "current-run IOCTL artifact directory already exists"
        }
        New-Item -ItemType Directory -Path $passDirectory -ErrorAction Stop | Out-Null
        $windowsDirectory = "C:\Windows"
        $dumpDirectory = Join-Path $windowsDirectory "Minidump"
        $beforeDumpNames = @()
        $dumpBeforeState = "error"
        $dumpObservationError = ""
        try {
            if (-not (Test-Path -LiteralPath $windowsDirectory -PathType Container -ErrorAction Stop)) {
                throw "C:\Windows parent is unavailable"
            }
            if (Test-Path -LiteralPath $dumpDirectory -PathType Leaf -ErrorAction Stop) {
                $dumpBeforeState = "non_directory"
                throw "C:\Windows\Minidump is not a directory"
            }
            if (Test-Path -LiteralPath $dumpDirectory -PathType Container -ErrorAction Stop) {
                $dumpBeforeState = "present"
                $beforeDumpNames = @(Get-ChildItem -LiteralPath $dumpDirectory -Filter "*.dmp" -File -ErrorAction Stop |
                    ForEach-Object { $_.Name })
            }
            else {
                $dumpBeforeState = "absent"
            }
        }
        catch {
            $dumpObservationError = $_.Exception.Message
        }
        $startUtc = (Get-Date).ToUniversalTime()
        $validator = Join-Path $Stage "Invoke-WinDriveIoctlValidation.ps1"
        $validatorArguments = @(
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            $validator,
            "-ArtifactDir",
            $passDirectory
        )
        if ($VerifierOn) {
            $validatorArguments += "-Verifier"
        }
        $consoleLines = @(& powershell.exe @validatorArguments 2>&1 | ForEach-Object { [string]$_ })
        $processExit = [int]$LASTEXITCODE
        $endUtc = (Get-Date).ToUniversalTime()
        $statusLines = @($consoleLines | Where-Object { $_ -match '^STATUS=' })
        $status = if ($statusLines.Count -eq 1) { [string]$statusLines[0].Substring(7) } else { "" }
        $vpdSerials = @()
        foreach ($consoleLine in $consoleLines) {
            if ($consoleLine -match '(?i)\bVPD_SERIAL_MATCH=1\b.*\bSerial=(?<serial>[A-Za-z0-9]+)\b') {
                $vpdSerials += [string]$Matches["serial"]
            }
        }
        $vpdSerialObservationError = ""
        $vpdSerial = ""
        if ($vpdSerials.Count -ne 1) {
            $vpdSerialObservationError = "expected exactly one VPD_SERIAL_MATCH console observation, found $($vpdSerials.Count)"
        }
        else {
            $vpdSerial = [string]$vpdSerials[0]
            if ($vpdSerial -cne $ExpectedSerial) {
                $vpdSerialObservationError = "VPD serial does not match the sealed current-run serial"
            }
        }
        $verdict = $null
        $verdictError = ""
        $verdictFiles = @(Get-ChildItem -LiteralPath $passDirectory -Filter "verdict-*.json" -File -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTimeUtc -ge $startUtc })
        if ($verdictFiles.Count -eq 1) {
            try {
                $verdict = Get-Content -LiteralPath $verdictFiles[0].FullName -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
            }
            catch {
                $verdictError = $_.Exception.Message
            }
        }
        else {
            $verdictError = "expected exactly one current-interval verdict, found $($verdictFiles.Count)"
        }
        $event153 = @()
        $eventError = ""
        try {
            $event153 = @(Get-WinEvent -FilterHashtable @{
                    LogName = "System"
                    ProviderName = "disk"
                    Id = 153
                    StartTime = $startUtc
                    EndTime = $endUtc
                } -ErrorAction Stop)
        }
        catch {
            if ($_.FullyQualifiedErrorId -notmatch "NoMatchingEventsFound") {
                $eventError = $_.Exception.Message
            }
        }
        $afterDumpNames = @()
        $dumpAfterState = "error"
        if ([string]::IsNullOrEmpty($dumpObservationError)) {
            try {
                if (-not (Test-Path -LiteralPath $windowsDirectory -PathType Container -ErrorAction Stop)) {
                    throw "C:\Windows parent is unavailable after the pass"
                }
                if (Test-Path -LiteralPath $dumpDirectory -PathType Leaf -ErrorAction Stop) {
                    $dumpAfterState = "non_directory"
                    throw "C:\Windows\Minidump became a non-directory object"
                }
                if (Test-Path -LiteralPath $dumpDirectory -PathType Container -ErrorAction Stop) {
                    $dumpAfterState = "present"
                    $afterDumpNames = @(Get-ChildItem -LiteralPath $dumpDirectory -Filter "*.dmp" -File -ErrorAction Stop |
                        ForEach-Object { $_.Name })
                }
                else {
                    $dumpAfterState = "absent"
                    if ($dumpBeforeState -ceq "present") {
                        throw "C:\Windows\Minidump disappeared during the pass"
                    }
                }
            }
            catch {
                $dumpObservationError = $_.Exception.Message
            }
        }
        $newDumps = @($afterDumpNames | Where-Object { $beforeDumpNames -notcontains $_ })
        [pscustomobject]@{
            schema = [int]1
            run_id = $CurrentRunId
            pass_name = $CurrentPassName
            verifier_expected = [bool]$VerifierOn
            status = $status
            exit_code = $processExit
            start_utc = $startUtc.ToString("o")
            end_utc = $endUtc.ToString("o")
            console = @($consoleLines)
            verdict = $verdict
            verdict_error = $verdictError
            vpd_serial = $vpdSerial
            vpd_serial_observation_error = $vpdSerialObservationError
            event153_count = [int]$event153.Count
            event153_error = $eventError
            new_dump_count = [int]$newDumps.Count
            new_dumps = @($newDumps)
            dump_observation_error = $dumpObservationError
            dump_before_state = $dumpBeforeState
            dump_after_state = $dumpAfterState
        } | ConvertTo-Json -Depth 12 -Compress
    } -ArgumentList @($RunId, $GuestStage, $PassName, $VerifierExpected, $ExpectedVpdSerial)
    $evidence = ConvertFrom-GuestVerifierRows -Rows $rows -Operation "$PassName IOCTL pass"
    $diskWait = Wait-GuestVerifierResidueProviderZero -ProviderCode "disk"
    $pnpDiskWait = Wait-GuestVerifierResidueProviderZero -ProviderCode "pnp_disk"
    $evidence | Add-Member -NotePropertyName cleanup -NotePropertyValue ([pscustomobject]@{
            ramshared_disks = [int]$diskWait.terminal_count
            ramshared_pnp_disks = [int]$pnpDiskWait.terminal_count
            ramshared_retired_pnp_disks = [int]$pnpDiskWait.retired_count
            disk_wait = $diskWait
            pnp_disk_wait = $pnpDiskWait
        }) -Force
    $evidence
}

function Enable-GuestVerifier {
    [CmdletBinding()]
    param()

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 90 -ScriptBlock {
        $ErrorActionPreference = "Stop"
        function Get-GuestVerifierTargetLines {
            param([Parameter(Mandatory = $true)][string]$Text)

            @([regex]::Split($Text, '\r?\n') | Where-Object {
                    $_ -match '(?i)\.sys\b'
                })
        }
        $setOutput = & verifier /flags 0x2093B /driver ramshared.sys 2>&1 | Out-String
        $setExit = [int]$LASTEXITCODE
        $bootOutput = & verifier /bootmode persistent 2>&1 | Out-String
        $bootExit = [int]$LASTEXITCODE
        $queryOutput = & verifier /query 2>&1 | Out-String
        $queryExit = [int]$LASTEXITCODE
        $settingsOutput = & verifier /querysettings 2>&1 | Out-String
        $settingsExit = [int]$LASTEXITCODE
        $memoryManager = Get-ItemProperty -LiteralPath "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management" -ErrorAction Stop
        $driverLevel = [uint32]$memoryManager.VerifyDriverLevel
        $queryText = ($setOutput, $bootOutput, $queryOutput, $settingsOutput) -join [Environment]::NewLine
        $queryTargets = @(Get-GuestVerifierTargetLines -Text $settingsOutput)
        [pscustomobject]@{
            schema = [int]1
            set_exit_code = $setExit
            set_reboot_required = [bool]($setExit -eq 2)
            boot_exit_code = $bootExit
            query_exit_code = $queryExit
            settings_exit_code = $settingsExit
            target_present = [bool]($queryTargets -match '(?i)\bramshared\.sys\b')
            target_count = [int]$queryTargets.Count
            all_drivers = [bool]($settingsOutput -match '(?i)\ball drivers\b')
            flags_exact = [bool]($driverLevel -eq 0x2093B)
            driver_level = $driverLevel
            query = $queryText
        } | ConvertTo-Json -Compress
    }
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "Driver Verifier enable/query"
}

function Get-GuestVerifierQuery {
    [CmdletBinding()]
    param()

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 60 -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $queryOutput = & verifier /query 2>&1 | Out-String
        $queryExit = [int]$LASTEXITCODE
        $settingsOutput = & verifier /querysettings 2>&1 | Out-String
        $settingsExit = [int]$LASTEXITCODE
        $memoryManager = Get-ItemProperty -LiteralPath "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management" -ErrorAction Stop
        $driverLevel = if ($null -eq $memoryManager.PSObject.Properties["VerifyDriverLevel"]) {
            [uint32]0
        }
        else {
            [uint32]$memoryManager.VerifyDriverLevel
        }
        $queryText = ($queryOutput, $settingsOutput) -join [Environment]::NewLine
        $queryTargets = @($queryOutput -split [Environment]::NewLine | Where-Object {
                $_ -match '(?i)\.sys\b'
            })
        [pscustomobject]@{
            schema = [int]1
            query_exit_code = $queryExit
            settings_exit_code = $settingsExit
            target_present = [bool]($queryTargets -match '(?i)\bramshared\.sys\b')
            target_count = [int]$queryTargets.Count
            all_drivers = [bool]($queryOutput -match '(?i)\ball drivers\b')
            flags_exact = [bool]($driverLevel -eq 0x2093B)
            driver_level = $driverLevel
            query = $queryText
        } | ConvertTo-Json -Compress
    }
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "Driver Verifier query"
}

function Assert-GuestVerifierEnabled {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence
    )

    foreach ($propertyName in @(
            "schema", "set_exit_code", "set_reboot_required", "boot_exit_code", "query_exit_code",
            "settings_exit_code", "target_present", "target_count",
            "all_drivers", "flags_exact")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "Driver Verifier enable/query evidence is missing $propertyName"
        }
    }
    if ([int]$Evidence.schema -ne 1 -or
        [int]$Evidence.set_exit_code -cnotin @([int]0, [int]2) -or
        ($Evidence.set_reboot_required -isnot [bool]) -or
        ([bool]$Evidence.set_reboot_required -ne ([int]$Evidence.set_exit_code -eq 2)) -or
        [int]$Evidence.boot_exit_code -ne 0 -or
        [int]$Evidence.query_exit_code -ne 0 -or
        [int]$Evidence.settings_exit_code -ne 0 -or
        ([bool]$Evidence.target_present -ne $true) -or
        [int]$Evidence.target_count -ne 1 -or
        ([bool]$Evidence.all_drivers -ne $false) -or
        ([bool]$Evidence.flags_exact -ne $true)) {
        throw "Driver Verifier enable/query evidence is missing, nonzero, or ambiguous"
    }
    $true
}

function Reset-GuestVerifier {
    [CmdletBinding()]
    param()

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 60 -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $resetOutput = & verifier /reset 2>&1 | Out-String
        $resetExit = [int]$LASTEXITCODE
        $queryOutput = & verifier /query 2>&1 | Out-String
        $queryExit = [int]$LASTEXITCODE
        $queryTargets = @($queryOutput -split [Environment]::NewLine | Where-Object {
                $_ -match '(?i)\.sys\b'
            })
        [pscustomobject]@{
            schema = [int]1
            reset_exit_code = $resetExit
            reset_reboot_required = [bool]($resetExit -eq 2)
            query_exit_code = $queryExit
            target_present = [bool]($queryTargets -match '(?i)\bramshared\.sys\b')
            target_count = [int]$queryTargets.Count
            all_drivers = [bool]($queryOutput -match '(?i)\ball drivers\b')
            query = ($resetOutput, $queryOutput -join [Environment]::NewLine)
        } | ConvertTo-Json -Compress
    }
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "Driver Verifier reset/query"
}

function Assert-GuestVerifierReset {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence
    )

    foreach ($propertyName in @(
            "schema", "reset_exit_code", "reset_reboot_required", "query_exit_code", "target_present",
            "target_count", "all_drivers")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "Driver Verifier reset/query evidence is missing $propertyName"
        }
    }
    $resetExitCode = [int]$Evidence.reset_exit_code
    $targetPresent = [bool]$Evidence.target_present
    $targetCount = [int]$Evidence.target_count
    $targetShapeIsExact = if ($resetExitCode -eq 0) {
        -not $targetPresent -and $targetCount -eq 0
    }
    else {
        (-not $targetPresent -and $targetCount -eq 0) -or
        ($targetPresent -and $targetCount -eq 1)
    }
    if ([int]$Evidence.schema -ne 1 -or
        $resetExitCode -cnotin @([int]0, [int]2) -or
        ($Evidence.reset_reboot_required -isnot [bool]) -or
        ([bool]$Evidence.reset_reboot_required -ne ($resetExitCode -eq 2)) -or
        [int]$Evidence.query_exit_code -ne 0 -or
        -not $targetShapeIsExact -or
        ([bool]$Evidence.all_drivers -ne $false)) {
        throw "Driver Verifier reset/query evidence is missing, nonzero, or stale"
    }
    $true
}

function Invoke-GuestVerifierPreflightStage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet("driver_store", "system_driver", "pnp_root", "disk", "pnp_disk",
            "verifier_query", "certificate_stores", "testsigning_query")]
        [string]$ProviderCode,
        [Parameter(Mandatory = $true)]
        [ValidateRange(2, 600)]
        [int]$TimeoutSeconds,
        [Parameter(Mandatory = $true)]
        [scriptblock]$ScriptBlock,
        [object[]]$ArgumentList = @(),
        [Parameter(Mandatory = $true)]
        [string]$ArtifactPrefix
    )

    $startedUtc = [DateTime]::UtcNow
    try {
        $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds $TimeoutSeconds `
            -ScriptBlock $ScriptBlock -ArgumentList @($ArgumentList)
        $evidence = ConvertFrom-GuestVerifierRows -Rows $rows `
            -Operation ("guest preflight provider " + $ProviderCode)
        if ([int]$evidence.schema -ne 1) {
            throw "guest preflight provider receipt has an invalid schema"
        }
        $completedUtc = [DateTime]::UtcNow
        $receipt = [pscustomobject]@{
            schema = [int]1
            provider_code = $ProviderCode
            status = "completed"
            timeout_seconds = [int]$TimeoutSeconds
            started_utc = $startedUtc.ToString("o")
            completed_utc = $completedUtc.ToString("o")
            duration_ms = [int][Math]::Max(0,
                [Math]::Round(($completedUtc - $startedUtc).TotalMilliseconds))
            evidence = $evidence
        }
        Write-GuestVerifierArtifact -Name ($ArtifactPrefix + "-" + $ProviderCode + ".json") `
            -Value $receipt | Out-Null
        $receipt
    }
    catch {
        $completedUtc = [DateTime]::UtcNow
        Write-GuestVerifierArtifact -Name ($ArtifactPrefix + "-" + $ProviderCode + ".json") `
            -Value ([pscustomobject]@{
                schema = [int]1
                provider_code = $ProviderCode
                status = "failed"
                timeout_seconds = [int]$TimeoutSeconds
                started_utc = $startedUtc.ToString("o")
                completed_utc = $completedUtc.ToString("o")
                duration_ms = [int][Math]::Max(0,
                    [Math]::Round(($completedUtc - $startedUtc).TotalMilliseconds))
                failure_code = "provider_stage_failed"
            }) | Out-Null
        throw ("guest preflight provider failed: " + $ProviderCode)
    }
}

function Invoke-GuestVerifierPreflight {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSignerSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSignerThumbprint
    )

    $script:GuestVerifierPreflightSequence++
    $artifactPrefix = "preflight-{0:D2}" -f $script:GuestVerifierPreflightSequence
    $stages = [Collections.Generic.List[object]]::new()

    [void]$stages.Add((Invoke-GuestVerifierPreflightStage -ProviderCode "driver_store" `
        -TimeoutSeconds 420 -ArtifactPrefix $artifactPrefix -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $packages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
            })
        [pscustomobject]@{
            schema = [int]1
            package_count = [int]$packages.Count
        } | ConvertTo-Json -Compress
    }))

    [void]$stages.Add((Invoke-GuestVerifierPreflightStage -ProviderCode "system_driver" `
        -TimeoutSeconds 120 -ArtifactPrefix $artifactPrefix -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $services = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
        [pscustomobject]@{
            schema = [int]1
            service_count = [int]$services.Count
        } | ConvertTo-Json -Compress
    }))

    [void]$stages.Add((Invoke-GuestVerifierPreflightStage -ProviderCode "pnp_root" `
        -TimeoutSeconds 120 -ArtifactPrefix $artifactPrefix -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $roots = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
            })
        [pscustomobject]@{
            schema = [int]1
            root_count = [int]$roots.Count
        } | ConvertTo-Json -Compress
    }))

    [void]$stages.Add((Invoke-GuestVerifierPreflightStage -ProviderCode "disk" `
        -TimeoutSeconds 90 -ArtifactPrefix $artifactPrefix -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $ramsharedDisks = @(Get-Disk -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.SerialNumber -match '(?i)ramshare|ramshared'
            })
        [pscustomobject]@{
            schema = [int]1
            ramshared_disk_count = [int]$ramsharedDisks.Count
        } | ConvertTo-Json -Compress
    }))

    [void]$stages.Add((Invoke-GuestVerifierPreflightStage -ProviderCode "pnp_disk" `
        -TimeoutSeconds 90 -ArtifactPrefix $artifactPrefix -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $ramsharedPnpDisks = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
            })
        [pscustomobject]@{
            schema = [int]1
            ramshared_pnp_disk_count = [int]$ramsharedPnpDisks.Count
        } | ConvertTo-Json -Compress
    }))

    [void]$stages.Add((Invoke-GuestVerifierPreflightStage -ProviderCode "verifier_query" `
        -TimeoutSeconds 90 -ArtifactPrefix $artifactPrefix -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $verifierOutput = & verifier /query 2>&1 | Out-String
        $verifierExit = [int]$LASTEXITCODE
        $verifierTargets = @($verifierOutput -split [Environment]::NewLine | Where-Object {
                $_ -match '(?i)\.sys\b'
        })
        $verifierAllDrivers = [bool]($verifierOutput -match '(?i)\ball drivers\b')
        [pscustomobject]@{
            schema = [int]1
            verifier_query_exit_code = $verifierExit
            verifier_target_present = [bool]($verifierTargets.Count -gt 0)
            verifier_target_count = [int]$verifierTargets.Count
            verifier_all_drivers = $verifierAllDrivers
            verifier_query = $verifierOutput
        } | ConvertTo-Json -Compress
    }))

    [void]$stages.Add((Invoke-GuestVerifierPreflightStage -ProviderCode "certificate_stores" `
        -TimeoutSeconds 90 -ArtifactPrefix $artifactPrefix -ScriptBlock {
        param($ExpectedSubject, $ExpectedThumbprint)
        $ErrorActionPreference = "Stop"
        $expectedThumbprintCanonical = ($ExpectedThumbprint -replace '\s', '').ToUpperInvariant()
        $rootCertificates = @(Get-ChildItem -LiteralPath "Cert:\LocalMachine\Root" -ErrorAction Stop)
        $trustedPublisherCertificates = @(Get-ChildItem -LiteralPath "Cert:\LocalMachine\TrustedPublisher" -ErrorAction Stop)
        $rootExpected = @($rootCertificates | Where-Object {
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $expectedThumbprintCanonical
            })
        $trustedPublisherExpected = @($trustedPublisherCertificates | Where-Object {
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -ceq $expectedThumbprintCanonical
            })
        $rootForeignSubject = @($rootCertificates | Where-Object {
                [string]$_.Subject -ceq $ExpectedSubject -and
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -cne $expectedThumbprintCanonical
            })
        $trustedPublisherForeignSubject = @($trustedPublisherCertificates | Where-Object {
                [string]$_.Subject -ceq $ExpectedSubject -and
                (($_.Thumbprint -replace '\s', '').ToUpperInvariant()) -cne $expectedThumbprintCanonical
            })
        [pscustomobject]@{
            schema = [int]1
            root_expected_thumbprint_count = [int]$rootExpected.Count
            trusted_publisher_expected_thumbprint_count = [int]$trustedPublisherExpected.Count
            root_foreign_subject_count = [int]$rootForeignSubject.Count
            trusted_publisher_foreign_subject_count = [int]$trustedPublisherForeignSubject.Count
        } | ConvertTo-Json -Compress
    } -ArgumentList @($ExpectedSignerSubject, $ExpectedSignerThumbprint)))

    [void]$stages.Add((Invoke-GuestVerifierPreflightStage -ProviderCode "testsigning_query" `
        -TimeoutSeconds 90 -ArtifactPrefix $artifactPrefix -ScriptBlock {
        $ErrorActionPreference = "Stop"
        $testSigningOutput = & bcdedit.exe /enum "{current}" 2>&1 | Out-String
        $testSigningExit = [int]$LASTEXITCODE
        $testSigningEnabled = [bool]($testSigningOutput -match '(?im)^\s*testsigning\s+(yes|on|true|1)\s*$')
        [pscustomobject]@{
            schema = [int]1
            testsigning_query_exit_code = $testSigningExit
            testsigning_enabled = $testSigningEnabled
        } | ConvertTo-Json -Compress
    }))

    if ($stages.Count -ne 8 -or @($stages.provider_code | Select-Object -Unique).Count -ne 8) {
        throw "guest preflight provider stage set is incomplete or ambiguous"
    }
    $byCode = @{}
    foreach ($stage in $stages) {
        $byCode[[string]$stage.provider_code] = $stage.evidence
    }
    [pscustomobject]@{
        schema = [int]1
        package_count = [int]$byCode.driver_store.package_count
        service_count = [int]$byCode.system_driver.service_count
        root_count = [int]$byCode.pnp_root.root_count
        ramshared_disk_count = [int]$byCode.disk.ramshared_disk_count
        ramshared_pnp_disk_count = [int]$byCode.pnp_disk.ramshared_pnp_disk_count
        verifier_query_exit_code = [int]$byCode.verifier_query.verifier_query_exit_code
        verifier_target_present = [bool]$byCode.verifier_query.verifier_target_present
        verifier_target_count = [int]$byCode.verifier_query.verifier_target_count
        verifier_all_drivers = [bool]$byCode.verifier_query.verifier_all_drivers
        verifier_query = [string]$byCode.verifier_query.verifier_query
        testsigning_query_exit_code = [int]$byCode.testsigning_query.testsigning_query_exit_code
        testsigning_enabled = [bool]$byCode.testsigning_query.testsigning_enabled
        root_expected_thumbprint_count = [int]$byCode.certificate_stores.root_expected_thumbprint_count
        trusted_publisher_expected_thumbprint_count = [int]$byCode.certificate_stores.trusted_publisher_expected_thumbprint_count
        root_foreign_subject_count = [int]$byCode.certificate_stores.root_foreign_subject_count
        trusted_publisher_foreign_subject_count = [int]$byCode.certificate_stores.trusted_publisher_foreign_subject_count
        provider_stages = @($stages | ForEach-Object {
                [pscustomobject]@{
                    provider_code = $_.provider_code
                    timeout_seconds = $_.timeout_seconds
                    started_utc = $_.started_utc
                    completed_utc = $_.completed_utc
                    duration_ms = $_.duration_ms
                }
            })
    }
}

function Assert-GuestVerifierPreflight {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [object]$Evidence
    )

    foreach ($propertyName in @(
            "verifier_target_count", "verifier_all_drivers", "testsigning_query_exit_code",
            "testsigning_enabled", "root_expected_thumbprint_count",
            "trusted_publisher_expected_thumbprint_count", "root_foreign_subject_count",
            "trusted_publisher_foreign_subject_count")) {
        if ($null -eq $Evidence.PSObject.Properties[$propertyName]) {
            throw "guest preflight evidence is missing $propertyName"
        }
    }
    if (
        [int]$Evidence.schema -ne 1 -or
        [int]$Evidence.package_count -ne 0 -or
        [int]$Evidence.service_count -ne 0 -or
        [int]$Evidence.root_count -ne 0 -or
        [int]$Evidence.ramshared_disk_count -ne 0 -or
        [int]$Evidence.ramshared_pnp_disk_count -ne 0 -or
        [int]$Evidence.verifier_query_exit_code -ne 0 -or
        ([bool]$Evidence.verifier_target_present -ne $false) -or
        [int]$Evidence.verifier_target_count -ne 0 -or
        ([bool]$Evidence.verifier_all_drivers -ne $false) -or
        [int]$Evidence.testsigning_query_exit_code -ne 0 -or
        ([bool]$Evidence.testsigning_enabled -ne $false) -or
        [int]$Evidence.root_expected_thumbprint_count -ne 0 -or
        [int]$Evidence.trusted_publisher_expected_thumbprint_count -ne 0 -or
        [int]$Evidence.root_foreign_subject_count -ne 0 -or
        [int]$Evidence.trusted_publisher_foreign_subject_count -ne 0) {
        throw "guest preflight refused pre-existing RamShared, Verifier, TestSigning, or signer certificate state"
    }
    $true
}

function Get-GuestVerifierPostPublishCleanupMode {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$State,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$ExpectedPublishedInf,
        [Parameter(Mandatory = $true)][string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)][string]$ExpectedInfHash,
        [Parameter(Mandatory = $true)][string]$ExpectedCatalogHash
    )

    $publishedInf = $ExpectedPublishedInf.Trim().ToLowerInvariant()
    $driverHash = $ExpectedDriverHash.Trim().ToUpperInvariant()
    $infHash = $ExpectedInfHash.Trim().ToUpperInvariant()
    $catalogHash = $ExpectedCatalogHash.Trim().ToUpperInvariant()
    foreach ($propertyName in @(
            "ramshared_pnp_disk_count", "ramshared_present_pnp_disk_count",
            "ramshared_retired_pnp_disk_count", "retired_pnp_instance_id")) {
        if ($null -eq $State.PSObject.Properties[$propertyName]) {
            throw "post-publish cleanup state is missing $propertyName"
        }
    }
    $pnpCount = [int]$State.ramshared_pnp_disk_count
    $presentPnpCount = [int]$State.ramshared_present_pnp_disk_count
    $retiredPnpCount = [int]$State.ramshared_retired_pnp_disk_count
    $retiredPnpInstanceId = [string]$State.retired_pnp_instance_id
    $zeroPnp = $pnpCount -eq 0 -and $presentPnpCount -eq 0 -and
        $retiredPnpCount -eq 0 -and [string]::IsNullOrEmpty($retiredPnpInstanceId)
    $oneExactRetiredPnp = $pnpCount -eq 1 -and $presentPnpCount -eq 0 -and
        $retiredPnpCount -eq 1 -and
        $retiredPnpInstanceId -cmatch '^SCSI\\DISK&VEN_RAMSHARE&PROD_VRAMDISK\\[^\\]+$'
    if ($ExpectedPublishedInf -cne $ExpectedPublishedInf.Trim() -or
        $publishedInf -notmatch '^oem[0-9]+\.inf$' -or
        $driverHash -notmatch '^[0-9A-F]{64}$' -or $infHash -notmatch '^[0-9A-F]{64}$' -or
        $catalogHash -notmatch '^[0-9A-F]{64}$' -or [int]$State.schema -ne 1 -or
        [string]$State.run_id -cne $RunId -or [string]$State.published_inf -cne $publishedInf -or
        [int]$State.package_count -ne 1 -or [int]$State.published_inf_count -ne 1 -or
        [string]$State.package_original_inf -cne "ramshared.inf" -or
        [string]$State.driver_store_sys_sha256 -cne $driverHash -or
        [string]$State.driver_store_inf_sha256 -cne $infHash -or
        [string]$State.driver_store_catalog_sha256 -cne $catalogHash -or
        [int]$State.ramshared_disk_count -ne 0) {
        throw "post-publish cleanup state is foreign, ambiguous, hash-mismatched, or has a product LUN"
    }
    if ([int]$State.root_count -eq 0 -and [int]$State.service_count -eq 0 -and
        $zeroPnp -and
        [string]::IsNullOrEmpty([string]$State.root_instance_id) -and
        [string]::IsNullOrEmpty([string]$State.hardware_id) -and
        [string]::IsNullOrEmpty([string]$State.service_name) -and
        [string]::IsNullOrEmpty([string]$State.service_state) -and
        [string]::IsNullOrEmpty([string]$State.service_sha256) -and
        [string]::IsNullOrEmpty([string]$State.service_inf_sha256) -and
        [string]::IsNullOrEmpty([string]$State.service_catalog_sha256)) {
        return "published_only"
    }
    if ([int]$State.root_count -eq 1 -and
        ($zeroPnp -or $oneExactRetiredPnp) -and
        [string]$State.root_instance_id -cmatch '^ROOT\\RAMSHARED\\[^\\]+$' -and
        [string]$State.hardware_id -ceq "ROOT\RAMSHARED" -and
        [int]$State.service_count -eq 1 -and [string]$State.service_name -ceq "ramshared" -and
        [string]$State.service_state -cin @("Running", "Stopped") -and
        [string]$State.service_sha256 -ceq $driverHash -and
        [string]$State.service_inf_sha256 -ceq $infHash -and
        [string]$State.service_catalog_sha256 -ceq $catalogHash) {
        return "root_bound"
    }
    throw "post-publish cleanup state is neither exact package-only nor exact ROOT-bound identity"
}

function Get-GuestVerifierPostPublishCleanupState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$ExpectedPublishedInf
    )

    $publishedInf = Normalize-GuestVerifierPublishedInf $ExpectedPublishedInf "post-publish cleanup published INF"
    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        param($CurrentRunId, $PublishedInf)
        $ErrorActionPreference = "Stop"
        $packages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
            })
        $publishedPackages = @($packages | Where-Object {
                ([IO.Path]::GetFileName([string]$_.Driver)).ToLowerInvariant() -ceq $PublishedInf
            })
        $packageOriginalInf = ""
        $packageSysHash = ""
        $packageInfHash = ""
        $packageCatHash = ""
        if ($publishedPackages.Count -eq 1) {
            $packageInf = [string]$publishedPackages[0].OriginalFileName
            $packageOriginalInf = [IO.Path]::GetFileName($packageInf).ToLowerInvariant()
            $packageDirectory = Split-Path -Parent $packageInf
            $packageSys = Join-Path $packageDirectory "ramshared.sys"
            $packageCat = Join-Path $packageDirectory "ramshared.cat"
            foreach ($packagePath in @($packageSys, $packageInf, $packageCat)) {
                if (-not (Test-Path -LiteralPath $packagePath -PathType Leaf)) {
                    throw "post-publish package immutable file is missing"
                }
            }
            $packageSysHash = (Get-FileHash -LiteralPath $packageSys -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
            $packageInfHash = (Get-FileHash -LiteralPath $packageInf -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
            $packageCatHash = (Get-FileHash -LiteralPath $packageCat -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        }
        $roots = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
            })
        $hardwareId = ""
        if ($roots.Count -eq 1) {
            $hardwareProperties = @(Get-PnpDeviceProperty -InstanceId ([string]$roots[0].InstanceId) `
                    -KeyName "DEVPKEY_Device_HardwareIds" -ErrorAction Stop)
            $hardwareIds = @()
            if ($hardwareProperties.Count -eq 1) {
                $hardwareIds = @($hardwareProperties[0].Data | ForEach-Object {
                        ([string]$_).Trim().ToUpperInvariant()
                    })
            }
            if ($hardwareIds.Count -ne 1) { throw "post-publish ROOT hardware identity is ambiguous" }
            $hardwareId = [string]($hardwareIds[0])
        }
        $services = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
        $servicePath = ""
        $serviceHash = ""
        $serviceInfHash = ""
        $serviceCatHash = ""
        if ($services.Count -eq 1) {
            $candidate = ([string]$services[0].PathName).Trim().Trim('"')
            if ($candidate -match '(?i)^\\SystemRoot\\') { $candidate = Join-Path $env:SystemRoot $candidate.Substring(12) }
            if ($candidate -match '(?i)^\\\?\?\\') { $candidate = $candidate.Substring(4) }
            $servicePath = (Resolve-Path -LiteralPath $candidate -ErrorAction Stop).Path
            $serviceDirectory = Split-Path -Parent $servicePath
            $serviceInf = Join-Path $serviceDirectory "ramshared.inf"
            $serviceCat = Join-Path $serviceDirectory "ramshared.cat"
            $serviceHash = (Get-FileHash -LiteralPath $servicePath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
            $serviceInfHash = (Get-FileHash -LiteralPath $serviceInf -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
            $serviceCatHash = (Get-FileHash -LiteralPath $serviceCat -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        }
        $disks = @(Get-Disk -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or $_.SerialNumber -match '(?i)ramshare|ramshared'
            })
        $pnpDisks = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
            $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
            })
        $presentPnpDisks = @($pnpDisks | Where-Object { [bool]$_.Present })
        $retiredPnpDisks = @($pnpDisks | Where-Object {
                ([bool]$_.Present -eq $false) -and [string]$_.Status -ceq "Unknown" -and
                [int]$_.Problem -eq 45 -and
                [string]$_.InstanceId -cmatch '^SCSI\\DISK&VEN_RAMSHARE&PROD_VRAMDISK\\[^\\]+$'
            })
        [pscustomobject]@{
            schema = [int]1; run_id = $CurrentRunId; published_inf = $PublishedInf
            package_count = [int]$packages.Count; published_inf_count = [int]$publishedPackages.Count
            package_original_inf = $packageOriginalInf
            driver_store_sys_sha256 = $packageSysHash; driver_store_inf_sha256 = $packageInfHash
            driver_store_catalog_sha256 = $packageCatHash
            root_count = [int]$roots.Count
            root_instance_id = if ($roots.Count -eq 1) { [string]$roots[0].InstanceId } else { "" }
            hardware_id = $hardwareId
            service_count = [int]$services.Count
            service_name = if ($services.Count -eq 1) { [string]$services[0].Name } else { "" }
            service_state = if ($services.Count -eq 1) { [string]$services[0].State } else { "" }
            service_path = $servicePath; service_sha256 = $serviceHash
            service_inf_sha256 = $serviceInfHash; service_catalog_sha256 = $serviceCatHash
            ramshared_disk_count = [int]$disks.Count; ramshared_pnp_disk_count = [int]$pnpDisks.Count
            ramshared_present_pnp_disk_count = [int]$presentPnpDisks.Count
            ramshared_retired_pnp_disk_count = [int]$retiredPnpDisks.Count
            retired_pnp_instance_id = if ($retiredPnpDisks.Count -eq 1) {
                [string]$retiredPnpDisks[0].InstanceId
            } else { "" }
        } | ConvertTo-Json -Compress
    } -ArgumentList @($RunId, $publishedInf)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "post-publish cleanup state"
}

function Remove-GuestVerifierPublishedPackageOnly {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object]$Before,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$ExpectedPublishedInf,
        [Parameter(Mandatory = $true)][string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)][string]$ExpectedInfHash,
        [Parameter(Mandatory = $true)][string]$ExpectedCatalogHash
    )

    if ((Get-GuestVerifierPostPublishCleanupMode -State $Before -RunId $RunId `
            -ExpectedPublishedInf $ExpectedPublishedInf -ExpectedDriverHash $ExpectedDriverHash `
            -ExpectedInfHash $ExpectedInfHash -ExpectedCatalogHash $ExpectedCatalogHash) -cne "published_only") {
        throw "package-only cleanup requires the exact published-only state"
    }
    $driverHash = Normalize-GuestVerifierSha256 $ExpectedDriverHash "package-only cleanup driver hash"
    $infHash = Normalize-GuestVerifierSha256 $ExpectedInfHash "package-only cleanup INF hash"
    $catalogHash = Normalize-GuestVerifierSha256 $ExpectedCatalogHash "package-only cleanup catalog hash"
    $deleteRows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        param($PublishedInf, $ExpectedSysHash, $ExpectedInfFileHash, $ExpectedCatHash)
        $ErrorActionPreference = "Stop"
        $packages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
            })
        $publishedPackages = @($packages | Where-Object {
                ([IO.Path]::GetFileName([string]$_.Driver)).ToLowerInvariant() -ceq $PublishedInf
            })
        $roots = @(Get-PnpDevice -ErrorAction Stop | Where-Object { $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\' })
        $services = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
        $disks = @(Get-Disk -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or $_.SerialNumber -match '(?i)ramshare|ramshared'
            })
        $pnpDisks = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
            })
        if ($packages.Count -ne 1 -or $publishedPackages.Count -ne 1 -or
            $roots.Count -ne 0 -or $services.Count -ne 0 -or $disks.Count -ne 0 -or $pnpDisks.Count -ne 0) {
            throw "package-only cleanup live revalidation is no longer exact"
        }
        $packageInf = [string]$publishedPackages[0].OriginalFileName
        $packageDirectory = Split-Path -Parent $packageInf
        $packageSys = Join-Path $packageDirectory "ramshared.sys"
        $packageCat = Join-Path $packageDirectory "ramshared.cat"
        foreach ($packagePath in @($packageSys, $packageInf, $packageCat)) {
            if (-not (Test-Path -LiteralPath $packagePath -PathType Leaf)) {
                throw "package-only cleanup immutable file is missing"
            }
        }
        if ((Get-FileHash -LiteralPath $packageSys -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant() -cne $ExpectedSysHash -or
            (Get-FileHash -LiteralPath $packageInf -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant() -cne $ExpectedInfFileHash -or
            (Get-FileHash -LiteralPath $packageCat -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant() -cne $ExpectedCatHash) {
            throw "package-only cleanup immutable hashes drifted before deletion"
        }
        $deleteOutput = & pnputil.exe /delete-driver $PublishedInf /uninstall 2>&1 | Out-String
        $deleteExit = [int]$LASTEXITCODE
        if ($deleteExit -ne 0) { throw "exact package-only deletion failed exit=$deleteExit" }
        [pscustomobject]@{
            schema = [int]1; action = "delete_exact_published_package_only"
            published_inf = $PublishedInf; uninstall = $true; force = $false; exit_code = $deleteExit
        } | ConvertTo-Json -Compress
    } -ArgumentList @($ExpectedPublishedInf, $driverHash, $infHash, $catalogHash)
    $action = ConvertFrom-GuestVerifierRows -Rows $deleteRows -Operation "exact published-only package deletion"
    $after = Get-GuestVerifierPostPublishCleanupState -RunId $RunId -ExpectedPublishedInf $ExpectedPublishedInf
    if ([int]$after.package_count -ne 0 -or [int]$after.published_inf_count -ne 0 -or
        [int]$after.root_count -ne 0 -or [int]$after.service_count -ne 0 -or
        [int]$after.ramshared_disk_count -ne 0 -or [int]$after.ramshared_pnp_disk_count -ne 0) {
        throw "published-only package cleanup did not reach terminal zero"
    }
    [pscustomobject]@{ schema = [int]1; run_id = $RunId; before = $Before; action = $action; after = $after }
}

function Publish-GuestVerifierPackage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [string]$GuestStage,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSignerSubject,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedSignerThumbprint
    )

    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        param($CurrentRunId, $Stage, $ExpectedSubject, $ExpectedThumbprint)
        $ErrorActionPreference = "Stop"
        $sys = Join-Path $Stage "ramshared.sys"
        $cat = Join-Path $Stage "ramshared.cat"
        $inf = Join-Path $Stage "ramshared.inf"
        foreach ($path in @($sys, $cat, $inf)) {
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                throw "staged package input is missing: $path"
            }
        }
        $sysSignature = Get-AuthenticodeSignature -FilePath $sys -ErrorAction Stop
        $catSignature = Get-AuthenticodeSignature -FilePath $cat -ErrorAction Stop
        $expectedThumbprintCanonical = ($ExpectedThumbprint -replace '\s', '').ToUpperInvariant()
        foreach ($signatureItem in @(
                [pscustomobject]@{ role = "driver"; signature = $sysSignature },
                [pscustomobject]@{ role = "catalog"; signature = $catSignature })) {
            if ([string]$signatureItem.signature.Status -cne "Valid" -or
                $null -eq $signatureItem.signature.SignerCertificate -or
                [string]$signatureItem.signature.SignerCertificate.Subject -cne $ExpectedSubject -or
                (([string]$signatureItem.signature.SignerCertificate.Thumbprint -replace '\s', '').ToUpperInvariant() -cne $expectedThumbprintCanonical)) {
                throw "staged signed package Authenticode signer is not the sealed public certificate"
            }
        }
        $infText = Get-Content -LiteralPath $inf -Raw -ErrorAction Stop
        if ($infText -notmatch '(?im)^\s*DriverVer\s*=\s*08/09/2026,10\.0\.26200\.8\s*$') {
            throw "staged ramshared.inf has the wrong DriverVer"
        }
        $publishOutput = & pnputil.exe /add-driver $inf 2>&1 | Out-String
        $publishExit = [int]$LASTEXITCODE
        if ($publishExit -ne 0) {
            throw "staged package publish failed exit=$($publishExit): $publishOutput"
        }
        $publishedInfCandidates = @([regex]::Matches($publishOutput, '(?i)\boem[0-9]+\.inf\b') |
            ForEach-Object { $_.Value.ToLowerInvariant() } | Select-Object -Unique)
        if ($publishedInfCandidates.Count -ne 1) {
            throw "pnputil publish output did not yield exactly one published OEM INF"
        }
        $publishedInf = [string]$publishedInfCandidates[0]
        $publishedPackageDeadline = (Get-Date).ToUniversalTime().AddSeconds(60)
        $ramsharedPackages = @()
        $publishedPackages = @()
        do {
            $ramsharedPackages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                    [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
                })
            $publishedPackages = @($ramsharedPackages | Where-Object {
                    ([IO.Path]::GetFileName([string]$_.Driver)).ToLowerInvariant() -ceq $publishedInf
                })
            if ($ramsharedPackages.Count -eq 1 -and $publishedPackages.Count -eq 1) {
                break
            }
            Start-Sleep -Seconds 2
        } while ((Get-Date).ToUniversalTime() -lt $publishedPackageDeadline)
        if ($ramsharedPackages.Count -ne 1 -or $publishedPackages.Count -ne 1 -or
            ([IO.Path]::GetFileName([string]$publishedPackages[0].OriginalFileName)).ToLowerInvariant() -cne "ramshared.inf") {
            throw "published OEM INF does not bind exactly one ramshared.inf DriverStore package"
        }
        [pscustomobject]@{
            schema = [int]1
            run_id = $CurrentRunId
            sys_signature = [string]$sysSignature.Status
            cat_signature = [string]$catSignature.Status
            driver_subject = [string]$sysSignature.SignerCertificate.Subject
            catalog_subject = [string]$catSignature.SignerCertificate.Subject
            driver_thumbprint = [string]$sysSignature.SignerCertificate.Thumbprint
            catalog_thumbprint = [string]$catSignature.SignerCertificate.Thumbprint
            publish_exit_code = $publishExit
            published_inf = $publishedInf
            published_package_count = [int]$publishedPackages.Count
            package_original_inf = ([IO.Path]::GetFileName([string]$publishedPackages[0].OriginalFileName)).ToLowerInvariant()
        } | ConvertTo-Json -Compress
    } -ArgumentList @($RunId, $GuestStage, $ExpectedSignerSubject, $ExpectedSignerThumbprint)
    ConvertFrom-GuestVerifierRows -Rows $rows -Operation "signed package publish"
}

function ConvertFrom-GuestVerifierRootCreationResult {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string]$Value)

    $success = [regex]::Match($Value, '^OK reboot=(?<reboot>True|False)$',
        [Text.RegularExpressions.RegexOptions]::CultureInvariant)
    if ($success.Success) {
        return [pscustomobject]@{
            root_creation_ok = $true
            root_creation_stage = "ok"
            root_creation_win32_error = [int]0
            root_creation_reboot_required = [bool]::Parse($success.Groups["reboot"].Value)
        }
    }
    $failure = [regex]::Match($Value,
        '^(?<stage>CreateDeviceInfoList|CreateDeviceInfo|SetHWID|RegisterDevice|UpdateDriver) err=(?<error>-?[0-9]+)$',
        [Text.RegularExpressions.RegexOptions]::CultureInvariant)
    if ($failure.Success) {
        return [pscustomobject]@{
            root_creation_ok = $false
            root_creation_stage = [string]$failure.Groups["stage"].Value
            root_creation_win32_error = [int]::Parse($failure.Groups["error"].Value,
                [Globalization.CultureInfo]::InvariantCulture)
            root_creation_reboot_required = $false
        }
    }
    [pscustomobject]@{
        root_creation_ok = $false
        root_creation_stage = "malformed"
        root_creation_win32_error = [int]-1
        root_creation_reboot_required = $false
    }
}

function Create-GuestVerifierRoot {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunId,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedPublishedInf,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedDriverHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedInfHash,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedCatalogHash
    )

    $publishedInfCanonical = Normalize-GuestVerifierPublishedInf $ExpectedPublishedInf "published INF for ROOT creation"
    $driverHashCanonical = Normalize-GuestVerifierSha256 $ExpectedDriverHash "driver hash for ROOT creation"
    $infHashCanonical = Normalize-GuestVerifierSha256 $ExpectedInfHash "INF hash for ROOT creation"
    $catalogHashCanonical = Normalize-GuestVerifierSha256 $ExpectedCatalogHash "catalog hash for ROOT creation"
    $rows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 420 -ScriptBlock {
        param($CurrentRunId, $PublishedInf, $ExpectedSysHash, $ExpectedInfFileHash, $ExpectedCatHash)
        $ErrorActionPreference = "Stop"
        $workerStage = "package_query"
        $observedPublishedPackageCount = [int]0
        $observedPackageOriginalInf = ""
        $packageVisibilityElapsedMs = [int64]0
        $observedSysHash = ""
        $observedInfHash = ""
        $observedCatHash = ""
        $observedRootCount = [int]0
        $observedRootInstanceId = ""
        $observedServiceCount = [int]0
        $observedServiceName = ""
        $observedServiceState = ""
        $rootResult = "not_executed"
        try {
        $ramsharedPackages = @(Get-WindowsDriver -Online -All -ErrorAction Stop | Where-Object {
                [string]$_.OriginalFileName -match '(?i)(^|\\)ramshared\.inf$'
            })
        $publishedPackages = @($ramsharedPackages | Where-Object {
                ([IO.Path]::GetFileName([string]$_.Driver)).ToLowerInvariant() -ceq $PublishedInf
            })
        if ($ramsharedPackages.Count -ne 1 -or $publishedPackages.Count -ne 1) {
            throw "ROOT creation requires one exact published package"
        }
        $observedPublishedPackageCount = [int]$publishedPackages.Count
        $workerStage = "package_identity"
        $packageInf = [string]$publishedPackages[0].OriginalFileName
        if (-not [IO.Path]::IsPathRooted($packageInf)) {
            throw "published package original INF path is not exact"
        }
        $packageInfCanonical = [IO.Path]::GetFullPath($packageInf)
        $driverStoreFileRepositoryRoot = [IO.Path]::GetFullPath(
            (Join-Path $env:SystemRoot "System32\DriverStore\FileRepository"))
        $driverStoreFileRepositoryPrefix = $driverStoreFileRepositoryRoot.TrimEnd(
            [char[]]@([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)) +
            [IO.Path]::DirectorySeparatorChar
        if ([IO.Path]::GetFileName($packageInfCanonical).ToLowerInvariant() -cne "ramshared.inf" -or
            -not $packageInfCanonical.StartsWith(
                $driverStoreFileRepositoryPrefix,
                [StringComparison]::OrdinalIgnoreCase)) {
            throw "published package original INF path is not exact"
        }
        $packageInf = $packageInfCanonical
        $packageDirectory = Split-Path -Parent $packageInf
        $packageSys = Join-Path $packageDirectory "ramshared.sys"
        $packageCat = Join-Path $packageDirectory "ramshared.cat"
        $packagePaths = @($packageSys, $packageInf, $packageCat)
        $workerStage = "package_file_visibility"
        $packageVisibilityStarted = [DateTime]::UtcNow
        $packageVisibilityDeadline = $packageVisibilityStarted.AddSeconds(60)
        $packageVisibilityStopwatch = [Diagnostics.Stopwatch]::StartNew()
        $packageFilesVisible = $false
        do {
            $packageFilesVisible = $true
            foreach ($packagePath in $packagePaths) {
                if (-not (Test-Path -LiteralPath $packagePath -PathType Leaf)) {
                    $packageFilesVisible = $false
                    break
                }
            }
            if ($packageFilesVisible) { break }
            if ($packageVisibilityStopwatch.Elapsed.TotalSeconds -ge 60 -or
                [DateTime]::UtcNow -ge $packageVisibilityDeadline) { break }
            Start-Sleep -Milliseconds 250
        } while ($true)
        $packageVisibilityStopwatch.Stop()
        $packageVisibilityElapsedMs = [int64][Math]::Max(0, $packageVisibilityStopwatch.ElapsedMilliseconds)
        if (-not $packageFilesVisible) {
            throw "published package immutable files did not become visible before the deadline"
        }
        $packageDirectoryItem = Get-Item -LiteralPath $packageDirectory -Force -ErrorAction Stop
        if (($packageDirectoryItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "published package directory is a reparse point"
        }
        foreach ($packagePath in $packagePaths) {
            $packageItem = Get-Item -LiteralPath $packagePath -Force -ErrorAction Stop
            if (($packageItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
                -not $packageItem.FullName.Equals(
                    [IO.Path]::GetFullPath($packagePath),
                    [StringComparison]::OrdinalIgnoreCase)) {
                throw "published package immutable file identity drifted"
            }
        }
        $observedPackageOriginalInf = ([IO.Path]::GetFileName(
                [string]$publishedPackages[0].OriginalFileName)).ToLowerInvariant()
        $workerStage = "immutable_hash"
        $sysHash = (Get-FileHash -LiteralPath $packageSys -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        $infHash = (Get-FileHash -LiteralPath $packageInf -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        $catHash = (Get-FileHash -LiteralPath $packageCat -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
        if ($sysHash -cne $ExpectedSysHash -or $infHash -cne $ExpectedInfFileHash -or
            $catHash -cne $ExpectedCatHash) {
            throw "published package immutable hash mismatch before ROOT creation"
        }
        $observedSysHash = $sysHash
        $observedInfHash = $infHash
        $observedCatHash = $catHash
        $workerStage = "pre_root_observation"
        $rootsBefore = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
            })
        $servicesBefore = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
        $disksBefore = @(Get-Disk -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.SerialNumber -match '(?i)ramshare|ramshared'
            })
        $pnpDisksBefore = @(Get-PnpDevice -Class DiskDrive -ErrorAction Stop | Where-Object {
                $_.FriendlyName -match '(?i)ramshare|ramshared|vramdisk' -or
                $_.InstanceId -match '(?i)VEN_RAMSHARE|PROD_VRAMDISK'
            })
        if ($rootsBefore.Count -ne 0 -or $servicesBefore.Count -ne 0 -or
            $disksBefore.Count -ne 0 -or $pnpDisksBefore.Count -ne 0) {
            throw "ROOT creation precondition has product device or service residue"
        }
        $workerStage = "native_type_compile"
        if (-not ("RamSharedGuestRootEnum" -as [type])) {
            Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class RamSharedGuestRootEnum {
  static readonly Guid ScsiClass = new Guid("4d36e97b-e325-11ce-bfc1-08002be10318");
  [StructLayout(LayoutKind.Sequential)]
  struct SP_DEVINFO_DATA {
    public int cbSize; public Guid ClassGuid; public int DevInst; public IntPtr Reserved;
  }
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid g, IntPtr h);
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool SetupDiCreateDeviceInfo(IntPtr list, string name, ref Guid g, string desc, IntPtr hwnd, int flags, ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool SetupDiSetDeviceRegistryProperty(IntPtr list, ref SP_DEVINFO_DATA data, int prop, byte[] buf, int size);
  [DllImport("setupapi.dll", SetLastError=true)]
  static extern bool SetupDiCallClassInstaller(int dif, IntPtr list, ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll", SetLastError=true)]
  static extern bool SetupDiDestroyDeviceInfoList(IntPtr list);
  [DllImport("newdev.dll", CharSet=CharSet.Auto, SetLastError=true)]
  static extern bool UpdateDriverForPlugAndPlayDevices(IntPtr hwnd, string hwid, string inf, uint flags, out bool reboot);
  public static string Install(string infPath) {
    Guid g = ScsiClass;
    IntPtr list = SetupDiCreateDeviceInfoList(ref g, IntPtr.Zero);
    if (list == IntPtr.Zero || list == new IntPtr(-1))
      return "CreateDeviceInfoList err=" + Marshal.GetLastWin32Error();
    try {
      SP_DEVINFO_DATA data = new SP_DEVINFO_DATA();
      data.cbSize = Marshal.SizeOf(typeof(SP_DEVINFO_DATA));
      if (!SetupDiCreateDeviceInfo(list, "RamShared", ref g, "RamShared VRAM Virtual Disk", IntPtr.Zero, 1, ref data))
        return "CreateDeviceInfo err=" + Marshal.GetLastWin32Error();
      byte[] buf = Encoding.Unicode.GetBytes("Root\\RamShared\0\0");
      if (!SetupDiSetDeviceRegistryProperty(list, ref data, 1, buf, buf.Length))
        return "SetHWID err=" + Marshal.GetLastWin32Error();
      if (!SetupDiCallClassInstaller(0x19, list, ref data))
        return "RegisterDevice err=" + Marshal.GetLastWin32Error();
      bool reboot;
      if (!UpdateDriverForPlugAndPlayDevices(IntPtr.Zero, "Root\\RamShared", infPath, 1, out reboot))
        return "UpdateDriver err=" + Marshal.GetLastWin32Error();
      return "OK reboot=" + reboot;
    } finally { SetupDiDestroyDeviceInfoList(list); }
  }
}
'@ -ErrorAction Stop
        }
        $workerStage = "native_install"
        $rootResult = [RamSharedGuestRootEnum]::Install($packageInf)
        $workerStage = "post_root_observation"
        $rootsAfter = @(Get-PnpDevice -ErrorAction Stop | Where-Object {
                $_.InstanceId -match '(?i)^ROOT\\RAMSHARED\\'
            })
        $servicesAfter = @(Get-CimInstance -ClassName Win32_SystemDriver -Filter "Name = 'ramshared'" -ErrorAction Stop)
        $observedRootCount = [int]$rootsAfter.Count
        $observedRootInstanceId = if ($rootsAfter.Count -eq 1) { [string]$rootsAfter[0].InstanceId } else { "" }
        $observedServiceCount = [int]$servicesAfter.Count
        $observedServiceName = if ($servicesAfter.Count -eq 1) { [string]$servicesAfter[0].Name } else { "" }
        $observedServiceState = if ($servicesAfter.Count -eq 1) { [string]$servicesAfter[0].State } else { "" }
        [pscustomobject]@{
            schema = [int]1
            run_id = $CurrentRunId
            worker_status = "ok"
            worker_failure_stage = ""
            worker_failure_hresult = [int]0
            worker_failure_type = ""
            published_inf = $PublishedInf
            published_package_count = $observedPublishedPackageCount
            package_original_inf = $observedPackageOriginalInf
            package_visibility_elapsed_ms = $packageVisibilityElapsedMs
            driver_store_sys_sha256 = $observedSysHash
            driver_store_inf_sha256 = $observedInfHash
            driver_store_catalog_sha256 = $observedCatHash
            root_creation = $rootResult
            root_count = $observedRootCount
            root_instance_id = $observedRootInstanceId
            service_count = $observedServiceCount
            service_name = $observedServiceName
            service_state = $observedServiceState
        } | ConvertTo-Json -Compress
        }
        catch {
            $exceptionType = $_.Exception.GetType().FullName
            $safeExceptionType = switch -CaseSensitive ($exceptionType) {
                "System.EntryPointNotFoundException" { "entry_point_not_found"; break }
                "System.ComponentModel.Win32Exception" { "win32_exception"; break }
                "System.UnauthorizedAccessException" { "unauthorized_access"; break }
                "System.InvalidOperationException" { "invalid_operation"; break }
                "System.Management.Automation.RuntimeException" { "powershell_runtime"; break }
                default { "other" }
            }
            [pscustomobject]@{
                schema = [int]1
                run_id = $CurrentRunId
                worker_status = "failed"
                worker_failure_stage = $workerStage
                worker_failure_hresult = [int]$_.Exception.HResult
                worker_failure_type = $safeExceptionType
                published_inf = $PublishedInf
                published_package_count = $observedPublishedPackageCount
                package_original_inf = $observedPackageOriginalInf
                package_visibility_elapsed_ms = $packageVisibilityElapsedMs
                driver_store_sys_sha256 = $observedSysHash
                driver_store_inf_sha256 = $observedInfHash
                driver_store_catalog_sha256 = $observedCatHash
                root_creation = $rootResult
                root_count = $observedRootCount
                root_instance_id = $observedRootInstanceId
                service_count = $observedServiceCount
                service_name = $observedServiceName
                service_state = $observedServiceState
            } | ConvertTo-Json -Compress
        }
    } -ArgumentList @($RunId, $publishedInfCanonical, $driverHashCanonical, $infHashCanonical, $catalogHashCanonical)
    $receipt = ConvertFrom-GuestVerifierRows -Rows $rows -Operation "exact ROOT creation"
    $classification = ConvertFrom-GuestVerifierRootCreationResult -Value ([string]$receipt.root_creation)
    foreach ($property in @(
            "root_creation_ok", "root_creation_stage", "root_creation_win32_error",
            "root_creation_reboot_required")) {
        $receipt | Add-Member -NotePropertyName $property -NotePropertyValue $classification.$property -Force
    }
    $receipt
}

$script:GuestVerifierVmName = $VMName
$script:GuestVerifierUser = $User
$script:GuestVerifierPassword = $Password
$script:GuestVerifierConnectTimeoutSeconds = $PsDirectConnectTimeoutSeconds
$script:GuestVerifierPreflightSequence = 0
$runId = [guid]::NewGuid().ToString("D")
$script:GuestVerifierArtifactDirectory = Join-Path $ArtifactRoot ("guest-verifier-" + $runId)
New-Item -ItemType Directory -Path $script:GuestVerifierArtifactDirectory -ErrorAction Stop | Out-Null

$binding = Get-GuestVerifierInputBinding -DriverPackage $DriverPackage -HostBinDir $HostBinDir `
    -ExpectedDriverSha256 $ExpectedDriverSha256 -ExpectedInfSha256 $ExpectedInfSha256 `
    -ExpectedCatalogSha256 $ExpectedCatalogSha256 -ExpectedIoctlValidationSha256 $ExpectedIoctlValidationSha256 `
    -DriverSignerCert $DriverSignerCert -ExpectedDriverSignerCertSha256 $ExpectedDriverSignerCertSha256 `
    -ExpectedDriverSignerSubject $ExpectedDriverSignerSubject `
    -ExpectedDriverSignerThumbprint $ExpectedDriverSignerThumbprint
$hostSignature = Get-GuestVerifierHostAuthenticodeEvidence -Binding $binding
$signerCertificateArtifact = @($binding.artifacts | Where-Object { [string]$_.role -ceq "driver_signer_cert" })
if ($signerCertificateArtifact.Count -ne 1) {
    throw "sealed input binding does not identify exactly one public signer certificate"
}
$hostHarnessPath = (Resolve-Path -LiteralPath $PSCommandPath -ErrorAction Stop).Path
$hostHarnessHash = (Get-FileHash -LiteralPath $hostHarnessPath -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
Write-GuestVerifierArtifact -Name "input-binding.json" -Value ([pscustomobject]@{
        schema = [int]1
        run_id = $runId
        harness_path = $hostHarnessPath
        harness_sha256 = $hostHarnessHash
        host_authenticode = $hostSignature
        binding = $binding
    }) | Out-Null

$guestStage = "C:\ramshared\guest-verifier\$runId"
$expectedHardwareId = "ROOT\RAMSHARED"
$expectedVpdSerial = "ABCDEF0123456789"
$expectedServiceName = "ramshared"
$failure = $null
$verifierArmed = $false
$signerTrustMayBePresent = $false
$testSigningMayBeEnabled = $false
$testSigningRollbackProven = $false
$currentRunPackageMayBePresent = $false
$currentRunTeardownBinding = $null
$currentRunPublishedInf = ""
$ioPassStarted = $false
$normalVpdSerial = ""
$verifierVpdSerial = ""
$currentRunTeardownProven = $false
$pass = $false
$failurePhase = "host_preflight"
try {
    $vm = Get-VM -Name $VMName -ErrorAction Stop
    $expectedVmIdCanonical = ([guid]$ExpectedVMId).ToString("D").ToUpperInvariant()
    $actualVmIdCanonical = ([guid]$vm.Id).ToString("D").ToUpperInvariant()
    if ($actualVmIdCanonical -cne $expectedVmIdCanonical -or [int]$vm.Generation -ne 2) {
        throw "guest verifier exact Generation-2 VM identity is not the approved disposable lab"
    }
    if ($vm.State -ne "Running") {
        throw "refusing VM lifecycle mutation: $VMName must already be Running, observed $($vm.State)"
    }
    $firmware = Get-VMFirmware -VMName $VMName -ErrorAction Stop
    if ([string]$firmware.SecureBoot -cne "Off") {
        throw "guest verifier requires Secure Boot Off before any guest mutation"
    }
    Write-GuestVerifierArtifact -Name "firmware-preflight.json" -Value ([pscustomobject]@{
            schema = [int]1
            vm_name = $VMName
            vm_id = $actualVmIdCanonical
            generation = [int]$vm.Generation
            secure_boot = [string]$firmware.SecureBoot
        }) | Out-Null

    $readyRows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 45 -ScriptBlock {
        [pscustomobject]@{
            schema = [int]1
            computer_name = $env:COMPUTERNAME
            boot_time_utc = (Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop).LastBootUpTime.ToUniversalTime().ToString("o")
        } | ConvertTo-Json -Compress
    }
    $ready = ConvertFrom-GuestVerifierRows -Rows $readyRows -Operation "guest readiness query"
    Write-GuestVerifierArtifact -Name "guest-readiness.json" -Value $ready | Out-Null

    $failurePhase = "guest_preflight"
    $preflight = Invoke-GuestVerifierPreflight -ExpectedSignerSubject ([string]$binding.signer.subject) `
        -ExpectedSignerThumbprint ([string]$binding.signer.thumbprint)
    Write-GuestVerifierArtifact -Name "guest-preflight.json" -Value $preflight | Out-Null
    Assert-GuestVerifierPreflight -Evidence $preflight | Out-Null

    $stageRows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 45 -ScriptBlock {
        param($Stage)
        if (Test-Path -LiteralPath $Stage) {
            throw "fresh guest stage already exists"
        }
        New-Item -ItemType Directory -Path $Stage -ErrorAction Stop | Out-Null
        [pscustomobject]@{ schema = [int]1; stage = $Stage } | ConvertTo-Json -Compress
    } -ArgumentList @($guestStage)
    $stageReceipt = ConvertFrom-GuestVerifierRows -Rows $stageRows -Operation "fresh guest stage creation"
    if ([int]$stageReceipt.schema -ne 1 -or [string]$stageReceipt.stage -cne $guestStage) {
        throw "fresh guest stage receipt is invalid"
    }
    Write-GuestVerifierArtifact -Name "guest-stage.json" -Value $stageReceipt | Out-Null

    foreach ($artifact in @($binding.artifacts)) {
        Assert-GuestVerifierInputBindingUnchanged -Binding $binding | Out-Null
        $guestPath = Join-Path $guestStage ([string]$artifact.leaf)
        $beforeRows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 45 -ScriptBlock {
            param($Path)
            [pscustomobject]@{
                schema = [int]1
                exists = [bool](Test-Path -LiteralPath $Path -PathType Leaf)
                sha256 = if (Test-Path -LiteralPath $Path -PathType Leaf) {
                    (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
                } else {
                    ""
                }
            } | ConvertTo-Json -Compress
        } -ArgumentList @($guestPath)
        $beforeGuest = ConvertFrom-GuestVerifierRows -Rows $beforeRows -Operation "guest pre-copy hash $($artifact.role)"
        if ([int]$beforeGuest.schema -ne 1 -or [bool]$beforeGuest.exists) {
            throw "fresh guest copy destination is not absent for $($artifact.role)"
        }
        Invoke-GuestVerifierRemote -Operation copy_to -TimeoutSeconds $GuestOperationTimeoutSeconds -SourcePath ([string]$artifact.path) -DestinationPath $guestStage | Out-Null
        Assert-GuestVerifierInputBindingUnchanged -Binding $binding | Out-Null
        $afterRows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 45 -ScriptBlock {
            param($Path)
            if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
                throw "guest copy destination is missing"
            }
            [pscustomobject]@{
                schema = [int]1
                exists = $true
                sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
            } | ConvertTo-Json -Compress
        } -ArgumentList @($guestPath)
        $afterGuest = ConvertFrom-GuestVerifierRows -Rows $afterRows -Operation "guest post-copy hash $($artifact.role)"
        if ([int]$afterGuest.schema -ne 1 -or -not [bool]$afterGuest.exists -or
            (Normalize-GuestVerifierSha256 ([string]$afterGuest.sha256) "$($artifact.role) guest SHA-256") -cne
            (Normalize-GuestVerifierSha256 ([string]$artifact.sha256) "$($artifact.role) bound SHA-256")) {
            throw "guest copy hash is not the sealed source for $($artifact.role)"
        }
        Write-GuestVerifierArtifact -Name ("copy-" + $artifact.role + ".json") -Value ([pscustomobject]@{
                schema = [int]1
                run_id = $runId
                role = $artifact.role
                source_sha256 = $artifact.sha256
                guest_before = $beforeGuest
                guest_after = $afterGuest
            }) | Out-Null
    }

    $guestBindingRows = Invoke-GuestVerifierRemote -Operation invoke -TimeoutSeconds 60 -ScriptBlock {
        param($Stage, $ExpectedSys, $ExpectedInf, $ExpectedCat, $ExpectedIoctl, $ExpectedSignerCert, $SignerCertLeaf)
        $inputs = @(
            [pscustomobject]@{ role = "driver_sys"; leaf = "ramshared.sys"; expected = $ExpectedSys },
            [pscustomobject]@{ role = "driver_inf"; leaf = "ramshared.inf"; expected = $ExpectedInf },
            [pscustomobject]@{ role = "driver_cat"; leaf = "ramshared.cat"; expected = $ExpectedCat },
            [pscustomobject]@{ role = "ioctl_validation"; leaf = "Invoke-WinDriveIoctlValidation.ps1"; expected = $ExpectedIoctl },
            [pscustomobject]@{ role = "driver_signer_cert"; leaf = $SignerCertLeaf; expected = $ExpectedSignerCert }
        )
        $observed = @()
        foreach ($input in $inputs) {
            $path = Join-Path $Stage $input.leaf
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                throw "sealed guest input is missing: $($input.role)"
            }
            $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256 -ErrorAction Stop).Hash.ToUpperInvariant()
            if ($actual -cne ([string]$input.expected).ToUpperInvariant()) {
                throw "sealed guest input changed: $($input.role)"
            }
            $observed += [pscustomobject]@{
                role = $input.role
                path = (Resolve-Path -LiteralPath $path -ErrorAction Stop).Path
                sha256 = $actual
            }
        }
        [pscustomobject]@{
            schema = [int]1
            artifacts = @($observed)
        } | ConvertTo-Json -Depth 6 -Compress
    } -ArgumentList @($guestStage, $ExpectedDriverSha256, $ExpectedInfSha256, $ExpectedCatalogSha256, `
        $ExpectedIoctlValidationSha256, [string]$signerCertificateArtifact[0].sha256, [string]$signerCertificateArtifact[0].leaf)
    $guestBinding = ConvertFrom-GuestVerifierRows -Rows $guestBindingRows -Operation "sealed guest staging rehash"
    if ([int]$guestBinding.schema -ne 1 -or @($guestBinding.artifacts).Count -ne 5) {
        throw "sealed guest staging rehash is incomplete"
    }
    Write-GuestVerifierArtifact -Name "guest-stage-binding.json" -Value ([pscustomobject]@{
            schema = [int]1
            run_id = $runId
            guest_binding = $guestBinding
        }) | Out-Null
    Assert-GuestVerifierInputBindingUnchanged -Binding $binding | Out-Null

    $stagedSigner = Get-GuestVerifierStagedSignerCertificateEvidence -GuestStage $guestStage `
        -CertificateLeaf ([string]$signerCertificateArtifact[0].leaf) `
        -ExpectedSubject ([string]$binding.signer.subject) `
        -ExpectedThumbprint ([string]$binding.signer.thumbprint)
    Write-GuestVerifierArtifact -Name "staged-signer-certificate.json" -Value $stagedSigner | Out-Null
    Assert-GuestVerifierSignerIdentity -Certificate $stagedSigner -ExpectedSubject ([string]$binding.signer.subject) `
        -ExpectedThumbprint ([string]$binding.signer.thumbprint) -Role "staged public" | Out-Null

    $failurePhase = "signer_trust"
    $signerTrustMayBePresent = $true
    $signerTrust = Install-GuestVerifierSignerTrust -GuestStage $guestStage `
        -CertificateLeaf ([string]$signerCertificateArtifact[0].leaf) `
        -ExpectedSubject ([string]$binding.signer.subject) `
        -ExpectedThumbprint ([string]$binding.signer.thumbprint)
    Write-GuestVerifierArtifact -Name "signer-trust-install.json" -Value $signerTrust | Out-Null
    Assert-GuestVerifierSignerTrustReceipt -Evidence $signerTrust -ExpectedSubject ([string]$binding.signer.subject) `
        -ExpectedThumbprint ([string]$binding.signer.thumbprint) | Out-Null

    $failurePhase = "testsigning_enable"
    $testSigningMayBeEnabled = $true
    $testSigningEnable = Set-GuestVerifierTestSigning -Enabled $true
    Write-GuestVerifierArtifact -Name "testsigning-enable-query.json" -Value $testSigningEnable | Out-Null
    Assert-GuestVerifierTestSigningReceipt -Evidence $testSigningEnable -ExpectedEnabled $true | Out-Null
    $testSigningBootBefore = Get-GuestVerifierBootTime
    $testSigningRestart = Request-GuestVerifierRestart -DelaySeconds $GuestRestartDelaySeconds
    Write-GuestVerifierArtifact -Name "testsigning-enable-restart-receipt.json" -Value $testSigningRestart | Out-Null
    $testSigningBootChange = Wait-GuestVerifierBootTimeChange -Before $testSigningBootBefore
    Write-GuestVerifierArtifact -Name "testsigning-enable-boot-change.json" -Value $testSigningBootChange | Out-Null
    Assert-GuestVerifierBootChangeReceipt -Receipt $testSigningBootChange | Out-Null
    $testSigningPostBoot = Get-GuestVerifierTestSigningState
    Write-GuestVerifierArtifact -Name "testsigning-post-boot-query.json" -Value $testSigningPostBoot | Out-Null
    Assert-GuestVerifierTestSigningState -Evidence $testSigningPostBoot -ExpectedEnabled $true | Out-Null
    $signerTrustPostBoot = Get-GuestVerifierSignerTrustEvidence -ExpectedSubject ([string]$binding.signer.subject) `
        -ExpectedThumbprint ([string]$binding.signer.thumbprint)
    Write-GuestVerifierArtifact -Name "signer-trust-post-boot.json" -Value $signerTrustPostBoot | Out-Null
    Assert-GuestVerifierSignerTrustReceipt -Evidence $signerTrustPostBoot `
        -ExpectedSubject ([string]$binding.signer.subject) -ExpectedThumbprint ([string]$binding.signer.thumbprint) | Out-Null
    $guestSignature = Get-GuestVerifierGuestSignatureEvidence -GuestStage $guestStage
    Write-GuestVerifierArtifact -Name "guest-post-trust-authenticode.json" -Value $guestSignature | Out-Null
    Assert-GuestVerifierGuestSignatureEvidence -Evidence $guestSignature `
        -ExpectedSubject ([string]$binding.signer.subject) -ExpectedThumbprint ([string]$binding.signer.thumbprint) | Out-Null

    $failurePhase = "package_publish"
    $currentRunPackageMayBePresent = $true
    $publish = Publish-GuestVerifierPackage -RunId $runId -GuestStage $guestStage `
        -ExpectedSignerSubject ([string]$binding.signer.subject) -ExpectedSignerThumbprint ([string]$binding.signer.thumbprint)
    Write-GuestVerifierArtifact -Name "signed-package-publish.json" -Value $publish | Out-Null
    if ([int]$publish.schema -ne 1 -or [string]$publish.run_id -cne $runId -or
        [string]$publish.sys_signature -cne "Valid" -or [string]$publish.cat_signature -cne "Valid" -or
        [string]$publish.driver_subject -cne [string]$binding.signer.subject -or
        [string]$publish.catalog_subject -cne [string]$binding.signer.subject -or
        (Normalize-GuestVerifierSignerThumbprint ([string]$publish.driver_thumbprint) "published driver signer thumbprint") -cne [string]$binding.signer.thumbprint -or
        (Normalize-GuestVerifierSignerThumbprint ([string]$publish.catalog_thumbprint) "published catalog signer thumbprint") -cne [string]$binding.signer.thumbprint -or
        [int]$publish.publish_exit_code -ne 0 -or [int]$publish.published_package_count -ne 1 -or
        [string]$publish.package_original_inf -ine "ramshared.inf") {
        throw "signed package publish evidence is incomplete"
    }
    $currentRunPublishedInf = Normalize-GuestVerifierPublishedInf ([string]$publish.published_inf) "signed package publish published INF"

    $failurePhase = "root_creation"
    $rootCreation = Create-GuestVerifierRoot -RunId $runId `
        -ExpectedPublishedInf $currentRunPublishedInf -ExpectedDriverHash $ExpectedDriverSha256 `
        -ExpectedInfHash $ExpectedInfSha256 -ExpectedCatalogHash $ExpectedCatalogSha256
    Write-GuestVerifierArtifact -Name "signed-package-root.json" -Value $rootCreation | Out-Null
    if ([int]$rootCreation.schema -ne 1 -or [string]$rootCreation.run_id -cne $runId -or
        [string]$rootCreation.worker_status -cne "ok" -or
        -not [string]::IsNullOrEmpty([string]$rootCreation.worker_failure_stage) -or
        [int]$rootCreation.worker_failure_hresult -ne 0 -or
        -not [string]::IsNullOrEmpty([string]$rootCreation.worker_failure_type) -or
        (Normalize-GuestVerifierPublishedInf ([string]$rootCreation.published_inf) "ROOT receipt published INF") -cne $currentRunPublishedInf -or
        [int]$rootCreation.published_package_count -ne 1 -or
        [string]$rootCreation.package_original_inf -ine "ramshared.inf" -or
        -not ($rootCreation.PSObject.Properties.Name -contains "package_visibility_elapsed_ms") -or
        [int64]$rootCreation.package_visibility_elapsed_ms -lt 0 -or
        [string]$rootCreation.driver_store_sys_sha256 -cne $ExpectedDriverSha256 -or
        [string]$rootCreation.driver_store_inf_sha256 -cne $ExpectedInfSha256 -or
        [string]$rootCreation.driver_store_catalog_sha256 -cne $ExpectedCatalogSha256 -or
        -not [bool]$rootCreation.root_creation_ok -or
        [string]$rootCreation.root_creation_stage -cne "ok" -or
        [int]$rootCreation.root_creation_win32_error -ne 0 -or
        [int]$rootCreation.root_count -ne 1 -or
        [string]$rootCreation.root_instance_id -cnotmatch '(?i)^ROOT\\RAMSHARED\\' -or
        [int]$rootCreation.service_count -ne 1 -or
        [string]$rootCreation.service_name -cne "ramshared") {
        throw "signed package ROOT evidence is incomplete"
    }
    $install = [pscustomobject]@{
        schema = [int]1
        run_id = $runId
        sys_signature = [string]$publish.sys_signature
        cat_signature = [string]$publish.cat_signature
        driver_subject = [string]$publish.driver_subject
        catalog_subject = [string]$publish.catalog_subject
        driver_thumbprint = [string]$publish.driver_thumbprint
        catalog_thumbprint = [string]$publish.catalog_thumbprint
        install_exit_code = [int]$publish.publish_exit_code
        published_inf = $currentRunPublishedInf
        published_package_count = [int]$rootCreation.published_package_count
        package_original_inf = [string]$rootCreation.package_original_inf
        root_creation = [string]$rootCreation.root_creation
        root_instance_id = [string]$rootCreation.root_instance_id
    }
    Write-GuestVerifierArtifact -Name "signed-package-install.json" -Value $install | Out-Null

    $failurePhase = "normal_restart"
    $normalBootBefore = Get-GuestVerifierBootTime
    $normalRestart = Request-GuestVerifierRestart -DelaySeconds $GuestRestartDelaySeconds
    Write-GuestVerifierArtifact -Name "normal-restart-receipt.json" -Value $normalRestart | Out-Null
    $normalBootChange = Wait-GuestVerifierBootTimeChange -Before $normalBootBefore
    Write-GuestVerifierArtifact -Name "normal-restart-boot-change.json" -Value $normalBootChange | Out-Null
    Assert-GuestVerifierBootChangeReceipt -Receipt $normalBootChange | Out-Null

    $failurePhase = "normal_io"
    $normalIdentity = Get-GuestVerifierCurrentIdentity -RunId $runId -GuestStage $guestStage -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedIoctlHash $ExpectedIoctlValidationSha256
    Write-GuestVerifierArtifact -Name "normal-current-identity.json" -Value $normalIdentity | Out-Null
    Assert-GuestVerifierCurrentIdentity -Identity $normalIdentity -RunId $runId -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedIoctlHash $ExpectedIoctlValidationSha256 | Out-Null

    $ioPassStarted = $true
    $normalPass = Invoke-GuestVerifierIoctlPass -RunId $runId -GuestStage $guestStage -PassName "normal" -VerifierExpected $false `
        -ExpectedVpdSerial $expectedVpdSerial
    Write-GuestVerifierArtifact -Name "normal-ioctl-evidence.json" -Value $normalPass | Out-Null
    Assert-GuestVerifierPassEvidence -Evidence $normalPass -RunId $runId -VerifierExpected $false `
        -ExpectedVpdSerial $expectedVpdSerial | Out-Null
    $normalVpdSerial = [string]$normalPass.vpd_serial

    $failurePhase = "verifier_enable"
    $verifierArmed = $true
    $verifierEnable = Enable-GuestVerifier
    Write-GuestVerifierArtifact -Name "verifier-enable-query.json" -Value $verifierEnable | Out-Null
    Assert-GuestVerifierEnabled -Evidence $verifierEnable | Out-Null
    $failurePhase = "verifier_restart"
    $verifierBootBefore = Get-GuestVerifierBootTime
    $verifierRestart = Request-GuestVerifierRestart -DelaySeconds $GuestRestartDelaySeconds
    Write-GuestVerifierArtifact -Name "verifier-restart-receipt.json" -Value $verifierRestart | Out-Null
    $verifierBootChange = Wait-GuestVerifierBootTimeChange -Before $verifierBootBefore
    Write-GuestVerifierArtifact -Name "verifier-restart-boot-change.json" -Value $verifierBootChange | Out-Null
    Assert-GuestVerifierBootChangeReceipt -Receipt $verifierBootChange | Out-Null

    $failurePhase = "verifier_io"
    $verifierQuery = Get-GuestVerifierQuery
    Write-GuestVerifierArtifact -Name "verifier-post-restart-query.json" -Value $verifierQuery | Out-Null
    if ([int]$verifierQuery.schema -ne 1 -or [int]$verifierQuery.query_exit_code -ne 0 -or
        [int]$verifierQuery.settings_exit_code -ne 0 -or ([bool]$verifierQuery.target_present -ne $true) -or
        [int]$verifierQuery.target_count -ne 1 -or ([bool]$verifierQuery.all_drivers -ne $false) -or
        ([bool]$verifierQuery.flags_exact -ne $true)) {
        throw "Driver Verifier did not remain exactly armed after its guest reboot"
    }

    $verifierIdentity = Get-GuestVerifierCurrentIdentity -RunId $runId -GuestStage $guestStage -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedIoctlHash $ExpectedIoctlValidationSha256
    Write-GuestVerifierArtifact -Name "verifier-current-identity.json" -Value $verifierIdentity | Out-Null
    Assert-GuestVerifierCurrentIdentity -Identity $verifierIdentity -RunId $runId -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedIoctlHash $ExpectedIoctlValidationSha256 | Out-Null

    $verifierPass = Invoke-GuestVerifierIoctlPass -RunId $runId -GuestStage $guestStage -PassName "verifier" -VerifierExpected $true `
        -ExpectedVpdSerial $expectedVpdSerial
    Write-GuestVerifierArtifact -Name "verifier-ioctl-evidence.json" -Value $verifierPass | Out-Null
    Assert-GuestVerifierPassEvidence -Evidence $verifierPass -RunId $runId -VerifierExpected $true `
        -ExpectedVpdSerial $expectedVpdSerial | Out-Null
    $verifierVpdSerial = [string]$verifierPass.vpd_serial

    $failurePhase = "verifier_reset"
    $reset = Reset-GuestVerifier
    Write-GuestVerifierArtifact -Name "verifier-reset-query.json" -Value $reset | Out-Null
    Assert-GuestVerifierReset -Evidence $reset | Out-Null
    $resetBootBefore = Get-GuestVerifierBootTime
    $resetRestart = Request-GuestVerifierRestart -DelaySeconds $GuestRestartDelaySeconds
    Write-GuestVerifierArtifact -Name "verifier-reset-restart-receipt.json" -Value $resetRestart | Out-Null
    $resetBootChange = Wait-GuestVerifierBootTimeChange -Before $resetBootBefore
    Write-GuestVerifierArtifact -Name "verifier-reset-restart-boot-change.json" -Value $resetBootChange | Out-Null
    Assert-GuestVerifierBootChangeReceipt -Receipt $resetBootChange | Out-Null

    $resetQuery = Get-GuestVerifierQuery
    Write-GuestVerifierArtifact -Name "verifier-post-reset-query.json" -Value $resetQuery | Out-Null
    if ([int]$resetQuery.schema -ne 1 -or [int]$resetQuery.query_exit_code -ne 0 -or
        [int]$resetQuery.settings_exit_code -ne 0 -or ([bool]$resetQuery.target_present -ne $false) -or
        [int]$resetQuery.target_count -ne 0 -or ([bool]$resetQuery.all_drivers -ne $false)) {
        throw "Driver Verifier reset did not persist across its required guest reboot"
    }
    $verifierArmed = $false

    $failurePhase = "current_run_teardown"
    $currentRunTeardownBinding = Get-GuestVerifierCurrentRunTeardownBinding -RunId $runId `
        -PublishedInf $currentRunPublishedInf -ExpectedDriverHash $ExpectedDriverSha256 `
        -ExpectedInfHash $ExpectedInfSha256 -ExpectedCatalogHash $ExpectedCatalogSha256 `
        -ExpectedHardwareId $expectedHardwareId -ExpectedVpdSerial $expectedVpdSerial `
        -ExpectedServiceName $expectedServiceName -IoPassStarted $ioPassStarted -NormalVpdSerial $normalVpdSerial `
        -VerifierVpdSerial $verifierVpdSerial
    Write-GuestVerifierArtifact -Name "current-run-teardown-binding.json" -Value $currentRunTeardownBinding | Out-Null
    Assert-GuestVerifierCurrentRunTeardownBinding -Binding $currentRunTeardownBinding -RunId $runId `
        -ExpectedPublishedInf $currentRunPublishedInf -ExpectedDriverHash $ExpectedDriverSha256 `
        -ExpectedInfHash $ExpectedInfSha256 -ExpectedCatalogHash $ExpectedCatalogSha256 `
        -ExpectedHardwareId $expectedHardwareId -ExpectedVpdSerial $expectedVpdSerial `
        -ExpectedServiceName $expectedServiceName | Out-Null

    $rootRemoval = Remove-GuestVerifierCurrentRunRoot -Binding $currentRunTeardownBinding `
        -RunId $runId -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedInfHash $ExpectedInfSha256 `
        -ExpectedCatalogHash $ExpectedCatalogSha256 -ExpectedHardwareId $expectedHardwareId `
        -ExpectedVpdSerial $expectedVpdSerial -ExpectedServiceName $expectedServiceName
    Write-GuestVerifierArtifact -Name "current-run-root-removal.json" -Value $rootRemoval | Out-Null
    $rootRemovedState = Get-GuestVerifierPostPublishCleanupState -RunId $runId `
        -ExpectedPublishedInf $currentRunPublishedInf
    Assert-GuestVerifierRootRemovedState -State $rootRemovedState -Binding $currentRunTeardownBinding `
        -RunId $runId -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedInfHash $ExpectedInfSha256 `
        -ExpectedCatalogHash $ExpectedCatalogSha256 | Out-Null
    Write-GuestVerifierArtifact -Name "current-run-root-removed-state.json" -Value $rootRemovedState | Out-Null
    $rootRemovalRebooted = $false
    if ([string]$rootRemovedState.service_state -ceq "Running") {
        $rootRemovalBootBefore = Get-GuestVerifierBootTime
        $rootRemovalRestart = Request-GuestVerifierRestart -DelaySeconds $GuestRestartDelaySeconds
        Write-GuestVerifierArtifact -Name "current-run-root-removal-restart-receipt.json" `
            -Value $rootRemovalRestart | Out-Null
        $rootRemovalBootChange = Wait-GuestVerifierBootTimeChange -Before $rootRemovalBootBefore
        Write-GuestVerifierArtifact -Name "current-run-root-removal-boot-change.json" `
            -Value $rootRemovalBootChange | Out-Null
        Assert-GuestVerifierBootChangeReceipt -Receipt $rootRemovalBootChange | Out-Null
        $rootRemovalRebooted = $true
        $rootRemovedState = Get-GuestVerifierPostPublishCleanupState -RunId $runId `
            -ExpectedPublishedInf $currentRunPublishedInf
    }
    Assert-GuestVerifierRootRemovedState -State $rootRemovedState -Binding $currentRunTeardownBinding `
        -RunId $runId -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedInfHash $ExpectedInfSha256 `
        -ExpectedCatalogHash $ExpectedCatalogSha256 -RequireStopped | Out-Null
    Write-GuestVerifierArtifact -Name "current-run-root-removed-ready.json" -Value $rootRemovedState | Out-Null
    $rootRemovedActions = Remove-GuestVerifierRootRemovedArtifacts -Binding $currentRunTeardownBinding `
        -RootRemovedState $rootRemovedState -RunId $runId -ExpectedDriverHash $ExpectedDriverSha256 `
        -ExpectedInfHash $ExpectedInfSha256 -ExpectedCatalogHash $ExpectedCatalogSha256
    Write-GuestVerifierArtifact -Name "current-run-root-removed-actions.json" -Value $rootRemovedActions | Out-Null
    $currentRunFinalState = Get-GuestVerifierPostPublishCleanupState -RunId $runId `
        -ExpectedPublishedInf $currentRunPublishedInf
    $currentRunTeardown = [pscustomobject]@{
        schema = [int]1
        run_id = $runId
        published_inf = $currentRunPublishedInf
        service_stop_exit_code = [int]0
        service_stop_action = if ($rootRemovalRebooted) { "stopped_after_root_reboot" } else { "stopped_by_root_removal" }
        device_remove_exit_code = [int]$rootRemoval.action.device_remove_exit_code
        service_delete_exit_code = [int]$rootRemovedActions.service_delete_exit_code
        service_delete_action = [string]$rootRemovedActions.service_delete_action
        driver_delete_exit_code = [int]$rootRemovedActions.driver_delete_exit_code
        retired_node_delete_exit_code = [int]$rootRemovedActions.retired_node_delete_exit_code
        retired_node_delete_action = [string]$rootRemovedActions.retired_node_delete_action
        retired_node_instance_id = [string]$rootRemovedActions.retired_node_instance_id
        package_count = [int]$currentRunFinalState.package_count
        published_inf_count = [int]$currentRunFinalState.published_inf_count
        root_count = [int]$currentRunFinalState.root_count
        service_count = [int]$currentRunFinalState.service_count
        ramshared_disk_count = [int]$currentRunFinalState.ramshared_disk_count
        ramshared_pnp_disk_count = [int]$currentRunFinalState.ramshared_pnp_disk_count
    }
    Write-GuestVerifierArtifact -Name "current-run-teardown.json" -Value $currentRunTeardown | Out-Null
    Assert-GuestVerifierCurrentRunTeardownEvidence -Evidence $currentRunTeardown -RunId $runId `
        -ExpectedPublishedInf $currentRunPublishedInf -RequireActionReceipts | Out-Null
    $currentRunPackageMayBePresent = $false

    $failurePhase = "signer_trust_remove"
    $signerTrustRemoval = Remove-GuestVerifierSignerTrust -ExpectedSubject ([string]$binding.signer.subject) `
        -ExpectedThumbprint ([string]$binding.signer.thumbprint)
    Write-GuestVerifierArtifact -Name "signer-trust-removal.json" -Value $signerTrustRemoval | Out-Null
    Assert-GuestVerifierSignerTrustRemovalReceipt -Evidence $signerTrustRemoval `
        -ExpectedSubject ([string]$binding.signer.subject) -ExpectedThumbprint ([string]$binding.signer.thumbprint) | Out-Null
    $signerTrustMayBePresent = $false

    $failurePhase = "testsigning_disable"
    $testSigningDisable = Set-GuestVerifierTestSigning -Enabled $false
    Write-GuestVerifierArtifact -Name "testsigning-disable-query.json" -Value $testSigningDisable | Out-Null
    Assert-GuestVerifierTestSigningReceipt -Evidence $testSigningDisable -ExpectedEnabled $false | Out-Null
    $testSigningDisableBootBefore = Get-GuestVerifierBootTime
    $testSigningDisableRestart = Request-GuestVerifierRestart -DelaySeconds $GuestRestartDelaySeconds
    Write-GuestVerifierArtifact -Name "testsigning-disable-restart-receipt.json" -Value $testSigningDisableRestart | Out-Null
    $testSigningDisableBootChange = Wait-GuestVerifierBootTimeChange -Before $testSigningDisableBootBefore
    Write-GuestVerifierArtifact -Name "testsigning-disable-boot-change.json" -Value $testSigningDisableBootChange | Out-Null
    Assert-GuestVerifierBootChangeReceipt -Receipt $testSigningDisableBootChange | Out-Null
    $testSigningDisabledPostBoot = Get-GuestVerifierTestSigningState
    Write-GuestVerifierArtifact -Name "testsigning-disabled-post-boot-query.json" -Value $testSigningDisabledPostBoot | Out-Null
    Assert-GuestVerifierTestSigningState -Evidence $testSigningDisabledPostBoot -ExpectedEnabled $false | Out-Null
    $testSigningMayBeEnabled = $false

    $failurePhase = "final_zero_residue"
    $finalCurrentRunZeroResidue = Get-GuestVerifierCurrentRunZeroResidueEvidence -RunId $runId `
        -PublishedInf $currentRunPublishedInf
    Write-GuestVerifierArtifact -Name "final-current-run-zero-residue.json" -Value $finalCurrentRunZeroResidue | Out-Null
    Assert-GuestVerifierCurrentRunTeardownEvidence -Evidence $finalCurrentRunZeroResidue -RunId $runId `
        -ExpectedPublishedInf $currentRunPublishedInf | Out-Null
    $currentRunTeardownProven = $true

    $finalPreflight = Invoke-GuestVerifierPreflight -ExpectedSignerSubject ([string]$binding.signer.subject) `
        -ExpectedSignerThumbprint ([string]$binding.signer.thumbprint)
    Write-GuestVerifierArtifact -Name "final-zero-residue-preflight.json" -Value $finalPreflight | Out-Null
    Assert-GuestVerifierPreflight -Evidence $finalPreflight | Out-Null
    $testSigningRollbackProven = $true
    $pass = $true
}
catch {
    $failure = $_
}
finally {
    $rollbackNeeded = [bool]($verifierArmed -or $signerTrustMayBePresent -or $testSigningMayBeEnabled -or
        $currentRunPackageMayBePresent)
    if ($rollbackNeeded) {
        $rollbackErrors = [Collections.Generic.List[string]]::new()
        $rollbackResetEvidence = $null
        $rollbackVerifierResetProven = $false
        $rollbackCurrentRunTeardown = $null
        $rollbackPackageCleanupAttempted = $false
        $rollbackRemovalEvidence = $null
        $rollbackDisableEvidence = $null
        $rollbackBootChange = $null
        try {
            $rollbackResetEvidence = Reset-GuestVerifier
            Write-GuestVerifierArtifact -Name "failure-verifier-reset-query.json" -Value $rollbackResetEvidence | Out-Null
            Assert-GuestVerifierReset -Evidence $rollbackResetEvidence | Out-Null
            if ([bool]$rollbackResetEvidence.reset_reboot_required) {
                $rollbackVerifierBootBefore = Get-GuestVerifierBootTime
                $rollbackVerifierRestart = Request-GuestVerifierRestart -DelaySeconds $GuestRestartDelaySeconds
                Write-GuestVerifierArtifact -Name "failure-verifier-reset-restart-receipt.json" `
                    -Value $rollbackVerifierRestart | Out-Null
                $rollbackVerifierBootChange = Wait-GuestVerifierBootTimeChange -Before $rollbackVerifierBootBefore
                Write-GuestVerifierArtifact -Name "failure-verifier-reset-boot-change.json" `
                    -Value $rollbackVerifierBootChange | Out-Null
                Assert-GuestVerifierBootChangeReceipt -Receipt $rollbackVerifierBootChange | Out-Null
            }
            $rollbackVerifierPostReset = Get-GuestVerifierQuery
            Write-GuestVerifierArtifact -Name "failure-verifier-post-reset-query.json" `
                -Value $rollbackVerifierPostReset | Out-Null
            if ([int]$rollbackVerifierPostReset.schema -ne 1 -or
                [int]$rollbackVerifierPostReset.query_exit_code -ne 0 -or
                [int]$rollbackVerifierPostReset.settings_exit_code -ne 0 -or
                ([bool]$rollbackVerifierPostReset.target_present -ne $false) -or
                [int]$rollbackVerifierPostReset.target_count -ne 0 -or
                ([bool]$rollbackVerifierPostReset.all_drivers -ne $false)) {
                throw "Driver Verifier reset did not reach exact zero before rollback teardown"
            }
            $verifierArmed = $false
            $rollbackVerifierResetProven = $true
        }
        catch {
            $rollbackErrors.Add("verifier_reset_failed")
        }
        if ($currentRunPackageMayBePresent) {
            if (-not $rollbackVerifierResetProven) {
                $rollbackErrors.Add("current_run_teardown_refused_verifier_reset_unproven")
            }
            elseif ([string]::IsNullOrWhiteSpace($currentRunPublishedInf) -or
                ($ioPassStarted -and [string]::IsNullOrWhiteSpace($normalVpdSerial) -and
                    [string]::IsNullOrWhiteSpace($verifierVpdSerial))) {
                $rollbackErrors.Add("current_run_teardown_refused_unbound")
            }
            else {
                try {
                    $postPublishState = Get-GuestVerifierPostPublishCleanupState -RunId $runId `
                        -ExpectedPublishedInf $currentRunPublishedInf
                    Write-GuestVerifierArtifact -Name "failure-post-publish-cleanup-state.json" `
                        -Value $postPublishState | Out-Null
                    $postPublishMode = Get-GuestVerifierPostPublishCleanupMode -State $postPublishState `
                        -RunId $runId -ExpectedPublishedInf $currentRunPublishedInf `
                        -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedInfHash $ExpectedInfSha256 `
                        -ExpectedCatalogHash $ExpectedCatalogSha256
                    if ($postPublishMode -ceq "published_only") {
                        $rollbackPackageCleanupAttempted = $true
                        $rollbackCurrentRunTeardown = Remove-GuestVerifierPublishedPackageOnly `
                            -Before $postPublishState -RunId $runId `
                            -ExpectedPublishedInf $currentRunPublishedInf `
                            -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedInfHash $ExpectedInfSha256 `
                            -ExpectedCatalogHash $ExpectedCatalogSha256
                        Write-GuestVerifierArtifact -Name "failure-published-only-teardown.json" `
                            -Value $rollbackCurrentRunTeardown | Out-Null
                    }
                    elseif ($postPublishMode -ceq "root_bound") {
                        $currentRunTeardownBinding = Get-GuestVerifierCurrentRunTeardownBinding -RunId $runId `
                            -PublishedInf $currentRunPublishedInf -ExpectedDriverHash $ExpectedDriverSha256 `
                            -ExpectedInfHash $ExpectedInfSha256 -ExpectedCatalogHash $ExpectedCatalogSha256 `
                            -ExpectedHardwareId $expectedHardwareId -ExpectedVpdSerial $expectedVpdSerial `
                            -ExpectedServiceName $expectedServiceName -IoPassStarted $ioPassStarted `
                            -NormalVpdSerial $normalVpdSerial -VerifierVpdSerial $verifierVpdSerial
                        Write-GuestVerifierArtifact -Name "failure-current-run-teardown-binding.json" `
                            -Value $currentRunTeardownBinding | Out-Null
                        Assert-GuestVerifierCurrentRunTeardownBinding -Binding $currentRunTeardownBinding -RunId $runId `
                            -ExpectedPublishedInf $currentRunPublishedInf -ExpectedDriverHash $ExpectedDriverSha256 `
                            -ExpectedInfHash $ExpectedInfSha256 -ExpectedCatalogHash $ExpectedCatalogSha256 `
                            -ExpectedHardwareId $expectedHardwareId -ExpectedVpdSerial $expectedVpdSerial `
                            -ExpectedServiceName $expectedServiceName | Out-Null
                        $rollbackPackageCleanupAttempted = $true
                        $rollbackCurrentRunTeardown = Remove-GuestVerifierCurrentRunArtifacts -Binding $currentRunTeardownBinding `
                            -RunId $runId -ExpectedDriverHash $ExpectedDriverSha256 -ExpectedInfHash $ExpectedInfSha256 `
                            -ExpectedCatalogHash $ExpectedCatalogSha256 -ExpectedHardwareId $expectedHardwareId `
                            -ExpectedVpdSerial $expectedVpdSerial -ExpectedServiceName $expectedServiceName
                        Write-GuestVerifierArtifact -Name "failure-current-run-teardown.json" `
                            -Value $rollbackCurrentRunTeardown | Out-Null
                        Assert-GuestVerifierCurrentRunTeardownEvidence -Evidence $rollbackCurrentRunTeardown -RunId $runId `
                            -ExpectedPublishedInf $currentRunPublishedInf -RequireActionReceipts | Out-Null
                    }
                    else {
                        throw "post-publish cleanup mode is invalid"
                    }
                    $currentRunPackageMayBePresent = $false
                }
                catch {
                    $rollbackErrors.Add("current_run_teardown_failed")
                }
            }
        }
        if ($signerTrustMayBePresent) {
            try {
                $rollbackRemovalEvidence = Remove-GuestVerifierSignerTrust `
                    -ExpectedSubject ([string]$binding.signer.subject) `
                    -ExpectedThumbprint ([string]$binding.signer.thumbprint)
                Write-GuestVerifierArtifact -Name "failure-signer-trust-removal.json" -Value $rollbackRemovalEvidence | Out-Null
                Assert-GuestVerifierSignerTrustRemovalReceipt -Evidence $rollbackRemovalEvidence `
                    -ExpectedSubject ([string]$binding.signer.subject) `
                    -ExpectedThumbprint ([string]$binding.signer.thumbprint) | Out-Null
            }
            catch {
                $rollbackErrors.Add("signer_trust_removal_failed")
            }
        }
        if ($testSigningMayBeEnabled) {
            try {
                $rollbackDisableEvidence = Set-GuestVerifierTestSigning -Enabled $false
                Write-GuestVerifierArtifact -Name "failure-testsigning-disable-query.json" -Value $rollbackDisableEvidence | Out-Null
                Assert-GuestVerifierTestSigningReceipt -Evidence $rollbackDisableEvidence -ExpectedEnabled $false | Out-Null
            }
            catch {
                $rollbackErrors.Add("testsigning_disable_failed")
            }
        }

        $requiresRollbackReboot = [bool]($verifierArmed -or $testSigningMayBeEnabled -or $rollbackPackageCleanupAttempted)
        if ($requiresRollbackReboot) {
            $rollbackBootBefore = $null
            try {
                $rollbackBootBefore = Get-GuestVerifierBootTime
            }
            catch {
                $rollbackErrors.Add("rollback_boot_time_query_failed")
            }
            if ($null -ne $rollbackBootBefore) {
                try {
                    $rollbackRestart = Request-GuestVerifierRestart -DelaySeconds $GuestRestartDelaySeconds
                    Write-GuestVerifierArtifact -Name "failure-rollback-restart-receipt.json" -Value $rollbackRestart | Out-Null
                    $rollbackBootChange = Wait-GuestVerifierBootTimeChange -Before $rollbackBootBefore
                    Write-GuestVerifierArtifact -Name "failure-rollback-boot-change.json" -Value $rollbackBootChange | Out-Null
                    Assert-GuestVerifierBootChangeReceipt -Receipt $rollbackBootChange | Out-Null
                }
                catch {
                    $rollbackErrors.Add("rollback_guest_reboot_failed")
                }
            }
        }

        if ($requiresRollbackReboot -and $null -ne $rollbackBootChange) {
            try {
                $rollbackTestSigningState = Get-GuestVerifierTestSigningState
                Write-GuestVerifierArtifact -Name "failure-testsigning-post-boot-query.json" -Value $rollbackTestSigningState | Out-Null
                Assert-GuestVerifierTestSigningState -Evidence $rollbackTestSigningState -ExpectedEnabled $false | Out-Null
            }
            catch {
                $rollbackErrors.Add("post_rollback_testsigning_query_failed")
            }
            try {
                $rollbackVerifierQuery = Get-GuestVerifierQuery
                Write-GuestVerifierArtifact -Name "failure-verifier-post-rollback-query.json" -Value $rollbackVerifierQuery | Out-Null
                if ([int]$rollbackVerifierQuery.schema -ne 1 -or [int]$rollbackVerifierQuery.query_exit_code -ne 0 -or
                    [int]$rollbackVerifierQuery.settings_exit_code -ne 0 -or
                    ([bool]$rollbackVerifierQuery.target_present -ne $false) -or
                    [int]$rollbackVerifierQuery.target_count -ne 0 -or
                    ([bool]$rollbackVerifierQuery.all_drivers -ne $false)) {
                    throw "Driver Verifier reset did not persist across rollback reboot"
                }
            }
            catch {
                $rollbackErrors.Add("post_rollback_verifier_query_failed")
            }
        }
        elseif ($requiresRollbackReboot) {
            $rollbackErrors.Add("rollback_guest_reboot_unproven")
        }
        if ($requiresRollbackReboot -and $null -ne $rollbackBootChange -and
            -not [string]::IsNullOrWhiteSpace($currentRunPublishedInf)) {
            try {
                $failureCurrentRunZeroResidue = Get-GuestVerifierCurrentRunZeroResidueEvidence -RunId $runId `
                    -PublishedInf $currentRunPublishedInf
                Write-GuestVerifierArtifact -Name "failure-final-current-run-zero-residue.json" -Value $failureCurrentRunZeroResidue | Out-Null
                Assert-GuestVerifierCurrentRunTeardownEvidence -Evidence $failureCurrentRunZeroResidue -RunId $runId `
                    -ExpectedPublishedInf $currentRunPublishedInf | Out-Null
                if ($currentRunPackageMayBePresent) {
                    throw "current-run package zero-residue query cannot prove an unbound cleanup"
                }
                $currentRunTeardownProven = $true
            }
            catch {
                $rollbackErrors.Add("post_rollback_current_run_zero_residue_failed")
            }
        }
        elseif ($currentRunPackageMayBePresent) {
            $rollbackErrors.Add("current_run_zero_residue_requires_reboot")
        }
        try {
            $rollbackPreflight = Invoke-GuestVerifierPreflight -ExpectedSignerSubject ([string]$binding.signer.subject) `
                -ExpectedSignerThumbprint ([string]$binding.signer.thumbprint)
            Write-GuestVerifierArtifact -Name "failure-zero-residue-preflight.json" -Value $rollbackPreflight | Out-Null
            Assert-GuestVerifierPreflight -Evidence $rollbackPreflight | Out-Null
        }
        catch {
            $rollbackErrors.Add("rollback_zero_residue_preflight_failed")
        }
        if ($rollbackErrors.Count -eq 0) {
            $verifierArmed = $false
            $signerTrustMayBePresent = $false
            $testSigningMayBeEnabled = $false
            $testSigningRollbackProven = $true
        }
        else {
            Write-GuestVerifierArtifact -Name "failure-rollback-error.json" -Value ([pscustomobject]@{
                    schema = [int]1
                    run_id = $runId
                    errors = @($rollbackErrors)
            }) | Out-Null
            if ($null -eq $failure) {
                $failure = New-Object -TypeName System.InvalidOperationException -ArgumentList @(
                    "guest TestSigning rollback is unproven: " + ($rollbackErrors -join "; "))
            }
        }
    }
    try {
        Assert-GuestVerifierInputBindingUnchanged -Binding $binding | Out-Null
    }
    catch {
        Write-GuestVerifierArtifact -Name "input-binding-final-error.json" -Value ([pscustomobject]@{
                schema = [int]1
                run_id = $runId
                error_code = "input_binding_final_failed"
            }) | Out-Null
        if ($null -eq $failure) {
            $failure = $_
        }
    }
    $failureCode = Get-GuestVerifierFailureCode -Failure $failure
    $summary = [pscustomobject]@{
        schema = [int]1
        run_id = $runId
        status = if ($pass -and $null -eq $failure -and -not $verifierArmed -and
            -not $signerTrustMayBePresent -and -not $testSigningMayBeEnabled -and
            -not $currentRunPackageMayBePresent -and $currentRunTeardownProven -and
            $testSigningRollbackProven) { "PASS" } else { "FAIL" }
        verifier_armed = [bool]$verifierArmed
        signer_trust_may_be_present = [bool]$signerTrustMayBePresent
        testsigning_may_be_enabled = [bool]$testSigningMayBeEnabled
        current_run_package_may_be_present = [bool]$currentRunPackageMayBePresent
        current_run_teardown_proven = [bool]$currentRunTeardownProven
        testsigning_rollback_proven = [bool]$testSigningRollbackProven
        error_code = $failureCode
        error = $failureCode
        failure_phase = $failurePhase
        artifact_directory = $script:GuestVerifierArtifactDirectory
    }
    Write-GuestVerifierArtifact -Name "summary.json" -Value $summary | Out-Null
}

if ($null -ne $failure) {
    throw ("guest Driver Verifier drill failed: {0}" -f (Get-GuestVerifierFailureCode -Failure $failure))
}
if (-not $pass -or $verifierArmed -or $signerTrustMayBePresent -or
    $testSigningMayBeEnabled -or $currentRunPackageMayBePresent -or
    -not $currentRunTeardownProven -or -not $testSigningRollbackProven) {
    throw "guest Driver Verifier drill did not reach a safe PASS terminal state"
}
Write-Output ("STATUS=PASS run_id={0} artifacts={1}" -f $runId, $script:GuestVerifierArtifactDirectory)
