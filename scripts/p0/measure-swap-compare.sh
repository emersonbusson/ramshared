#!/usr/bin/env bash
# measure-swap-compare.sh — Q1b/Q1d do benchmark decisivo.
# Roda fio 4K (randread+randwrite, QD1 e QD8) BOUNDED contra um alvo (arquivo no NVMe
# ou block device do VRAM-swap), pra comparar o VRAM-swap com um NVMe-swap CONTIDO.
# Bounded/não-disruptivo: arquivo de teste pequeno, tempo curto, apagado no fim.
#
# uso: measure-swap-compare.sh <alvo> [rótulo] [size] [runtime_s]
#   <alvo> = caminho de arquivo (NVMe) ou /dev/nbdX (VRAM-swap). Se for arquivo, é criado/apagado.
set -euo pipefail

TARGET="${1:?alvo (arquivo ou block device) requerido}"
LABEL="${2:-$TARGET}"
SIZE="${3:-256M}"
RT="${4:-12}"

# Guard check: fio exists
command -v fio >/dev/null || { echo "Error: fio ausente" >&2; exit 69; }

# Guard check: Validate inputs
if [[ ! "$RT" =~ ^[0-9]+$ ]] || [[ "$RT" -eq 0 ]]; then
  echo "Error: runtime_s ($RT) must be a positive integer." >&2
  exit 64
fi

if [[ ! "$SIZE" =~ ^[0-9]+[bkmgBKMG]?$ ]]; then
  echo "Error: size ($SIZE) format invalid." >&2
  exit 64
fi

# Guard check: both baseline and comparison swap devices must be configured and active.
# We decode \040 to space for path validation.
if ! sed 's/\\040/ /g' /proc/swaps | awk '
  NR > 1 {
    if ($1 ~ /^\/dev\/(nbd|ublk|zram)/) comp = 1
    else base = 1
  }
  END { exit (comp && base ? 0 : 1) }
'; then
  echo "Error: Both baseline and comparison swap devices must be configured and active in /proc/swaps." >&2
  exit 78
fi


IS_FILE=0
[ -b "$TARGET" ] || IS_FILE=1
[ "$IS_FILE" = 1 ] && trap 'rm -f "$TARGET"' EXIT

run() { # rw qd
  local rw="$1" qd="$2"
  fio --name="${LABEL}-${rw}-qd${qd}" --filename="$TARGET" --rw="$rw" --bs=4k \
      --direct=1 --ioengine=libaio --iodepth="$qd" --size="$SIZE" \
      --runtime="$RT" --time_based --ramp_time=2 --group_reporting --output-format=normal 2>&1 \
    | grep -E "IOPS=|clat \(|^\s*lat \(|50.00th|99.00th|99.99th" \
    | sed "s/^/[${rw} qd${qd}] /"
}

echo "===== measure-swap-compare: ${LABEL} (size=${SIZE}, ${RT}s, direct=1) ====="
echo "--- alvo: $TARGET (arquivo=${IS_FILE}) ---"
run randread 1
run randwrite 1
run randread 8
run randwrite 8
echo "===== fim ${LABEL} ====="
