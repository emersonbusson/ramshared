#!/usr/bin/env bash
set -euo pipefail

echo "Running tests for generate-sbom.sh..."

SCRIPT="./scripts/package/generate-sbom.sh"
TEST_OUT="/tmp/test-sbom-out-$$"

# Test 1: Fail without required arguments if given invalid arg
if $SCRIPT --invalid-arg &>/dev/null; then
    echo "FAIL: Expected script to fail on invalid argument"
    exit 1
fi

# Test 2: Execute successfully with --out
mkdir -p "$TEST_OUT"
if ! $SCRIPT --out "$TEST_OUT"; then
    echo "FAIL: Expected script to succeed"
    exit 1
fi

# Verify artifacts
if [[ ! -f "$TEST_OUT/ramshared-sbom.spdx.json" ]]; then
    echo "FAIL: Missing SPDX artifact"
    exit 1
fi

if [[ ! -d "$TEST_OUT/ramshared-cli" ]] || [[ ! -f "$TEST_OUT/ramshared-cli/ramshared-sbom.cdx.json" ]]; then
    echo "FAIL: Missing CycloneDX artifact for ramshared-cli"
    exit 1
fi

echo "All tests passed for generate-sbom.sh"
rm -rf "$TEST_OUT"
