#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: $0 <module.ko> <private_key.priv> <certificate.der>"
    echo "Signs a kernel module using kmodsign or scripts/sign-file."
    exit 1
}

if [ "$#" -ne 3 ]; then
    usage
fi

MODULE_PATH="$1"
PRIV_KEY="$2"
CERT="$3"

if [ ! -f "$MODULE_PATH" ]; then
    echo "Error: Module not found at $MODULE_PATH" >&2
    exit 1
fi

if [ ! -f "$PRIV_KEY" ]; then
    echo "Error: Private key not found at $PRIV_KEY" >&2
    exit 1
fi

if [ ! -f "$CERT" ]; then
    echo "Error: Certificate not found at $CERT" >&2
    exit 1
fi

# Check key permissions
PERMS=$(stat -c "%a" "$PRIV_KEY")
if [ "$PERMS" != "600" ]; then
    echo "Error: Private key $PRIV_KEY must have 0600 permissions, found $PERMS" >&2
    exit 1
fi

KVER=$(uname -r)
SIGN_FILE="/usr/src/linux-headers-$KVER/scripts/sign-file"

if command -v kmodsign >/dev/null 2>&1; then
    kmodsign sha256 "$PRIV_KEY" "$CERT" "$MODULE_PATH"
elif [ -x "$SIGN_FILE" ]; then
    "$SIGN_FILE" sha256 "$PRIV_KEY" "$CERT" "$MODULE_PATH"
else
    echo "Error: Neither kmodsign nor $SIGN_FILE found." >&2
    exit 1
fi

echo "Module $MODULE_PATH signed successfully."
