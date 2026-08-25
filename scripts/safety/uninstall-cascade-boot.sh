#!/usr/bin/env bash
# uninstall-cascade-boot.sh — disable sealed units; leave /etc/ramshared/cascade.conf.
set -euo pipefail

[[ "$(id -u)" -eq 0 ]] || { echo "run with sudo" >&2; exit 1; }

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
SYSTEMD_UNIT_DIR=/etc/systemd/system
if [[ -x $REPO/bin/ramshared ]]; then
  DEFAULT_BIN_DIR="$REPO/bin"
else
  DEFAULT_BIN_DIR="$REPO/target/release"
fi
BIN_DIR="${RAMSHARED_BIN_DIR:-$DEFAULT_BIN_DIR}"
CLI="${RAMSHARED_CLI:-$BIN_DIR/ramshared}"
REMOVED_SEALED_UNITS=0

remove_sealed_unit_if_owned() {
  local unit=$1 stop_before_remove=${2:-1} expected="$REPO/systemd/$1" target="$SYSTEMD_UNIT_DIR/$1"
  [[ -f $expected && ! -L $expected ]] || {
    echo "  [warn] expected sealed unit is unavailable: $unit" >&2
    return 0
  }
  if [[ ! -e $target && ! -L $target ]]; then
    return 0
  fi
  if [[ -L $target || ! -f $target ]]; then
    echo "  [warn] preserving non-regular unit definition: $target" >&2
    return 0
  fi
  if ! cmp -s "$expected" "$target"; then
    echo "  [warn] preserving non-matching unit definition: $target" >&2
    return 0
  fi

  if (( stop_before_remove )); then
    systemctl stop "$unit" 2>/dev/null || true
    systemctl disable "$unit" 2>/dev/null || true
  fi
  rm -f -- "$target"
  REMOVED_SEALED_UNITS=$((REMOVED_SEALED_UNITS + 1))
}

echo "== uninstall cascade boot =="

if command -v systemctl >/dev/null 2>&1; then
  remove_sealed_unit_if_owned ramshared-cascade-health.service
  remove_sealed_unit_if_owned ramshared-cascade.service
  remove_sealed_unit_if_owned ramshared-workloads.slice 0
  if (( REMOVED_SEALED_UNITS > 0 )); then
    systemctl daemon-reload
    echo "  [ok] sealed units removed=$REMOVED_SEALED_UNITS"
  else
    echo "  [note] no matching sealed unit definitions were removed"
  fi
else
  echo "  [warn] systemctl missing — remove sealed unit files by hand if present"
fi

# Extra safety: ordered down if cascade still live.
if [[ -x "$CLI" ]]; then
  "$CLI" down 2>/dev/null || true
fi

echo "  [note] /etc/ramshared/cascade.conf left in place (your sizes)."
echo "Done."
