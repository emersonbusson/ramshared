#!/usr/bin/env bash
# swap-sanitize.sh — Read-only diagnosis + *safe* recovery hints for RamShared swap.
#
# NEVER kill -9 ramsharedd here. NEVER ublk del while device is in /proc/swaps.
# Kahneman #16: curator must not destroy the only path to swapoff.
#
# Usage:
#   ./scripts/safety/swap-sanitize.sh           # diagnose
#   ./scripts/safety/swap-sanitize.sh --fix     # only swapoff live managed paths (no kill)
set -euo pipefail

FIX=0
[[ "${1:-}" == "--fix" ]] && FIX=1

echo "=== /proc/swaps ==="
cat /proc/swaps || true
echo
echo "=== swapon --show ==="
swapon --show 2>/dev/null || true
echo
echo "=== ramsharedd ==="
pgrep -a -x ramsharedd || echo "(none)"
echo
echo "=== devices ==="
ls -la /dev/ublk* /dev/nbd* /dev/zram* 2>/dev/null || echo "(none matching)"
echo
echo "=== /run/ramshared ==="
ls -la /run/ramshared 2>/dev/null || echo "(missing)"

GHOST=0
while read -r line; do
  case "$line" in
    Filename*|*"Type"*) continue ;;
    *ublk*|*nbd*|*zram*)
      if echo "$line" | grep -qE '\(deleted\)|\\040\(deleted\)'; then
        echo "GHOST: $line"
        GHOST=1
      fi
      ;;
  esac
done < /proc/swaps

if [[ "$GHOST" -eq 1 ]]; then
  echo
  echo "ACAO: ghost swap com device deleted."
  echo "  1) No Windows (PowerShell):  wsl --shutdown"
  echo "  2) Reabra Ubuntu / WSL"
  echo "  3) sudo ./target/release/ramshared down"
  echo "  4) sudo ./target/release/ramshared up --vram 2048 --zram 1024"
  echo "NAO: kill -9 ramsharedd | ublk del -a | swapoff -a sob pressao"
  exit 2
fi

if [[ "$FIX" -eq 1 ]]; then
  echo "=== --fix: swapoff managed live paths only ==="
  if [[ -f /run/ramshared/swap-dev ]]; then
    dev=$(tr -d '\n' </run/ramshared/swap-dev)
    echo "swapoff $dev"
    swapoff "$dev" 2>/dev/null || true
  fi
  if [[ -f /run/ramshared/zram-dev ]]; then
    z=$(tr -d '\n' </run/ramshared/zram-dev)
    echo "swapoff $z"
    swapoff "$z" 2>/dev/null || true
    zramctl -r "$z" 2>/dev/null || true
  fi
  # live scan
  while read -r name _ rest; do
    case "$name" in
      Filename) continue ;;
      *nbd*|*zram*|*ublk*)
        if [[ "$name" != *deleted* ]]; then
          echo "swapoff $name"
          swapoff "$name" 2>/dev/null || true
        fi
        ;;
    esac
  done < /proc/swaps
  echo "depois: sudo ramshared down  # para nbd-client -d + stop daemon"
fi

echo "OK diagnose complete (exit 0 = sem ghost)"
exit 0
