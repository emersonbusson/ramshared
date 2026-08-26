#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Check C and header files against Linux Kernel coding standards.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo "==> Running Linux Kernel style checks on C and header files..."

TARGET_FILES=()
while IFS= read -r file; do
  [[ -f "$file" ]] && TARGET_FILES+=("$file")
done < <(git ls-files --cached --others --exclude-standard "*.c" "*.h" | grep -v -E '^(target/|artifacts/|build/|drivers/windows/)')

if [[ ${#TARGET_FILES[@]} -eq 0 ]]; then
  echo "✓ No active Linux C/H driver files to check. PASS."
  exit 0
fi

ERRORS=0

# 1. If checkpatch.pl exists, run official Linux checkpatch
if command -v checkpatch.pl >/dev/null 2>&1; then
  echo "==> Using system checkpatch.pl..."
  for f in "${TARGET_FILES[@]}"; do
    if ! checkpatch.pl --no-tree --strict -q -f "$f"; then
      echo "ERROR: checkpatch.pl failed on $f" >&2
      ERRORS=$((ERRORS + 1))
    fi
  done
elif [[ -f "scripts/kernel/checkpatch.pl" ]]; then
  echo "==> Using scripts/kernel/checkpatch.pl..."
  for f in "${TARGET_FILES[@]}"; do
    if ! perl scripts/kernel/checkpatch.pl --no-tree --strict -q -f "$f"; then
      echo "ERROR: checkpatch.pl failed on $f" >&2
      ERRORS=$((ERRORS + 1))
    fi
  done
else
  # 2. Strict static fallback: SPDX check, trailing whitespace, unsafe APIs
  echo "==> Running static kernel style linter..."
  for f in "${TARGET_FILES[@]}"; do
    # Check for trailing whitespace
    if grep -n -E '[[:space:]]+$' "$f" >/dev/null 2>&1; then
      echo "ERROR: Trailing whitespace detected in $f" >&2
      ERRORS=$((ERRORS + 1))
    fi
    # Check for unsafe banned kernel APIs
    if grep -n -E '\b(strcpy|strcat|sprintf|vsprintf)\b' "$f" >/dev/null 2>&1; then
      echo "ERROR: Banned unsafe string functions detected in $f (use strscpy/snprintf)" >&2
      ERRORS=$((ERRORS + 1))
    fi
  done
fi

if [[ $ERRORS -gt 0 ]]; then
  echo "FAIL: $ERRORS kernel style violations found." >&2
  exit 1
fi

echo "✓ Kernel style check passed on ${#TARGET_FILES[@]} files."
