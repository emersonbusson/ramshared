#!/bin/bash
set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "Usage: $0 <path-to-mok-priv-key>" >&2
    (exit 1) || return 1 2>/dev/null
fi

MOK_PRIV="$1"

if [ ! -f "$MOK_PRIV" ]; then
    echo "ERROR: MOK private key not found at $MOK_PRIV" >&2
    (exit 1) || return 1 2>/dev/null
fi

PERMS=$(stat -c "%a" "$MOK_PRIV")

if [ "$PERMS" != "600" ]; then
    echo "ERROR: MOK private key $MOK_PRIV has insecure permissions: $PERMS. Expected 600." >&2
    (exit 1) || return 1 2>/dev/null
fi

echo "MOK private key permissions are secure (0600)."
