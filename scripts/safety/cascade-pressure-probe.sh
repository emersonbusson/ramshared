#!/usr/bin/env bash
# cascade-pressure-probe.sh — prove swap order zram → VRAM/nbd → disk.
#
# Method: finite cgroup v2 limits contain one guest worker.
# In WSL2, guest allocations still consume shared Windows physical RAM; the
# campaign's Windows watchdog and host admission gates remain required.
# Direct invocation is refused; use the gated freeze campaign harness.
#
# Usage: scripts/safety/wsl2-freeze-campaign.sh --run-isolated or
#        scripts/safety/wsl2-freeze-campaign.sh --run-shared-daily-host
set -euo pipefail

MEM_MAX="${MEM_MAX:-1200M}"
ALLOC_GIB="${ALLOC_GIB:-6.5}"
MAX_SEC="${MAX_SEC:-90}"
PROVE_DISK=0
CG="${CG:-/sys/fs/cgroup/ramshared-probe-$$}"
INTEGRITY_RESULT="${INTEGRITY_RESULT:-/tmp/ramshared-integrity-result.json.$$}"
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source "$SCRIPT_DIR/guest-pressure-runtime-guard.sh"

CG_CREATED=0
MEMORY_CONTROLLER_ENABLED_BY_US=0
WORKER_STARTED=0
WORKER=""
START_GATE_DIR=""
GUARD_FAILURE=""
GUEST_MEM_AVAILABLE_KIB=""
GUEST_SWAP_FREE_KIB=""
GUEST_PSI_FULL_AVG10=""
MEM_MAX_BYTES=""

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

if [[ "${RAMSHARED_PRESSURE_PROBE_ADMITTED:-0}" != "1" ]]; then
  log "FAIL: run through wsl2-freeze-campaign.sh after its host and guest gates"
  exit 77
fi

guard_guest_pressure() {
  local phase=$1 sample reason
  if ! sample=$(ramshared_guest_pressure_read_sample /proc/meminfo /proc/pressure/memory); then
    GUARD_FAILURE="${phase}:guest_pressure_telemetry_invalid"
    return 1
  fi
  read -r GUEST_MEM_AVAILABLE_KIB GUEST_SWAP_FREE_KIB GUEST_PSI_FULL_AVG10 <<<"$sample"
  reason=$(ramshared_guest_pressure_guard_reason \
    "$GUEST_MEM_AVAILABLE_KIB" "$GUEST_SWAP_FREE_KIB" "$GUEST_PSI_FULL_AVG10")
  if [[ -n "$reason" ]]; then
    GUARD_FAILURE="${phase}:${reason}"
    return 1
  fi
  GUARD_FAILURE=""
  log "$phase guest MemAvailable=${GUEST_MEM_AVAILABLE_KIB}kB SwapFree=${GUEST_SWAP_FREE_KIB}kB PSI-full-avg10=${GUEST_PSI_FULL_AVG10}%"
}

