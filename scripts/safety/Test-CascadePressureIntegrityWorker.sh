#!/usr/bin/env bash
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1

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
grep -q 'interrupted_before_allocation' "$WORKER"

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

# The benchmark pattern must be deterministic but not trivially compressible.
# This is a pure 4 MiB manufactured check; it does not allocate pressure or
# touch swap/cgroups.
python3 - "$WORKER" <<'PY'
import importlib.util
import sys
import zlib

spec = importlib.util.spec_from_file_location("ramshared_integrity_worker", sys.argv[1])
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

first = module.chunk_pattern(7, 4 * 1024 * 1024, "shake256-v1")
second = module.chunk_pattern(7, 4 * 1024 * 1024, "shake256-v1")
assert first == second
assert len(zlib.compress(first, level=1)) > len(first) * 0.98
PY
echo "PASS shake256_pattern_is_deterministic_and_incompressible"

rm -f -- "$RESULT" "$LOG"
python3 "$WORKER" --allocate-mib 16 --pattern shake256-v1 --result "$RESULT" >"$LOG" 2>&1 &
PID=$!
for _ in $(seq 1 200); do
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
assert result["pattern"] == "shake256-v1", result
assert result["checksum_before"] == result["checksum_after"], result
PY

rm -f -- "$RESULT" "$LOG"
python3 "$WORKER" --allocate-mib 128 --chunk-mib 16 --chunk-delay-ms 50 --result "$RESULT" >"$LOG" 2>&1 &
PID=$!
for _ in $(seq 1 100); do
	grep -q '^ALLOC ' "$LOG" 2>/dev/null && break
	sleep 0.05
done
grep -q '^ALLOC ' "$LOG"
kill -TERM "$PID"
wait "$PID"
PID=""

python3 - "$RESULT" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    result = json.load(source)

assert result["status"] == "PASS", result
assert result["allocated_mib"] > 0, result
assert result["allocated_mib"] < 128, result
assert result["verified_chunks"] > 0, result
assert result["checksum_before"] == result["checksum_after"], result
PY

echo "PASS Test-CascadePressureIntegrityWorker"
