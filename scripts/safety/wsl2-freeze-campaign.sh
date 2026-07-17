#!/usr/bin/env bash
# wsl2-freeze-campaign.sh — isolated WSL2 hang/freeze audit scaffold (RamShared).
#
# Policy (mandatory):
#   - NEVER thrash swap/ublk/cascade pressure on the daily WSL2 host.
#   - Default mode is dry-run / baseline capture only (read-only probes).
#   - Destructive "action" phase requires --allow-isolated-lab AND RAMSHARED_ISOLATED_LAB=1
#     and still refuses when the environment looks like the daily host.
#
# Promotion claim for "WSL2 freeze elimination" still requires a full live campaign:
#   before → action → after, twice, with watchdog/timeout, swapoff-first, ghost check,
#   BINARY_MATCH, D-state/hung-task evidence, and cleanup. This script only automates
#   the safe baseline + gate checks until that isolated lab is available.
#
# Usage:
#   ./scripts/safety/wsl2-freeze-campaign.sh                 # dry-run baseline
#   ./scripts/safety/wsl2-freeze-campaign.sh --check-gates   # exit 0 only if claim-ready gates pass
#   ./scripts/safety/wsl2-freeze-campaign.sh --json          # machine-readable sample
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MODE="dry-run"
JSON=0
CHECK_GATES=0
ALLOW_ISOLATED=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) MODE="dry-run"; shift ;;
    --check-gates) CHECK_GATES=1; shift ;;
    --json) JSON=1; shift ;;
    --allow-isolated-lab) ALLOW_ISOLATED=1; shift ;;
    -h|--help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
done

ts="$(date -Iseconds 2>/dev/null || date)"
hostname_s="$(hostname 2>/dev/null || echo unknown)"
is_daily_host=0
# Heuristic: daily host markers used elsewhere in this repo.
if [[ "${WSL_DISTRO_NAME:-}" == "Ubuntu-24.04" ]] && [[ -d /mnt/c/Users ]]; then
  is_daily_host=1
fi
if [[ "${RAMSHARED_FORCE_DAILY_HOST:-0}" == "1" ]]; then
  is_daily_host=1
fi

ghost=false
binary_match="n/a"
daemon_pid=""
if pgrep -x ramsharedd >/dev/null 2>&1; then
  daemon_pid="$(pgrep -n -x ramsharedd || true)"
  if [[ -n "$daemon_pid" && -r "/proc/$daemon_pid/exe" ]]; then
    exe="$(readlink -f "/proc/$daemon_pid/exe" 2>/dev/null || true)"
    if [[ -n "$exe" && -x "$ROOT/target/release/ramsharedd" ]]; then
      want="$(readlink -f "$ROOT/target/release/ramsharedd")"
      if [[ "$exe" == "$want" ]]; then binary_match=true; else binary_match=false; fi
    fi
    if grep -q '(deleted)' "/proc/$daemon_pid/exe" 2>/dev/null; then
      ghost=true
    fi
  fi
fi

swaps_raw="$(cat /proc/swaps 2>/dev/null || true)"
used_total="$(awk 'NR>1 {s+=$4} END {print s+0}' /proc/swaps 2>/dev/null || echo 0)"
has_deleted_swap=false
if echo "$swaps_raw" | grep -q '(deleted)'; then
  has_deleted_swap=true
fi

health_json="{}"
if [[ -x "$ROOT/scripts/safety/cascade-health.sh" ]]; then
  health_json="$("$ROOT/scripts/safety/cascade-health.sh" --once 2>/dev/null || echo '{}')"
fi

# Claim-ready gates (all must be true to even *start* an elimination campaign).
gates_ok=1
reasons=()
if [[ "$is_daily_host" -eq 1 && "$ALLOW_ISOLATED" -eq 0 ]]; then
  gates_ok=0
  reasons+=("daily_host_refused_without_isolated_lab_flag")
fi
if [[ "${RAMSHARED_ISOLATED_LAB:-0}" != "1" && "$ALLOW_ISOLATED" -eq 1 ]]; then
  gates_ok=0
  reasons+=("RAMSHARED_ISOLATED_LAB!=1")
fi
if [[ "$ghost" == "true" ]]; then
  gates_ok=0
  reasons+=("ghost_daemon_or_deleted_exe")
fi
if [[ "$has_deleted_swap" == "true" && "$used_total" -gt 0 ]]; then
  gates_ok=0
  reasons+=("ghost_swap_used_kb_gt_0")
fi

if [[ "$JSON" -eq 1 ]]; then
  reason_csv="$(IFS=,; echo "${reasons[*]-}")"
  printf '{"ts":"%s","mode":"%s","host":"%s","daily_host":%s,"ghost":%s,"binary_match":"%s","used_kib":%s,"has_deleted_swap":%s,"gates_ok":%s,"reasons":"%s","health":%s}\n' \
    "$ts" "$MODE" "$hostname_s" \
    "$([[ $is_daily_host -eq 1 ]] && echo true || echo false)" \
    "$ghost" "$binary_match" "$used_total" "$has_deleted_swap" \
    "$([[ $gates_ok -eq 1 ]] && echo true || echo false)" \
    "$reason_csv" "$health_json"
else
  echo "=== WSL2 freeze campaign scaffold ==="
  echo "ts:            $ts"
  echo "mode:          $MODE"
  echo "host:          $hostname_s"
  echo "daily_host:    $is_daily_host"
  echo "ghost:         $ghost"
  echo "BINARY_MATCH:  $binary_match"
  echo "swap used_kib: $used_total"
  echo "deleted swap:  $has_deleted_swap"
  echo "gates_ok:      $gates_ok"
  if [[ ${#reasons[@]} -gt 0 ]]; then
    echo "refuse_reasons:"
    for r in "${reasons[@]}"; do echo "  - $r"; done
  fi
  echo "cascade-health: $health_json"
  echo
  echo "CLAIM STATUS: WSL2 freeze-elimination is NOT claimed."
  echo "Next (isolated lab only): 2× before→action→after with watchdog, swapoff-first,"
  echo "ghost check, BINARY_MATCH, D-state/hung_task capture, cleanup. No thrash here."
fi

if [[ "$CHECK_GATES" -eq 1 ]]; then
  if [[ "$gates_ok" -eq 1 ]]; then
    exit 0
  fi
  exit 1
fi

# Always exit 0 in dry-run baseline mode: the script's job is capture + honest refuse.
exit 0
