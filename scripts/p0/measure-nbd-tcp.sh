#!/usr/bin/env bash
# P0 ITEM-1 — p50/p99 of RAW NBD/TCP on the virt-switch, with NO project code (honest baseline).
# nbdkit memory 1G  +  nbd-client (TCP)  +  fio (randread/randwrite 4k, lat_percentiles).
# Usage: measure-nbd-tcp.sh [HOST] [PORT] [ROUNDS]   (run as root)
# SPEC: docs/memory-broker/SPECv2.md ITEM-1; R4 (virt-switch latency). NOTHING enters the product.
# Discipline #3: compare with p50 241µs (ublk) / 326µs (NBD-Unix) from Phase B.
set -euo pipefail

HOST="${1:-127.0.0.1}"
PORT="${2:-10810}"
ROUNDS="${3:-3}"
DEV=/dev/nbd0
LOG_PREFIX="[p0-nbdtcp]"
log() { echo "$LOG_PREFIX $*" >&2; }

# --- Dependency preflight (F17: the measurement host may lack nbdkit/nbd-server) ---
SERVER=""
command -v nbdkit     >/dev/null && SERVER=nbdkit
[ -z "$SERVER" ] && command -v nbd-server >/dev/null && SERVER=nbd-server
[ -n "$SERVER" ]                  || { log "ERROR: install the server: sudo apt install nbdkit"; exit 1; }
command -v nbd-client >/dev/null  || { log "ERROR: sudo apt install nbd-client"; exit 1; }
command -v fio        >/dev/null  || { log "ERROR: sudo apt install fio"; exit 1; }
[ "$(id -u)" -eq 0 ]              || { log "ERROR: root is required (nbd-client/modprobe nbd)"; exit 1; }
[ "$SERVER" = nbdkit ]            || { log "ERROR: use nbdkit (nbd-server requires manual configuration)"; exit 1; }

modprobe nbd nbds_max=1 2>/dev/null || true

SRV_PID=""
cleanup() {
	nbd-client -d "$DEV" 2>/dev/null || true
	if [ -n "$SRV_PID" ]; then kill "$SRV_PID" 2>/dev/null || true; fi
}
trap cleanup EXIT

log "starting nbdkit memory 1G on $HOST:$PORT"
nbdkit --foreground --port "$PORT" --ipaddr "$HOST" memory 1G & SRV_PID=$!
sleep 1

# --- TCP & Network Reachability Preflight ---
if ! nc -z -w 2 "$HOST" "$PORT" >/dev/null 2>&1; then
	log "ERROR: NBD server at $HOST:$PORT is not reachable via TCP (nc -z failed)"
	exit 69 # EX_UNAVAILABLE
fi

IFACE=$(ip route get "$HOST" 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="dev") {print $(i+1); break}}' || true)
if [ -n "$IFACE" ]; then
	MTU=$(ip link show "$IFACE" 2>/dev/null | grep -oE 'mtu [0-9]+' | awk '{print $2}' || true)
	if [ -n "$MTU" ]; then
		if [ "$MTU" -lt 1500 ]; then
			log "ERROR: Network interface $IFACE has MTU $MTU, which is below the physical minimum of 1500 required for NBD/TCP testing"
			exit 78 # EX_CONFIG
		fi
	fi
fi

log "connecting nbd-client -> $DEV (-timeout 30, never -persist)"
nbd-client "$HOST" "$PORT" "$DEV" -timeout 30
sleep 1

for r in $(seq 1 "$ROUNDS"); do
	for mode in randread randwrite; do
		log "round $r/$ROUNDS $mode 4k iodepth=1 ..."
		fio --name="nbdtcp-$mode" --filename="$DEV" --direct=1 --bs=4k --iodepth=1 \
			--rw="$mode" --runtime=15 --time_based --lat_percentiles=1 \
			--output-format=normal 2>&1 \
			| awk '/lat \(usec\)|percentiles|50.00th|99.00th|IOPS=/'
	done
done

log "append p50/p99/stddev by mode to P0-RESULTS.md (vs. ublk 241µs / NBD-Unix 326µs from Phase B)"
