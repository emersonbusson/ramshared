#!/usr/bin/env bash
# cascade-up.sh — source conf and run Day-1 `ramshared up` (systemd-friendly).
# SPEC: docs/specs/no-milestone/wsl2-cascade-boot/SPEC.md ITEM-3/4
set -euo pipefail

REPO="${RAMSHARED_REPO:-$(cd "$(dirname "$0")/../.." && pwd)}"
CONF="${RAMSHARED_CASCADE_CONF:-/etc/ramshared/cascade.conf}"
BIN_DIR="${RAMSHARED_BIN_DIR:-$REPO/target/release}"
CLI="${RAMSHARED_CLI:-$BIN_DIR/ramshared}"
DAEMON="${RAMSHARED_DAEMON:-$BIN_DIR/ramsharedd}"

if [[ -f "$CONF" ]]; then
  # shellcheck source=/dev/null
  source "$CONF"
fi

export RAMSHARED_VRAM_MIB="${VRAM_MIB:-${RAMSHARED_VRAM_MIB:-1024}}"
export RAMSHARED_ZRAM_MIB="${ZRAM_MIB:-${RAMSHARED_ZRAM_MIB:-1024}}"
VRAM_MIB="${RAMSHARED_VRAM_MIB}"
ZRAM_MIB="${RAMSHARED_ZRAM_MIB}"

exec "$CLI" up --vram "$VRAM_MIB" --zram "$ZRAM_MIB" --daemon "$DAEMON"
