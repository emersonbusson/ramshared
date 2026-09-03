#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Build Debian/Ubuntu package (.deb) for RamShared
# Usage: scripts/package/build-deb-package.sh [version]
set -euo pipefail

for cmd in dpkg-deb fakeroot lintian; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: Required command '$cmd' is not available." >&2
    exit 69
  fi
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${1:-${RAMSHARED_PACKAGE_VERSION:-v0.9.0-beta.2}}"
VERSION_CLEAN="${VERSION#v}"
DEB_VERSION="$(echo "$VERSION_CLEAN" | sed "s/-beta\./-beta/")"
ARCH="amd64"

OUT_DIR="$ROOT/artifacts/packages"
STAGE_DIR="$OUT_DIR/deb-stage/ramshared_${DEB_VERSION}_${ARCH}"
DEB_FILE="$OUT_DIR/ramshared_${DEB_VERSION}_${ARCH}.deb"

echo "==> Building Debian package for RamShared ${VERSION} (${ARCH})..."

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

# Clean previous staging
rm -rf "$STAGE_DIR" "$DEB_FILE"
mkdir -p "$STAGE_DIR/DEBIAN" \
         "$STAGE_DIR/usr/bin" \
         "$STAGE_DIR/usr/share/ramshared/scripts" \
         "$STAGE_DIR/lib/systemd/system" \
         "$STAGE_DIR/etc/ramshared" \
         "$STAGE_DIR/usr/share/doc/ramshared"

# Install binaries
install -m 0755 "$CLI_BIN" "$STAGE_DIR/usr/bin/ramshared"
install -m 0755 "$DAEMON_BIN" "$STAGE_DIR/usr/bin/ramsharedd"

# Install safety scripts
for script in install-cascade-boot.sh uninstall-cascade-boot.sh cascade-up.sh \
              cascade-down.sh cascade-controller.sh provision-origin-swap.sh \
              lifecycle-recovery-status.sh cascade-health.sh nbd-product-preflight.sh \
              nbd-benchmark-cell.sh nbd-benchmark-cgroup-launch.sh \
              cascade_pressure_integrity_worker.py wsl-relay-health.sh \
              manage-control-plane.sh ramshared-host-gate.sh \
              ramshared-session-launcher.sh; do
  if [[ -f "$ROOT/scripts/safety/$script" ]]; then
    install -m 0755 "$ROOT/scripts/safety/$script" "$STAGE_DIR/usr/share/ramshared/scripts/"
  fi
done

# Install libraries and configs
if [[ -f "$ROOT/scripts/safety/nbd-benchmark-lib.sh" ]]; then
  install -m 0644 "$ROOT/scripts/safety/nbd-benchmark-lib.sh" "$STAGE_DIR/usr/share/ramshared/scripts/"
fi
if [[ -f "$ROOT/scripts/safety/cascade.conf.example" ]]; then
  install -m 0644 "$ROOT/scripts/safety/cascade.conf.example" "$STAGE_DIR/etc/ramshared/cascade.conf.example"
fi

# Install systemd service and slice units
for unit in ramshared-cascade.service ramshared-cascade-health.service \
            ramshared-workloads.slice ramshared-control.slice \
            ramshared-host-gate.service ramshared-supervisor.service; do
  if [[ -f "$ROOT/scripts/safety/systemd/$unit" ]]; then
    install -m 0644 "$ROOT/scripts/safety/systemd/$unit" "$STAGE_DIR/lib/systemd/system/"
  fi
done

if [[ -f "$ROOT/packaging/systemd/ramshared-vram.service" ]]; then
  install -m 0644 "$ROOT/packaging/systemd/ramshared-vram.service" "$STAGE_DIR/lib/systemd/system/"
fi
if [[ -f "$ROOT/packaging/systemd/60-ramshared.rules" ]]; then
  mkdir -p "$STAGE_DIR/lib/udev/rules.d"
  install -m 0644 "$ROOT/packaging/systemd/60-ramshared.rules" "$STAGE_DIR/lib/udev/rules.d/"
