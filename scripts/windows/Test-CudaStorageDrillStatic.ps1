#Requires -Version 5.1
[CmdletBinding()]
param([string]$ScriptPath)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) `
        "Invoke-CudaStorageDrill.ps1"
}

$tokens = $null
$parseErrors = $null
[void][Management.Automation.Language.Parser]::ParseFile(
    $ScriptPath, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) {
    throw "powershell51_script_parses failed: $($parseErrors[0].Message)"
}
Write-Output "PASS powershell51_script_parses"

$bytes = [IO.File]::ReadAllBytes($ScriptPath)
if (@($bytes | Where-Object { $_ -gt 0x7F }).Count -ne 0) {
    throw "script_is_ascii_for_bomless_powershell51 failed"
}
Write-Output "PASS script_is_ascii_for_bomless_powershell51"
Write-Output "PASS Test-CudaStorageDrillStatic"
