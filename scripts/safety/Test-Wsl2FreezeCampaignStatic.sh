#!/usr/bin/env bash
# Static gate for wsl2-freeze-campaign.sh — required tokens for supervised pressure.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT/scripts/safety/wsl2-freeze-campaign.sh"

[[ -x "$SCRIPT" ]] || chmod +x "$SCRIPT"
bash -n "$SCRIPT"

required=(
  "RAMSHARED_ISOLATED_LAB"
  "daily_host_refused_without_isolated_lab_flag"
  "ghost_swap_used_kb_gt_0"
  "BINARY_MATCH"
  "before"
  "after"
  "swap-sanitize"
  "cascade-pressure-probe"
  "WATCHDOG"
  "NOT_CLAIMED"
  "daily_host_refuses_run_isolated"
  "shared_windows_desktop_refuses_run_isolated"
  "shared_windows_desktop"
  "oom_hits"
  "recent_dmesg_sec"
  "RAMSHARED_FREEZE_RECENT_DMESG_SEC"
  "recent_oom_marker"
  "RAMSHARED_ALLOW_RECENT_OOM_MARKER"
  "Memory cgroup out of memory"
  "--run-isolated"
  "--approve-shared-daily-host"
  "--run-shared-daily-host"
  "RAMSHARED_SHARED_HOST_APPROVAL"
  "I_ACCEPT_WSL_TERMINATION"
  "RAMSHARED_WINDOWS_WATCHDOG_ARMED"
  "missing_shared_host_ack_token"
  "windows_watchdog_not_armed"
  "shared-daily-host-complete.txt"
  "--artifact-dir"
  "--integrity-result"
  "ACTION_CLEANUP_GRACE_SEC"
  "watchdog_deadline"
  "action_cleanup_timeout"
)

src="$(cat "$SCRIPT")"
for token in "${required[@]}"; do
  if ! grep -Fq -- "$token" <<<"$src"; then
    echo "FAIL missing token: $token" >&2
    exit 1
  fi
done

# The action watchdog must leave a grace interval for the pressure probe to
# release its worker and verify integrity. Killing the controller can orphan a
# worker in D-state while NBD swap is still active.
if grep -Fq 'kill -KILL "$action_pid"' <<<"$src"; then
  echo "FAIL action watchdog must never SIGKILL the pressure controller" >&2
  exit 1
fi

# Dry-run must not require root and must exit 0 on daily host.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
out="$("$SCRIPT" --dry-run --artifact-dir "$tmp/art" 2>&1)" || {
  echo "FAIL dry-run exited non-zero" >&2
  echo "$out" >&2
  exit 1
}
[[ -f "$tmp/art/baseline.txt" ]] || { echo "FAIL missing baseline.txt" >&2; exit 1; }
[[ -f "$tmp/art/summary.json" ]] || { echo "FAIL missing summary.json" >&2; exit 1; }
if ! grep -q 'NOT_CLAIMED' "$tmp/art/summary.json"; then
  echo "FAIL summary must remain NOT_CLAIMED on dry-run" >&2
  exit 1
fi

# --run-isolated on daily host without flags must refuse (exit != 0).
set +e
"$SCRIPT" --run-isolated --artifact-dir "$tmp/art2" >/dev/null 2>&1
rc=$?
set -e
if [[ "$rc" -eq 0 ]]; then
  echo "FAIL --run-isolated must refuse on daily host without isolated flags" >&2
  exit 1
fi

# Shared daily-host action must also refuse without the explicit ack token and
# external Windows watchdog marker.
set +e
"$SCRIPT" --approve-shared-daily-host --run-shared-daily-host --artifact-dir "$tmp/art3" >/dev/null 2>&1
rc=$?
set -e
if [[ "$rc" -eq 0 ]]; then
  echo "FAIL shared daily-host run must refuse without ack token/watchdog" >&2
  exit 1
fi

echo "STATIC_WSL2_FREEZE_CAMPAIGN=PASS"
exit 0
