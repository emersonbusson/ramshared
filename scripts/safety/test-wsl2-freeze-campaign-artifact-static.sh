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

echo "PASS test-wsl2-freeze-campaign-artifact-static"
