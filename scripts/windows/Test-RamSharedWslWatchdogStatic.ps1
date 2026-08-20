#Requires -Version 5.1
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$target = Join-Path $PSScriptRoot "Watch-RamSharedWsl.ps1"
if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
    throw "ramshared_wsl_watchdog: target script is missing"
}
$source = Get-Content -Raw -LiteralPath $target
foreach ($required in @(
    "ramshared-heartbeat.json",
    "ramshared-workloads.slice",
    '--signal=$Signal',
    'ValidateSet("TERM", "KILL")',
    "System.Collections.IDictionary",
    "WaitForExit",
    "watchdog-events.jsonl"
)) {
    if (-not $source.Contains($required)) {
        throw "ramshared_wsl_watchdog: missing contract $required"
    }
}
foreach ($forbidden in @("--shutdown", "--terminate")) {
    if ($source.Contains($forbidden)) {
        throw "ramshared_wsl_watchdog: forbidden WSL lifecycle action $forbidden"
    }
}
Write-Output "PASS ramshared_wsl_watchdog_is_bounded_and_scope_only"
