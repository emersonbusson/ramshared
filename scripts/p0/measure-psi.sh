#!/usr/bin/env bash
# P0 ITEM-1 — amostra /proc/pressure/memory (linhas `some` e `full`) a 1 Hz, em CSV.
# Uso: measure-psi.sh [DURATION_s] [OUT_csv]
# SPEC: docs/memory-broker/SPECv2.md ITEM-1 (gate P0; PRD §10). Sem dependência de produto.
# Disciplina Kahneman #3 (número, não adjetivo): a saída alimenta o P0-RESULTS.md.
set -euo pipefail

DURATION="${1:-300}"
OUT="${2:-psi-$(date +%Y%m%d-%H%M%S).csv}"
PSI=/proc/pressure/memory
LOG_PREFIX="[p0-psi]"

log() { echo "$LOG_PREFIX $*" >&2; }

# Preflight: sem CONFIG_PSI o arquivo não existe — o broker depende de PSI (DT-15).
[ -r "$PSI" ] || {
	log "ERRO: $PSI ilegível. Kernel sem CONFIG_PSI/PSI_DEFAULT_DISABLED? PSI é pré-requisito."
	exit 1
}

log "amostrando $PSI por ${DURATION}s -> $OUT"
echo "ts,kind,avg10,avg60,avg300,total_us" > "$OUT"

end=$(( $(date +%s) + DURATION ))
while [ "$(date +%s)" -lt "$end" ]; do
	now=$(date +%s)
	# Linhas: "some avg10=0.00 avg60=0.00 avg300=0.00 total=N"
	while read -r kind a10 a60 a300 total; do
		[ -n "$kind" ] || continue
		printf '%s,%s,%s,%s,%s,%s\n' \
			"$now" "$kind" \
			"${a10#avg10=}" "${a60#avg60=}" "${a300#avg300=}" "${total#total=}" >> "$OUT"
	done < "$PSI"
	sleep 1
done

log "fim: $(( $(wc -l < "$OUT") - 1 )) amostras em $OUT"
