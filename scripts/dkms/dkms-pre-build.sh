#!/usr/bin/env bash
set -euo pipefail

# Compare cc --version with /proc/version compiler to warn on potential symbol mismatch.
# This script is meant to be run by DKMS as a PRE_BUILD step.

if ! command -v cc >/dev/null 2>&1; then
    echo "ERROR: cc not found." >&2
    (exit 1) || return 1 2>/dev/null
fi

if ! test -f /proc/version; then
    echo "WARNING: /proc/version not found, cannot check compiler version." >&2
    (exit 0) || return 0 2>/dev/null
fi

# Extract the major version of cc
CC_VER=$(cc -dumpversion || true)
if [ -z "$CC_VER" ]; then
    CC_VER=$(cc --version | head -n1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n1 || true)
fi

# Extract the major version from /proc/version
PROC_VER=$(grep -oE '(gcc|clang)[^)]*\)[[:space:]]+[0-9]+\.[0-9]+' /proc/version | grep -oE '[0-9]+\.[0-9]+' | head -n1 || true)
if [ -z "$PROC_VER" ]; then
    PROC_VER=$(grep -oE '(gcc|clang) version [0-9]+\.[0-9]+' /proc/version | grep -oE '[0-9]+\.[0-9]+' | head -n1 || true)
fi

if [ -z "$PROC_VER" ]; then
    PROC_VER=$(grep -oE '(gcc|clang).*?[0-9]+\.[0-9]+\.[0-9]+' /proc/version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n1 || true)
fi

CC_MAJOR=$(echo "$CC_VER" | cut -d. -f1)
PROC_MAJOR=$(echo "$PROC_VER" | cut -d. -f1)

if [ -n "$CC_MAJOR" ] && [ -n "$PROC_MAJOR" ]; then
    if [ "$CC_MAJOR" != "$PROC_MAJOR" ]; then
        echo "WARNING: Compiler version mismatch! cc major version: $CC_MAJOR vs kernel compiler major version: $PROC_MAJOR" >&2
    fi
fi

(exit 0) || return 0 2>/dev/null
