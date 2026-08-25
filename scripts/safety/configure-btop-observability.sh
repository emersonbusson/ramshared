#!/usr/bin/env bash
# Plan-first, reversible btop GPU visibility tuning for WSL2.
set -euo pipefail

MODE=plan
ROLLBACK=""
CONFIG="${XDG_CONFIG_HOME:-${HOME:?HOME is required}/.config}/btop/btop.conf"

usage() {
  printf '%s\n' \
    'usage: configure-btop-observability.sh [--config PATH]' \
    '       configure-btop-observability.sh --apply [--config PATH]' \
    '       configure-btop-observability.sh --rollback BACKUP [--config PATH]'
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --apply) MODE=apply; shift ;;
    --rollback) MODE=rollback; ROLLBACK=${2:-}; shift 2 ;;
    --config) CONFIG=${2:-}; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
done

if [ ! -f "$CONFIG" ] || [ -L "$CONFIG" ]; then
  printf 'btop config must be an existing regular non-symlink file: %s\n' "$CONFIG" >&2
  exit 1
fi

if [ "$MODE" = plan ]; then
  printf '%s\n' \
    'state=PLAN' \
    "config=$CONFIG" \
    'shown_boxes+=gpu0' \
    'update_ms=2000' \
    'show_gpu_info=On' \
    'nvml_measure_pcie_speeds=False' \
    'note=btop shows physical GPU memory; use ramshared monitor for swap-tier attribution'
  exit 0
fi

config_dir=$(dirname -- "$CONFIG")
if [ "$MODE" = rollback ]; then
  case "$ROLLBACK" in
    "$CONFIG".ramshared-backup.*) ;;
    *) printf 'rollback path is not a backup for this config\n' >&2; exit 2 ;;
  esac
  if [ ! -f "$ROLLBACK" ] || [ -L "$ROLLBACK" ]; then
    printf 'rollback backup must be a regular non-symlink file\n' >&2
    exit 1
  fi
  rollback_tmp=$(mktemp "$config_dir/.btop.conf.ramshared-rollback.XXXXXX")
  trap 'rm -f -- "${rollback_tmp:-}"' EXIT
  cp -p -- "$ROLLBACK" "$rollback_tmp"
  mv -f -- "$rollback_tmp" "$CONFIG"
  trap - EXIT
  printf 'state=ROLLED_BACK\nconfig=%s\nbackup=%s\n' "$CONFIG" "$ROLLBACK"
  exit 0
fi

timestamp=$(date -u +%Y%m%dT%H%M%SZ)
backup="$CONFIG.ramshared-backup.$timestamp"
if [ -e "$backup" ]; then
  printf 'refusing to overwrite existing backup: %s\n' "$backup" >&2
  exit 1
fi
cp -p -- "$CONFIG" "$backup"

temporary=$(mktemp "$config_dir/.btop.conf.ramshared.XXXXXX")
trap 'rm -f -- "${temporary:-}"' EXIT
awk '
BEGIN { boxes=0; update=0; gpu_info=0; pcie=0 }
/^[[:space:]]*shown_boxes[[:space:]]*=/ {
  boxes=1
  if ($0 !~ /(^|[[:space:]])gpu0([[:space:]]|"|$)/) {
    sub(/[[:space:]]*"[[:space:]]*$/, " gpu0\"")
  }
}
/^[[:space:]]*update_ms[[:space:]]*=/ { $0="update_ms = 2000"; update=1 }
/^[[:space:]]*show_gpu_info[[:space:]]*=/ { $0="show_gpu_info = On"; gpu_info=1 }
/^[[:space:]]*nvml_measure_pcie_speeds[[:space:]]*=/ { $0="nvml_measure_pcie_speeds = False"; pcie=1 }
{ print }
END {
  if (!boxes) print "shown_boxes = \"cpu mem net proc gpu0\""
  if (!update) print "update_ms = 2000"
  if (!gpu_info) print "show_gpu_info = On"
  if (!pcie) print "nvml_measure_pcie_speeds = False"
}
' "$CONFIG" >"$temporary"
chmod --reference="$CONFIG" "$temporary"
mv -f -- "$temporary" "$CONFIG"
trap - EXIT

printf 'state=APPLIED\nconfig=%s\nbackup=%s\n' "$CONFIG" "$backup"
