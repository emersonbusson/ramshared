#!/usr/bin/env bash
# cascade-health.sh — amostra de saude da cascata WSL2 (append-only JSONL).
#
# Uso:
#   ./scripts/safety/cascade-health.sh              # 1 amostra → stdout
#   ./scripts/safety/cascade-health.sh --once        # idem
#   ./scripts/safety/cascade-health.sh --loop         # a cada INTERVAL_S (default 30)
#   ./scripts/safety/cascade-health.sh --loop --out /var/log/ramshared/cascade-health.jsonl
#
# Host-safety: so le /proc, nvidia-smi e pgrep. Nao aloca, nao thrash, nao swapoff.
# Kahneman #3: numero + unidade + timestamp. #1: estado completo da cascata, nao so "ok".
#
# Auto-melhoria (dev): o JSONL alimenta comparacao entre commits e deteccao de
# regressao (ghost, order, daemon down, used por tier). NAO altera thresholds
# sozinho — mudanca de politica de demote/re-promote exige SPEC/PRD.
set -u

INTERVAL_S="${INTERVAL_S:-30}"
OUT=""
MODE=once
MAX_BYTES="${MAX_BYTES:-$((50 * 1024 * 1024))}"  # rotaciona .1 se estourar

usage() {
  sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --once) MODE=once; shift ;;
    --loop) MODE=loop; shift ;;
    --out) OUT="${2:-}"; shift 2 ;;
    --interval) INTERVAL_S="${2:-30}"; shift 2 ;;
    -h|--help) usage ;;
    *) echo "arg desconhecido: $1" >&2; exit 2 ;;
  esac
done

json_escape() {
  # minimal JSON string escape
  printf '%s' "$1" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()), end="")' 2>/dev/null \
    || printf '"%s"' "$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g')"
}

