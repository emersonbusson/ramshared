#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ScriptPath,
    [string]$ServicePath
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
if ([string]::IsNullOrWhiteSpace($ScriptPath)) {
    $ScriptPath = Join-Path $root "scripts\windows\Invoke-PagefileRefusalManufactured.ps1"
}
if ([string]::IsNullOrWhiteSpace($ServicePath)) {
    $ServicePath = Join-Path $root "crates\ramshared-winsvc\src\service.rs"
}

$ps = Get-Content -LiteralPath $ScriptPath -Raw
$required = @(
    "PagingFiles",
    "pagefile.sys",
    "gate_a_active",
    "PAGEFILE_REFUSAL_MANUFACTURED",
    "restored",
    "stop.request"
)
foreach ($t in $required) {
    if ($ps -notmatch [regex]::Escape($t)) {
        throw "pagefile refusal manufactured script missing token: $t"
    }
}

$rs = Get-Content -LiteralPath $ServicePath -Raw
if ($rs -notmatch 'manufactured_pagefile_on_product_volume_refuses_gate_a') {
    throw "missing unit test manufactured_pagefile_on_product_volume_refuses_gate_a"
}
if ($rs -notmatch 'gate_a_active') {
    throw "service path missing gate_a_active refuse marker"
}
if ($rs -notmatch 'pagefile_refusal_to_runtime') {
    throw "service path missing pagefile_refusal_to_runtime mapping"
}

Write-Host "STATIC_PAGEFILE_REFUSAL_MANUFACTURED=PASS"
exit 0
