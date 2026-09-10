#!/usr/bin/env bash
set -euo pipefail

# Ensure the script is run with required permissions and commands exist
if ! command -v dmesg >/dev/null 2>&1; then
    echo "ERROR: dmesg command not found."
    (exit 1) || return 1 2>/dev/null
fi
if ! command -v modprobe >/dev/null 2>&1; then
    echo "ERROR: modprobe command not found."
    (exit 1) || return 1 2>/dev/null
fi
if ! command -v rmmod >/dev/null 2>&1; then
    echo "ERROR: rmmod command not found."
    (exit 1) || return 1 2>/dev/null
fi

echo "Testing modprobe ramshared..."
if ! modprobe ramshared; then
    echo "ERROR: Failed to load ramshared module."
    (exit 1) || return 1 2>/dev/null
fi

echo "Verifying RamShared banner in dmesg..."
if ! dmesg | grep -E -q 'RamShared Hardware-Accelerated VRAM Block Driver v[0-9\.\-A-Za-z]+ loaded'; then
    echo "ERROR: RamShared banner not found in dmesg."
    rmmod ramshared || true
    (exit 1) || return 1 2>/dev/null
fi

echo "RamShared banner successfully found in dmesg."

if ! rmmod ramshared; then
    echo "ERROR: Failed to unload ramshared module."
    (exit 1) || return 1 2>/dev/null
fi

echo "DKMS installation verification successful."
