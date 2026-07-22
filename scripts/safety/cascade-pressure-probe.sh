#!/usr/bin/env bash
# cascade-pressure-probe.sh — prove swap order zram → VRAM/nbd → disk.
#
# Method: cgroup v2 MemoryMax on a worker only (host stays mostly free).
# Host safety: no full-VM thrash; hard time cap; releases on exit.
#
# Usage:
#   sudo bash scripts/safety/cascade-pressure-probe.sh
#   sudo bash scripts/safety/cascade-pressure-probe.sh --prove-disk
set -euo pipefail

MEM_MAX="${MEM_MAX:-1200M}"
ALLOC_GIB="${ALLOC_GIB:-6.5}"
MAX_SEC="${MAX_SEC:-90}"
PROVE_DISK=0
CG="${CG:-/sys/fs/cgroup/ramshared-probe}"
INTEGRITY_RESULT="${INTEGRITY_RESULT:-/tmp/ramshared-integrity-result.json.$$}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mem-max) MEM_MAX="$2"; shift 2 ;;
    --alloc-gib) ALLOC_GIB="$2"; shift 2 ;;
    --max-sec) MAX_SEC="$2"; shift 2 ;;
    --integrity-result) INTEGRITY_RESULT="$2"; shift 2 ;;
    --prove-disk) PROVE_DISK=1; shift ;;
    -h|--help) sed -n '1,16p' "$0"; exit 0 ;;
    *) echo "unknown: $1" >&2; exit 2 ;;
  esac
done

log() { echo "[pressure] $*"; }

need_root() {
  if [[ "$(id -u)" -ne 0 ]]; then
    log "FAIL: run as root (cgroup + accurate swaps)"
    exit 1
  fi
}

read_used() {
  python3 - <<'PY'
z=n=d=0
with open("/proc/swaps") as f:
    next(f, None)
    for line in f:
        c = line.split()
        if len(c) < 5:
            continue
        name, used = c[0], int(c[3])
        low = name.lower()
        if "zram" in low:
            z += used
        elif "nbd" in low or "ublk" in low:
            n += used
        else:
            d += used
print(f"{z} {n} {d}")
PY
}

read_prios() {
  python3 - <<'PY'
z = n = d = None
with open("/proc/swaps") as f:
    next(f, None)
    for line in f:
        c = line.split()
        if len(c) < 5:
            continue
        name, prio = c[0], int(c[4])
        low = name.lower()
        if "zram" in low:
            z = prio if z is None else max(z, prio)
        elif "nbd" in low or "ublk" in low:
            n = prio if n is None else max(n, prio)
        else:
            d = prio if d is None else min(d, prio)
# Always integers (-1 if missing) so bash set -u arithmetic is safe.
print(f"{z if z is not None else -1} {n if n is not None else -1} {d if d is not None else -1}")
PY
}

need_root
read -r PZ PN PD <<<"$(read_prios)"
if [[ -z "${PZ:-}" || -z "${PN:-}" || -z "${PD:-}" || "$PZ" -lt 0 || "$PN" -lt 0 || "$PD" -eq -1 ]]; then
  log "FAIL: need live zram + nbd + disk (sudo ramshared up first) prios=z:$PZ n:$PN d:$PD"
  swapon --show || true
  exit 1
fi
if ! (( PZ > PN && PN > PD )); then
  log "FAIL: priority not zram($PZ) > nbd($PN) > disk($PD)"
  exit 1
fi
log "baseline prios ok: zram=$PZ nbd=$PN disk=$PD"
read -r UZ0 UN0 UD0 <<<"$(read_used)"
log "baseline used_kb: zram=$UZ0 nbd=$UN0 disk=$UD0"

if [[ ! -d /sys/fs/cgroup ]]; then
  log "FAIL: cgroup v2 required"
  exit 1
fi
mkdir -p "$CG"
# enable memory controller in parent if possible
if [[ -w /sys/fs/cgroup/cgroup.subtree_control ]]; then
  echo '+memory' > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true
fi
echo "$MEM_MAX" > "$CG/memory.max"
if [[ -f "$CG/memory.swap.max" ]]; then
  echo max > "$CG/memory.swap.max" 2>/dev/null || true
fi

