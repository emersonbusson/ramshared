#!/usr/bin/env bash
# Inspect and, only when explicitly requested, reap exact orphaned WSL Relay
# processes affected by microsoft/WSL#41242. This script never terminates WSL,
# restarts services, changes swap, or applies workload pressure.
set -euo pipefail

MODE=check
EXPECT_COUNT=
TEST_MODE=${WSL_RELAY_TEST_MODE:-0}

usage() {
  cat <<'USAGE'
Usage:
  wsl-relay-health.sh [--check]
  wsl-relay-health.sh --reap --expect-count N

--check is read-only and is the default. --reap performs one attended,
bounded pass over the exact candidate set and requires its observed count.
USAGE
}

while (($# > 0)); do
  case "$1" in
    --check)
      MODE=check
      shift
      ;;
    --reap)
      MODE=reap
      shift
      ;;
    --expect-count)
      (($# >= 2)) || { usage >&2; exit 2; }
      EXPECT_COUNT=$2
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$TEST_MODE" != 1 ]]; then
  for injected in \
    WSL_RELAY_PROC_ROOT WSL_RELAY_RUN_ROOT WSL_RELAY_UPTIME_FILE \
    WSL_RELAY_CLK_TCK WSL_RELAY_NOW_UTC WSL_RELAY_KILL_CMD \
    WSL_RELAY_TERM_GRACE_S WSL_RELAY_FINAL_GRACE_S \
    WSL_RELAY_BEFORE_TERM_HOOK WSL_RELAY_BEFORE_KILL_HOOK; do
    [[ ! -v "$injected" ]] || {
      printf 'REFUSED reason=test_override_in_production\n' >&2
      exit 2
    }
  done
fi

PROC_ROOT=${WSL_RELAY_PROC_ROOT:-/proc}
RUN_ROOT=${WSL_RELAY_RUN_ROOT:-/run/WSL}
UPTIME_FILE=${WSL_RELAY_UPTIME_FILE:-$PROC_ROOT/uptime}
CLK_TCK=${WSL_RELAY_CLK_TCK:-$(getconf CLK_TCK)}
MAX_CANDIDATES=${WSL_RELAY_MAX_CANDIDATES:-128}
MIN_AGE_S=600
TERM_GRACE_S=${WSL_RELAY_TERM_GRACE_S:-10}
FINAL_GRACE_S=${WSL_RELAY_FINAL_GRACE_S:-5}
KILL_CMD=${WSL_RELAY_KILL_CMD:-kill}
NOW_UTC=${WSL_RELAY_NOW_UTC:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}
BEFORE_TERM_HOOK=${WSL_RELAY_BEFORE_TERM_HOOK:-}
BEFORE_KILL_HOOK=${WSL_RELAY_BEFORE_KILL_HOOK:-}

[[ "$CLK_TCK" =~ ^[1-9][0-9]*$ ]] || {
  printf 'REFUSED reason=invalid_clk_tck\n' >&2
  exit 2
}
[[ "$MAX_CANDIDATES" =~ ^[1-9][0-9]*$ ]] && ((MAX_CANDIDATES <= 128)) || {
  printf 'REFUSED reason=invalid_candidate_cap\n' >&2
  exit 2
}
[[ "$TERM_GRACE_S" =~ ^[0-9]+$ ]] && ((TERM_GRACE_S <= 10)) || {
  printf 'REFUSED reason=invalid_term_grace\n' >&2
  exit 2
}
[[ "$FINAL_GRACE_S" =~ ^[0-9]+$ ]] && ((FINAL_GRACE_S <= 5)) || {
  printf 'REFUSED reason=invalid_final_grace\n' >&2
  exit 2
}

if [[ "$MODE" == reap ]]; then
  [[ "$EXPECT_COUNT" =~ ^[1-9][0-9]*$ ]] && ((EXPECT_COUNT <= MAX_CANDIDATES)) || {
    printf 'REFUSED reason=invalid_expected_count\n' >&2
    exit 2
  }
elif [[ -n "$EXPECT_COUNT" ]]; then
  printf 'REFUSED reason=expected_count_requires_reap\n' >&2
  exit 2
fi

declare -a CANDIDATE_PIDS=()
declare -a CANDIDATE_STARTS=()
declare -a CANDIDATE_AGES=()
INSPECTED=0
ERROR_REASON=
CLASSIFY_START=
CLASSIFY_AGE=

read_uptime_seconds() {
  local raw integer
  [[ -r "$UPTIME_FILE" ]] || return 1
  read -r raw _ <"$UPTIME_FILE" || return 1
  integer=${raw%%.*}
  [[ "$integer" =~ ^[0-9]+$ ]] || return 1
  printf '%s\n' "$integer"
}

classify_pid() {
  local pid=$1 base="$PROC_ROOT/$pid" comm size cmdline ppid children
  local -a stat_fields=()
  local start_ticks uptime_s age_s

  CLASSIFY_START=
  CLASSIFY_AGE=
  [[ "$pid" =~ ^[1-9][0-9]*$ ]] || return 2
  [[ -r "$base/comm" ]] || return 2
  IFS= read -r comm <"$base/comm" || return 2
  [[ "$comm" == Relay ]] || return 1

  for required in cmdline status "task/$pid/children" stat; do
    [[ -r "$base/$required" ]] || return 2
  done

  size=$(wc -c <"$base/cmdline") || return 2
  [[ "$size" =~ ^[0-9]+$ ]] || return 2
  cmdline=
  IFS= read -r -d '' cmdline <"$base/cmdline" || return 2
  [[ "$size" -eq 6 && "$cmdline" == /init ]] || return 1

  ppid=$(awk '$1 == "PPid:" { if (seen++) exit 3; print $2 } END { if (!seen) exit 4 }' \
    "$base/status") || return 2
  [[ "$ppid" =~ ^[0-9]+$ ]] || return 2
  [[ "$ppid" == 1 || "$ppid" == 2 ]] || return 1

  children=$(<"$base/task/$pid/children") || return 2
  [[ -z "${children//[[:space:]]/}" ]] || return 1
  [[ ! -e "$RUN_ROOT/${pid}_interop" ]] || return 1

  read -r -a stat_fields <"$base/stat" || return 2
  ((${#stat_fields[@]} >= 22)) || return 2
  [[ "${stat_fields[0]}" == "$pid" && "${stat_fields[1]}" == '(Relay)' ]] || return 2
  start_ticks=${stat_fields[21]}
  [[ "$start_ticks" =~ ^[0-9]+$ ]] || return 2
  uptime_s=$(read_uptime_seconds) || return 2
  ((uptime_s >= start_ticks / CLK_TCK)) || return 2
  age_s=$((uptime_s - start_ticks / CLK_TCK))
  ((age_s >= MIN_AGE_S)) || return 1

  CLASSIFY_START=$start_ticks
  CLASSIFY_AGE=$age_s
  return 0
}

discover_candidates() {
  local comm_path pid rc record
  local -a records=()
  CANDIDATE_PIDS=()
  CANDIDATE_STARTS=()
  CANDIDATE_AGES=()
  INSPECTED=0
  ERROR_REASON=

  shopt -s nullglob
  for comm_path in "$PROC_ROOT"/[0-9]*/comm; do
    pid=${comm_path%/comm}
    pid=${pid##*/}
    if [[ -r "$comm_path" ]] && [[ "$(<"$comm_path")" == Relay ]]; then
      INSPECTED=$((INSPECTED + 1))
    fi
    if classify_pid "$pid"; then
      rc=0
    else
      rc=$?
    fi
    case "$rc" in
      0)
        records+=("$pid:$CLASSIFY_START:$CLASSIFY_AGE")
        if ((${#records[@]} > MAX_CANDIDATES)); then
          ERROR_REASON=candidate_cap_exceeded
          return 2
        fi
        ;;
      1) ;;
      2)
        if [[ -r "$comm_path" ]] && [[ "$(<"$comm_path")" == Relay ]]; then
          ERROR_REASON=malformed_relay_state
          return 2
        fi
        ;;
    esac
  done
  shopt -u nullglob

  if ((${#records[@]} > 0)); then
    mapfile -t records < <(printf '%s\n' "${records[@]}" | sort -n -t: -k1,1)
    for record in "${records[@]}"; do
      IFS=: read -r pid CLASSIFY_START CLASSIFY_AGE <<<"$record"
      CANDIDATE_PIDS+=("$pid")
      CANDIDATE_STARTS+=("$CLASSIFY_START")
      CANDIDATE_AGES+=("$CLASSIFY_AGE")
    done
  fi
}

candidate_json() {
  local first=1 index
  printf '['
  for index in "${!CANDIDATE_PIDS[@]}"; do
    ((first == 1)) || printf ','
    first=0
    printf '{"pid":%s,"age_seconds":%s}' \
      "${CANDIDATE_PIDS[$index]}" "${CANDIDATE_AGES[$index]}"
  done
  printf ']'
}

emit_json() {
  local reason=$1 verdict=$2 term_survivors=$3 kill_survivors=$4 duration_ms=$5
  printf '{"schema_version":1,"timestamp_utc":"%s","mode":"%s","inspected":%s,"candidates":%s,"candidate_count":%s,"term_survivors":%s,"kill_survivors":%s,"duration_ms":%s,"reason":"%s","verdict":"%s"}\n' \
    "$NOW_UTC" "$MODE" "$INSPECTED" "$(candidate_json)" \
    "${#CANDIDATE_PIDS[@]}" "$term_survivors" "$kill_survivors" \
    "$duration_ms" "$reason" "$verdict"
}

revalidate_snapshot() {
  local index pid rc
  for index in "${!CANDIDATE_PIDS[@]}"; do
    pid=${CANDIDATE_PIDS[$index]}
    if classify_pid "$pid"; then
      rc=0
    else
      rc=$?
    fi
    [[ "$rc" -eq 0 && "$CLASSIFY_START" == "${CANDIDATE_STARTS[$index]}" ]] || return 1
  done
}

run_test_hook() {
  local hook=$1
  [[ -z "$hook" ]] && return 0
  [[ "$TEST_MODE" == 1 && -x "$hook" ]] || return 1
  "$hook"
}

signal_exact() {
  local signal=$1
  shift
  (($# > 0)) || return 0
  local pid
  for pid in "$@"; do
    [[ "$pid" =~ ^[1-9][0-9]*$ ]] || return 1
  done
  "$KILL_CMD" "-$signal" "$@"
}

start_ms=$(date +%s%3N)
if discover_candidates; then
  discover_rc=0
else
  discover_rc=$?
fi
if [[ "$discover_rc" -eq 2 ]]; then
  emit_json "${ERROR_REASON:-discovery_failed}" REFUSED 0 0 0
  exit 2
fi

if [[ "$MODE" == check ]]; then
  if ((${#CANDIDATE_PIDS[@]} == 0)); then
    emit_json clean CLEAN 0 0 0
    exit 0
  fi
  emit_json attention_required ATTENTION 0 0 0
  exit 1
fi

if ((${#CANDIDATE_PIDS[@]} != EXPECT_COUNT)); then
  emit_json expected_count_mismatch REFUSED 0 0 0
  exit 2
fi

run_test_hook "$BEFORE_TERM_HOOK" || {
  emit_json before_term_hook_failed REFUSED 0 0 0
  exit 2
}
revalidate_snapshot || {
  emit_json identity_changed_before_term REFUSED 0 0 0
  exit 2
}
signal_exact TERM "${CANDIDATE_PIDS[@]}" || {
  emit_json term_signal_failed REFUSED 0 0 0
  exit 2
}

for ((waited = 0; waited < TERM_GRACE_S; waited++)); do
  any_alive=0
  for pid in "${CANDIDATE_PIDS[@]}"; do
    [[ ! -d "$PROC_ROOT/$pid" ]] || any_alive=1
  done
  ((any_alive == 1)) || break
  sleep 1
done

declare -a SURVIVOR_PIDS=()
declare -a SURVIVOR_STARTS=()
for index in "${!CANDIDATE_PIDS[@]}"; do
  pid=${CANDIDATE_PIDS[$index]}
  if [[ -d "$PROC_ROOT/$pid" ]]; then
    SURVIVOR_PIDS+=("$pid")
    SURVIVOR_STARTS+=("${CANDIDATE_STARTS[$index]}")
  fi
done
term_survivors=${#SURVIVOR_PIDS[@]}

run_test_hook "$BEFORE_KILL_HOOK" || {
  emit_json before_kill_hook_failed REFUSED "$term_survivors" 0 0
  exit 2
}
for index in "${!SURVIVOR_PIDS[@]}"; do
  pid=${SURVIVOR_PIDS[$index]}
  if classify_pid "$pid"; then
    rc=0
  else
    rc=$?
  fi
  if [[ "$rc" -ne 0 || "$CLASSIFY_START" != "${SURVIVOR_STARTS[$index]}" ]]; then
    emit_json identity_changed_before_kill REFUSED "$term_survivors" 0 0
    exit 2
  fi
done

if ((term_survivors > 0)); then
  signal_exact KILL "${SURVIVOR_PIDS[@]}" || {
    emit_json kill_signal_failed REFUSED "$term_survivors" "$term_survivors" 0
    exit 2
  }
fi

for ((waited = 0; waited < FINAL_GRACE_S; waited++)); do
  any_alive=0
  for pid in "${SURVIVOR_PIDS[@]}"; do
    [[ ! -d "$PROC_ROOT/$pid" ]] || any_alive=1
  done
  ((any_alive == 1)) || break
  sleep 1
done

kill_survivors=0
for pid in "${SURVIVOR_PIDS[@]}"; do
  [[ ! -d "$PROC_ROOT/$pid" ]] || kill_survivors=$((kill_survivors + 1))
done

if discover_candidates; then
  post_rc=0
else
  post_rc=$?
fi
end_ms=$(date +%s%3N)
duration_ms=$((end_ms - start_ms))
if [[ "$post_rc" -eq 2 ]]; then
  emit_json "${ERROR_REASON:-post_discovery_failed}" REFUSED "$term_survivors" "$kill_survivors" "$duration_ms"
  exit 2
fi
if ((kill_survivors > 0 || ${#CANDIDATE_PIDS[@]} > 0 || duration_ms > 20000)); then
  emit_json cleanup_incomplete FAILED "$term_survivors" "$kill_survivors" "$duration_ms"
  exit 1
fi
emit_json reaped CLEAN "$term_survivors" 0 "$duration_ms"
