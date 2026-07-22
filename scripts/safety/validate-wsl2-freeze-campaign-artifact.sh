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

[[ -n "$ARTIFACT_DIR" ]] || fail "usage: validate-wsl2-freeze-campaign-artifact.sh ARTIFACT_DIR"
[[ -d "$ARTIFACT_DIR" ]] || fail "missing_artifact_dir:$ARTIFACT_DIR"

need_file "$ARTIFACT_DIR/summary.json"

COMPLETE_FILE="$ARTIFACT_DIR/isolated-complete.txt"
MODE="isolated"
if [[ ! -f "$COMPLETE_FILE" && -f "$ARTIFACT_DIR/shared-daily-host-complete.txt" ]]; then
  COMPLETE_FILE="$ARTIFACT_DIR/shared-daily-host-complete.txt"
  MODE="shared-daily-host"
fi
need_file "$COMPLETE_FILE"

if grep -q '"daily_host":true' "$ARTIFACT_DIR/summary.json"; then
  if [[ "$MODE" != "shared-daily-host" ]]; then
    fail "daily_host_true"
  fi
  grep -q '"shared_host_approved":true' "$ARTIFACT_DIR/summary.json" || fail "shared_host_not_approved"
  grep -q '"windows_watchdog":true' "$ARTIFACT_DIR/summary.json" || fail "windows_watchdog_missing"
fi
if grep -q '"gates_ok":false' "$ARTIFACT_DIR/summary.json"; then
  fail "gates_not_ok"
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
  forbidden_text "$rdir/before.txt" 'hung_task|Blocked for more than|Out of memory'
  forbidden_text "$rdir/after.txt" 'hung_task|Blocked for more than|Out of memory'
  grep -q 'OK diagnose complete' "$rdir/swap-sanitize-before.txt" || fail "sanitize_before_not_ok:round-$round"
  grep -q 'OK diagnose complete' "$rdir/swap-sanitize-after.txt" || fail "sanitize_after_not_ok:round-$round"
  if grep -qE '\(deleted\)|\\040\(deleted\)' "$rdir/swaps-after-cleanup.txt"; then
    fail "deleted_swap_after_cleanup:round-$round"
  fi
  round=$((round + 1))
done

echo "WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS mode=$MODE rounds=$ROUNDS artifact=$ARTIFACT_DIR"
