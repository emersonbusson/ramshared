#!/usr/bin/env bash
# P0 ITEM-1 — PSI do sistema SOB CARGA DE MEMÓRIA, via hog ANÔNIMO confinado em cgroup v2.
# Pressão REAL e BOUNDED: `memory.max` é teto de segurança e `memory.swap.max=0` impede
# qualquer swap-out — NADA de swap em device / daemon / block device (longe do cenário do
# freeze de 2026-06-09). Substitui o "cargo build -j4" do SPEC: o P0 achou que build é
# CPU-bound e NÃO gera PSI de memória (ver P0-RESULTS §1, célula "carga").
# Uso: measure-psi-load.sh [DUR_s] [OUT_csv] [WS_MB] [HIGH_MB] [MAX_MB]   (root)
# SPEC: docs/memory-broker/SPECv2.md ITEM-1; calibra delta_psi (P0-RESULTS §5). Reusável p/ civm.
set -euo pipefail

DUR="${1:-40}"
OUT="${2:-psi-load-$(date +%Y%m%d-%H%M%S).csv}"
WS_MB="${3:-300}"   # working set anônimo do hog
HIGH_MB="${4:-64}"  # memory.high: throttle (stall/PSI) bem abaixo do WS
MAX_MB="${5:-512}"  # memory.max: TETO de segurança (bounded — sem OOM enquanto WS < MAX)
CG=/sys/fs/cgroup/p0load
LOG_PREFIX="[p0-load]"
log() { echo "$LOG_PREFIX $*" >&2; }

[ "$(id -u)" -eq 0 ]                                || { log "ERRO: precisa de root (escrita em cgroup)"; exit 1; }
[ "$(stat -fc %T /sys/fs/cgroup)" = cgroup2fs ]     || { log "ERRO: cgroup v2 ausente"; exit 1; }
grep -qw memory /sys/fs/cgroup/cgroup.subtree_control || { log "ERRO: controlador memory não delegado"; exit 1; }
command -v python3 >/dev/null                       || { log "ERRO: python3 ausente"; exit 1; }

HOG=""
cleanup() {
	[ -n "$HOG" ] && kill "$HOG" 2>/dev/null || true
	sleep 1
	if [ -d "$CG" ]; then
		xargs -r kill -9 < "$CG/cgroup.procs" 2>/dev/null || true
		sleep 1
		rmdir "$CG" 2>/dev/null && log "cgroup limpo" || log "AVISO: cgroup $CG restou (verificar)"
	fi
}
trap cleanup EXIT

mkdir -p "$CG"
echo "${HIGH_MB}M" > "$CG/memory.high"
echo "${MAX_MB}M"  > "$CG/memory.max"
echo 0             > "$CG/memory.swap.max" 2>/dev/null || true
log "cgroup p0load: high=${HIGH_MB}M max=${MAX_MB}M swap=0; hog WS=${WS_MB}M por ${DUR}s"

# Hog anônimo DENTRO do cgroup; teto memory.max garante bounded (sem freeze).
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

sleep 2  # deixa o hog encher e o throttle estabilizar
log "amostrando /proc/pressure/memory (sistema) por ${DUR}s -> $OUT"
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
log "fim: $(( $(wc -l < "$OUT") - 1 )) amostras em $OUT"
