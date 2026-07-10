#!/usr/bin/env bash
# install-cascade-boot.sh — opt-in WSL2 cascade on boot (fail-closed).
# SPEC: docs/specs/no-milestone/wsl2-cascade-boot/SPEC.md ITEM-1
#
# Default: install unit + conf only (does NOT enable).
# Enable with:  sudo bash scripts/safety/install-cascade-boot.sh --enable
set -euo pipefail

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
SD="$REPO/scripts/safety/systemd"
SCRIPTS="$REPO/scripts/safety"
BIN_DIR="${RAMSHARED_BIN_DIR:-$REPO/target/release}"
ENABLE=0

for arg in "$@"; do
  case "$arg" in
    --enable) ENABLE=1 ;;
    --help|-h)
      echo "Usage: sudo bash $0 [--enable]"
      echo "  Installs ramshared-cascade.service. Add --enable to start now and on boot."
      exit 0
      ;;
  esac
done

[[ "$(id -u)" -eq 0 ]] || { echo "run with sudo" >&2; exit 1; }

echo "== RamShared cascade boot install =="

if [[ ! -x "$BIN_DIR/ramshared" || ! -x "$BIN_DIR/ramsharedd" ]]; then
  echo "  building release binaries..."
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found and binaries missing under $BIN_DIR" >&2
    exit 1
  fi
  (cd "$REPO" && cargo build -p ramshared-cli -p ramshared-wsl2d --release)
fi

CLI="$BIN_DIR/ramshared"
export PATH="/usr/lib/wsl/lib:${PATH:-}"

echo "  running ramshared check..."
if ! "$CLI" check; then
  echo "CASCADE-INSTALL: refuse — ramshared check is not ready. Fix doctor output first." >&2
  exit 1
fi
echo "  [ok] check ready"

export RAMSHARED_REPO="$REPO"
export RAMSHARED_BIN_DIR="$BIN_DIR"
if ! "$SCRIPTS/cascade-preflight.sh"; then
  echo "CASCADE-INSTALL: refuse — preflight failed." >&2
  exit 1
fi

install -d -m 0755 /etc/ramshared
if [[ ! -f /etc/ramshared/cascade.conf ]]; then
  install -m 0644 "$SCRIPTS/cascade.conf.example" /etc/ramshared/cascade.conf
  echo "  [ok] wrote /etc/ramshared/cascade.conf (edit sizes there)"
else
  echo "  [ok] keeping existing /etc/ramshared/cascade.conf"
fi

sed -e "s|@REPO_PATH@|$REPO|g" \
    -e "s|@SCRIPTS_PATH@|$SCRIPTS|g" \
    -e "s|@BIN_DIR@|$BIN_DIR|g" \
    "$SD/ramshared-cascade.service" > /etc/systemd/system/ramshared-cascade.service
chmod 0644 /etc/systemd/system/ramshared-cascade.service
echo "  [ok] /etc/systemd/system/ramshared-cascade.service"

# Ensure scripts are executable in the tree we point at.
chmod +x "$SCRIPTS/cascade-preflight.sh" "$SCRIPTS/cascade-up.sh" "$SCRIPTS/cascade-down.sh"

if ! command -v systemctl >/dev/null 2>&1; then
  echo "CASCADE-INSTALL: systemctl missing. Enable systemd in /etc/wsl.conf:" >&2
  echo "  [boot]" >&2
  echo "  systemd=true" >&2
  echo "Then: wsl --shutdown and reopen the distro." >&2
  exit 1
fi

systemctl daemon-reload
echo "  [ok] daemon-reload"

if [[ "$ENABLE" -eq 1 ]]; then
  systemctl enable ramshared-cascade.service
  systemctl start ramshared-cascade.service
  echo "  [ok] enabled and started"
  "$CLI" status || true
else
  echo
  echo "Installed but NOT enabled (on purpose)."
  echo "When you want it on every WSL boot:"
  echo "  sudo systemctl enable --now ramshared-cascade.service"
  echo "Or re-run: sudo bash $0 --enable"
fi

echo
echo "Done."
echo "  Config:  /etc/ramshared/cascade.conf"
echo "  Logs:    journalctl -u ramshared-cascade -b"
echo "  Stop:    sudo systemctl stop ramshared-cascade   # runs ramshared down"
echo "  Remove:  sudo bash $SCRIPTS/uninstall-cascade-boot.sh"
echo
echo "If you open a heavy game on Windows, RamShared tries to give GPU memory back"
echo "by itself (DEMOTE). You may feel a short slowdown in WSL — that is not a freeze."
