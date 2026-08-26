#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# RamShared One-Line Automated Installer for Linux & WSL2
# Usage: curl -fsSL https://raw.githubusercontent.com/emersonbusson/ramshared/main/scripts/install.sh | sudo bash
set -euo pipefail

REPO="emersonbusson/ramshared"
VERSION="${RAMSHARED_VERSION:-v0.9.0-beta.2}"
ARCH="amd64"
INSTALL_PREFIX="/usr/local"
BIN_DIR="${INSTALL_PREFIX}/bin"
SHARE_DIR="${INSTALL_PREFIX}/share/ramshared"
SYSTEMD_DIR="/etc/systemd/system"
CONF_DIR="/etc/ramshared"

echo ""
echo "  ======================================================="
echo "    RamShared Installer — High-Performance VRAM Tier     "
echo "  ======================================================="
echo ""

# Check root permissions
if [[ $EUID -ne 0 ]]; then
  echo "Error: This installer must be run as root (use sudo)." >&2
  exit 1
fi

# Detect environment
IS_WSL=0
if grep -qi microsoft /proc/version 2>/dev/null; then
  IS_WSL=1
  echo "  [+] Environment detected: Microsoft WSL2"
else
  echo "  [+] Environment detected: Native Linux"
fi

# Check GPU / NVIDIA tools
if command -v nvidia-smi >/dev/null 2>&1; then
  GPU_NAME=$(nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | head -n 1 || echo "NVIDIA GPU")
  echo "  [+] GPU detected: ${GPU_NAME}"
else
  echo "  [!] Warning: nvidia-smi not found. Ensure NVIDIA drivers are installed."
fi

# Determine source: local directory or GitHub download
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || echo "")"
LOCAL_SRC=""
if [[ -n "$SCRIPT_DIR" && -d "${SCRIPT_DIR}/../target/release" ]]; then
  LOCAL_SRC="${SCRIPT_DIR}/.."
fi

TMP_DIR="$(mktemp -d /tmp/ramshared-install.XXXXXX)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

if [[ -n "$LOCAL_SRC" && -x "${LOCAL_SRC}/target/release/ramshared" ]]; then
  echo "  [+] Installing from local build artifacts..."
  cp "${LOCAL_SRC}/target/release/ramshared" "${TMP_DIR}/ramshared"
  cp "${LOCAL_SRC}/target/release/ramsharedd" "${TMP_DIR}/ramsharedd"
  cp -r "${LOCAL_SRC}/scripts/safety" "${TMP_DIR}/safety"
else
  echo "  [+] Downloading RamShared ${VERSION} release bundle..."
  TARBALL="ramshared-linux-${VERSION}.tar.gz"
  URL="https://github.com/${REPO}/releases/download/${VERSION}/${TARBALL}"
  SHA_URL="${URL}.sha256"

  if ! curl -fsSL --retry 3 "${URL}" -o "${TMP_DIR}/${TARBALL}"; then
    echo "Error: Failed to download release tarball from ${URL}" >&2
    exit 1
  fi

  if curl -fsSL --retry 3 "${SHA_URL}" -o "${TMP_DIR}/${TARBALL}.sha256"; then
    echo "  [+] Verifying SHA-256 integrity checksum..."
    (cd "${TMP_DIR}" && sha256sum -c "${TARBALL}.sha256" >/dev/null 2>&1) || {
      echo "Error: SHA-256 checksum verification failed!" >&2
      exit 1
    }
    echo "  [+] SHA-256 checksum verified OK."
  fi

  echo "  [+] Extracting release bundle..."
  tar -xzf "${TMP_DIR}/${TARBALL}" -C "${TMP_DIR}"
  RELEASE_DIR="${TMP_DIR}/ramshared-linux-${VERSION}"

  cp "${RELEASE_DIR}/bin/ramshared" "${TMP_DIR}/ramshared"
  cp "${RELEASE_DIR}/bin/ramsharedd" "${TMP_DIR}/ramsharedd"
  cp -r "${RELEASE_DIR}/scripts/safety" "${TMP_DIR}/safety"
  if [[ -d "${RELEASE_DIR}/systemd" ]]; then
    cp -r "${RELEASE_DIR}/systemd" "${TMP_DIR}/systemd"
  fi
fi

# Create target directories
mkdir -p "${BIN_DIR}" "${SHARE_DIR}/scripts" "${CONF_DIR}" "${SYSTEMD_DIR}"

# Install binaries
install -m 0755 "${TMP_DIR}/ramshared" "${BIN_DIR}/ramshared"
install -m 0755 "${TMP_DIR}/ramsharedd" "${BIN_DIR}/ramsharedd"
echo "  [+] Installed binaries to ${BIN_DIR}/"

# Install safety scripts
if [[ -d "${TMP_DIR}/safety" ]]; then
  cp -r "${TMP_DIR}/safety/"* "${SHARE_DIR}/scripts/"
  chmod -R 0755 "${SHARE_DIR}/scripts"
  echo "  [+] Installed safety scripts to ${SHARE_DIR}/scripts/"
fi

# Install systemd units
if [[ -d "${TMP_DIR}/systemd" ]]; then
  cp -r "${TMP_DIR}/systemd/"*.service "${SYSTEMD_DIR}/" 2>/dev/null || true
  cp -r "${TMP_DIR}/systemd/"*.slice "${SYSTEMD_DIR}/" 2>/dev/null || true
  if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
  fi
  echo "  [+] Installed systemd units to ${SYSTEMD_DIR}/"
fi

# Create default configuration if not present
if [[ ! -f "${CONF_DIR}/cascade.conf" ]]; then
  if [[ -f "${SHARE_DIR}/scripts/cascade.conf.example" ]]; then
    cp "${SHARE_DIR}/scripts/cascade.conf.example" "${CONF_DIR}/cascade.conf"
  else
    cat << CONF_EOF > "${CONF_DIR}/cascade.conf"
# RamShared default cascade configuration
VRAM_CAPACITY_MIB=1024
ZRAM_CAPACITY_MIB=1024
LOGICAL_CAPACITY_MIB=4096
CONF_EOF
  fi
  echo "  [+] Created default configuration at ${CONF_DIR}/cascade.conf"
fi

echo ""
echo "  ======================================================="
echo "    RamShared Installation Complete!                     "
echo "  ======================================================="
echo ""
echo "  To test your system readiness:"
echo "    sudo ramshared check"
echo ""
echo "  To start the VRAM memory cushion:"
echo "    sudo ramshared up --vram 1024 --zram 1024"
echo ""
echo "  To view active status:"
echo "    sudo ramshared status"
echo ""
