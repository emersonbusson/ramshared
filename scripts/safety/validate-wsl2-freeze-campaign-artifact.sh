#!/usr/bin/env bash
# Validate WSL2 freeze-elimination campaign evidence without running pressure.
set -euo pipefail

ARTIFACT_DIR="${1:-}"
ROUNDS="${RAMSHARED_FREEZE_REQUIRED_ROUNDS:-2}"

fail() {
  echo "WSL2_FREEZE_CAMPAIGN_VALIDATION=FAIL reason=$*" >&2
  exit 1
}

need_file() {
  local path="$1"
  [[ -f "$path" ]] || fail "missing_file:$path"
}

forbidden_text() {
  local path="$1"
  local pattern="$2"
  if grep -qiE "$pattern" "$path" 2>/dev/null; then
    fail "forbidden_marker:$path:$pattern"
  fi
}

validate_integrity_result() {
  local path="$1"
  local round="$2"
  need_file "$path"
  local reason
  if ! reason="$(
    python3 - "$path" <<'PY'
import json
import sys

path = sys.argv[1]
try:
    with open(path, "r", encoding="utf-8") as fh:
        data = json.load(fh)
except Exception as exc:
    print(f"integrity_json_invalid:{exc}")
    sys.exit(1)

status = data.get("status")
allocated_mib = data.get("allocated_mib")
verified_chunks = data.get("verified_chunks")
checksum_before = data.get("checksum_before")
checksum_after = data.get("checksum_after")

if status != "PASS":
    print("integrity_status_not_pass")
    sys.exit(1)
if not isinstance(allocated_mib, int) or allocated_mib <= 0:
    print("integrity_allocated_mib_invalid")
    sys.exit(1)
if not isinstance(verified_chunks, int) or verified_chunks <= 0:
    print("integrity_verified_chunks_invalid")
    sys.exit(1)
if not checksum_before or not checksum_after or checksum_before != checksum_after:
    print("integrity_checksum_mismatch")
    sys.exit(1)
print("ok")
PY
  )"; then
    fail "$reason:round-$round"
  fi
}

validate_summary() {
  local path="$1"
  python3 - "$path" <<'PY'
import json
import sys

try:
    with open(sys.argv[1], "r", encoding="utf-8") as fh:
        summary = json.load(fh)
except Exception as exc:
    print(f"summary_json_invalid:{exc}")
    sys.exit(1)

if summary.get("gates_ok") is not True:
    print("summary_gates_not_ok")
    sys.exit(1)
if not isinstance(summary.get("daily_host"), bool):
    print("summary_daily_host_missing")
    sys.exit(1)
if summary["daily_host"]:
    if summary.get("shared_host_approved") is not True:
        print("shared_host_not_approved")
        sys.exit(1)
    if summary.get("windows_watchdog") is not True:
        print("windows_watchdog_missing")
        sys.exit(1)
print("ok")
PY
}

[[ -n "$ARTIFACT_DIR" ]] || fail "usage: validate-wsl2-freeze-campaign-artifact.sh ARTIFACT_DIR"
[[ -d "$ARTIFACT_DIR" ]] || fail "missing_artifact_dir:$ARTIFACT_DIR"

need_file "$ARTIFACT_DIR/summary.json"
if ! summary_reason="$(validate_summary "$ARTIFACT_DIR/summary.json")"; then
  fail "$summary_reason"
fi

COMPLETE_FILE="$ARTIFACT_DIR/isolated-complete.txt"
MODE="isolated"
if [[ ! -f "$COMPLETE_FILE" && -f "$ARTIFACT_DIR/shared-daily-host-complete.txt" ]]; then
  COMPLETE_FILE="$ARTIFACT_DIR/shared-daily-host-complete.txt"
  MODE="shared-daily-host"
fi
need_file "$COMPLETE_FILE"

if grep -Eq '"daily_host"[[:space:]]*:[[:space:]]*true' "$ARTIFACT_DIR/summary.json"; then
  if [[ "$MODE" != "shared-daily-host" ]]; then
    fail "daily_host_true"
  fi
  grep -Eq '"shared_host_approved"[[:space:]]*:[[:space:]]*true' \
    "$ARTIFACT_DIR/summary.json" || fail "shared_host_not_approved"
  grep -Eq '"windows_watchdog"[[:space:]]*:[[:space:]]*true' \
    "$ARTIFACT_DIR/summary.json" || fail "windows_watchdog_missing"
elif [[ "$MODE" == "shared-daily-host" ]]; then
  fail "shared_mode_without_daily_host"
fi
if ! grep -q "rounds=$ROUNDS" "$COMPLETE_FILE"; then
  fail "complete_round_count"
fi

round=1
while [[ "$round" -le "$ROUNDS" ]]; do
  rdir="$ARTIFACT_DIR/round-$round"
  [[ -d "$rdir" ]] || fail "missing_round_dir:$rdir"
  for f in \
    before.txt before-health.json \
    swap-sanitize-before.txt action-rc.txt \
    after.txt after-health.json \
    swap-sanitize-after.txt swaps-after-cleanup.txt; do
    need_file "$rdir/$f"
  done
  if [[ -f "$rdir/watchdog.txt" ]]; then
    fail "watchdog_fired:round-$round"
  fi
  grep -q '^action_rc=0$' "$rdir/action-rc.txt" || fail "action_rc_not_zero:round-$round"
  forbidden_text "$rdir/before.txt" 'hung_task|Blocked for more than'
  forbidden_text "$rdir/after.txt" 'hung_task|Blocked for more than'
  if [[ "${RAMSHARED_ALLOW_RECENT_OOM_MARKER:-0}" != "1" ]]; then
    forbidden_text "$rdir/before.txt" 'Out of memory'
    forbidden_text "$rdir/after.txt" 'Out of memory'
  fi
  grep -q 'OK diagnose complete' "$rdir/swap-sanitize-before.txt" || fail "sanitize_before_not_ok:round-$round"
  grep -q 'OK diagnose complete' "$rdir/swap-sanitize-after.txt" || fail "sanitize_after_not_ok:round-$round"
  validate_integrity_result "$rdir/integrity-result.json" "$round"
  if grep -qE '\(deleted\)|\\040\(deleted\)' "$rdir/swaps-after-cleanup.txt"; then
    fail "deleted_swap_after_cleanup:round-$round"
  fi
  round=$((round + 1))
done

echo "WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS mode=$MODE rounds=$ROUNDS artifact=$ARTIFACT_DIR"
