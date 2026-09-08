#!/usr/bin/env bash
# install-cascade-app.sh — install .desktop launcher for the control app.
# SPEC: docs/specs/no-milestone/cascade-desktop-app/SPEC.md ITEM-3
set -euo pipefail

host_arch=$(uname -m)
if [[ "$host_arch" != "x86_64" && "$host_arch" != "aarch64" ]]; then
  echo "Error: Unsupported architecture $host_arch. Cascade requires x86_64 or aarch64." >&2
  exit 69 # EX_UNAVAILABLE
fi

host_kernel=$(uname -r)
if [[ ! "$host_kernel" =~ ^[5-9]\. && ! "$host_kernel" =~ ^[1-9][0-9]\. ]]; then
  echo "Error: Unsupported kernel version $host_kernel. Cascade requires Linux 5.0+." >&2
  exit 69 # EX_UNAVAILABLE
fi

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPTS="$REPO/scripts/safety"
TEMPLATE="$SCRIPTS/ramshared-cushion.desktop.in"

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
echo "Open it from the app menu as \"RamShared Cushion\","
echo "or run:  $SCRIPTS/cascade-app.sh --gui"
echo
echo "CLI:  $SCRIPTS/cascade-app.sh status|check|start|stop"
