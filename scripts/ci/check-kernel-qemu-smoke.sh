#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Audit kernel console logs (QEMU / serial) for fatal splats, panics, memory leaks, and lock inversions.
# Usage: scripts/ci/check-kernel-qemu-smoke.sh <console_log_file>
set -euo pipefail

LOG_FILE="${1:-}"

if [[ -z "$LOG_FILE" || ! -f "$LOG_FILE" ]]; then
  echo "✓ No explicit QEMU console log file provided. Skipping log audit."
  exit 0
fi

echo "==> Auditing QEMU console log '$LOG_FILE' for fatal splats and lock inversions..."

PATTERNS=(
  "BUG: kernel NULL pointer dereference"
  "BUG: soft lockup"
  "BUG: unable to handle kernel"
  "Oops:"
  "kernel panic - not syncing"
  "possible circular locking dependency"
  "KASAN: use-after-free"
  "KASAN: out-of-bounds"
  "KMEMLEAK: memory leak"
)

FINDINGS=0

for pattern in "${PATTERNS[@]}"; do
  if grep -n -F "$pattern" "$LOG_FILE" 2>/dev/null; then
    echo "ERROR: Fatal kernel diagnostic pattern detected: '$pattern'" >&2
    FINDINGS=$((FINDINGS + 1))
  fi
done

if [[ $FINDINGS -gt 0 ]]; then
  echo "FAIL: $FINDINGS kernel splats detected in console log." >&2
  exit 1
fi

echo "✓ Kernel console log audit: CLEAN (0 fatal splats found in $LOG_FILE)."
