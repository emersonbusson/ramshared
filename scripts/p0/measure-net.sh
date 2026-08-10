#!/usr/bin/env bash
# P0 ITEM-1 — VM<->WSL2 reachability and RTT: ping (p50/p99) plus a TCP port test.
# Usage: measure-net.sh PEER_HOST [PORT] [PING_COUNT]
# SPEC: docs/memory-broker/SPECv2.md ITEM-1; R1 (WSL2 NAT — PRD inference to validate).
# Run in BOTH directions (WSL2->civm and civm->WSL2), with and without Tailscale; decide the transport.
set -euo pipefail

PEER="${1:?usage: measure-net.sh PEER_HOST [PORT] [PING_COUNT]}"
PORT="${2:-10809}"
COUNT="${3:-100}"
LOG_PREFIX="[p0-net]"
log() { echo "$LOG_PREFIX $*" >&2; }

command -v ping >/dev/null || { log "ERROR: ping missing"; exit 1; }
command -v nc   >/dev/null || { log "ERROR: nc missing — sudo apt install netcat-openbsd"; exit 1; }

log "ping $COUNT x $PEER ..."
rtts=$(ping -n -c "$COUNT" -i 0.2 "$PEER" 2>/dev/null \
	| awk -F'time=' '/time=/{split($2,a," "); print a[1]}' | sort -n || true)
n=$(printf '%s\n' "$rtts" | grep -c . || true)
if [ "${n:-0}" -gt 0 ]; then
	p50=$(printf '%s\n' "$rtts" | awk -v n="$n" 'NR==int((n+1)/2){print; exit}')
	p99=$(printf '%s\n' "$rtts" | awk -v n="$n" 'NR>=int(n*0.99+0.5){print; exit}')
	log "RTT(ms): p50=$p50 p99=$p99 (n=$n)"
else
	log "FAILURE: $PEER is unreachable over ICMP (it may be a firewall; test the port below)"
fi

log "TCP port $PEER:$PORT ..."
if nc -z -w 3 "$PEER" "$PORT" 2>/dev/null; then
	log "TCP $PEER:$PORT OPEN"
else
	log "TCP $PEER:$PORT CLOSED/filtered"
fi

log "append RTT and port state (in both directions, with/without Tailscale) to P0-RESULTS.md"
