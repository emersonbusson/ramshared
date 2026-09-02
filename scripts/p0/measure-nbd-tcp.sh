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

# Guard clauses for inputs
if ! [[ "$PORT" =~ ^[0-9]+$ ]] || [ "$PORT" -lt 1 ] || [ "$PORT" -gt 65535 ]; then
    log "ERROR: Invalid port: $PORT"
    exit 64
fi
if ! [[ "$ROUNDS" =~ ^[0-9]+$ ]] || [ "$ROUNDS" -lt 1 ]; then
    log "ERROR: Invalid rounds: $ROUNDS"
    exit 64
fi

SERVER=""
command -v nbdkit     >/dev/null && SERVER=nbdkit
[ -z "$SERVER" ] && command -v nbd-server >/dev/null && SERVER=nbd-server
[ -n "$SERVER" ]                  || { log "ERROR: install the server: sudo apt install nbdkit"; exit 69; }
command -v nbd-client >/dev/null  || { log "ERROR: sudo apt install nbd-client"; exit 69; }
command -v fio        >/dev/null  || { log "ERROR: sudo apt install fio"; exit 69; }
command -v nc         >/dev/null  || { log "ERROR: sudo apt install netcat-openbsd (nc)"; exit 69; }
command -v ip         >/dev/null  || { log "ERROR: iproute2 missing"; exit 69; }
[ "$(id -u)" -eq 0 ]              || { log "ERROR: root is required (nbd-client/modprobe nbd)"; exit 78; }
[ "$SERVER" = nbdkit ]            || { log "ERROR: use nbdkit (nbd-server requires manual configuration)"; exit 78; }

# Verify MTU of the interface routing to HOST
IFACE=$(ip route get "$HOST" 2>/dev/null | awk 'NR==1 {for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}' || true)
if [ -z "$IFACE" ]; then
    log "ERROR: Could not determine network interface for $HOST"
    exit 78
fi
MTU=$(cat "/sys/class/net/$IFACE/mtu" 2>/dev/null || echo 0)
if [ "$MTU" -eq 0 ]; then
    log "ERROR: Could not read MTU for interface $IFACE"
    exit 74
fi
log "Interface $IFACE MTU is $MTU"
if [ "$MTU" -lt 576 ]; then
    log "ERROR: MTU $MTU is too low"
    exit 78
fi

modprobe nbd nbds_max=1 2>/dev/null || true

SRV_PID=""
cleanup() {
	nbd-client -d "$DEV" 2>/dev/null || true
	[ -n "$SRV_PID" ] && kill "$SRV_PID" 2>/dev/null || true
}
trap cleanup EXIT

log "starting nbdkit memory 1G on $HOST:$PORT"
nbdkit --foreground --port "$PORT" --ipaddr "$HOST" memory 1G & SRV_PID=$!

# Wait up to 5 seconds for reachability
READY=0
for i in {1..5}; do
    if nc -z "$HOST" "$PORT" >/dev/null 2>&1; then
        READY=1
        break
    fi
    sleep 1
done
if [ "$READY" -eq 0 ]; then
    log "ERROR: NBD server at $HOST:$PORT not reachable via nc -z"
    exit 69
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
