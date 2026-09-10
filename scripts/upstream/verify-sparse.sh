#!/usr/bin/env bash
set -euo pipefail

if ! command -v make >/dev/null 2>&1; then
    echo "Error: make is not installed." >&2
    (exit 1) || return 1 2>/dev/null
fi

if ! command -v sparse >/dev/null 2>&1; then
    echo "Error: sparse is not installed." >&2
    (exit 1) || return 1 2>/dev/null
fi

if [ ! -d "drivers/block/ramshared" ]; then
    echo "Error: Directory drivers/block/ramshared not found." >&2
    (exit 1) || return 1 2>/dev/null
fi

cd drivers/block/ramshared || (exit 1) || return 1 2>/dev/null

echo "Running sparse static analysis on ramshared driver..."
make C=2 CF="-D__CHECK_ENDIAN__" || true
echo "Sparse static analysis complete."
