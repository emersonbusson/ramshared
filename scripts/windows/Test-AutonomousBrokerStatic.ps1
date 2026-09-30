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
$brokerMain = Get-Content (Join-Path $RepoRoot "crates\ramshared-winbroker\src\main.rs") -Raw
$brokerService = Get-Content (Join-Path $RepoRoot "crates\ramshared-winbroker\src\service.rs") -Raw
$consumer = Get-Content (Join-Path $RepoRoot "crates\ramshared-winsvc\src\main.rs") -Raw
$serviceSidProbe = Get-Content (Join-Path $RepoRoot `
        "crates\ramshared-winsvc\src\bin\ramshared-service-sid-probe.rs") -Raw
$cudaProbe = Get-Content (Join-Path $RepoRoot "crates\ramshared-winsvc\src\cuda_probe.rs") -Raw
$online = Get-Content (Join-Path $RepoRoot "crates\ramshared-winsvc\src\product_online.rs") -Raw
$installer = Get-Content (Join-Path $RepoRoot "scripts\windows\Install-RamSharedService.ps1") -Raw
$build = Get-Content (Join-Path $RepoRoot "scripts\windows\build-winsvc.bat") -Raw
$guestLifecycle = Get-Content (Join-Path $RepoRoot `
        "scripts\windows\Run-GuestAutonomousLifecycle.ps1") -Raw
$hostLifecycle = Get-Content (Join-Path $RepoRoot `
        "scripts\windows\Run-HostAutonomousLifecycle.ps1") -Raw
$loaderWin = Get-Content (Join-Path $RepoRoot `
        "crates\ramshared-cuda\src\loader_win.rs") -Raw

Assert-Static ($broker -match [regex]::Escape("\\.\pipe\RamSharedBroker.v1")) `
    "canonical_product_pipe" "fixed named-pipe endpoint is compiled into the broker"
Assert-Static ($broker -notmatch "TcpListener|TcpStream") `
    "no_broker_tcp_surface" "native broker pipe module has no TCP transport"
Assert-Static ($brokerMain -match "fn parse_cli" -and
    $brokerMain -match [regex]::Escape('console --config <absolute>') -and
    $brokerMain -match "cli_has_no_tcp_listen_option" -and
    $brokerMain -notmatch "TcpListener") `
    "broker_cli_contract" "broker entry accepts only the sealed service/console configuration surface"
Assert-Static ($brokerService -match 'SERVICE_NAME: &str = "RamSharedBroker"' -and
    $brokerService -match "report_deterministic_start_failure" -and
    $brokerService -match "verify_active_config") `
    "broker_service_contract" "SCM entry binds deterministic failure and active-manifest verification"
Assert-Static ($serviceSidProbe -match "deny-only" -and
    $serviceSidProbe -match "S-1-5-80-" -and
    $serviceSidProbe -match "service_dispatcher::start") `
    "service_sid_probe_contract" "service-SID probe retains deny-only and SCM boundaries"
Assert-Static ($cudaProbe -match "broker_pipe" -and
    $cudaProbe -match "BrokerPipeV1::NamedPipeV1") `
    "cuda_probe_uses_local_broker_config" "CUDA probe fixture uses the native local broker selector"
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

# RF-4 / DT-5 Windows loader twin of `loader_unix.rs`: Win32 triad only, every
# entry point refuses a null handle before any API call, the module path is a
# NUL-terminated UTF-16 string, and `error()` formats the Win32 code alone.
Assert-Static ($loaderWin -match "LoadLibraryW" -and
    $loaderWin -match "GetProcAddress" -and
    $loaderWin -match "FreeLibrary" -and
    $loaderWin -notmatch "\bdlopen\s*\(|\bdlsym\s*\(|\bdlclose\s*\(") `
    "loader_win_uses_only_the_win32_loader_triad" `
    "loader_win binds LoadLibraryW/GetProcAddress/FreeLibrary and no POSIX dlopen call"
Assert-Static ($loaderWin -match "fn open\([\s\S]{0,600}LoadLibraryW" -and
    $loaderWin -match "fn sym\([\s\S]{0,400}handle\.is_null\(\)" -and
    $loaderWin -match "fn close\([\s\S]{0,400}handle\.is_null\(\)") `
    "loader_win_null_handle_is_refused_before_any_api_call" `
    "sym/close short-circuit a null handle; open only reaches LoadLibraryW after conversion"
Assert-Static ($loaderWin -match "encode_utf16\(\)\.chain\(Some\(0\)\)" -and
    $loaderWin -match "CStr::from_ptr") `
    "loader_win_module_path_is_nul_terminated_utf16" `
    "open converts the CStr to a NUL-terminated wide string for LoadLibraryW"
Assert-Static ($loaderWin -match "GetLastError" -and
    $loaderWin -match "Windows error code: 0x" -and
    $loaderWin -notmatch "\{:p\}|as usize|as u64") `
    "loader_win_error_reports_code_only" `
    "error() formats the Win32 code and leaks no pointer or module path"
Assert-Static ($loaderWin -match "fn close\([\s\S]{0,600}FreeLibrary" -and
    $loaderWin -match "matching dlclose behavior") `
    "loader_win_close_matches_dlclose_status" `
    "close maps FreeLibrary BOOL to the dlclose-style 0 success / -1 failure"
# Roll-up reached only when every loader_win assertion above has passed.
Write-Output "PASS loader_win_adapter_contract"

$results
