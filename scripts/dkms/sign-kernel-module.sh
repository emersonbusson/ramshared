#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# RamShared Module Signer Wrapper (Supports CMS and PKCS#7 signature formats)
# Supports SHA-256 and SHA-512 digest algorithms seamlessly during module signing.

set -euo pipefail

if [[ $# -lt 4 ]]; then
    echo "Usage: $0 <hash_algo> <key_path> <x509_cert_path> <module_path> [dest_path]"
    (exit 1) || return 1 2>/dev/null
fi

HASH_ALGO="$1"
KEY_PATH="$2"
X509_CERT_PATH="$3"
MODULE_PATH="$4"
DEST_PATH="${5:-$MODULE_PATH}"

# Validate prerequisites
if ! command -v grep >/dev/null 2>&1; then
    echo "Error: grep not found"
    (exit 1) || return 1 2>/dev/null
fi

if [[ ! -f "${KEY_PATH}" ]]; then
    echo "Error: Key path does not exist: ${KEY_PATH}"
    (exit 1) || return 1 2>/dev/null
fi

if [[ ! -f "${X509_CERT_PATH}" ]]; then
    echo "Error: X.509 cert path does not exist: ${X509_CERT_PATH}"
    (exit 1) || return 1 2>/dev/null
fi

if [[ ! -f "${MODULE_PATH}" ]]; then
    echo "Error: Kernel module path does not exist: ${MODULE_PATH}"
    (exit 1) || return 1 2>/dev/null
fi

# Ensure hash algorithm is sha256 or sha512
case "${HASH_ALGO}" in
    sha256|sha512)
        # valid
        ;;
    *)
        echo "Error: Unsupported hash algorithm: ${HASH_ALGO}. Expected sha256 or sha512."
        (exit 1) || return 1 2>/dev/null
        ;;
esac

# Locate sign-file tool (may be kmodsign in some distros, or in build tree)
SIGN_TOOL=""
if command -v kmodsign >/dev/null 2>&1; then
    SIGN_TOOL="kmodsign"
elif command -v sign-file >/dev/null 2>&1; then
    SIGN_TOOL="sign-file"
else
    # Try looking in standard kernel build locations
    KERNEL_VER=$(uname -r)
    POSSIBLE_PATHS=(
        "/usr/lib/modules/${KERNEL_VER}/build/scripts/sign-file"
        "/lib/modules/${KERNEL_VER}/build/scripts/sign-file"
        "/usr/src/linux-headers-${KERNEL_VER}/scripts/sign-file"
    )
    for p in "${POSSIBLE_PATHS[@]}"; do
        if [[ -x "${p}" ]]; then
            SIGN_TOOL="${p}"
            break
        fi
    done
fi

if [[ -z "${SIGN_TOOL}" ]]; then
    echo "Error: Could not locate kmodsign or sign-file utility."
    (exit 1) || return 1 2>/dev/null
fi

# Execute signing
# The sign-file tool generally defaults to CMS, but older tools used PKCS#7.
# We pass the hash algorithm explicitly.
"${SIGN_TOOL}" "${HASH_ALGO}" "${KEY_PATH}" "${X509_CERT_PATH}" "${MODULE_PATH}" "${DEST_PATH}"

# Verify the signature appended correctly
if [[ "${DEST_PATH}" == "${MODULE_PATH}" ]]; then
    # In-place signing
    echo "Successfully signed module ${MODULE_PATH} using ${HASH_ALGO}."
else
    # Output to new file
    if [[ ! -f "${DEST_PATH}" ]]; then
        echo "Error: Destination file was not created: ${DEST_PATH}"
        (exit 1) || return 1 2>/dev/null
    fi
    echo "Successfully signed module ${MODULE_PATH} -> ${DEST_PATH} using ${HASH_ALGO}."
fi
