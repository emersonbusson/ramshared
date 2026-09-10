#!/usr/bin/env bash
#
# dkms-pre-build.sh - Verify kernel headers presence before DKMS build
#
# Usage: dkms-pre-build.sh <kernel_version>

set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "ERROR: Kernel version argument is required." >&2
    echo "Usage: $0 <kernel_version>" >&2
    exit 1
fi

KERNEL_VER="$1"
BUILD_DIR="/lib/modules/${KERNEL_VER}/build"
MAKEFILE="${BUILD_DIR}/Makefile"

echo "INFO: Validating kernel headers for ${KERNEL_VER}..."

if ! test -d "${BUILD_DIR}"; then
    echo "ERROR: Kernel build directory not found: ${BUILD_DIR}" >&2
    echo "Please install the appropriate linux-headers package for kernel ${KERNEL_VER}." >&2
    exit 1
fi

if ! test -f "${MAKEFILE}"; then
    echo "ERROR: Kernel Makefile not found: ${MAKEFILE}" >&2
    echo "The kernel headers installation appears incomplete or corrupted." >&2
    exit 1
fi

echo "INFO: Kernel headers validation successful for ${KERNEL_VER}."
exit 0
