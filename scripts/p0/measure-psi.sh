#!/usr/bin/env bash
# P0 ITEM-1 — samples /proc/pressure/memory (`some` and `full` lines) at 1 Hz, in CSV.
# Usage: measure-psi.sh [DURATION_s] [OUT_csv]
# SPEC: docs/memory-broker/SPECv2.md ITEM-1 (P0 gate; PRD §10). No product dependency.
# Kahneman discipline #3 (number, not adjective): the output feeds P0-RESULTS.md.
set -euo pipefail

DURATION="${1:-300}"
OUT="${2:-psi-$(date +%Y%m%d-%H%M%S).csv}"
PSI=/proc/pressure/memory
LOG_PREFIX="[p0-psi]"

log() { echo "$LOG_PREFIX $*" >&2; }

# Preflight: without CONFIG_PSI the file does not exist — the broker depends on PSI (DT-15).
[ -r "$PSI" ] || {
	log "ERROR: $PSI is unreadable. Kernel without CONFIG_PSI/PSI_DEFAULT_DISABLED? PSI is required."
	exit 69
}

log "sampling $PSI for ${DURATION}s -> $OUT"
echo "ts,kind,avg10,avg60,avg300,total_us" > "$OUT"

end=$(( $(date +%s) + DURATION ))
while [ "$(date +%s)" -lt "$end" ]; do
	now=$(date +%s)
	# Lines: "some avg10=0.00 avg60=0.00 avg300=0.00 total=N"
	while read -r kind a10 a60 a300 total; do
		[ -n "$kind" ] || continue
		printf '%s,%s,%s,%s,%s,%s\n' \
			"$now" "$kind" \
			"${a10#avg10=}" "${a60#avg60=}" "${a300#avg300=}" "${total#total=}" >> "$OUT"
	done < "$PSI"
	sleep 1
done

log "end: $(( $(wc -l < "$OUT") - 1 )) samples in $OUT"
