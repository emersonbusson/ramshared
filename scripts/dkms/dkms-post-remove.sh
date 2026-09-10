#!/bin/bash
#
# dkms-post-remove.sh - RamShared DKMS post-remove hook
#
# Cleans up module cache and runs depmod after module removal.
#

set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "Usage: $0 <kernel-version>"
    exit 1
fi

KERNEL_VER="$1"
MODULE_NAME="ramshared"

echo "Executing RamShared DKMS post-remove hook for kernel $KERNEL_VER"

if command -v depmod >/dev/null 2>&1; then
    echo "Running depmod -a $KERNEL_VER..."
    depmod -a "$KERNEL_VER"
else
    echo "Warning: depmod not found, skipping module dependency update."
fi

# Clean up any stale signature artifacts specific to this version
SIG_FILE="/lib/modules/${KERNEL_VER}/updates/dkms/${MODULE_NAME}.ko.sig"
if [ -f "$SIG_FILE" ]; then
    echo "Removing stale signature file: $SIG_FILE"
    rm -f "$SIG_FILE"
fi

echo "DKMS post-remove hook completed successfully."
exit 0
