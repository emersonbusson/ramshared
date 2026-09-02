#!/usr/bin/env bash
# install-cascade-app.sh — install .desktop launcher for the control app.
# SPEC: docs/specs/no-milestone/cascade-desktop-app/SPEC.md ITEM-3
set -euo pipefail

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPTS="$REPO/scripts/safety"
TEMPLATE="$SCRIPTS/ramshared-cushion.desktop.in"

TARGET_ARCH="$(uname -m)"
if [[ "$TARGET_ARCH" != "x86_64" && "$TARGET_ARCH" != "aarch64" ]]; then
  echo "EX_CONFIG: unsupported architecture: $TARGET_ARCH" >&2
  exit 78
fi

if ! command -v file >/dev/null 2>&1; then
  echo "EX_UNAVAILABLE: 'file' command is required for architecture validation" >&2
  exit 69
fi

CLI_BIN="$REPO/target/release/ramshared"
if [[ ! -x "$CLI_BIN" ]]; then
  CLI_BIN="$REPO/target/debug/ramshared"
fi

if [[ ! -x "$CLI_BIN" ]]; then
  echo "EX_UNAVAILABLE: ramshared binary not found. Build it first." >&2
  exit 69
fi

BIN_ARCH=$(file -b "$CLI_BIN" || true)
if [[ "$TARGET_ARCH" == "x86_64" && ! "$BIN_ARCH" =~ x86-64 ]]; then
  echo "EX_CONFIG: binary architecture ($BIN_ARCH) does not match host ($TARGET_ARCH)" >&2
  exit 78
elif [[ "$TARGET_ARCH" == "aarch64" ]]; then
  if [[ ! "$BIN_ARCH" =~ aarch64 ]] && [[ ! "$BIN_ARCH" =~ ARM ]]; then
    echo "EX_CONFIG: binary architecture ($BIN_ARCH) does not match host ($TARGET_ARCH)" >&2
    exit 78
  fi
fi

BIN_VERSION=$("$CLI_BIN" --version 2>/dev/null | awk '{print $2}' || true)
if [[ -z "$BIN_VERSION" ]]; then
  echo "EX_IOERR: failed to get ramshared binary version" >&2
  exit 74
fi
if [[ ! "$BIN_VERSION" =~ ^[0-9] ]]; then
  echo "EX_CONFIG: invalid ramshared binary version: $BIN_VERSION" >&2
  exit 78
fi

chmod +x "$SCRIPTS/cascade-app.sh" \
  "$SCRIPTS/cascade-preflight.sh" \
  "$SCRIPTS/cascade-up.sh" \
  "$SCRIPTS/cascade-down.sh" \
  "$SCRIPTS/install-cascade-boot.sh" \
  "$SCRIPTS/uninstall-cascade-boot.sh" 2>/dev/null || true

if [[ "$(id -u)" -eq 0 ]]; then
  DEST_DIR="/usr/local/share/applications"
else
  DEST_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
fi
mkdir -p "$DEST_DIR"
OUT="$DEST_DIR/ramshared-cushion.desktop"

sed -e "s|@SCRIPTS_PATH@|$SCRIPTS|g" "$TEMPLATE" > "$OUT"
chmod 0644 "$OUT"

# Refresh menu cache if tools exist
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$DEST_DIR" 2>/dev/null || true
fi

echo "Installed launcher: $OUT"
echo "Open it from the app menu as “RamShared Cushion”,"
echo "or run:  $SCRIPTS/cascade-app.sh --gui"
echo
echo "CLI:  $SCRIPTS/cascade-app.sh status|check|start|stop"
