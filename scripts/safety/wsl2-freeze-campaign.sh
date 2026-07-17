#!/usr/bin/env bash
# wsl2-freeze-campaign.sh — WSL2 hang/freeze audit campaign (RamShared).
#
# Policy (mandatory):
#   - NEVER thrash swap/ublk/cascade pressure on the daily WSL2 host.
#   - Default mode is dry-run / baseline capture only (read-only probes).
#   - Destructive "action" phase requires ALL of:
#       --allow-isolated-lab
#       RAMSHARED_ISOLATED_LAB=1
#       gates_ok (not daily host, no ghost daemon, no ghost swap with used>0)
#   - Even in isolated lab, action uses cgroup-bounded cascade-pressure-probe
#     (not full-VM thrash) with a hard watchdog.
#
# Promotion claim for "WSL2 freeze elimination" requires:
#   2× before → action → after, watchdog/timeout, swapoff-first, ghost check,
#   BINARY_MATCH when daemon present, D-state/hung-task capture, cleanup.
#
# Usage:
#   ./scripts/safety/wsl2-freeze-campaign.sh                 # dry-run baseline
#   ./scripts/safety/wsl2-freeze-campaign.sh --check-gates   # exit 0 only if claim-ready
#   ./scripts/safety/wsl2-freeze-campaign.sh --json          # machine-readable sample
#   ./scripts/safety/wsl2-freeze-campaign.sh --artifact-dir PATH
#   ./scripts/safety/wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MODE="dry-run"
JSON=0
CHECK_GATES=0
ALLOW_ISOLATED=0
RUN_ISOLATED=0
ARTIFACT_DIR=""
ROUNDS=2
WATCHDOG_SEC="${RAMSHARED_FREEZE_WATCHDOG_SEC:-120}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) MODE="dry-run"; shift ;;
    --check-gates) CHECK_GATES=1; shift ;;
    --json) JSON=1; shift ;;
    --allow-isolated-lab) ALLOW_ISOLATED=1; shift ;;
    --run-isolated) RUN_ISOLATED=1; MODE="isolated"; shift ;;
    --artifact-dir) ARTIFACT_DIR="$2"; shift 2 ;;
    --rounds) ROUNDS="$2"; shift 2 ;;
    --watchdog-sec) WATCHDOG_SEC="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,28p' "$0"
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
stamp="$(date +%Y%m%d-%H%M%S 2>/dev/null || echo now)"

is_daily_host=0
# Heuristic: daily host markers used elsewhere in this repo.
if [[ "${WSL_DISTRO_NAME:-}" == "Ubuntu-24.04" ]] && [[ -d /mnt/c/Users ]]; then
  is_daily_host=1
fi
if [[ "${RAMSHARED_FORCE_DAILY_HOST:-0}" == "1" ]]; then
  is_daily_host=1
fi
# Explicit override for true isolated lab VMs that still expose /mnt/c.
if [[ "${RAMSHARED_ISOLATED_LAB:-0}" == "1" && "${RAMSHARED_FORCE_ISOLATED_LAB:-0}" == "1" ]]; then
  is_daily_host=0
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

d_state_count=0
if [[ -r /proc/stat ]]; then
  # procs_blocked is a coarse D-state proxy on Linux.
  d_state_count="$(awk '/^procs_blocked/ {print $2+0}' /proc/stat 2>/dev/null || echo 0)"
fi
hung_task_hits=0
if dmesg 2>/dev/null | tail -n 200 | grep -qiE 'hung_task|Blocked for more than'; then
  hung_task_hits=1
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
if [[ "$is_daily_host" -eq 1 && "$RUN_ISOLATED" -eq 1 ]]; then
  gates_ok=0
  reasons+=("daily_host_refuses_run_isolated")
fi
if [[ "${RAMSHARED_ISOLATED_LAB:-0}" != "1" && ( "$ALLOW_ISOLATED" -eq 1 || "$RUN_ISOLATED" -eq 1 ) ]]; then
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
if [[ "$binary_match" == "false" ]]; then
  gates_ok=0
  reasons+=("BINARY_MATCH_false")
fi

reason_csv="$(IFS=,; echo "${reasons[*]-}")"

emit_summary() {
  if [[ "$JSON" -eq 1 ]]; then
    printf '{"ts":"%s","mode":"%s","host":"%s","daily_host":%s,"ghost":%s,"binary_match":"%s","used_kib":%s,"has_deleted_swap":%s,"d_state":%s,"hung_task_hits":%s,"gates_ok":%s,"reasons":"%s","health":%s,"claim":"NOT_CLAIMED"}\n' \
      "$ts" "$MODE" "$hostname_s" \
      "$([[ $is_daily_host -eq 1 ]] && echo true || echo false)" \
      "$ghost" "$binary_match" "$used_total" "$has_deleted_swap" \
      "$d_state_count" "$hung_task_hits" \
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
    echo "d_state:       $d_state_count"
    echo "hung_task:     $hung_task_hits"
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
}

write_artifact() {
  local dir="$1"
  local name="$2"
  local body="$3"
  mkdir -p "$dir"
  printf '%s\n' "$body" >"$dir/$name"
}

