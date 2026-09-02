#!/usr/bin/env bash
# measure-vram-headroom.sh — Q1a of the decisive benchmark (RamShared value gate).
# Samples, under the CURRENT LOAD (machine in use), how much VRAM/RAM is actually idle and
# how STABLE it is — because "harvesting idle VRAM" is only useful if there is stable, idle VRAM.
# READ-ONLY: reads telemetry only (nvidia-smi / free / /proc/diskstats). Allocates nothing.
#
# usage: measure-vram-headroom.sh [seconds] [interval]
# output: CSV on stdout plus a summary (free min/max/mean plus volatility).
set -euo pipefail

# sysexits.h codes
readonly EX_USAGE=64
readonly EX_UNAVAILABLE=69

DUR="${1:-30}"
STEP="${2:-2}"

if ! [[ "$DUR" =~ ^[0-9]+$ ]] || ! [[ "$STEP" =~ ^[0-9]+$ ]]; then
  echo "Error: Duration and step must be positive integers." >&2
  exit "$EX_USAGE"
fi

if (( DUR <= 0 || STEP <= 0 || STEP > DUR )); then
  echo "Error: Invalid duration or step values." >&2
  exit "$EX_USAGE"
fi

if (( DUR > 86400 )); then
  echo "Error: Duration exceeds maximum allowed (86400s)." >&2
  exit "$EX_USAGE"
fi

gpu_detected=0
if command -v nvidia-smi >/dev/null 2>&1 && nvidia-smi -L >/dev/null 2>&1; then
  gpu_detected=1
elif command -v vulkaninfo >/dev/null 2>&1 && vulkaninfo >/dev/null 2>&1; then
  gpu_detected=1
fi

if (( gpu_detected == 0 )); then
  echo "Error: Neither nvidia-smi nor vulkaninfo detected a working GPU." >&2
  exit "$EX_UNAVAILABLE"
fi

N=$(( DUR / STEP ))

echo "ts_s,vram_free_mib,vram_used_mib,ram_avail_mib,ram_free_mib,swap_used_mib"
free_vram_samples=()
t=0
for _ in $(seq 1 "$N"); do
  # VRAM (MiB) through nvidia-smi (GPU-PV in WSL2)
  read -r vfree vused < <(nvidia-smi --query-gpu=memory.free,memory.used --format=csv,noheader,nounits 2>/dev/null | tr -d ',' | awk '{print $1, $2}')
  # RAM/swap (MiB) through free
  read -r ravail rfree swused < <(free -m | awk '/^Mem:/{a=$7; f=$4} /^Swap:/{s=$3} END{print a, f, s}')
  echo "${t},${vfree:-NA},${vused:-NA},${ravail:-NA},${rfree:-NA},${swused:-NA}"
  [ -n "${vfree:-}" ] && free_vram_samples+=("$vfree")
  t=$(( t + STEP ))
  sleep "$STEP"
done

# Free-VRAM summary (the number that decides the "harvest" angle)
printf '%s\n' "${free_vram_samples[@]}" | awk '
  NR==1{min=$1; max=$1}
  {sum+=$1; if($1<min)min=$1; if($1>max)max=$1; v[NR]=$1; n=NR}
  END{
    mean=sum/n;
    for(i=1;i<=n;i++){d=v[i]-mean; ss+=d*d}
    sd=(n>1)?sqrt(ss/(n-1)):0;
    printf "\n# Free VRAM (MiB) under current load: n=%d  min=%d  max=%d  mean=%.0f  stddev=%.0f  (range=%d)\n", n, min, max, mean, sd, max-min;
    printf "# volatility = range/mean = %.1f%%  -> the greater it is, the less reliable idle-VRAM harvesting becomes\n", (max-min)*100.0/mean;
  }'