read_cgroup_counter() {
  local path=$1 value
  value=$(cat -- "$path" 2>/dev/null) || return 1
  [[ "$value" =~ ^[0-9]+$ ]] || return 1
  ((${#value} <= 18)) || return 1
  printf '%s\n' "$((10#$value))"
}

refresh_pressure_cgroup_limits() {
  local phase=$1 current_memory_bytes current_swap_bytes memory_limit_bytes swap_limit_bytes
  if ! guard_guest_pressure "$phase"; then
    return 1
  fi
  current_memory_bytes=$(read_cgroup_counter "$CG/memory.current") || {
    GUARD_FAILURE="${phase}:guest_pressure_cgroup_memory_unavailable"
    return 1
  }
  current_swap_bytes=$(read_cgroup_counter "$CG/memory.swap.current") || {
    GUARD_FAILURE="${phase}:guest_pressure_cgroup_swap_unavailable"
    return 1
  }
  memory_limit_bytes=$(ramshared_guest_memory_limit_bytes \
    "$GUEST_MEM_AVAILABLE_KIB" "$current_memory_bytes" "$MEM_MAX_BYTES") || {
    GUARD_FAILURE="${phase}:guest_pressure_memory_limit_invalid"
    return 1
  }
  swap_limit_bytes=$(ramshared_guest_swap_limit_bytes \
    "$GUEST_SWAP_FREE_KIB" "$current_swap_bytes") || {
    GUARD_FAILURE="${phase}:guest_pressure_swap_limit_invalid"
    return 1
  }
  if ! printf '%s\n' "$memory_limit_bytes" >"$CG/memory.max"; then
    GUARD_FAILURE="${phase}:guest_pressure_memory_limit_write_failed"
    return 1
  fi
  if ! printf '%s\n' "$swap_limit_bytes" >"$CG/memory.swap.max"; then
    GUARD_FAILURE="${phase}:guest_pressure_swap_limit_write_failed"
    return 1
  fi
}

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

MEM_MAX_BYTES=$(ramshared_guest_pressure_parse_cgroup_bytes "$MEM_MAX") || {
  log "FAIL: invalid bounded --mem-max value: $MEM_MAX"
  exit 2
}
if ((MEM_MAX_BYTES == 0)); then
  log "FAIL: --mem-max must be greater than zero"
  exit 2
fi

need_root

if [[ ! -f /sys/fs/cgroup/cgroup.controllers ]]; then
  log "FAIL: cgroup v2 required"
  exit 69
fi
if ! grep -qw memory /sys/fs/cgroup/cgroup.controllers; then
  log "FAIL: memory controller not available"
  exit 69
fi
if [[ ! -f /proc/pressure/memory ]]; then
  log "FAIL: PSI interface not available"
  exit 69
fi

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

if ! guard_guest_pressure preflight; then
  log "FAIL: $GUARD_FAILURE"
  exit 1
fi
if ((GUEST_MEM_AVAILABLE_KIB <= 614400 || GUEST_SWAP_FREE_KIB <= 1048576)); then
  log "FAIL: guest has no allocatable headroom above the protected memory/swap reserves"
  exit 1
fi

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
  if ((CG_CREATED)) && [[ -f "$CG/cgroup.procs" ]]; then
    while read -r p; do
      [[ "$p" =~ ^[0-9]+$ ]] || continue
      kill -0 "$p" 2>/dev/null || continue
      if ! printf '%s\n' "$p" >/sys/fs/cgroup/cgroup.procs; then
        log "FAIL: could not release cgroup process $p"
        rc=1
      fi
    done <"$CG/cgroup.procs"
  fi
  if ((CG_CREATED)); then
    if rmdir -- "$CG"; then
      CG_CREATED=0
    else
      log "FAIL: could not remove owned cgroup $CG; preserving it for inspection"
      rc=1
    fi
  fi
  if ((MEMORY_CONTROLLER_ENABLED_BY_US)); then
    if printf '%s\n' '-memory' >/sys/fs/cgroup/cgroup.subtree_control; then
      MEMORY_CONTROLLER_ENABLED_BY_US=0
    else
      log "FAIL: memory controller remains enabled because the parent cgroup is no longer safe to change"
      rc=1
    fi
  fi
  log "final used_kb: $(read_used)"
  swapon --show || true
  if [[ -n "$GUARD_FAILURE" ]]; then
    log "FAIL: $GUARD_FAILURE"
    rc=1
  fi
  if [[ -n "$START_GATE_DIR" ]]; then
    rm -f -- "$START_GATE_DIR/start"
    if ! rmdir -- "$START_GATE_DIR"; then
      log "FAIL: could not remove owned worker start gate $START_GATE_DIR"
      rc=1
    fi
    START_GATE_DIR=""
  fi
  if ((WORKER_STARTED)) && [[ "$worker_rc" -ne 0 ]]; then
    log "FAIL: integrity worker exit=$worker_rc"
    rc=1
  elif ((WORKER_STARTED)) && [[ ! -s "$INTEGRITY_RESULT" ]]; then
    log "FAIL: integrity_result_missing path=$INTEGRITY_RESULT"
    rc=1
  elif ((WORKER_STARTED)) && ! python3 - "$INTEGRITY_RESULT" <<'PY'
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
  elif ((WORKER_STARTED)); then
    log "PASS: integrity result=$INTEGRITY_RESULT"
  fi
  trap - EXIT
  exit "$rc"
}
trap cleanup EXIT

if ! grep -qw memory /sys/fs/cgroup/cgroup.subtree_control; then
  if [[ ! -w /sys/fs/cgroup/cgroup.subtree_control ]] || \
    ! printf '+memory\n' >/sys/fs/cgroup/cgroup.subtree_control; then
    log "FAIL: could not enable the cgroup v2 memory controller"
    exit 69
  fi
  MEMORY_CONTROLLER_ENABLED_BY_US=1
fi
if [[ -e "$CG" ]]; then
  log "FAIL: cgroup path already exists; refusing to reuse or modify it: $CG"
  exit 1
fi
if ! mkdir -- "$CG"; then
  log "FAIL: could not create unique cgroup: $CG"
  exit 1
fi
CG_CREATED=1
for cgroup_file in memory.max memory.current memory.swap.max memory.swap.current cgroup.procs; do
  if [[ ! -e "$CG/$cgroup_file" ]]; then
    log "FAIL: required cgroup v2 file is unavailable: $CG/$cgroup_file"
    exit 69
  fi
done
for cgroup_file in memory.max memory.swap.max; do
  if [[ ! -w "$CG/$cgroup_file" ]]; then
    log "FAIL: required cgroup v2 limit is not writable: $CG/$cgroup_file"
    exit 69
  fi
done
initial_memory_current=$(read_cgroup_counter "$CG/memory.current") || {
  log "FAIL: could not read initial cgroup memory usage"
  exit 1
}
initial_swap_current=$(read_cgroup_counter "$CG/memory.swap.current") || {
  log "FAIL: could not read initial cgroup swap usage"
  exit 1
}
INITIAL_MEMORY_LIMIT_BYTES=$(ramshared_guest_memory_limit_bytes \
  "$GUEST_MEM_AVAILABLE_KIB" "$initial_memory_current" "$MEM_MAX_BYTES") || {
  log "FAIL: guest memory headroom cannot admit a bounded worker"
  exit 1
}
INITIAL_SWAP_LIMIT_BYTES=$(ramshared_guest_swap_limit_bytes \
  "$GUEST_SWAP_FREE_KIB" "$initial_swap_current") || {
  log "FAIL: guest swap headroom cannot admit a bounded worker"
  exit 1
}
if ((INITIAL_MEMORY_LIMIT_BYTES == 0 || INITIAL_SWAP_LIMIT_BYTES == 0)); then
  log "FAIL: no positive guest memory/swap budget remains above the protected reserves"
  exit 1
fi
if ! printf '%s\n' "$INITIAL_MEMORY_LIMIT_BYTES" >"$CG/memory.max"; then
  log "FAIL: could not apply the bounded guest memory limit"
  exit 1
fi
if ! printf '%s\n' "$INITIAL_SWAP_LIMIT_BYTES" >"$CG/memory.swap.max"; then
  log "FAIL: could not apply the protected guest swap limit"
  exit 1
fi
log "cgroup limits: memory.max=$INITIAL_MEMORY_LIMIT_BYTES memory.swap.max=$INITIAL_SWAP_LIMIT_BYTES"

rm -f -- "$INTEGRITY_RESULT"
START_GATE_DIR=$(mktemp -d "${TMPDIR:-/tmp}/ramshared-pressure-probe.XXXXXX")
chmod 700 "$START_GATE_DIR"
mkfifo "$START_GATE_DIR/start"
bash -c 'IFS= read -r -N 1 _ < "$1"; exec python3 "$2" --allocate-gib "$3" --result "$4"' \
  ramshared-pressure-worker \
  "$START_GATE_DIR/start" \
  "$SCRIPT_DIR/cascade_pressure_integrity_worker.py" \
  "$ALLOC_GIB" \
  "$INTEGRITY_RESULT" &
WORKER=$!
WORKER_STARTED=1
if ! printf '%s\n' "$WORKER" >"$CG/cgroup.procs"; then
  log "FAIL: could not attach integrity worker to bounded cgroup"
  exit 1
fi
if ! printf 'x' >"$START_GATE_DIR/start"; then
  GUARD_FAILURE="worker_start_gate_release_failed"
  log "FAIL: $GUARD_FAILURE"
  exit 1
fi
rm -f -- "$START_GATE_DIR/start"
if ! rmdir -- "$START_GATE_DIR"; then
  GUARD_FAILURE="worker_start_gate_cleanup_failed"
  log "FAIL: $GUARD_FAILURE"
  exit 1
fi
START_GATE_DIR=""
log "worker=$WORKER mem.max=$INITIAL_MEMORY_LIMIT_BYTES alloc_gib=$ALLOC_GIB"

first_z=""
first_n=""
first_d=""
TH=8192
DISK_TH=$((UD0 + 400))
t=0
while kill -0 "$WORKER" 2>/dev/null && (( t < MAX_SEC )); do
  sleep 1
  t=$((t + 1))
  if ! refresh_pressure_cgroup_limits runtime; then
    log "FAIL: $GUARD_FAILURE"
    exit 1
  fi
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
