#!/usr/bin/env bash
# measure-swap-compare.sh — Q1b/Q1d do benchmark decisivo.
# Roda fio 4K (randread+randwrite, QD1 e QD8) BOUNDED contra um alvo (arquivo no NVMe
# ou block device do VRAM-swap), pra comparar o VRAM-swap com um NVMe-swap CONTIDO.
# Bounded/não-disruptivo: arquivo de teste pequeno, tempo curto, apagado no fim.
#
# uso: measure-swap-compare.sh <alvo> [rótulo] [size] [runtime_s]
#   <alvo> = caminho de arquivo (NVMe) ou /dev/nbdX (VRAM-swap). Se for arquivo, é criado/apagado.
set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "Error: alvo (arquivo ou block device) requerido" >&2
    exit 64
fi

TARGET="$1"
LABEL="${2:-$TARGET}"
SIZE="${3:-256M}"
RT="${4:-12}"

command -v fio >/dev/null || { echo "fio ausente" >&2; exit 69; }
command -v swapon >/dev/null || { echo "swapon ausente" >&2; exit 69; }

# Guard check that both baseline and comparison swap devices are configured and active.
if [ "$(swapon --show --noheadings | wc -l)" -lt 2 ]; then
    echo "Error: Both baseline and comparison swap devices must be configured and active." >&2
    exit 78
fi

# Sanity Checks
if [ ! -b "$TARGET" ]; then
    target_dir="$(dirname "$TARGET")"
    if [ ! -d "$target_dir" ]; then
        echo "Error: Directory for target file does not exist: $target_dir" >&2
        exit 74
    fi
fi

if ! [[ "$RT" =~ ^[0-9]+$ ]]; then
    echo "Error: Runtime must be numeric: $RT" >&2
    exit 64
fi

if [ "$RT" -eq 0 ] || [ "$RT" -gt 86400 ]; then
    echo "Error: Runtime out of physical bounds (1-86400): $RT" >&2
    exit 64
fi

if ! [[ "$SIZE" =~ ^[0-9]+[KMGkmg]?$ ]]; then
    echo "Error: Invalid size format: $SIZE" >&2
    exit 64
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