fi
if [[ -f "$ROOT/packaging/systemd/65-ramshared-observability.rules" ]]; then
  mkdir -p "$STAGE_DIR/lib/udev/rules.d"
  install -m 0644 "$ROOT/packaging/systemd/65-ramshared-observability.rules" "$STAGE_DIR/lib/udev/rules.d/"
fi

# Install documentation & licenses
install -m 0644 "$ROOT/README.md" "$STAGE_DIR/usr/share/doc/ramshared/README.md"
install -m 0644 "$ROOT/LICENSE" "$STAGE_DIR/usr/share/doc/ramshared/copyright" 2>/dev/null || true

# Generate DEBIAN/control file
printf "Package: ramshared\n" > "$STAGE_DIR/DEBIAN/control"
printf "Version: %s\n" "${DEB_VERSION}" >> "$STAGE_DIR/DEBIAN/control"
printf "Section: admin\n" >> "$STAGE_DIR/DEBIAN/control"
printf "Priority: optional\n" >> "$STAGE_DIR/DEBIAN/control"
printf "Architecture: %s\n" "${ARCH}" >> "$STAGE_DIR/DEBIAN/control"
printf "Depends: libc6 (>= 2.31)\n" >> "$STAGE_DIR/DEBIAN/control"
printf "Maintainer: Emerson Busson\n" >> "$STAGE_DIR/DEBIAN/control"
printf "Description: High-Performance VRAM memory tier for Linux and WSL2\n" >> "$STAGE_DIR/DEBIAN/control"
printf " RamShared is an R&D system that utilizes idle GPU Video RAM (VRAM)\n" >> "$STAGE_DIR/DEBIAN/control"
printf " over PCIe as an accelerated, high-throughput memory tier for Linux and WSL2.\n" >> "$STAGE_DIR/DEBIAN/control"
printf " Absorbs memory pressure spikes and prevents desktop/WSL2 swap freezes.\n" >> "$STAGE_DIR/DEBIAN/control"

# Generate DEBIAN/postinst (post-installation script)
cat << 'POSTINST_EOF' > "$STAGE_DIR/DEBIAN/postinst"
#!/bin/sh
set -e

if [ "$1" = "configure" ]; then
  if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
    udevadm trigger --subsystem-match=drm || true
  fi
  if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
  fi
  echo "RamShared installed successfully. Udev auto-activation enabled."
  echo "Run 'sudo ramshared check' to test machine readiness."
fi

exit 0
POSTINST_EOF
chmod 0755 "$STAGE_DIR/DEBIAN/postinst"

# Generate DEBIAN/prerm (pre-removal script)
cat << 'PRERM_EOF' > "$STAGE_DIR/DEBIAN/prerm"
#!/bin/sh
set -e

if [ "$1" = "remove" ]; then
  if command -v systemctl >/dev/null 2>&1; then
    systemctl stop ramshared-vram.service 2>/dev/null || true
    systemctl stop ramshared-cascade.service 2>/dev/null || true
  fi
  if command -v ramshared >/dev/null 2>&1; then
    ramshared cascade down 2>/dev/null || true
  fi
fi

exit 0
PRERM_EOF
chmod 0755 "$STAGE_DIR/DEBIAN/prerm"

# Build the .deb archive
mkdir -p "$OUT_DIR"
dpkg-deb --build --root-owner-group "$STAGE_DIR" "$DEB_FILE"
rm -rf "$STAGE_DIR"

# Compute SHA-256
(cd "$OUT_DIR" && sha256sum "$(basename "$DEB_FILE")" > "$(basename "$DEB_FILE").sha256")

echo "==> Package built: $DEB_FILE"
echo "==> SHA-256: $(cat "${DEB_FILE}.sha256")"
