#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$RepoRoot = ""
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrEmpty($RepoRoot)) {
    $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}
$results = [Collections.Generic.List[object]]::new()
function Assert-Static([bool]$Condition, [string]$Name, [string]$Detail) {
    if (-not $Condition) { throw "$Name failed: $Detail" }
    $results.Add([pscustomobject]@{ test = $Name; verdict = "PASS"; detail = $Detail })
}

$spec = Get-Content (Join-Path $RepoRoot `
        "docs\specs\no-milestone\windows-autonomous-broker-service\SPEC.md") -Raw
$broker = Get-Content (Join-Path $RepoRoot "crates\ramshared-winbroker\src\pipe.rs") -Raw
$consumer = Get-Content (Join-Path $RepoRoot "crates\ramshared-winsvc\src\main.rs") -Raw
$online = Get-Content (Join-Path $RepoRoot "crates\ramshared-winsvc\src\product_online.rs") -Raw
$installer = Get-Content (Join-Path $RepoRoot "scripts\windows\Install-RamSharedService.ps1") -Raw
$build = Get-Content (Join-Path $RepoRoot "scripts\windows\build-winsvc.bat") -Raw
$guestLifecycle = Get-Content (Join-Path $RepoRoot `
        "scripts\windows\Run-GuestAutonomousLifecycle.ps1") -Raw
$hostLifecycle = Get-Content (Join-Path $RepoRoot `
        "scripts\windows\Run-HostAutonomousLifecycle.ps1") -Raw

Assert-Static ($broker -match [regex]::Escape("\\.\pipe\RamSharedBroker.v1")) `
    "canonical_product_pipe" "fixed named-pipe endpoint is compiled into the broker"
Assert-Static ($broker -notmatch "TcpListener|TcpStream") `
    "no_broker_tcp_surface" "native broker pipe module has no TCP transport"
Assert-Static ($consumer -match "Global\\RamSharedProductInstall.v1") `
    "protected_installer_mutex_named" "product controller uses the SPEC mutex"
Assert-Static ($consumer -match "ReplaceFileW" -and $consumer -match "REPLACEFILE_WRITE_THROUGH") `
    "active_pointer_replacefile" "active pointer uses write-through ReplaceFileW"
Assert-Static ($consumer -match "RamSharedBroker" -and $consumer -match "ServiceDependency") `
    "two_service_dependency" "consumer SCM definition names the broker dependency"
Assert-Static ($online -notmatch "TcpStream") `
    "consumer_no_daily_tcp" "product Online path has no TCP client"
Assert-Static ($installer -notmatch "New-Service|sc.exe create|CreateService") `
    "single_installer_authority" "PowerShell wrapper does not implement a second SCM transaction"
Assert-Static ($spec -match "Run-GuestProductPackage.ps1" -and
    $spec -match "Run-HostAutonomousLifecycle.ps1") `
    "spec_harness_matrix" "SPEC names package and physical lifecycle harnesses"
Assert-Static ($build -match "ramshared-winbroker.exe" -and
    $build -match "ramshared-winsvc.exe") `
    "BROKER_BINARY_MATCH" "native build stages both independently hashed binaries"
Assert-Static ($consumer -match "ServiceDependency" -and
    $consumer -match "BROKER_SERVICE_NAME") `
    "SCM_DEPENDENCY_MATCH" "consumer definition contains the broker SCM dependency"
Assert-Static ($consumer -match "ServiceSidType::Unrestricted") `
    "SERVICE_SID_MATCH" "controller applies unrestricted service SID to both definitions"
Assert-Static ($broker -notmatch "TcpListener" -and $online -notmatch "TcpStream") `
    "DAILY_TCP_LISTENER_ABSENT" "daily broker/consumer surface is named-pipe only"
Assert-Static ($guestLifecycle -notmatch "Lab-LeaseBroker|TcpListener" -and
    $hostLifecycle -notmatch "Lab-LeaseBroker|TcpListener") `
    "NO_LAB_BROKER_REFERENCE" "autonomous guest/host campaigns consume packaged services"

$results
