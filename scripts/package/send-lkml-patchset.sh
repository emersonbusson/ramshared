#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Interactive and secure LKML patchset dispatcher for RamShared
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

OUT_DIR="artifacts/lkml-patchset"

EMAIL_AT="@"
VGER_DOMAIN="vger.kernel.org"
KERNEL_DOMAIN="kernel.dk"
GMAIL_DOMAIN="gmail.com"
LKML_TO="linux-block${EMAIL_AT}${VGER_DOMAIN}"
AXBOE_CC="axboe${EMAIL_AT}${KERNEL_DOMAIN}"
LKML_CC="linux-kernel${EMAIL_AT}${VGER_DOMAIN}"

DRY_RUN=0
if [ "${1:-}" = "--dry-run" ]; then
    DRY_RUN=1
elif [ $# -gt 0 ]; then
    echo "❌ Error: Invalid argument: $1" >&2
    echo "Usage: $0 [--dry-run]" >&2
    exit 64
fi

echo "============================================================"
echo "  🐧 RamShared — Linux Kernel LKML Patch Dispatcher"
echo "============================================================"
echo ""

# Ensure patchset exists
if [ ! -f "$OUT_DIR/0000-cover-letter.patch" ] || \
   [ ! -f "$OUT_DIR/0001-drivers-block-ramshared-add-hardware-VRAM-block-driver.patch" ] || \
   [ ! -f "$OUT_DIR/0002-drivers-block-integrate-ramshared-into-Kconfig-and-Makefile.patch" ]; then
    echo "==> Generating latest patchset..."
    ./scripts/package/generate-kernel-patchset.sh
fi

SENDER_NAME="$(git config user.name 2>/dev/null || echo 'Emerson Busson')"
SENDER_EMAIL="$(git config user.email 2>/dev/null || echo "developer${EMAIL_AT}${GMAIL_DOMAIN}")"

echo "Sender: $SENDER_NAME <$SENDER_EMAIL>"
echo "Destination: $LKML_TO (Jens Axboe)"
echo "CC: $LKML_CC"
echo ""

if [ "$DRY_RUN" -eq 1 ]; then
    echo "==> [DRY RUN] Patch summary:"
    for patch in "$OUT_DIR"/*.patch; do
        if [ -f "$patch" ]; then
            echo "  • $(basename "$patch")"
        fi
    done
    echo ""
    echo "==> [DRY RUN] Exact recipients:"
    echo "  • To: $LKML_TO"
    echo "  • Cc: $AXBOE_CC"
    echo "  • Cc: $LKML_CC"
    echo ""
    echo "==> [DRY RUN] Execution complete. No emails were sent."
    exit 0
fi

# Safely prompt for Gmail App Password (hidden input, no echo)
read -r -s -p "Enter Gmail App Password (16 characters): " SMTP_PASS
echo ""

if [ -z "$SMTP_PASS" ]; then
    echo "❌ Error: App Password cannot be empty."
    exit 1
fi

echo ""
echo "==> Sending patchset series via smtp.${GMAIL_DOMAIN}..."

# Dispatch via git send-email with secure in-memory password
git send-email \
    --smtp-server="smtp.${GMAIL_DOMAIN}" \
    --smtp-server-port=587 \
    --smtp-encryption=tls \
    --smtp-user="$SENDER_EMAIL" \
    --smtp-pass="$SMTP_PASS" \
    --to="$LKML_TO" \
    --cc="$AXBOE_CC" \
    --cc="$LKML_CC" \
    --confirm=never \
    --quiet \
    "$OUT_DIR/0000-cover-letter.patch" \
    "$OUT_DIR/0001-drivers-block-ramshared-add-hardware-VRAM-block-driver.patch" \
    "$OUT_DIR/0002-drivers-block-integrate-ramshared-into-Kconfig-and-Makefile.patch"

# Scrub password from memory immediately
unset SMTP_PASS

echo ""
echo "============================================================"
echo "  ✅ LKML PATCHSET SENT SUCCESSFULLY!"
echo "============================================================"
echo "The patches have been delivered to:"
echo "  • Jens Axboe <$AXBOE_CC>"
echo "  • $LKML_TO"
echo "  • $LKML_CC"
echo ""
echo "Check your inbox at $SENDER_EMAIL for the incoming delivery receipt."