sample() {
  local ts now_epoch
  ts=$(date -Is)
  now_epoch=$(date +%s)

  local daemon_alive=0 daemon_pid="" daemon_cmd=""
  if daemon_pid=$(pgrep -n -f '/ramsharedd( |$)' 2>/dev/null); then
    daemon_alive=1
    daemon_cmd=$(tr '\0' ' ' <"/proc/${daemon_pid}/cmdline" 2>/dev/null || true)
  fi

  # /proc/swaps → array JSON
  local swaps_json="[" first=1
  local ghost=0 order_ok=1
  local has_zram=0 has_vram=0 has_vhdx=0
  local p_zram="" p_vram="" p_vhdx=""
  local nbd_used=0 zram_used=0 vhdx_used=0

  while read -r name _type size used prio; do
    [ "$name" = "Filename" ] && continue
    [ -z "${name:-}" ] && continue
    local base
    base=$(basename "$name")
    # ghost: (deleted) in name or missing block after nbd/ublk path
    if printf '%s' "$name" | grep -q '(deleted)'; then
      ghost=1
    fi
    if [ "$first" = 1 ]; then first=0; else swaps_json+=","; fi
    swaps_json+=$(printf '{"name":%s,"size_kib":%s,"used_kib":%s,"prio":%s}' \
      "$(json_escape "$name")" "${size:-0}" "${used:-0}" "${prio:-0}")

    case "$name" in
      *zram*)
        has_zram=1; p_zram=$prio; zram_used=$used
        ;;
      *nbd*|*ublk*|*ublkb*)
        has_vram=1; p_vram=$prio; nbd_used=$used
        if [ ! -b "$name" ] && ! printf '%s' "$name" | grep -q '(deleted)'; then
          # path listed but node missing → orphan-ish
          ghost=1
        fi
        ;;
      /dev/sd*|/dev/vd*|/dev/xvd*|/dev/nvme*)
        has_vhdx=1; p_vhdx=$prio; vhdx_used=$used
        ;;
    esac
  done < /proc/swaps
  swaps_json+="]"

  # order: zram prio > vram prio > vhdx prio (when present)
  if [ -n "$p_zram" ] && [ -n "$p_vram" ] && [ "$p_zram" -le "$p_vram" ] 2>/dev/null; then
    order_ok=0
  fi
  if [ -n "$p_vram" ] && [ -n "$p_vhdx" ] && [ "$p_vram" -le "$p_vhdx" ] 2>/dev/null; then
    order_ok=0
  fi

  local mem_avail=0 mem_total=0 swap_free=0 swap_total=0
  mem_avail=$(awk '/MemAvailable:/{print $2}' /proc/meminfo)
  mem_total=$(awk '/MemTotal:/{print $2}' /proc/meminfo)
  swap_total=$(awk '/SwapTotal:/{print $2}' /proc/meminfo)
  swap_free=$(awk '/SwapFree:/{print $2}' /proc/meminfo)

  local vram_free="" vram_used="" vram_total=""
  if command -v nvidia-smi >/dev/null 2>&1; then
    local line
    line=$(nvidia-smi --query-gpu=memory.total,memory.used,memory.free --format=csv,noheader,nounits 2>/dev/null | head -1)
    if [ -n "$line" ]; then
      vram_total=$(echo "$line" | awk -F', ' '{print $1}')
      vram_used=$(echo "$line" | awk -F', ' '{print $2}')
      vram_free=$(echo "$line" | awk -F', ' '{print $3}')
    fi
  fi

  local reasons=() ok=1
  if [ "$daemon_alive" != 1 ] && [ "$has_vram" = 1 ]; then
    ok=0; reasons+=("daemon_dead_while_vram_swap")
  fi
  if [ "$ghost" = 1 ]; then
    ok=0; reasons+=("ghost_or_orphan_swap")
  fi
  if [ "$order_ok" != 1 ]; then
    ok=0; reasons+=("priority_order_bad")
  fi
  # expected healthy cushion shape when cascade is intended up
  if [ "$has_vram" = 1 ] && [ "$has_vhdx" != 1 ]; then
    ok=0; reasons+=("a1_no_vhdx_sink")
  fi

  local reasons_json="["
  first=1
  for r in "${reasons[@]+"${reasons[@]}"}"; do
    [ -z "${r:-}" ] && continue
    if [ "$first" = 1 ]; then first=0; else reasons_json+=","; fi
    reasons_json+=$(json_escape "$r")
  done
  reasons_json+="]"

  local vram_json="null"
  if [ -n "$vram_free" ]; then
    vram_json=$(printf '{"total_mib":%s,"used_mib":%s,"free_mib":%s}' \
      "${vram_total:-0}" "${vram_used:-0}" "${vram_free:-0}")
  fi

  printf '{"ts":%s,"epoch":%s,"ok":%s,"reasons":%s,"daemon":{"alive":%s,"pid":%s,"cmd":%s},"swaps":%s,"flags":{"ghost":%s,"order_ok":%s,"has_zram":%s,"has_vram":%s,"has_vhdx":%s},"used_kib":{"zram":%s,"vram":%s,"vhdx":%s},"mem":{"total_kib":%s,"available_kib":%s,"swap_total_kib":%s,"swap_free_kib":%s},"gpu":%s}\n' \
    "$(json_escape "$ts")" \
    "$now_epoch" \
    "$([ "$ok" = 1 ] && echo true || echo false)" \
    "$reasons_json" \
    "$([ "$daemon_alive" = 1 ] && echo true || echo false)" \
    "${daemon_pid:-null}" \
    "$(json_escape "${daemon_cmd:-}")" \
    "$swaps_json" \
    "$([ "$ghost" = 1 ] && echo true || echo false)" \
    "$([ "$order_ok" = 1 ] && echo true || echo false)" \
    "$([ "$has_zram" = 1 ] && echo true || echo false)" \
    "$([ "$has_vram" = 1 ] && echo true || echo false)" \
    "$([ "$has_vhdx" = 1 ] && echo true || echo false)" \
    "${zram_used:-0}" \
    "${nbd_used:-0}" \
    "${vhdx_used:-0}" \
    "${mem_total:-0}" \
    "${mem_avail:-0}" \
    "${swap_total:-0}" \
    "${swap_free:-0}" \
    "$vram_json"
}

maybe_rotate() {
  local f="$1"
  [ -n "$f" ] || return 0
  [ -f "$f" ] || return 0
  local sz
  sz=$(wc -c <"$f" 2>/dev/null || echo 0)
  if [ "${sz:-0}" -gt "$MAX_BYTES" ]; then
    mv -f "$f" "${f}.1" 2>/dev/null || true
  fi
}

emit() {
  local line
  line=$(sample)
  if [ -n "$OUT" ]; then
    mkdir -p "$(dirname "$OUT")" 2>/dev/null || true
    maybe_rotate "$OUT"
    printf '%s\n' "$line" >>"$OUT"
  fi
  printf '%s\n' "$line"
}

if [ "$MODE" = once ]; then
  emit
  exit 0
fi

# loop mode: log start line to stderr for operators
echo "[cascade-health] loop interval=${INTERVAL_S}s out=${OUT:-stdout}" >&2
while true; do
  emit || true
  sleep "$INTERVAL_S"
done