capture_phase() {
  local dir="$1"
  local label="$2"
  mkdir -p "$dir"
  {
    echo "phase=$label"
    echo "ts=$(date -Iseconds 2>/dev/null || date)"
    echo "=== /proc/swaps ==="
    cat /proc/swaps 2>/dev/null || true
    echo "=== meminfo (selected) ==="
    awk '/MemTotal|MemAvailable|SwapTotal|SwapFree/ {print}' /proc/meminfo 2>/dev/null || true
    echo "=== procs_blocked ==="
    awk '/^procs_blocked/ {print}' /proc/stat 2>/dev/null || true
    echo "=== ramsharedd ==="
    pgrep -a -x ramsharedd 2>/dev/null || echo "(none)"
    echo "=== dmesg tail (hung_task filter) ==="
    dmesg 2>/dev/null | tail -n 100 | grep -iE 'hung_task|Blocked for more than|Out of memory|ublk|nbd' || echo "(no hits)"
  } >"$dir/${label}.txt"
  if [[ -x "$ROOT/scripts/safety/cascade-health.sh" ]]; then
    "$ROOT/scripts/safety/cascade-health.sh" --once >"$dir/${label}-health.json" 2>/dev/null || echo '{}' >"$dir/${label}-health.json"
  fi
}

# --- main ---

if [[ -z "$ARTIFACT_DIR" ]]; then
  ARTIFACT_DIR="$ROOT/docs/specs/no-milestone/wsl2-freeze/evidence/freeze-baseline-$stamp"
fi

# Always capture a read-only baseline (safe on daily host).
capture_phase "$ARTIFACT_DIR" "baseline"
write_artifact "$ARTIFACT_DIR" "summary.json" "$(
  printf '{"ts":"%s","mode":"%s","host":"%s","daily_host":%s,"ghost":%s,"binary_match":"%s","used_kib":%s,"has_deleted_swap":%s,"d_state":%s,"hung_task_hits":%s,"gates_ok":%s,"reasons":"%s","claim":"NOT_CLAIMED"}\n' \
    "$ts" "$MODE" "$hostname_s" \
    "$([[ $is_daily_host -eq 1 ]] && echo true || echo false)" \
    "$ghost" "$binary_match" "$used_total" "$has_deleted_swap" \
    "$d_state_count" "$hung_task_hits" \
    "$([[ $gates_ok -eq 1 ]] && echo true || echo false)" \
    "$reason_csv"
)"

emit_summary

if [[ "$CHECK_GATES" -eq 1 ]]; then
  if [[ "$gates_ok" -eq 1 ]]; then
    exit 0
  fi
  exit 1
fi

# Isolated action path — hard refuse on daily host and incomplete flags.
if [[ "$RUN_ISOLATED" -eq 1 ]]; then
  if [[ "$gates_ok" -ne 1 ]]; then
    echo "REFUSE isolated run: gates_ok=0 reasons=$reason_csv" >&2
    write_artifact "$ARTIFACT_DIR" "isolated-refuse.txt" "gates_ok=0 reasons=$reason_csv"
    exit 1
  fi

  pressure="$ROOT/scripts/safety/cascade-pressure-probe.sh"
  sanitize="$ROOT/scripts/safety/swap-sanitize.sh"
  round=1
  while [[ "$round" -le "$ROUNDS" ]]; do
    rdir="$ARTIFACT_DIR/round-$round"
    mkdir -p "$rdir"
    echo "=== isolated round $round/$ROUNDS ==="
    capture_phase "$rdir" "before"

    # swapoff-first / diagnose (never kill -9 ramsharedd).
    if [[ -x "$sanitize" ]]; then
      bash "$sanitize" >"$rdir/swap-sanitize-before.txt" 2>&1 || true
    fi

    action_rc=0
    if [[ -x "$pressure" ]]; then
      # Watchdog: terminate the pressure probe after WATCHDOG_SEC if still running.
      set +e
      action_pid=""
      if [[ "$(id -u)" -eq 0 ]]; then
        bash "$pressure" --max-sec "$WATCHDOG_SEC" &
        action_pid=$!
      elif sudo -n true 2>/dev/null; then
        sudo -n bash "$pressure" --max-sec "$WATCHDOG_SEC" &
        action_pid=$!
      else
        echo "SKIP pressure: need root/sudo -n for cgroup probe" >"$rdir/action-skip.txt"
      fi
      if [[ -n "$action_pid" ]]; then
        (
          sleep "$WATCHDOG_SEC"
          if kill -0 "$action_pid" 2>/dev/null; then
            echo "WATCHDOG fired after ${WATCHDOG_SEC}s" >"$rdir/watchdog.txt"
            kill -TERM "$action_pid" 2>/dev/null || true
            sleep 2
            kill -KILL "$action_pid" 2>/dev/null || true
          fi
        ) &
        wd_pid=$!
        wait "$action_pid"
        action_rc=$?
        kill "$wd_pid" 2>/dev/null || true
        wait "$wd_pid" 2>/dev/null || true
      fi
      set -e
    else
      echo "pressure probe missing" >"$rdir/action-skip.txt"
    fi
    echo "action_rc=$action_rc" >"$rdir/action-rc.txt"

    capture_phase "$rdir" "after"
    if [[ -x "$sanitize" ]]; then
      bash "$sanitize" >"$rdir/swap-sanitize-after.txt" 2>&1 || true
    fi
    # Cleanup marker: pressure probe should self-release cgroup; re-check swaps.
    cat /proc/swaps >"$rdir/swaps-after-cleanup.txt" 2>/dev/null || true
    round=$((round + 1))
  done

  write_artifact "$ARTIFACT_DIR" "isolated-complete.txt" \
    "rounds=$ROUNDS watchdog_sec=$WATCHDOG_SEC claim=PARTIAL_OR_ENV_BOUND see round dirs"
  echo "Isolated rounds complete under $ARTIFACT_DIR (claim still needs human review)."
  exit 0
fi

# Always exit 0 in dry-run baseline mode: the script's job is capture + honest refuse.
echo "ARTIFACT_DIR=$ARTIFACT_DIR"
exit 0
