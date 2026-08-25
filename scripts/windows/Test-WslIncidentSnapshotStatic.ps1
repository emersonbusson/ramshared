#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path $PSScriptRoot "Capture-WslIncidentSnapshot.ps1"
}
if (-not (Test-Path -LiteralPath $HarnessPath -PathType Leaf)) {
    throw "wsl_incident_snapshot: target script is missing"
}

$source = Get-Content -LiteralPath $HarnessPath -Raw
foreach ($required in @(
        "Capture-WslIncidentSnapshot",
        "[switch]`$Run",
        'state = "PLAN"',
        "official_wsl_log_collector_url",
        "https://raw.githubusercontent.com/microsoft/WSL/master/diagnostics/collect-wsl-logs.ps1",
        "Invoke-BoundedNativeCommand",
        "WaitForExit",
        "snapshot_command_timeout",
        "wsl.exe",
        "StandardOutputEncoding",
        "StandardErrorEncoding",
        "[Text.Encoding]::Unicode",
        "--version",
        "--status",
        "-l -v",
        "Get-CimInstance Win32_Service",
        "WslService",
        "LxssManager",
        "vmcompute",
        "Get-WslConfigSnapshot",
        ".wslconfig",
        "custom_kernel_configured",
        "wsl-config.json",
        "Get-WinEvent",
        "Microsoft.Windows.Lxss.Manager",
        "Microsoft-Windows-Hyper-V-Compute",
        "wsl-incident-manifest.json",
        "Get-FileHash",
        "SHA256",
        "artifact_root"
    )) {
    if (-not $source.Contains($required)) {
        throw "wsl_incident_snapshot: missing contract $required"
    }
}

if ($source -notmatch '(?s)if \(-not \$Run\).*?exit 0') {
    throw "wsl_incident_snapshot: default path must stop after PLAN"
}

foreach ($forbidden in @(
        "--shutdown",
        "--terminate",
        "Stop-Service",
        "Restart-Service",
        "Start-Service",
        "Start-VM",
        "Stop-VM",
        "Invoke-WebRequest",
        "Invoke-RestMethod",
        "wsl.exe --install",
        "systemctl kill"
    )) {
    if ($source.Contains($forbidden)) {
        throw "wsl_incident_snapshot: forbidden mutation $forbidden"
    }
}

Write-Output "PASS wsl_incident_snapshot_is_bounded_read_only"
