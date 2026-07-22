#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT/scripts/safety/validate-wsl2-freeze-campaign-artifact.sh"

[[ -f "$SCRIPT" ]] || { echo "missing validator" >&2; exit 1; }
text="$(cat "$SCRIPT")"

needles=(
  "isolated-complete.txt"
  "shared-daily-host-complete.txt"
  '"daily_host":true'
  '"shared_host_approved":true'
  '"windows_watchdog":true'
  '"gates_ok":false'
  'round-$round'
  "before-health.json"
  "after-health.json"
  "swap-sanitize-before.txt"
  "swap-sanitize-after.txt"
  "watchdog.txt"
  "integrity-result.json"
  "integrity_status_not_pass"
  "integrity_checksum_mismatch"
  "integrity_verified_chunks_invalid"
  "action_rc=0"
  "hung_task|Blocked for more than|Out of memory"
  "deleted_swap_after_cleanup"
  "WSL2_FREEZE_CAMPAIGN_VALIDATION=PASS"
)

for needle in "${needles[@]}"; do
  grep -Fq "$needle" "$SCRIPT" || { echo "missing token: $needle" >&2; exit 1; }
done

for forbidden in \
  "cascade-pressure-probe.sh" \
  "swapoff " \
  "wsl --terminate" \
  "wsl --shutdown" \
  "Start-VM" \
  "Stop-VM" \
  "Initialize-Disk" \
  "Resize-VHD" \
  "Format-Volume"; do
  if grep -Fq "$forbidden" "$SCRIPT"; then
    echo "forbidden token: $forbidden" >&2
    exit 1
  fi
done

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

make_valid_artifact() {
  local artifact="$1"
  mkdir -p "$artifact/round-1"
  printf '{"gates_ok":true,"daily_host":false}\n' >"$artifact/summary.json"
  printf 'rounds=1\n' >"$artifact/isolated-complete.txt"
  printf 'before ok\n' >"$artifact/round-1/before.txt"
  printf '{}\n' >"$artifact/round-1/before-health.json"
  printf 'OK diagnose complete\n' >"$artifact/round-1/swap-sanitize-before.txt"
  printf 'action_rc=0\n' >"$artifact/round-1/action-rc.txt"
  printf 'after ok\n' >"$artifact/round-1/after.txt"
  printf '{}\n' >"$artifact/round-1/after-health.json"
  printf 'OK diagnose complete\n' >"$artifact/round-1/swap-sanitize-after.txt"
  printf 'Filename Type Size Used Priority\n' >"$artifact/round-1/swaps-after-cleanup.txt"
  printf '{"status":"PASS","allocated_mib":16,"verified_chunks":1,"checksum_before":"abc","checksum_after":"abc"}\n' \
    >"$artifact/round-1/integrity-result.json"
}

valid="$tmp/valid"
make_valid_artifact "$valid"
RAMSHARED_FREEZE_REQUIRED_ROUNDS=1 "$SCRIPT" "$valid" >/dev/null

missing_integrity="$tmp/missing-integrity"
make_valid_artifact "$missing_integrity"
rm "$missing_integrity/round-1/integrity-result.json"
if RAMSHARED_FREEZE_REQUIRED_ROUNDS=1 "$SCRIPT" "$missing_integrity" >/dev/null 2>&1; then
  echo "validator accepted missing integrity result" >&2
  exit 1
fi

bad_status="$tmp/bad-status"
make_valid_artifact "$bad_status"
printf '{"status":"FAIL","allocated_mib":16,"verified_chunks":1,"checksum_before":"abc","checksum_after":"abc"}\n' \
  >"$bad_status/round-1/integrity-result.json"
if RAMSHARED_FREEZE_REQUIRED_ROUNDS=1 "$SCRIPT" "$bad_status" >/dev/null 2>&1; then
  echo "validator accepted failed integrity status" >&2
  exit 1
fi

bad_checksum="$tmp/bad-checksum"
make_valid_artifact "$bad_checksum"
printf '{"status":"PASS","allocated_mib":16,"verified_chunks":1,"checksum_before":"abc","checksum_after":"def"}\n' \
  >"$bad_checksum/round-1/integrity-result.json"
if RAMSHARED_FREEZE_REQUIRED_ROUNDS=1 "$SCRIPT" "$bad_checksum" >/dev/null 2>&1; then
  echo "validator accepted integrity checksum mismatch" >&2
  exit 1
fi

echo "PASS test-wsl2-freeze-campaign-artifact-static"
