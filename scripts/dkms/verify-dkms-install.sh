#!/bin/bash
set -euo pipefail

# Ensure necessary commands are present
for cmd in dkms modprobe; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "Error: Required command '$cmd' is not installed."
        exit 1
    fi
done

echo "Verifying DKMS status for 'ramshared'..."
# Get the status of the 'ramshared' module in DKMS
# Fail if it's not installed or not built correctly for the current kernel
if ! dkms status -m ramshared | grep -q "installed"; then
    echo "Error: ramshared DKMS module is not installed."
    dkms status -m ramshared
    exit 1
fi

echo "DKMS status verified."

echo "Testing module loadability (dry-run)..."
# Dry-run modprobe to see if it would load successfully
if ! modprobe --dry-run ramshared; then
    echo "Error: modprobe --dry-run ramshared failed. The module might not be loadable or there are dependency issues."
    exit 1
fi

echo "Module loadability verified."
echo "DKMS verification passed."
