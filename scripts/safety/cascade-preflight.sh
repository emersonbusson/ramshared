#!/usr/bin/env bash
# cascade-preflight.sh — fail-closed gate before Day-1 NBD cascade (boot or manual).
# SPEC: docs/specs/no-milestone/wsl2-cascade-boot/SPEC.md ITEM-2
#
# exit 0 = safe to run `ramshared up`
# exit 1 = refuse (do not start)
#
# Read-only except optional modprobe nbd when root.
set -euo pipefail

REPO="${RAMSHARED_REPO:-$(cd "$(dirname "$0")/../.." && pwd)}"
CONF="${RAMSHARED_CASCADE_CONF:-/etc/ramshared/cascade.conf}"
BIN_DIR="${RAMSHARED_BIN_DIR:-$REPO/target/release}"
CLI="${RAMSHARED_CLI:-$BIN_DIR/ramshared}"
DAEMON="${RAMSHARED_DAEMON:-$BIN_DIR/ramsharedd}"

# shellcheck disable=SC1090
if [[ -f "$CONF" ]]; then
  # shellcheck source=/dev/null
  source "$CONF"
fi

VRAM_MIB="${VRAM_MIB:-${RAMSHARED_VRAM_MIB:-1024}}"
ZRAM_MIB="${ZRAM_MIB:-${RAMSHARED_ZRAM_MIB:-1024}}"
MIN_HEADROOM="${MIN_VRAM_HEADROOM_MIB:-${RAMSHARED_MIN_VRAM_FREE_MIB:-256}}"
FORCE="${RAMSHARED_FORCE:-0}"
# SPEC cascade-vram-ondemand: sparse default needs only headroom+canary+1 chunk free.
CHUNK_MIB="${RAMSHARED_VRAM_CHUNK_MIB:-128}"
PREALLOC="${RAMSHARED_VRAM_PREALLOC:-0}"

NVSMI="$(command -v nvidia-smi 2>/dev/null || true)"
[[ -x "$NVSMI" ]] || NVSMI="/usr/lib/wsl/lib/nvidia-smi"

fail() { echo "CASCADE-PREFLIGHT: refuse — $1" >&2; exit 1; }

echo "== RamShared cascade preflight (fail-closed) =="

[[ -x "$CLI" ]] || fail "ramshared binary missing: $CLI (cargo build -p ramshared-cli --release)"
[[ -x "$DAEMON" ]] || fail "ramsharedd binary missing: $DAEMON (cargo build -p ramshared-wsl2d --release)"
echo "  [ok] binaries present"

if [[ ! -r /proc/swaps ]]; then
  fail "cannot read /proc/swaps"
fi

# Ghost swap = historical WSL freeze vector. Never start on top of it.
if grep -E 'nbd|ublk|zram' /proc/swaps 2>/dev/null | grep -qiE 'deleted|\\040'; then
  fail "ghost swap (deleted device) in /proc/swaps — run wsl --shutdown on Windows, reopen distro, then ramshared down"
fi
echo "  [ok] no ghost managed swap"

if ! command -v nbd-client >/dev/null 2>&1; then
  fail "nbd-client not installed (apt install nbd-client)"
fi
echo "  [ok] nbd-client present"

if [[ "$(id -u)" -eq 0 ]]; then
  modprobe nbd nbds_max=1 max_part=0 2>/dev/null || true
fi

if [[ ! -x "$NVSMI" ]]; then
  fail "nvidia-smi not found — GPU path not usable in this WSL"
fi
SMI_OUT="$("$NVSMI" --query-gpu=memory.free --format=csv,noheader,nounits 2>/dev/null || true)"
[[ -n "$SMI_OUT" ]] || fail "nvidia-smi did not return free memory"
VRAM_FREE="$(echo "$SMI_OUT" | head -1 | tr -dc '0-9')"
[[ -n "$VRAM_FREE" ]] || fail "could not parse free VRAM"
# canary is 4 KiB (~0 MiB); round up to 1 MiB for gate simplicity
case "${PREALLOC}" in
  1|true|yes|on|TRUE|YES|ON)
    NEED=$((VRAM_MIB + MIN_HEADROOM))
    MODE_NOTE="prealloc: VRAM_MIB+headroom"
    ;;
  *)
    NEED=$((MIN_HEADROOM + 1 + CHUNK_MIB))
    MODE_NOTE="sparse: headroom+canary+chunk (capacity VRAM_MIB=${VRAM_MIB} not fully committed)"
    ;;
esac
if [[ "$VRAM_FREE" -lt "$NEED" ]]; then
  fail "free VRAM ${VRAM_FREE} MiB < need ${NEED} (${MODE_NOTE}) — close a game/render or lower sizes"
fi
echo "  [ok] free VRAM=${VRAM_FREE} MiB (need >= ${NEED}; ${MODE_NOTE})"

# Soft A1: prefer a lower swap tier (disk). If none and RAM is tight, refuse unless FORCE.
HAS_DISK_SWAP=0
if awk 'NR>1 && $1 !~ /zram|nbd|ublk/ { found=1 } END { exit !found }' /proc/swaps 2>/dev/null; then
  HAS_DISK_SWAP=1
fi
MEM_AVAIL_KIB="$(awk '/MemAvailable:/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)"
MEM_AVAIL_MIB=$((MEM_AVAIL_KIB / 1024))
if [[ "$HAS_DISK_SWAP" -eq 0 ]]; then
  DOUBLE=$((VRAM_MIB * 2))
  if [[ "$MEM_AVAIL_MIB" -lt "$DOUBLE" && "$FORCE" != "1" ]]; then
    fail "no disk swap tier and MemAvailable ${MEM_AVAIL_MIB} MiB < 2×VRAM (${DOUBLE}). Add WSL swap or set RAMSHARED_FORCE=1"
  fi
  echo "  [warn] no disk swap seen; demote safety depends on free RAM (${MEM_AVAIL_MIB} MiB)"
else
  echo "  [ok] lower swap tier present (disk)"
fi

echo "CASCADE-PREFLIGHT: OK — safe to start cascade."
exit 0
