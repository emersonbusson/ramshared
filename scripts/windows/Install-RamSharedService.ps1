#Requires -Version 5.1
<#
.SYNOPSIS
  Thin wrapper for the authoritative Rust two-service product controller.

.DESCRIPTION
  This script performs no copy, ACL, SCM, start, or rollback operation. The
  controller owns the complete manifest transaction.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Manifest,
    [string]$Controller = "C:\ramshared\bin\ramshared-winsvc.exe",
    [ValidateSet("install", "repair")]
    [string]$Operation = "install"
)

$ErrorActionPreference = "Stop"
if (-not [IO.Path]::IsPathRooted($Manifest) -or
    -not (Test-Path -LiteralPath $Manifest -PathType Leaf)) {
    throw "Manifest must be an existing absolute file: $Manifest"
}
if (-not [IO.Path]::IsPathRooted($Controller) -or
    -not (Test-Path -LiteralPath $Controller -PathType Leaf)) {
    throw "Controller must be an existing absolute file: $Controller"
}

& $Controller $Operation --manifest (Resolve-Path -LiteralPath $Manifest).Path
if ($LASTEXITCODE -ne 0) {
    throw "product controller failed: operation=$Operation exit=$LASTEXITCODE"
}
