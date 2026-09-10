#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# scripts/dkms/test-dkms-matrix.sh - Multi-kernel header build test runner for CI matrix validation.

set -euo pipefail

# Validate prerequisites
if ! command -v dkms >/dev/null 2>&1; then
    echo "ERROR: dkms command not found." >&2
    (exit 1) || return 1 2>/dev/null
fi

if ! command -v make >/dev/null 2>&1; then
    echo "ERROR: make command not found." >&2
    (exit 1) || return 1 2>/dev/null
fi

if [ ! -d "drivers/block/ramshared" ]; then
    echo "ERROR: Source directory drivers/block/ramshared not found." >&2
    (exit 1) || return 1 2>/dev/null
fi

if [ ! -f "packaging/dkms/dkms.conf" ]; then
    echo "ERROR: packaging/dkms/dkms.conf not found." >&2
    (exit 1) || return 1 2>/dev/null
fi

HEADERS_DIR="/usr/src"
if [ ! -d "$HEADERS_DIR" ]; then
    echo "ERROR: Headers directory $HEADERS_DIR not found." >&2
    (exit 1) || return 1 2>/dev/null
fi

echo "DKMS Matrix Build Test Runner"
echo "============================="

# Find available kernel headers
declare -a AVAILABLE_KERNELS=()
for kdir in "$HEADERS_DIR"/linux-headers-*; do
    if [ -d "$kdir" ] && [ -f "$kdir/Makefile" ]; then
        kver=$(basename "$kdir" | sed 's/^linux-headers-//')
        # Check if this kernel version has a valid modules build directory setup
        if [ -d "/lib/modules/$kver/build" ] || [ -L "/lib/modules/$kver/build" ]; then
            AVAILABLE_KERNELS+=("$kver")
        else
            echo "Skipping $kver: missing /lib/modules/$kver/build"
        fi
    fi
done

if [ ${#AVAILABLE_KERNELS[@]} -eq 0 ]; then
    echo "WARNING: No valid kernel headers with build symlinks found in $HEADERS_DIR. Skipping matrix test."
    (exit 0) || return 0 2>/dev/null
fi

echo "Found valid kernel headers: ${AVAILABLE_KERNELS[*]}"

# Create secure temporary directory for DKMS tree
TEMP_DIR=$(mktemp -d)
chmod 0700 "$TEMP_DIR"
trap 'rm -rf "$TEMP_DIR"' EXIT

DKMS_TREE="$TEMP_DIR/dkms"
SRC_TREE="$TEMP_DIR/src"
mkdir -p "$DKMS_TREE" "$SRC_TREE/ramshared-0.9.0/drivers/block"

# Copy source tree structure to support dkms.conf MAKE command
cp -a drivers/block/ramshared "$SRC_TREE/ramshared-0.9.0/drivers/block/"
cp packaging/dkms/dkms.conf "$SRC_TREE/ramshared-0.9.0/"

FAILURES=0

for kver in "${AVAILABLE_KERNELS[@]}"; do
    echo "------------------------------------------------"
    echo "Testing DKMS build for kernel: $kver"

    if ! dkms add -m ramshared -v 0.9.0 \
        --sourcetree "$SRC_TREE" \
        --dkmstree "$DKMS_TREE" >/dev/null 2>&1; then
        echo "ERROR: dkms add failed for $kver"
        FAILURES=$((FAILURES + 1))
        continue
    fi

    if dkms build -m ramshared -v 0.9.0 -k "$kver" \
        --sourcetree "$SRC_TREE" \
        --dkmstree "$DKMS_TREE" >/dev/null 2>&1; then
        echo "SUCCESS: DKMS build passed for $kver"
    else
        echo "ERROR: DKMS build failed for $kver"
        if [ -f "$DKMS_TREE/ramshared/0.9.0/build/make.log" ]; then
            cat "$DKMS_TREE/ramshared/0.9.0/build/make.log"
        fi
        FAILURES=$((FAILURES + 1))
    fi

    dkms remove -m ramshared -v 0.9.0 -k "$kver" --all \
        --sourcetree "$SRC_TREE" \
        --dkmstree "$DKMS_TREE" >/dev/null 2>&1 || true
done

if [ "$FAILURES" -gt 0 ]; then
    echo "Matrix test failed with $FAILURES errors."
    (exit 1) || return 1 2>/dev/null
else
    echo "Matrix test completed successfully."
    (exit 0) || return 0 2>/dev/null
fi
