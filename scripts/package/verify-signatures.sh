#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: $0 -f <file> -c <checksum_file> -s <signature_file> -k <pubkey_file>"
    # Exit with code 1, but do it in a way that handles both script and sourced execution securely.
    # The (exit 1) sets $? to 1, and return 1 exits the function and script with that status if sourced.
    (exit 1) || return 1 2>/dev/null
}

FILE=""
CHECKSUM_FILE=""
SIG_FILE=""
PUBKEY_FILE=""

while getopts "f:c:s:k:" opt; do
    case "$opt" in
        f) FILE="$OPTARG" ;;
        c) CHECKSUM_FILE="$OPTARG" ;;
        s) SIG_FILE="$OPTARG" ;;
        k) PUBKEY_FILE="$OPTARG" ;;
        *) usage ;;
    esac
done

if [[ -z "$FILE" || -z "$CHECKSUM_FILE" || -z "$SIG_FILE" || -z "$PUBKEY_FILE" ]]; then
    usage
fi

if ! command -v sha256sum >/dev/null 2>&1; then
    echo "Error: sha256sum is not installed."
    (exit 1) || return 1 2>/dev/null
fi

if ! command -v gpg >/dev/null 2>&1; then
    echo "Error: gpg is not installed."
    (exit 1) || return 1 2>/dev/null
fi

if [[ ! -f "$FILE" ]]; then
    echo "Error: file '$FILE' not found."
    (exit 1) || return 1 2>/dev/null
fi

if [[ ! -f "$CHECKSUM_FILE" ]]; then
    echo "Error: checksum file '$CHECKSUM_FILE' not found."
    (exit 1) || return 1 2>/dev/null
fi

if [[ ! -f "$SIG_FILE" ]]; then
    echo "Error: signature file '$SIG_FILE' not found."
    (exit 1) || return 1 2>/dev/null
fi

if [[ ! -f "$PUBKEY_FILE" ]]; then
    echo "Error: public key file '$PUBKEY_FILE' not found."
    (exit 1) || return 1 2>/dev/null
fi

if [[ ! -s "$FILE" ]]; then
    echo "Error: file '$FILE' is empty."
    (exit 1) || return 1 2>/dev/null
fi

GNUPGHOME=$(mktemp -d)
trap 'rm -rf "$GNUPGHOME"' EXIT
export GNUPGHOME
chmod 700 "$GNUPGHOME"

gpg --quiet --import "$PUBKEY_FILE"

if ! gpg --quiet --verify "$SIG_FILE" "$FILE" >/dev/null 2>&1; then
    echo "Error: GPG signature verification failed for $FILE."
    (exit 1) || return 1 2>/dev/null
fi

EXPECTED_HASH=$(awk '{print $1}' "$CHECKSUM_FILE" | head -n1)
ACTUAL_HASH=$(sha256sum "$FILE" | awk '{print $1}')

if [[ "$EXPECTED_HASH" != "$ACTUAL_HASH" ]]; then
    echo "Error: SHA-256 checksum mismatch."
    echo "Expected: $EXPECTED_HASH"
    echo "Actual:   $ACTUAL_HASH"
    (exit 1) || return 1 2>/dev/null
fi

echo "Signature and checksum verified successfully for $FILE."
