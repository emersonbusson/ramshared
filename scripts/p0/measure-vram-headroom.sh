#!/usr/bin/env bash
# measure-vram-headroom.sh — Q1a do benchmark decisivo (gate de valor do RamShared).
# Samples, under the CURRENT LOAD (machine in use), how much VRAM/RAM is actually idle and
# how STABLE it is — because "harvesting idle VRAM" is only useful if there is stable, idle VRAM.
# READ-ONLY: só lê telemetria (nvidia-smi / free / /proc/diskstats). Não aloca nada.
#
# uso: measure-vram-headroom.sh [segundos] [intervalo]
# saída: CSV em stdout + resumo (min/máx/média do livre + volatilidade).
set -euo pipefail

DUR="${1:-30}"
STEP="${2:-2}"
N=$(( DUR / STEP ))

echo "ts_s,vram_free_mib,vram_used_mib,ram_avail_mib,ram_free_mib,swap_used_mib"
free_vram_samples=()
t=0
for _ in $(seq 1 "$N"); do
  # VRAM (MiB) via nvidia-smi (GPU-PV no WSL2)
  read -r vfree vused < <(nvidia-smi --query-gpu=memory.free,memory.used --format=csv,noheader,nounits 2>/dev/null | tr -d ',' | awk '{print $1, $2}')
  # RAM/swap (MiB) via free
  read -r ravail rfree swused < <(free -m | awk '/^Mem:/{a=$7; f=$4} /^Swap:/{s=$3} END{print a, f, s}')
  echo "${t},${vfree:-NA},${vused:-NA},${ravail:-NA},${rfree:-NA},${swused:-NA}"
  [ -n "${vfree:-}" ] && free_vram_samples+=("$vfree")
  t=$(( t + STEP ))
  sleep "$STEP"
done

# Resumo da VRAM livre (o número que decide o ângulo "harvest")
printf '%s\n' "${free_vram_samples[@]}" | awk '
  NR==1{min=$1; max=$1}
  {sum+=$1; if($1<min)min=$1; if($1>max)max=$1; v[NR]=$1; n=NR}
  END{
    mean=sum/n;
    for(i=1;i<=n;i++){d=v[i]-mean; ss+=d*d}
    sd=(n>1)?sqrt(ss/(n-1)):0;
    printf "\n# VRAM livre (MiB) sob carga atual: n=%d  min=%d  max=%d  média=%.0f  desvio=%.0f  (amplitude=%d)\n", n, min, max, mean, sd, max-min;
    printf "# volatilidade = amplitude/média = %.1f%%  -> quanto maior, menos confiável colher VRAM ociosa\n", (max-min)*100.0/mean;
  }'
