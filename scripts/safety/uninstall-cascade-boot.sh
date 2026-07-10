#!/usr/bin/env bash
# uninstall-cascade-boot.sh — disable unit; leave /etc/ramshared/cascade.conf.
set -euo pipefail

[[ "$(id -u)" -eq 0 ]] || { echo "run with sudo" >&2; exit 1; }

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
BIN_DIR="${RAMSHARED_BIN_DIR:-$REPO/target/release}"
CLI="${RAMSHARED_CLI:-$BIN_DIR/ramshared}"

echo "== uninstall cascade boot =="

if command -v systemctl >/dev/null 2>&1; then
  systemctl stop ramshared-cascade.service 2>/dev/null || true
  systemctl disable ramshared-cascade.service 2>/dev/null || true
  rm -f /etc/systemd/system/ramshared-cascade.service
  systemctl daemon-reload
  echo "  [ok] unit removed"
else
  echo "  [warn] systemctl missing — remove unit file by hand if present"
fi

# Extra safety: ordered down if cascade still live.
if [[ -x "$CLI" ]]; then
  "$CLI" down 2>/dev/null || true
fi

echo "  [note] /etc/ramshared/cascade.conf left in place (your sizes)."
echo "Done."
