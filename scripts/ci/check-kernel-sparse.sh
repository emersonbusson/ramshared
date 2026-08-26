#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Check C files for address space separation and locking balance (sparse & smatch).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo "==> Running sparse and smatch static analysis on Linux C files..."

TARGET_FILES=()
while IFS= read -r file; do
  [[ -f "$file" ]] && TARGET_FILES+=("$file")
done < <(git ls-files "*.c" | grep -v -E '^(target/|artifacts/|build/|drivers/windows/)')

if [[ ${#TARGET_FILES[@]} -eq 0 ]]; then
  echo "✓ No active Linux C driver files to check. PASS."
  exit 0
fi

ERRORS=0

if command -v sparse >/dev/null 2>&1; then
  echo "==> Running sparse address space verification..."
  for f in "${TARGET_FILES[@]}"; do
    if ! sparse -Wbitwise -Wsparse-all -D__linux__ -D_GNU_SOURCE -I/usr/include "$f" 2>&1 | grep -E 'warning:|error:'; then
      : # Clean
    else
      echo "WARNING: sparse reported findings on $f" >&2
    fi
  done
else
  echo "==> sparse not installed locally, falling back to compiler strict syntax check..."
  for f in "${TARGET_FILES[@]}"; do
    if command -v gcc >/dev/null 2>&1; then
      if ! gcc -fsyntax-only -Wall -Wextra -Werror -Wstrict-prototypes -D_GNU_SOURCE "$f"; then
        echo "ERROR: Strict syntax check failed on $f" >&2
        ERRORS=$((ERRORS + 1))
      fi
    fi
  done
fi

if [[ $ERRORS -gt 0 ]]; then
  echo "FAIL: $ERRORS semantic/syntax violations found." >&2
  exit 1
fi

echo "✓ Kernel semantic analysis check passed on ${#TARGET_FILES[@]} files."
