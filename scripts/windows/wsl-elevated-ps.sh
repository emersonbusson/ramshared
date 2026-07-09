#!/usr/bin/env bash
# wsl-elevated-ps.sh — run elevated Windows PowerShell from WSL2 (Hyper-V, PSD, etc.)
#
# Requires: Windows "sudo" (C:\Windows\System32\sudo.exe), UAC not blocking agent use.
# Usage:
#   ./scripts/windows/wsl-elevated-ps.sh -File C:\path\script.ps1
#   ./scripts/windows/wsl-elevated-ps.sh -Command "Get-VM | ft"
#   ./scripts/windows/wsl-elevated-ps.sh                 # interactive elevated PS
set -euo pipefail

WIN_SUDO="${WIN_SUDO:-/mnt/c/Windows/System32/sudo.exe}"
if [[ ! -x "$WIN_SUDO" ]]; then
  echo "error: Windows sudo not found at $WIN_SUDO" >&2
  exit 127
fi

if [[ $# -eq 0 ]]; then
  exec "$WIN_SUDO" powershell.exe -NoProfile -ExecutionPolicy Bypass
fi

# Pass through -File / -Command / extra args to elevated powershell.exe
exec "$WIN_SUDO" powershell.exe -NoProfile -ExecutionPolicy Bypass "$@"
