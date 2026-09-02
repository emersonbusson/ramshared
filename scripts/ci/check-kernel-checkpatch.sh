#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Validate kernel driver files using checkpatch.pl from kernel source tree.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo "==> Running checkpatch.pl validation on kernel driver files..."

DRIVER_DIR="drivers/block/ramshared"
KERNEL_DIR="${KERNEL_DIR:-/lib/modules/$(uname -r)/build}"

if [[ ! -d "$KERNEL_DIR" ]]; then
  echo "FAIL: Kernel source path not found at $KERNEL_DIR" >&2
  exit 69
fi

CHECKPATCH="$KERNEL_DIR/scripts/checkpatch.pl"
if [[ ! -x "$CHECKPATCH" ]]; then
  echo "FAIL: checkpatch.pl not found or not executable at $CHECKPATCH" >&2
  exit 69
fi

TARGET_FILES=()
while IFS= read -r file; do
  [[ -f "$file" ]] && TARGET_FILES+=("$file")
done < <(find "$DRIVER_DIR" -maxdepth 1 -type f \( -name '*.c' -o -name '*.h' \) 2>/dev/null)

if [[ ${#TARGET_FILES[@]} -eq 0 ]]; then
  echo "✓ No kernel driver files found. PASS."
  exit 0
fi

ERRORS=0
WARNINGS=0

for f in "${TARGET_FILES[@]}"; do
  echo "  Checking $f ..."
  OUTPUT=$(perl "$CHECKPATCH" --no-tree --strict --show-types -f "$f" 2>&1) || true
  ERR_COUNT=$(echo "$OUTPUT" | grep -c "^ERROR:" || true)
  WARN_COUNT=$(echo "$OUTPUT" | grep -c "^WARNING:" || true)
  if [[ "$ERR_COUNT" -gt 0 ]]; then
    echo "$OUTPUT" | grep "^ERROR:" || true
    ERRORS=$((ERRORS + ERR_COUNT))
  fi
  WARNINGS=$((WARNINGS + WARN_COUNT))
done

echo "  Summary: $ERRORS error(s), $WARNINGS warning(s) across ${#TARGET_FILES[@]} files."

if [[ $ERRORS -gt 0 ]]; then
  echo "FAIL: $ERRORS checkpatch error(s) detected." >&2
  exit 1
fi

echo "✓ checkpatch.pl validation passed on ${#TARGET_FILES[@]} kernel driver files."
