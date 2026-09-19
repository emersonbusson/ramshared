#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# RamShared Checksum and GPG signature verification utility
# Validates release asset integrity

set -euo pipefail

function usage() {
  echo "Usage: $0 <release-dir>"
  echo "Verifies SHA-256 checksums and GPG detached signatures for RamShared release assets."
  exit 1
}

if [[ $# -ne 1 ]]; then
  usage
fi

RELEASE_DIR="$1"

if [[ ! -d "$RELEASE_DIR" ]]; then
  echo "ERROR: Directory $RELEASE_DIR not found." >&2
  exit 1
fi

echo "==> Verifying release assets in $RELEASE_DIR"

cd -- "$RELEASE_DIR"

if [[ ! -f "SHA256SUMS.txt" ]]; then
  echo "ERROR: SHA256SUMS.txt not found in $RELEASE_DIR" >&2
  exit 1
fi

echo "--> Verifying SHA-256 checksums..."
if ! sha256sum --check --strict SHA256SUMS.txt; then
  echo "ERROR: SHA-256 verification failed." >&2
  exit 1
fi

if [[ ! -f "SHA256SUMS.txt.sig" ]]; then
  echo "ERROR: SHA256SUMS.txt.sig not found. GPG signature verification is mandatory." >&2
  exit 1
fi

echo "--> Verifying GPG detached signature..."
if ! gpg --verify SHA256SUMS.txt.sig SHA256SUMS.txt; then
  echo "ERROR: GPG signature verification failed." >&2
  exit 1
fi

echo "==> Verification completed successfully."
