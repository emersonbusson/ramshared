#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Build RPM package (.rpm) for RamShared (Fedora, RHEL, CentOS, openSUSE).
# Usage: scripts/package/build-rpm-package.sh [version]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${1:-${RAMSHARED_PACKAGE_VERSION:-v0.14.1}}"
VERSION_CLEAN="${VERSION#v}"
RPM_VERSION="$(echo "$VERSION_CLEAN" | sed "s/-beta\./.beta/")"
ARCH="x86_64"

OUT_DIR="$ROOT/artifacts/packages"
RPM_ROOT="$OUT_DIR/rpmbuild"
SPEC_FILE="$RPM_ROOT/SPECS/ramshared.spec"

echo "==> Building RPM package for RamShared ${VERSION} (${ARCH})..."

# This packaging step consumes previously built release binaries. The heavy
# Cargo build is admitted and run separately by the caller.
CLI_BIN="$ROOT/target/release/ramshared"
DAEMON_BIN="$ROOT/target/release/ramsharedd"

if [[ ! -x "$CLI_BIN" || ! -x "$DAEMON_BIN" ]]; then
  echo "ERROR: Prebuilt release binaries not found ($CLI_BIN / $DAEMON_BIN)" >&2
  exit 1
fi

if ! command -v rpmbuild >/dev/null 2>&1; then
  echo "ERROR: rpmbuild is required to produce an RPM artifact" >&2
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
License:        GPL-2.0-only
URL:            https://github.com/emersonbusson/ramshared

%description
RamShared provides a bounded VRAM-backed memory tier with an authoritative
origin. Transport and performance depend on the qualified host configuration.

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
- Official v0.14.1 Linux RPM release for the documented support matrix.
SPEC_EOF

echo "==> Executing rpmbuild..."
rpmbuild --define "_topdir $RPM_ROOT" -bb "$SPEC_FILE"
shopt -s nullglob
rpm_artifacts=("$RPM_ROOT"/RPMS/*/*.rpm)
shopt -u nullglob
if (( ${#rpm_artifacts[@]} == 0 )); then
  echo "ERROR: rpmbuild produced no RPM artifact" >&2
  exit 1
fi
cp "${rpm_artifacts[@]}" "$OUT_DIR/"
echo "✓ RPM package built under $OUT_DIR/"
