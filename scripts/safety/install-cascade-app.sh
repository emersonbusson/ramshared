#!/usr/bin/env bash
# install-cascade-app.sh — install .desktop launcher for the control app.
# SPEC: docs/specs/no-milestone/cascade-desktop-app/SPEC.md ITEM-3
set -euo pipefail

host_arch=$(uname -m)
if [[ "$host_arch" != "x86_64" && "$host_arch" != "aarch64" ]]; then
  echo "Error: Unsupported architecture $host_arch. Cascade requires x86_64 or aarch64." >&2
  exit 69 # EX_UNAVAILABLE
fi

REPO="$(cd "$(dirname "$0")/../.." && pwd)"

BIN_DIR="${RAMSHARED_BIN_DIR:-}"
if [[ -z "$BIN_DIR" ]]; then
  if [[ -x "$REPO/target/release/ramshared" ]]; then
    BIN_DIR="$REPO/target/release"
  elif [[ -x "$REPO/target/debug/ramshared" ]]; then
    BIN_DIR="$REPO/target/debug"
  else
    BIN_DIR="$REPO/target/release"
  fi
fi
CLI="${RAMSHARED_CLI:-$BIN_DIR/ramshared}"

if [[ -n "${RAMSHARED_SKIP_BIN_CHECK:-}" ]]; then
  : # skip
elif [[ ! -x "$CLI" ]]; then
  echo "Error: Cascade binary not found or not executable at $CLI" >&2
  exit 69 # EX_UNAVAILABLE
else
  bin_info=$(file -b "$CLI" 2>/dev/null || true)
  if [[ "$host_arch" == "x86_64" && ! "$bin_info" =~ x86-64 ]]; then
    echo "Error: Binary architecture mismatch. Host is x86_64 but binary is not." >&2
    exit 69 # EX_UNAVAILABLE
  elif [[ "$host_arch" == "aarch64" && ! "$bin_info" =~ aarch64 && ! "$bin_info" =~ ARM ]]; then
    echo "Error: Binary architecture mismatch. Host is aarch64 but binary is not." >&2
    exit 69 # EX_UNAVAILABLE
  fi

  bin_version=$("$CLI" --version 2>/dev/null || echo "unknown")
  if [[ -z "$bin_version" || "$bin_version" == "unknown" ]]; then
    echo "Error: Could not determine Cascade binary version." >&2
    exit 69 # EX_UNAVAILABLE
  fi
fi
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
