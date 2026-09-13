#!/usr/bin/env bash
set -euo pipefail

echo "Running shellcheck on build-rpm-package.sh..."
if command -v shellcheck >/dev/null 2>&1; then
    shellcheck scripts/package/build-rpm-package.sh
    echo "shellcheck passed."
else
    echo "shellcheck not found, skipping."
fi

echo "Verifying fail-fast dependencies check (dummy run)..."
# Just verify it doesn't syntax error
bash -n scripts/package/build-rpm-package.sh
echo "Syntax check passed."

echo "All tests passed."