WORKER=""
cleanup() {
  local rc=$?
  local worker_rc=0
  if [[ -n "$WORKER" ]] && kill -0 "$WORKER" 2>/dev/null; then
    log "releasing worker $WORKER"
    kill -TERM "$WORKER" 2>/dev/null || true
    wait "$WORKER" 2>/dev/null || worker_rc=$?
  elif [[ -n "$WORKER" ]]; then
    wait "$WORKER" 2>/dev/null || worker_rc=$?
  fi
  # empty cgroup
  if [[ -f "$CG/cgroup.procs" ]]; then
    while read -r p; do
      [[ -n "$p" ]] && echo "$p" > /sys/fs/cgroup/cgroup.procs 2>/dev/null || true
    done < "$CG/cgroup.procs" 2>/dev/null || true
  fi
  log "final used_kb: $(read_used)"
  swapon --show || true
  if [[ "$worker_rc" -ne 0 ]]; then
    log "FAIL: integrity worker exit=$worker_rc"
    rc=1
  elif [[ ! -s "$INTEGRITY_RESULT" ]]; then
    log "FAIL: integrity_result_missing path=$INTEGRITY_RESULT"
    rc=1
  elif ! python3 - "$INTEGRITY_RESULT" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    result = json.load(source)
if result.get("status") != "PASS":
    raise SystemExit(1)
if result.get("checksum_before") != result.get("checksum_after"):
    raise SystemExit(1)
PY
  then
    log "FAIL: integrity_result_failed path=$INTEGRITY_RESULT"
    rc=1
  else
    log "PASS: integrity result=$INTEGRITY_RESULT"
  fi
  trap - EXIT
  exit "$rc"
}
trap cleanup EXIT

rm -f -- "$INTEGRITY_RESULT"
python3 "$(dirname "$0")/cascade_pressure_integrity_worker.py" \
  --allocate-gib "$ALLOC_GIB" \
  --result "$INTEGRITY_RESULT" &
WORKER=$!
echo "$WORKER" > "$CG/cgroup.procs"
log "worker=$WORKER mem.max=$MEM_MAX alloc_gib=$ALLOC_GIB"

first_z=""
first_n=""
first_d=""
TH=8192
DISK_TH=$((UD0 + 400))
t=0
while kill -0 "$WORKER" 2>/dev/null && (( t < MAX_SEC )); do
  sleep 1
  t=$((t + 1))
  read -r z n d <<<"$(read_used)"
  if [[ -z "$first_z" ]] && (( z > UZ0 + TH )); then
    first_z=$t
    log "FIRST USE zram t=${t}s used_kb=$z"
  fi
  if [[ -z "$first_n" ]] && (( n > UN0 + TH )); then
    first_n=$t
    log "FIRST USE nbd/VRAM t=${t}s used_kb=$n"
  fi
  if [[ -z "$first_d" ]] && (( d > DISK_TH )); then
    first_d=$t
    log "FIRST USE disk/SSD t=${t}s used_kb=$d"
  fi
  need_disk=0
  (( PROVE_DISK )) && need_disk=1
  if [[ -n "$first_z" && -n "$first_n" ]] && { (( !need_disk )) || [[ -n "$first_d" ]]; }; then
    if (( first_n < first_z )); then
      log "FAIL: nbd before zram (n=$first_n z=$first_z)"
      exit 1
    fi
    if [[ -n "$first_d" ]]; then
      if (( first_d < first_n )); then
        log "FAIL: disk before nbd (d=$first_d n=$first_n)"
        exit 1
      fi
      if (( first_d < first_z )); then
        log "FAIL: disk before zram"
        exit 1
      fi
    fi
    log "PASS order zram_first=${first_z}s nbd_first=${first_n}s disk_first=${first_d:-none}"
    exit 0
  fi
done

log "partial z=${first_z:-none} n=${first_n:-none} d=${first_d:-none}"
if [[ -n "$first_z" && -n "$first_n" ]] && (( first_n >= first_z )); then
  if (( PROVE_DISK )) && [[ -z "$first_d" ]]; then
    log "INCOMPLETE: disk not reached (raise --alloc-gib or lower --mem-max)"
    exit 2
  fi
  log "PASS (zram before nbd)"
  exit 0
fi
log "FAIL: did not observe expected tier growth"
exit 1
