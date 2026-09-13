#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Verify checksums and GPG signatures of RamShared release assets
# Usage: scripts/package/verify-signatures.sh <file> <sha256_file> <sig_file> <pubkey_file>
set -euo pipefail

if [[ $# -ne 4 ]]; then
    echo "Usage: $0 <target_file> <sha256_file> <sig_file> <pubkey_file>" >&2
    exit 1
fi

TARGET_FILE="$1"
SHA256_FILE="$2"
SIG_FILE="$3"
PUBKEY_FILE="$4"

if [[ ! -f "$TARGET_FILE" ]]; then
    echo "Error: target file '$TARGET_FILE' not found." >&2
    exit 2
fi

if [[ ! -f "$SHA256_FILE" ]]; then
    echo "Error: sha256 file '$SHA256_FILE' not found." >&2
    exit 2
fi

if [[ ! -f "$SIG_FILE" ]]; then
    echo "Error: signature file '$SIG_FILE' not found." >&2
    exit 2
fi

if [[ ! -f "$PUBKEY_FILE" ]]; then
    echo "Error: public key file '$PUBKEY_FILE' not found." >&2
    exit 2
fi

if ! command -v sha256sum >/dev/null 2>&1; then
    echo "Error: sha256sum command not found." >&2
    exit 3
fi

if ! command -v gpg >/dev/null 2>&1; then
    echo "Error: gpg command not found." >&2
    exit 3
fi

echo "Verifying SHA-256 checksum..."
# Extract the expected hash for the target file specifically, or the first hash if no filename matches
target_basename=$(basename "$TARGET_FILE")
expected_hash=$(grep -F "$target_basename" "$SHA256_FILE" | awk '{print $1}' | head -n 1 || true)
if [[ -z "$expected_hash" ]]; then
    # Fallback to the first hash if no matching filename found
    expected_hash=$(awk '{print $1}' "$SHA256_FILE" | head -n 1)
fi
actual_hash=$(sha256sum "$TARGET_FILE" | awk '{print $1}')

if [[ -z "$expected_hash" ]]; then
    echo "Error: Could not determine expected hash from $SHA256_FILE!" >&2
    exit 4
fi

if [[ "$expected_hash" != "$actual_hash" ]]; then
    echo "Error: SHA-256 checksum mismatch!" >&2
    echo "Expected: $expected_hash" >&2
    echo "Actual:   $actual_hash" >&2
    exit 4
fi
echo "SHA-256 checksum OK."

echo "Verifying GPG signature..."
GNUPGHOME=$(mktemp -d)
export GNUPGHOME
trap 'rm -rf "$GNUPGHOME"' EXIT

gpg --quiet --import "$PUBKEY_FILE"

if ! gpg --quiet --verify "$SIG_FILE" "$TARGET_FILE"; then
    echo "Error: GPG signature verification failed!" >&2
    exit 5
fi

echo "GPG signature OK."
echo "Verification successful!"
