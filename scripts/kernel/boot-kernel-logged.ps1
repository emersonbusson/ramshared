<#
.SYNOPSIS
  Wrapper com log persistente para boot-kernel-safe.ps1.

.DESCRIPTION
  Executa o launcher seguro em um PowerShell filho e grava stdout/stderr em
  C:\wsl\boot-ramshared.log. O processo filho isola o exit do launcher para que
  este wrapper consiga registrar o codigo final.
#>
param(
  [string]$Launcher = "C:\wsl\boot-kernel-safe.ps1",
  [string]$LogPath = "C:\wsl\boot-ramshared.log",
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$LauncherArgs = @()
)

$ErrorActionPreference = "Stop"

function Write-Log([string]$Line) {
  Write-Host $Line
  Add-Content -Path $LogPath -Value $Line -Encoding UTF8
}

$stamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
Set-Content -Path $LogPath -Value "=== ramshared boot attempt: $stamp ===" -Encoding UTF8
Write-Log "launcher=$Launcher"
Write-Log "args=$($LauncherArgs -join ' ')"

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Launcher @LauncherArgs *>&1 |
  ForEach-Object { Write-Log $_.ToString() }

$code = $LASTEXITCODE
if ($null -eq $code) {
  if ($?) { $code = 0 } else { $code = 1 }
}

Write-Log "=== exit=$code ==="
exit $code
