#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Orchestrator script building all distro packages in a single pipeline.
# Usage: scripts/package/package-all.sh [version]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

VERSION="${1:-${RAMSHARED_PACKAGE_VERSION:-v0.12.0}}"

echo "==> Starting consolidated package build pipeline for ${VERSION}..."

FAILURES=0

echo "--> Building Debian package..."
if ! "$ROOT/scripts/package/build-deb-package.sh" "$VERSION"; then
    echo "ERROR: Failed to build Debian package." >&2
    FAILURES=$((FAILURES + 1))
fi

echo "--> Building RPM package..."
if ! "$ROOT/scripts/package/build-rpm-package.sh" "$VERSION"; then
    echo "ERROR: Failed to build RPM package." >&2
    FAILURES=$((FAILURES + 1))
fi

echo "--> Building Arch Linux bundle (PKGBUILD)..."
ARCH_PKG_DIR="$ROOT/packaging/arch"
if [[ -f "$ARCH_PKG_DIR/PKGBUILD" ]]; then
    echo "    Found PKGBUILD at $ARCH_PKG_DIR/PKGBUILD"
    OUT_DIR="$ROOT/artifacts/packages/arch"
    mkdir -p "$OUT_DIR"
    cp "$ARCH_PKG_DIR/PKGBUILD" "$OUT_DIR/"
    cp "$ARCH_PKG_DIR/ramshared.install" "$OUT_DIR/" 2>/dev/null || true
    echo "    Arch Linux PKGBUILD staged in $OUT_DIR."
else
    echo "ERROR: PKGBUILD not found in $ARCH_PKG_DIR." >&2
    FAILURES=$((FAILURES + 1))
fi

echo "==> Pipeline finished."
if [[ $FAILURES -gt 0 ]]; then
    echo "==> Status: FAILED ($FAILURES errors)." >&2
    exit 1
else
    echo "==> Status: SUCCESS."
    exit 0
fi
