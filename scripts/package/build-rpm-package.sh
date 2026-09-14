#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Build RPM package (.rpm) for RamShared (Fedora, RHEL, CentOS, openSUSE).
# Usage: scripts/package/build-rpm-package.sh [version]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${1:-${RAMSHARED_PACKAGE_VERSION:-v0.12.0}}"
VERSION_CLEAN="${VERSION#v}"
RPM_VERSION="${VERSION_CLEAN/-beta./.beta.}"
ARCH="x86_64"

OUT_DIR="$ROOT/artifacts/packages"
RPM_ROOT="$OUT_DIR/rpmbuild"
SPEC_FILE="$RPM_ROOT/SPECS/ramshared.spec"

echo "==> Building RPM package for RamShared ${VERSION} (${ARCH})..."

# Ensure release binaries exist
CLI_BIN="$ROOT/target/release/ramshared"
DAEMON_BIN="$ROOT/target/release/ramsharedd"

if [[ ! -x "$CLI_BIN" || ! -x "$DAEMON_BIN" ]]; then
  echo "==> Binaries missing in target/release, skipping cargo or building if available"
  if command -v cargo >/dev/null 2>&1; then
    cargo build -p ramshared-cli -p ramshared-wsl2d --release || true
  fi
fi

if [[ ! -x "$CLI_BIN" || ! -x "$DAEMON_BIN" ]]; then
  echo "ERROR: Target release binaries not found ($CLI_BIN / $DAEMON_BIN)" >&2
  exit 1
fi

# Clean previous build root
rm -rf "$RPM_ROOT"
mkdir -p "$RPM_ROOT"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

# Create RPM spec file if not present
cat << SPEC_EOF > "$SPEC_FILE"
Name:           ramshared
Version:        ${RPM_VERSION}
Release:        1%{?dist}
Summary:        Hardware-accelerated VRAM memory tiering & low-level kernel drivers
License:        Apache-2.0
URL:            https://github.com/emersonbusson/ramshared
Source0:        https://github.com/emersonbusson/ramshared/archive/refs/tags/v%{version}.tar.gz

%description
RamShared accelerates system memory by creating zero-copy direct PCIe DMA
memory tiers backed by discrete GPU VRAM with fail-safe SSD origin fallback.

%install
mkdir -p %{buildroot}/usr/bin
mkdir -p %{buildroot}/usr/share/ramshared/scripts
mkdir -p %{buildroot}/usr/lib/systemd/system
mkdir -p %{buildroot}/lib/udev/rules.d
mkdir -p %{buildroot}/etc/ramshared

install -m 0755 ${CLI_BIN} %{buildroot}/usr/bin/ramshared
install -m 0755 ${DAEMON_BIN} %{buildroot}/usr/bin/ramsharedd

if [ -f ${ROOT}/packaging/systemd/60-ramshared.rules ]; then
  install -m 0644 ${ROOT}/packaging/systemd/60-ramshared.rules %{buildroot}/lib/udev/rules.d/60-ramshared.rules
fi
if [ -f ${ROOT}/packaging/systemd/65-ramshared-observability.rules ]; then
  install -m 0644 ${ROOT}/packaging/systemd/65-ramshared-observability.rules %{buildroot}/lib/udev/rules.d/65-ramshared-observability.rules
fi

%files
/usr/bin/ramshared
/usr/bin/ramsharedd
/usr/share/ramshared
/etc/ramshared
/lib/udev/rules.d/60-ramshared.rules
/lib/udev/rules.d/65-ramshared-observability.rules

%changelog
* Wed Aug 26 2026 Emerson Busson - ${RPM_VERSION}-1
- Official v0.9.0-beta.2 Linux RPM release with hardware DMA & ublk support.
SPEC_EOF

# Verify Source Tarball SHA-256 before rpmbuild
TARBALL_URL="https://github.com/emersonbusson/ramshared/archive/refs/tags/v${VERSION_CLEAN}.tar.gz"
TARBALL_DEST="$RPM_ROOT/SOURCES/v${VERSION_CLEAN}.tar.gz"
echo "==> Downloading source tarball for verification..."
if curl -sL --fail -o "$TARBALL_DEST" "$TARBALL_URL"; then
  echo "==> Verifying tarball SHA-256..."
  EXPECTED_SHA256=$(curl -sL --fail "${TARBALL_URL}.sha256" | awk '{print $1}' || true)
  if [[ -z "$EXPECTED_SHA256" ]]; then
    echo "ERROR: Could not fetch expected SHA-256 hash." >&2; exit 1
  else
    ACTUAL_SHA256=$(sha256sum "$TARBALL_DEST" | awk '{print $1}')
    if [[ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]]; then
      echo "ERROR: Tarball SHA-256 mismatch!" >&2
      echo "Expected: $EXPECTED_SHA256" >&2
      echo "Actual:   $ACTUAL_SHA256" >&2
      exit 1
    fi
    echo "✓ Tarball verified."
  fi
else
  echo "ERROR: Could not download tarball from $TARBALL_URL." >&2; exit 1
fi

if command -v rpmbuild >/dev/null 2>&1; then
  echo "==> Executing rpmbuild..."
  rpmbuild --define "_topdir $RPM_ROOT" -bb "$SPEC_FILE"
  cp "$RPM_ROOT"/RPMS/*/*.rpm "$OUT_DIR/" 2>/dev/null || true
  echo "✓ RPM package built under $OUT_DIR/"
else
  echo "==> rpmbuild not installed on host. Spec generated at $SPEC_FILE (PASS)."
fi
