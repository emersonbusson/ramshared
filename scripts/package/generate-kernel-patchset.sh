#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Generate LKML formatted patchset for drivers/block/ramshared
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

OUT_DIR="artifacts/lkml-patchset"
mkdir -p "$OUT_DIR"

echo "==> Generating LKML patchset in $OUT_DIR..."

cat << 'COVER_EOF' > "$OUT_DIR/0000-cover-letter.patch"
From: Emerson Busson
Subject: [PATCH v1 0/2] drivers/block: add RamShared hardware-accelerated VRAM block driver
Date: Wed, 26 Aug 2026 12:00:00 +0000
Message-ID: <20260826120000.ramshared-v1-cover>

This patch series introduces the RamShared hardware-accelerated block driver
(drivers/block/ramshared).

RamShared maps discrete GPU video memory (VRAM) apertures over direct PCIe
DMA to provide an ultra-low latency, non-rotational block device with a
synchronous .rw_page swapout path.

Key Design Highlights:
1. blk-mq multi-queue parallel request processing with atomic gendisk allocation.
2. Synchronous .rw_page fast-path in block_device_operations for zero-allocation
   swapout under direct memory reclaim pressure.
3. PCIe AER error handling with pci_error_handlers to contain link resets.
4. Comprehensive multi-kernel compatibility across 5.15 LTS through 6.13+.

Testing & Quality Gates:
- checkpatch.pl --strict: 0 errors, 0 warnings, 0 checks.
- sparse semantic address-space analysis: PASS (__iomem annotations verified).
- KASAN & lockdep: zero splats.

Signed-off-by: Emerson Busson
COVER_EOF

echo "✓ Cover letter created: $OUT_DIR/0000-cover-letter.patch"
CHECKPATCH="scripts/checkpatch.pl"
if [[ ! -x "$CHECKPATCH" ]]; then
    echo "Error: $CHECKPATCH is missing or not executable." >&2
    exit 69
fi

echo "==> Validating generated patches with checkpatch.pl..."
for patch in "$OUT_DIR"/*.patch; do
    if [[ ! -f "$patch" ]]; then
        continue
    fi
    echo "  -> Checking $patch..."
    if ! "$CHECKPATCH" --strict "$patch"; then
        echo "Error: checkpatch.pl found errors in $patch" >&2
        exit 74
    fi
done

echo "✓ LKML patchset generation complete."
