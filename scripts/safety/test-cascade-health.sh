#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
HEALTH="$ROOT/scripts/safety/cascade-health.sh"
FIXTURE=$(mktemp -d)
trap 'rm -rf -- "$FIXTURE"' EXIT

cat >"$FIXTURE/ramshared" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >"$RAMSHARED_TEST_ARGS"
printf '%s\n' '{"schema_version":3,"phase":"Off","protection_state":"OFF","sample_age_ms":0}'
SH
chmod 0755 "$FIXTURE/ramshared"

RAMSHARED_TEST_ARGS="$FIXTURE/args" RAMSHARED_BIN="$FIXTURE/ramshared" \
  "$HEALTH" --once --out "$FIXTURE/health.jsonl" \
  --heartbeat "$FIXTURE/heartbeat.json" --interval 3 >"$FIXTURE/stdout"

[[ $(cat "$FIXTURE/args") == \
  "monitor --jsonl --once --interval-ms 3000 --output $FIXTURE/health.jsonl --heartbeat $FIXTURE/heartbeat.json" ]]
python3 - "$FIXTURE/stdout" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    record = json.load(stream)
assert record["schema_version"] == 3, record
assert record["sample_age_ms"] == 0, record
PY

if grep -Eq '/proc/|nvidia-smi|pgrep|python3 -c' "$HEALTH"; then
  echo "health wrapper must not duplicate the typed collector" >&2
  exit 1
fi

printf 'PASS cascade_health_delegates_to_typed_monitor\n'
