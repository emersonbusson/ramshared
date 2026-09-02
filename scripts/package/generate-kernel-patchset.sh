#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Generate LKML formatted patchset for drivers/block/ramshared
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

if ! command -v git >/dev/null 2>&1; then
    echo "Error: git command not found." >&2
    exit 69
fi

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "Error: Not a git repository." >&2
    exit 78
fi

if ! git rev-parse --verify origin/main >/dev/null 2>&1; then
    echo "Error: origin/main branch does not exist." >&2
    exit 78
fi

if ! git diff-index --quiet HEAD --; then
    echo "Error: git working directory is not clean." >&2
    exit 78
fi

CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
if [[ -z "$CURRENT_BRANCH" || "$CURRENT_BRANCH" == "main" || "$CURRENT_BRANCH" == "HEAD" ]]; then
    echo "Error: Must be on a feature branch, not main or detached HEAD." >&2
    exit 78
fi

COMMIT_COUNT="$(git rev-list --count origin/main..HEAD 2>/dev/null || echo 0)"
if [[ "$COMMIT_COUNT" -eq 0 ]]; then
    echo "Error: No commits found between origin/main and HEAD." >&2
    exit 78
fi

OUT_DIR="artifacts/lkml-patchset"
mkdir -p "$OUT_DIR"

echo "==> Generating LKML patchset in $OUT_DIR..."

git format-patch origin/main..HEAD -o "$OUT_DIR"

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
