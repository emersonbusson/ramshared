#!/usr/bin/env bash
# P0 ITEM-1 — alcançabilidade e RTT VM<->WSL2: ping (p50/p99) + teste de porta TCP.
# Uso: measure-net.sh PEER_HOST [PORT] [PING_COUNT]
# SPEC: docs/memory-broker/SPECv2.md ITEM-1; R1 (NAT do WSL2 — Inferência do PRD a validar).
# Rode nos DOIS sentidos (WSL2->civm e civm->WSL2), com e sem Tailscale; decida o transporte.
set -euo pipefail

PEER="${1:?uso: measure-net.sh PEER_HOST [PORT] [PING_COUNT]}"
PORT="${2:-10809}"
COUNT="${3:-100}"
LOG_PREFIX="[p0-net]"
log() { echo "$LOG_PREFIX $*" >&2; }

command -v ping >/dev/null || { log "ERRO: ping ausente"; exit 1; }
command -v nc   >/dev/null || { log "ERRO: nc ausente — sudo apt install netcat-openbsd"; exit 1; }

log "ping $COUNT x $PEER ..."
rtts=$(ping -n -c "$COUNT" -i 0.2 "$PEER" 2>/dev/null \
	| awk -F'time=' '/time=/{split($2,a," "); print a[1]}' | sort -n || true)
n=$(printf '%s\n' "$rtts" | grep -c . || true)
if [ "${n:-0}" -gt 0 ]; then
	p50=$(printf '%s\n' "$rtts" | awk -v n="$n" 'NR==int((n+1)/2){print; exit}')
	p99=$(printf '%s\n' "$rtts" | awk -v n="$n" 'NR>=int(n*0.99+0.5){print; exit}')
	log "RTT(ms): p50=$p50 p99=$p99 (n=$n)"
else
	log "FALHA: $PEER inalcançável por ICMP (pode ser firewall; teste a porta abaixo)"
fi

log "porta TCP $PEER:$PORT ..."
if nc -z -w 3 "$PEER" "$PORT" 2>/dev/null; then
	log "TCP $PEER:$PORT ABERTA"
else
	log "TCP $PEER:$PORT FECHADA/filtrada"
fi

log "anexe RTT + estado da porta (nos dois sentidos, com/sem Tailscale) ao P0-RESULTS.md"
