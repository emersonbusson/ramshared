#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

OUT_DIR="artifacts/lkml-patchset"

echo "==> Generating LKML patchset in $OUT_DIR..."
if [[ -f "scripts/package/generate-kernel-patchset.sh" ]]; then
    ./scripts/package/generate-kernel-patchset.sh
else
    echo "ERROR: scripts/package/generate-kernel-patchset.sh not found." >&2
    exit 1
fi

echo "==> Validating patchset with checkpatch.pl --strict..."

# If system checkpatch.pl exists, use it, otherwise use local fallback.
CHECKPATCH_BIN=""
if command -v checkpatch.pl >/dev/null 2>&1; then
    CHECKPATCH_BIN="checkpatch.pl"
elif [[ -f "scripts/kernel/checkpatch.pl" ]]; then
    CHECKPATCH_BIN="perl scripts/kernel/checkpatch.pl"
else
    echo "ERROR: checkpatch.pl not found. Please install it or place it in scripts/kernel/checkpatch.pl" >&2
    exit 1
fi

ERRORS=0

for patch in "$OUT_DIR"/*.patch; do
    echo "  -> Checking $patch..."
    if ! $CHECKPATCH_BIN --no-tree --strict -q "$patch"; then
        echo "ERROR: checkpatch.pl failed on $patch" >&2
        ERRORS=$((ERRORS + 1))
    fi
done

if [[ $ERRORS -gt 0 ]]; then
    echo "FAIL: $ERRORS patch(es) failed checkpatch --strict validation." >&2
    exit 1
fi

echo "✓ All patches passed checkpatch validation."
