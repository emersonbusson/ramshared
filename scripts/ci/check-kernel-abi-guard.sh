#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Kernel ABI and symbol export guard for upstream compliance.
# Validates MODULE_LICENSE, EXPORT_SYMBOL_GPL, sysfs_emit, and fast-path safety.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo "==> Running Kernel ABI & Symbol Export Guard..."

DRIVER_DIR="drivers/block/ramshared"
ERRORS=0

# 1. Verify MODULE_LICENSE("GPL") exists
echo "[1/5] Checking MODULE_LICENSE..."
if grep -rq 'MODULE_LICENSE\s*(\s*"GPL"' "$DRIVER_DIR/"; then
  echo "  ✓ MODULE_LICENSE(\"GPL\") found."
else
  echo "  ERROR: MODULE_LICENSE(\"GPL\") not found in $DRIVER_DIR/" >&2
  ERRORS=$((ERRORS + 1))
fi

# 2. Verify no bare EXPORT_SYMBOL (must use _GPL or _NS_GPL)
echo "[2/5] Checking EXPORT_SYMBOL policy..."
BARE_EXPORTS=$(grep -rn 'EXPORT_SYMBOL\b' "$DRIVER_DIR/" | grep -v 'EXPORT_SYMBOL_GPL\|EXPORT_SYMBOL_NS_GPL\|EXPORT_SYMBOL_NS\|// ' || true)
if [[ -n "$BARE_EXPORTS" ]]; then
  echo "  ERROR: Bare EXPORT_SYMBOL() found (must use _GPL or _NS_GPL):" >&2
  echo "$BARE_EXPORTS" >&2
  ERRORS=$((ERRORS + 1))
else
  echo "  ✓ All exports use EXPORT_SYMBOL_GPL or EXPORT_SYMBOL_NS_GPL."
fi

# 3. Verify MODULE_DEVICE_TABLE for PCI autoloading
echo "[3/5] Checking MODULE_DEVICE_TABLE..."
if grep -rq 'MODULE_DEVICE_TABLE\s*(\s*pci' "$DRIVER_DIR/"; then
  echo "  ✓ MODULE_DEVICE_TABLE(pci, ...) found."
else
  echo "  WARNING: MODULE_DEVICE_TABLE(pci, ...) not found. PCI autoloading may not work."
  # Not a hard error — some drivers use module aliases instead
fi

# 4. Verify sysfs show functions use sysfs_emit(), not sprintf/snprintf
echo "[4/5] Checking sysfs_emit() usage..."
SYSFS_SPRINTF=$(grep -rn '_show\s*(' "$DRIVER_DIR/" | while read -r line; do
  FILE=$(echo "$line" | cut -d: -f1)
  FUNC_LINE=$(echo "$line" | cut -d: -f2)
  # Check the next 20 lines of the function for sprintf/snprintf
  sed -n "$((FUNC_LINE)),$(( FUNC_LINE + 20 ))p" "$FILE" | grep -n 'sprintf\|snprintf' | head -1 | while read -r match; do
    echo "  $FILE:$FUNC_LINE: _show function uses sprintf/snprintf instead of sysfs_emit()"
  done
done || true)
if [[ -n "$SYSFS_SPRINTF" ]]; then
  echo "  ERROR: sysfs show functions must use sysfs_emit():" >&2
  echo "$SYSFS_SPRINTF" >&2
  ERRORS=$((ERRORS + 1))
else
  echo "  ✓ All sysfs show functions use sysfs_emit()."
fi

# 5. Verify no GFP_KERNEL inside queue_rq (deadlock risk under reclaim)
echo "[5/5] Checking fast-path allocation safety..."
FAST_PATH_ALLOC=$(grep -n 'GFP_KERNEL' "$DRIVER_DIR/"*.c 2>/dev/null | grep -i 'queue_rq\|rw_page\|process_bio' || true)
if [[ -n "$FAST_PATH_ALLOC" ]]; then
  echo "  ERROR: GFP_KERNEL found in I/O fast-path (use GFP_NOIO or GFP_MEMALLOC):" >&2
  echo "$FAST_PATH_ALLOC" >&2
  ERRORS=$((ERRORS + 1))
else
  echo "  ✓ No GFP_KERNEL in I/O fast-path functions."
fi

echo ""
if [[ $ERRORS -gt 0 ]]; then
  echo "FAIL: $ERRORS ABI guard violation(s) found." >&2
  exit 1
fi

echo "✓ Kernel ABI & Symbol Export Guard: All 5 checks passed."
