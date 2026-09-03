#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Build RPM package (.rpm) for RamShared (Fedora, RHEL, CentOS, openSUSE).
# Usage: scripts/package/build-rpm-package.sh [version]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${1:-${RAMSHARED_PACKAGE_VERSION:-v0.9.0-beta.2}}"
VERSION_CLEAN="${VERSION#v}"
RPM_VERSION="$(echo "$VERSION_CLEAN" | sed "s/-beta\./.beta/")"
ARCH="x86_64"

# Guard clauses: ensure required binaries are available
if ! command -v rpmbuild >/dev/null 2>&1; then
  echo "ERROR: rpmbuild command not found" >&2
  exit 69
fi

if ! command -v rpmspec >/dev/null 2>&1; then
  echo "ERROR: rpmspec command not found" >&2
  exit 69
fi

OUT_DIR="$ROOT/artifacts/packages"
RPM_ROOT="$OUT_DIR/rpmbuild"
SPEC_FILE="$RPM_ROOT/SPECS/ramshared.spec"

mkdir -p "$OUT_DIR"
FREE_SPACE=$(df -kP "$OUT_DIR" | awk 'NR==2 {print $4}')
if [[ "$FREE_SPACE" -lt 102400 ]]; then
  echo "ERROR: Insufficient disk space in $OUT_DIR" >&2
  exit 74
fi

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
  exit 69
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
License:        GPL-2.0-only
URL:            https://github.com/emersonbusson/ramshared

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

if command -v rpmbuild >/dev/null 2>&1; then
  echo "==> Validating RPM spec file syntax..."
  if ! rpmspec -q "$SPEC_FILE" >/dev/null 2>&1; then
    echo "ERROR: Invalid RPM spec file syntax or missing macros in $SPEC_FILE" >&2
    exit 78
  fi

  echo "==> Executing rpmbuild..."
  rpmbuild --define "_topdir $RPM_ROOT" -bb "$SPEC_FILE"
  cp "$RPM_ROOT"/RPMS/*/*.rpm "$OUT_DIR/" 2>/dev/null || true
  echo "✓ RPM package built under $OUT_DIR/"
else
  echo "==> rpmbuild not installed on host. Spec generated at $SPEC_FILE (PASS)."
fi
