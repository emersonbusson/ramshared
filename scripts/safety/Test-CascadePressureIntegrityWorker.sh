#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKER="$ROOT/scripts/safety/cascade_pressure_integrity_worker.py"
PROBE="$ROOT/scripts/safety/cascade-pressure-probe.sh"
TMP_DIR="$(mktemp -d)"
RESULT="$TMP_DIR/result.json"
LOG="$TMP_DIR/worker.log"
PID=""

cleanup() {
	if [[ -n "$PID" ]] && kill -0 "$PID" 2>/dev/null; then
		kill -KILL "$PID" 2>/dev/null || true
		wait "$PID" 2>/dev/null || true
	fi
	rm -rf -- "$TMP_DIR"
}
trap cleanup EXIT

for token in \
	'cascade_pressure_integrity_worker.py' \
	'integrity-result.json' \
	'integrity_result_missing' \
	'integrity_result_failed'; do
	grep -q "$token" "$PROBE"
done
grep -q 'interrupted_during_allocation' "$WORKER"

python3 "$WORKER" --allocate-mib 16 --result "$RESULT" >"$LOG" 2>&1 &
PID=$!

for _ in $(seq 1 100); do
	grep -q '^HOLD ' "$LOG" 2>/dev/null && break
	sleep 0.05
done
grep -q '^HOLD ' "$LOG"

kill -TERM "$PID"
wait "$PID"
PID=""

python3 - "$RESULT" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    result = json.load(source)

assert result["status"] == "PASS", result
assert result["allocated_mib"] == 16, result
assert result["verified_chunks"] > 0, result
assert result["checksum_before"] == result["checksum_after"], result
PY

echo "PASS Test-CascadePressureIntegrityWorker"
