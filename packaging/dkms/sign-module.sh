#!/bin/bash
set -euo pipefail

# Secure Boot / Kernel Lockdown Module Signing Hook for DKMS
# Runs during POST_INSTALL

if [[ -z "${kernelver:-}" ]]; then
    echo "Error: kernelver not set."
    exit 1
fi

SIGN_TOOL="/usr/lib/modules/${kernelver}/build/scripts/sign-file"
MOK_SIGNING_KEY="/var/lib/dkms/mok.key"
MOK_CERTIFICATE="/var/lib/dkms/mok.pub"
MODULE_PATH="/lib/modules/${kernelver}/kernel/drivers/block/ramshared.ko"

if [[ ! -x "${SIGN_TOOL}" ]]; then
    echo "Warning: sign-file tool not found or not executable at ${SIGN_TOOL}. Skipping module signing."
    exit 0
fi

if [[ ! -f "${MOK_SIGNING_KEY}" || ! -f "${MOK_CERTIFICATE}" ]]; then
    echo "Warning: MOK key or certificate not found. Skipping module signing."
    exit 0
fi

if [[ ! -f "${MODULE_PATH}" ]]; then
    echo "Error: Module not found at ${MODULE_PATH}."
    exit 1
fi

echo "Signing module ${MODULE_PATH} with MOK..."
"${SIGN_TOOL}" sha256 "${MOK_SIGNING_KEY}" "${MOK_CERTIFICATE}" "${MODULE_PATH}"
echo "Module signed successfully."

exit 0
