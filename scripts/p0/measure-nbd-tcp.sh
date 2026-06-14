#!/usr/bin/env bash
# P0 ITEM-1 — p50/p99 de NBD/TCP CRU no virt-switch, SEM código nosso (baseline honesto).
# nbdkit memory 1G  +  nbd-client (TCP)  +  fio (randread/randwrite 4k, lat_percentiles).
# Uso: measure-nbd-tcp.sh [HOST] [PORT] [ROUNDS]   (rodar como root)
# SPEC: docs/memory-broker/SPECv2.md ITEM-1; R4 (latência virt-switch). NADA entra no produto.
# Disciplina #3: comparar com p50 241µs (ublk) / 326µs (NBD-Unix) da Fase B.
set -euo pipefail

HOST="${1:-127.0.0.1}"
PORT="${2:-10810}"
ROUNDS="${3:-3}"
DEV=/dev/nbd0
LOG_PREFIX="[p0-nbdtcp]"
log() { echo "$LOG_PREFIX $*" >&2; }

# --- Preflight de dependências (F17: host de medição pode não ter nbdkit/nbd-server) ---
SERVER=""
command -v nbdkit     >/dev/null && SERVER=nbdkit
[ -z "$SERVER" ] && command -v nbd-server >/dev/null && SERVER=nbd-server
[ -n "$SERVER" ]                  || { log "ERRO: instale o servidor: sudo apt install nbdkit"; exit 1; }
command -v nbd-client >/dev/null  || { log "ERRO: sudo apt install nbd-client"; exit 1; }
command -v fio        >/dev/null  || { log "ERRO: sudo apt install fio"; exit 1; }
[ "$(id -u)" -eq 0 ]              || { log "ERRO: precisa de root (nbd-client/modprobe nbd)"; exit 1; }
[ "$SERVER" = nbdkit ]            || { log "ERRO: use nbdkit (nbd-server exige config manual)"; exit 1; }

modprobe nbd nbds_max=1 2>/dev/null || true

SRV_PID=""
cleanup() {
	nbd-client -d "$DEV" 2>/dev/null || true
	[ -n "$SRV_PID" ] && kill "$SRV_PID" 2>/dev/null || true
}
trap cleanup EXIT

log "subindo nbdkit memory 1G em $HOST:$PORT"
nbdkit --foreground --port "$PORT" --ipaddr "$HOST" memory 1G & SRV_PID=$!
sleep 1
log "conectando nbd-client -> $DEV (-timeout 30, nunca -persist)"
nbd-client "$HOST" "$PORT" "$DEV" -timeout 30
sleep 1

for r in $(seq 1 "$ROUNDS"); do
	for mode in randread randwrite; do
		log "rodada $r/$ROUNDS $mode 4k iodepth=1 ..."
		fio --name="nbdtcp-$mode" --filename="$DEV" --direct=1 --bs=4k --iodepth=1 \
			--rw="$mode" --runtime=15 --time_based --lat_percentiles=1 \
			--output-format=normal 2>&1 \
			| awk '/lat \(usec\)|percentiles|50.00th|99.00th|IOPS=/'
	done
done

log "anexe p50/p99/stddev por modo ao P0-RESULTS.md (vs ublk 241µs / NBD-Unix 326µs da Fase B)"
