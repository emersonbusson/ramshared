#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Test for verify-signatures.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERIFY_SCRIPT="$ROOT/scripts/package/verify-signatures.sh"

TEST_DIR=$(mktemp -d)
GNUPGHOME=$(mktemp -d)
export GNUPGHOME

trap 'rm -rf "$TEST_DIR" "$GNUPGHOME"' EXIT

# Generate a temporary GPG key for testing
echo "--> Generating temporary GPG key..."
cat << 'GPG_OPTS' > "$GNUPGHOME/gen-key-script"
%echo Generating a standard key
Key-Type: default
Subkey-Type: default
Name-Real: RamShared Test Key
Name-Comment: for testing
Name-Email: test@ramshared.local
Expire-Date: 0
%no-protection
%commit
%echo done
GPG_OPTS
gpg --batch --generate-key "$GNUPGHOME/gen-key-script" >/dev/null 2>&1

echo "--> Setting up test directory..."
echo "test content" > "$TEST_DIR/file.txt"
cd "$TEST_DIR"
sha256sum file.txt > SHA256SUMS.txt
gpg --detach-sign --armor --output SHA256SUMS.txt.sig SHA256SUMS.txt >/dev/null 2>&1

echo "--> Testing successful verification..."
if ! "$VERIFY_SCRIPT" "$TEST_DIR"; then
    echo "ERROR: Should have succeeded"
    exit 1
fi

echo "--> Testing failed checksum verification..."
echo "bad content" > "$TEST_DIR/file.txt"
if "$VERIFY_SCRIPT" "$TEST_DIR"; then
    echo "ERROR: Should have failed"
    exit 1
fi

echo "--> Testing missing signature..."
# reset content
echo "test content" > "$TEST_DIR/file.txt"
rm -f "$TEST_DIR/SHA256SUMS.txt.sig"
if "$VERIFY_SCRIPT" "$TEST_DIR"; then
    echo "ERROR: Should have failed due to missing sig"
    exit 1
fi

echo "--> Testing invalid signature..."
echo "fake sig" > "$TEST_DIR/SHA256SUMS.txt.sig"
if "$VERIFY_SCRIPT" "$TEST_DIR"; then
    echo "ERROR: Should have failed due to invalid sig"
    exit 1
fi

echo "--> Tests passed."
