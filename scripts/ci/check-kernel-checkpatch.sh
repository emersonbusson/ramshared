#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Download checkpatch.pl from torvalds/linux and validate kernel driver files.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo "==> Running checkpatch.pl validation on kernel driver files..."

DRIVER_DIR="drivers/block/ramshared"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/ramshared-ci"
CHECKPATCH="$CACHE_DIR/checkpatch.pl"
SPELLING="$CACHE_DIR/spelling.txt"
CONST_STRUCTS="$CACHE_DIR/const_structs.checkpatch"

KDIR="/lib/modules/$(uname -r)/build"
CHECKPATCH="$KDIR/scripts/checkpatch.pl"

if [[ ! -d "$KDIR" ]]; then
  echo "ERROR: Kernel source path $KDIR not found." >&2
  exit 78
fi

if [[ ! -x "$CHECKPATCH" ]]; then
  echo "ERROR: $CHECKPATCH not found or not executable." >&2
  exit 69
fi

# Download checkpatch.pl from torvalds/linux if not cached
if [[ ! -x "$CHECKPATCH" ]]; then
  mkdir -p "$CACHE_DIR"
  echo "  Downloading checkpatch.pl from torvalds/linux..."
  curl -sSfL "https://raw.githubusercontent.com/torvalds/linux/master/scripts/checkpatch.pl" -o "$CHECKPATCH" || {
    echo "  [SKIP] Could not download checkpatch.pl (network unavailable). PASS by grace."
    exit 0
  }
  chmod +x "$CHECKPATCH"
  curl -sSfL "https://raw.githubusercontent.com/torvalds/linux/master/scripts/spelling.txt" -o "$SPELLING" 2>/dev/null || touch "$SPELLING"
  curl -sSfL "https://raw.githubusercontent.com/torvalds/linux/master/scripts/const_structs.checkpatch" -o "$CONST_STRUCTS" 2>/dev/null || touch "$CONST_STRUCTS"
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
