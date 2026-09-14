#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Integration tests for verify-signatures.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERIFY_SCRIPT="$ROOT/scripts/package/verify-signatures.sh"

TEST_DIR=$(mktemp -d)
trap 'rm -rf "$TEST_DIR"' EXIT
cd "$TEST_DIR"

echo "Creating dummy files..."
echo "test content" > dummy.txt
sha256sum dummy.txt > dummy.txt.sha256

echo "Creating dummy GPG key..."
GNUPGHOME=$(mktemp -d)
export GNUPGHOME
cat << 'KEY_EOF' > key_gen_script
%echo Generating a standard key
Key-Type: RSA
Key-Length: 2048
Subkey-Type: RSA
Subkey-Length: 2048
Name-Real: Dummy Test
Name-Email: dummy@test.local
Expire-Date: 0
%no-protection
%commit
%echo done
KEY_EOF

gpg --batch --gen-key key_gen_script >/dev/null 2>&1
gpg --armor --export dummy@test.local > dummy.pub 2>/dev/null
gpg --detach-sign --armor --output dummy.txt.sig dummy.txt 2>/dev/null

echo "Running verification script (EXPECT SUCCESS)..."
if ! "$VERIFY_SCRIPT" dummy.txt dummy.txt.sha256 dummy.txt.sig dummy.pub; then
    echo "ERROR: Expected success but got failure!"
    exit 1
fi
echo "Success test passed."

# Create a BAD checksum
echo "bad hash" > bad.sha256
echo "Running verification script (EXPECT FAILURE for bad hash)..."
if "$VERIFY_SCRIPT" dummy.txt bad.sha256 dummy.txt.sig dummy.pub; then
    echo "ERROR: Should have failed for bad hash!"
    exit 1
fi
echo "Bad hash test passed."

# Create a BAD signature
echo "bad sig" > bad.sig
echo "Running verification script (EXPECT FAILURE for bad sig)..."
if "$VERIFY_SCRIPT" dummy.txt dummy.txt.sha256 bad.sig dummy.pub; then
    echo "ERROR: Should have failed for bad sig!"
    exit 1
fi
echo "Bad sig test passed."

# Missing arguments
echo "Running verification script (EXPECT FAILURE for missing args)..."
if "$VERIFY_SCRIPT" dummy.txt dummy.txt.sha256 dummy.txt.sig; then
    echo "ERROR: Should have failed for missing arguments!"
    exit 1
fi
echo "Missing args test passed."

# Missing files
echo "Running verification script (EXPECT FAILURE for missing file)..."
if "$VERIFY_SCRIPT" missing.txt dummy.txt.sha256 dummy.txt.sig dummy.pub; then
    echo "ERROR: Should have failed for missing file!"
    exit 1
fi
echo "Missing file test passed."

echo "All integration tests passed successfully."
