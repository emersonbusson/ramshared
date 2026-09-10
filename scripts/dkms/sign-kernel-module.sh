#!/bin/bash
set -euo pipefail

# Fallback signature search paths for diverse Linux distributions
# Usage: $0 <module.ko> <privkey.priv> <pubkey.der>

if [ "$#" -lt 3 ]; then
    echo "Usage: $0 <module.ko> <privkey.priv> <pubkey.der>" >&2
    exit 1
fi

MODULE_KO="$1"
PRIV_KEY="$2"
PUB_KEY="$3"

if [ ! -f "${MODULE_KO}" ]; then
    echo "Error: Module file not found: ${MODULE_KO}" >&2
    exit 1
fi

if [ ! -f "${PRIV_KEY}" ]; then
    echo "Error: Private key not found: ${PRIV_KEY}" >&2
    exit 1
fi

if [ ! -f "${PUB_KEY}" ]; then
    echo "Error: Public key not found: ${PUB_KEY}" >&2
    exit 1
fi

KERNEL_VER=$(uname -r)
SIGN_FILE_PATHS=(
    "/usr/lib/modules/${KERNEL_VER}/build/scripts/sign-file"
    "/lib/modules/${KERNEL_VER}/build/scripts/sign-file"
    "/usr/src/linux-headers-${KERNEL_VER}/scripts/sign-file"
    "/usr/src/kernels/${KERNEL_VER}/scripts/sign-file"
    "/usr/src/linux-${KERNEL_VER}/scripts/sign-file"
)

SIGN_FILE=""

# Check if kmodsign is available in PATH (Ubuntu/Debian)
if command -v kmodsign >/dev/null 2>&1; then
    SIGN_FILE=$(command -v kmodsign)
else
    for path in "${SIGN_FILE_PATHS[@]}"; do
        if [ -x "${path}" ] || [ -f "${path}" ]; then
            SIGN_FILE="${path}"
            break
        fi
    done
fi

if [ -z "${SIGN_FILE}" ]; then
    echo "Error: sign-file binary not found in any standard distribution paths." >&2
    exit 1
fi

# Determine hash algorithm
HASH_ALGO="sha256"

# Execute the signing
"${SIGN_FILE}" "${HASH_ALGO}" "${PRIV_KEY}" "${PUB_KEY}" "${MODULE_KO}"
echo "Successfully signed ${MODULE_KO} using ${SIGN_FILE}"
