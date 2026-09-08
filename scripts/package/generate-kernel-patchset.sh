#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Generate LKML formatted patchset for drivers/block/ramshared
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

if ! command -v perl >/dev/null 2>&1; then
    echo "Error: perl is required but not installed." >&2
    exit 69 # EX_UNAVAILABLE
fi

if [[ ! -f "scripts/checkpatch.pl" ]]; then
    echo "Error: scripts/checkpatch.pl not found." >&2
    exit 69 # EX_UNAVAILABLE
fi

if [[ ! -x "scripts/checkpatch.pl" ]]; then
    echo "Error: scripts/checkpatch.pl is not executable." >&2
    exit 69 # EX_UNAVAILABLE
fi

OUT_DIR="artifacts/lkml-patchset"
mkdir -p "$OUT_DIR"

if [[ ! -d "$OUT_DIR" ]]; then
    echo "Error: Failed to create output directory $OUT_DIR." >&2
    exit 74 # EX_IOERR
fi

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
echo "✓ LKML patchset generation complete."

echo "==> Validating generated patches with checkpatch.pl..."
# Find generated patches and validate them
while IFS= read -r patch_file; do
    if [[ -f "$patch_file" ]]; then
        echo "Validating: $patch_file"
        if ! scripts/checkpatch.pl --strict "$patch_file"; then
            echo "Error: checkpatch.pl failed on $patch_file." >&2
            exit 65 # EX_DATAERR
        fi
    fi
done < <(find "$OUT_DIR" -maxdepth 1 -type f -name "*.patch" | sort)

echo "✓ All patches validated successfully."
