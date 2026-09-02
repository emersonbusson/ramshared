#!/usr/bin/env bash
# P0 ITEM-1 — system PSI UNDER MEMORY LOAD, using an ANONYMOUS hog confined to cgroup v2.
# REAL and BOUNDED pressure: `memory.max` is the safety ceiling and `memory.swap.max=0` prevents
# any swap-out — NO swap on a device / daemon / block device (away from the 2026-06-09 freeze
# scenario). Replaces the SPEC's "cargo build -j4": P0 found that a build is CPU-bound and does
# NOT generate memory PSI (see P0-RESULTS §1, "load" cell).
# Usage: measure-psi-load.sh [DUR_s] [OUT_csv] [WS_MB] [HIGH_MB] [MAX_MB] [WORKERS]  (root)
# SPEC: docs/memory-broker/SPECv2.md ITEM-1; calibrates delta_psi (P0-RESULTS §5). Reusable for civm.
set -euo pipefail

DUR="${1:-40}"
OUT="${2:-psi-load-$(date +%Y%m%d-%H%M%S).csv}"
WS_MB="${3:-300}"   # anonymous hog working set
HIGH_MB="${4:-64}"  # memory.high: throttle (stall/PSI) well below WS
MAX_MB="${5:-512}"  # memory.max: safety CEILING (bounded — no OOM while WS < MAX)
WORKERS="${6:-1}"   # load generation workers
CG=/sys/fs/cgroup/p0load
LOG_PREFIX="[p0-load]"
log() { echo "$LOG_PREFIX $*" >&2; }

[ "$(id -u)" -eq 0 ]                                || { log "ERROR: root is required (cgroup write)"; exit 1; }
[ "$(stat -fc %T /sys/fs/cgroup)" = cgroup2fs ]     || { log "ERROR: cgroup v2 missing"; exit 1; }
grep -qw memory /sys/fs/cgroup/cgroup.subtree_control || { log "ERROR: memory controller not delegated"; exit 1; }
command -v python3 >/dev/null                       || { log "ERROR: python3 missing"; exit 1; }
command -v stress-ng >/dev/null                     || { log "ERROR: stress-ng missing"; exit 69; }

CORES="$(nproc 2>/dev/null || echo 1)"
if [ "$WORKERS" -gt "$CORES" ]; then
	log "WARNING: clamping worker count ($WORKERS) to available CPU cores ($CORES)"
	WORKERS="$CORES"
fi

HOG=""
cleanup() {
	[ -n "$HOG" ] && kill "$HOG" 2>/dev/null || true
	sleep 1
	if [ -d "$CG" ]; then
		xargs -r kill -9 < "$CG/cgroup.procs" 2>/dev/null || true
		sleep 1
		rmdir "$CG" 2>/dev/null && log "cgroup clean" || log "WARNING: cgroup $CG remains (inspect)"
	fi
}
trap cleanup EXIT

mkdir -p "$CG"
echo "${HIGH_MB}M" > "$CG/memory.high"
echo "${MAX_MB}M"  > "$CG/memory.max"
echo 0             > "$CG/memory.swap.max" 2>/dev/null || true
log "cgroup p0load: high=${HIGH_MB}M max=${MAX_MB}M swap=0; hog WS=${WS_MB}M for ${DUR}s"

# Anonymous hog INSIDE the cgroup; the memory.max ceiling guarantees bounded operation (no freeze).
(
	echo $BASHPID > "$CG/cgroup.procs"
	exec timeout $((DUR + 8)) python3 -c "
import time
n=${WS_MB}*1024*1024
a=bytearray(n)
end=time.time()+${DUR}+3
while time.time()<end:
    for i in range(0,n,4096): a[i]=(a[i]+1)&255
"
) &
HOG=$!

sleep 2  # lets the hog fill and the throttle stabilize
log "sampling /proc/pressure/memory (system) for ${DUR}s -> $OUT"
echo "ts,kind,avg10,avg60,avg300,total_us" > "$OUT"
end=$(( $(date +%s) + DUR ))
while [ "$(date +%s)" -lt "$end" ]; do
	now=$(date +%s)
	while read -r kind a10 a60 a300 total; do
		[ -n "$kind" ] || continue
		printf '%s,%s,%s,%s,%s,%s\n' "$now" "$kind" \
			"${a10#avg10=}" "${a60#avg60=}" "${a300#avg300=}" "${total#total=}" >> "$OUT"
	done < /proc/pressure/memory
	sleep 1
done

log "cgroup memory.current=$(cat "$CG/memory.current" 2>/dev/null)"
log "cgroup memory.pressure: $(tr '\n' ' ' < "$CG/memory.pressure" 2>/dev/null)"
log "cgroup memory.events: $(tr '\n' ' ' < "$CG/memory.events" 2>/dev/null)"
log "end: $(( $(wc -l < "$OUT") - 1 )) samples in $OUT"
