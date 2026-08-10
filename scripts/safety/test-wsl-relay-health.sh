#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
PRODUCT="$REPO_ROOT/scripts/safety/wsl-relay-health.sh"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf -- "$TEST_ROOT"' EXIT

pass_count=0

pass() {
  printf 'PASS %s\n' "$1"
  pass_count=$((pass_count + 1))
}

fail() {
  printf 'FAIL %s: %s\n' "$1" "$2" >&2
  exit 1
}

assert_contains() {
  local name=$1 haystack=$2 needle=$3
  [[ "$haystack" == *"$needle"* ]] || fail "$name" "missing expected marker: $needle"
}

new_fixture() {
  local name=$1
  local root="$TEST_ROOT/$name"
  mkdir -p "$root/proc" "$root/run"
  printf '10000.00 0.00\n' >"$root/uptime"
  printf '%s\n' "$root"
}

write_process() {
  local root=$1 pid=$2 comm=$3 cmd=$4 ppid=$5 start_ticks=$6 children=${7:-}
  mkdir -p "$root/proc/$pid/task/$pid"
  printf '%s\n' "$comm" >"$root/proc/$pid/comm"
  printf '%s\0' "$cmd" >"$root/proc/$pid/cmdline"
  printf 'Name:\t%s\nState:\tS (sleeping)\nPPid:\t%s\n' "$comm" "$ppid" >"$root/proc/$pid/status"
  printf '%s\n' "$children" >"$root/proc/$pid/task/$pid/children"
  printf '%s (%s) S %s 0 0 0 0 0 0 0 0 0 0 0 0 0 20 0 1 0 %s 0 0 0\n' \
    "$pid" "$comm" "$ppid" "$start_ticks" >"$root/proc/$pid/stat"
}

run_product() {
  local root=$1
  shift
  set +e
  RUN_OUTPUT=$(env \
    WSL_RELAY_TEST_MODE=1 \
    WSL_RELAY_PROC_ROOT="$root/proc" \
    WSL_RELAY_RUN_ROOT="$root/run" \
    WSL_RELAY_UPTIME_FILE="$root/uptime" \
    WSL_RELAY_CLK_TCK=100 \
    WSL_RELAY_NOW_UTC=2026-08-09T15:00:00Z \
    "$PRODUCT" "$@" 2>"$root/stderr")
  RUN_EXIT=$?
  set -e
}

test_clean_fixture_returns_zero() {
  local root
  root=$(new_fixture clean)
  run_product "$root" --check
  [[ $RUN_EXIT -eq 0 ]] || fail clean_fixture_returns_zero "exit=$RUN_EXIT"
  assert_contains clean_fixture_returns_zero "$RUN_OUTPUT" '"candidate_count":0'
  assert_contains clean_fixture_returns_zero "$RUN_OUTPUT" '"verdict":"CLEAN"'
  pass clean_fixture_returns_zero
}

test_exact_orphan_fixture_requires_attention() {
  local root
  root=$(new_fixture orphan)
  write_process "$root" 41 Relay /init 1 1000 ''
  run_product "$root" --check
  [[ $RUN_EXIT -eq 1 ]] || fail exact_orphan_fixture_requires_attention "exit=$RUN_EXIT"
  assert_contains exact_orphan_fixture_requires_attention "$RUN_OUTPUT" '"candidate_count":1'
  assert_contains exact_orphan_fixture_requires_attention "$RUN_OUTPUT" '"pid":41'
  pass exact_orphan_fixture_requires_attention
}

test_non_candidates_are_not_actionable() {
  local root
  root=$(new_fixture refusals)
  write_process "$root" 42 Relay /init 99 1000 ''
  write_process "$root" 43 Relay /init 1 1000 '999'
  write_process "$root" 44 Relay /init 1 999900 ''
  write_process "$root" 45 SessionLeader /init 1 1000 ''
  write_process "$root" 46 Relay /init 1 1000 ''
  : >"$root/run/46_interop"
  run_product "$root" --check
  [[ $RUN_EXIT -eq 0 ]] || fail non_candidates_are_not_actionable "exit=$RUN_EXIT"
  assert_contains non_candidates_are_not_actionable "$RUN_OUTPUT" '"candidate_count":0'
  pass live_parent_is_refused
  pass child_process_is_refused
  pass interop_socket_is_refused
  pass young_process_is_refused
}

test_malformed_proc_state_fails_closed() {
  local root
  root=$(new_fixture malformed)
  write_process "$root" 50 Relay /init 1 1000 ''
  printf 'malformed\n' >"$root/proc/50/stat"
  run_product "$root" --check
  [[ $RUN_EXIT -eq 2 ]] || fail malformed_proc_state_fails_closed "exit=$RUN_EXIT"
  assert_contains malformed_proc_state_fails_closed "$RUN_OUTPUT" '"verdict":"REFUSED"'
  pass malformed_proc_state_fails_closed
}

make_kill_mock() {
  local root=$1
  local mock="$root/mock-kill.sh"
  cat >"$mock" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$WSL_RELAY_SIGNAL_LOG"
signal=$1
shift
if [[ "$signal" == "-KILL" && "${WSL_RELAY_MOCK_STICKY:-0}" != 1 ]]; then
  for pid in "$@"; do
    rm -rf -- "$WSL_RELAY_PROC_ROOT/$pid"
  done
fi
MOCK
  chmod +x "$mock"
  printf '%s\n' "$mock"
}

test_reap_requires_exact_expected_count() {
  local root mock
  root=$(new_fixture count)
  write_process "$root" 60 Relay /init 1 1000 ''
  mock=$(make_kill_mock "$root")
  : >"$root/signals"
  set +e
  RUN_OUTPUT=$(env WSL_RELAY_TEST_MODE=1 WSL_RELAY_PROC_ROOT="$root/proc" \
    WSL_RELAY_RUN_ROOT="$root/run" WSL_RELAY_UPTIME_FILE="$root/uptime" \
    WSL_RELAY_CLK_TCK=100 WSL_RELAY_KILL_CMD="$mock" \
    WSL_RELAY_SIGNAL_LOG="$root/signals" "$PRODUCT" --reap --expect-count 2)
  RUN_EXIT=$?
  set -e
  [[ $RUN_EXIT -eq 2 ]] || fail reap_requires_exact_expected_count "exit=$RUN_EXIT"
  [[ ! -s "$root/signals" ]] || fail reap_requires_exact_expected_count 'signal sent'
  pass reap_requires_exact_expected_count
}

test_reap_signals_only_sealed_numeric_pids() {
  local root mock
  root=$(new_fixture reap)
  write_process "$root" 71 Relay /init 1 1000 ''
  write_process "$root" 72 Relay /init 2 2000 ''
  mock=$(make_kill_mock "$root")
  : >"$root/signals"
  set +e
  RUN_OUTPUT=$(env WSL_RELAY_TEST_MODE=1 WSL_RELAY_PROC_ROOT="$root/proc" \
    WSL_RELAY_RUN_ROOT="$root/run" WSL_RELAY_UPTIME_FILE="$root/uptime" \
    WSL_RELAY_CLK_TCK=100 WSL_RELAY_KILL_CMD="$mock" \
    WSL_RELAY_SIGNAL_LOG="$root/signals" WSL_RELAY_TERM_GRACE_S=1 \
    WSL_RELAY_FINAL_GRACE_S=1 WSL_RELAY_NOW_UTC=2026-08-09T15:00:00Z \
    "$PRODUCT" --reap --expect-count 2)
  RUN_EXIT=$?
  set -e
  [[ $RUN_EXIT -eq 0 ]] || fail reap_signals_only_sealed_numeric_pids "exit=$RUN_EXIT output=$RUN_OUTPUT"
  local signals
  signals=$(<"$root/signals")
  assert_contains reap_signals_only_sealed_numeric_pids "$signals" '-TERM 71 72'
  assert_contains reap_signals_only_sealed_numeric_pids "$signals" '-KILL 71 72'
  assert_contains reap_signals_only_sealed_numeric_pids "$RUN_OUTPUT" '"candidate_count":0'
  pass reap_signals_only_sealed_numeric_pids
}

test_reap_deadline_is_bounded() {
  local root mock started ended elapsed_ms
  root=$(new_fixture deadline)
  write_process "$root" 73 Relay /init 1 1000 ''
  mock=$(make_kill_mock "$root")
  : >"$root/signals"
  started=$(date +%s%3N)
  set +e
  RUN_OUTPUT=$(env WSL_RELAY_TEST_MODE=1 WSL_RELAY_PROC_ROOT="$root/proc" \
    WSL_RELAY_RUN_ROOT="$root/run" WSL_RELAY_UPTIME_FILE="$root/uptime" \
    WSL_RELAY_CLK_TCK=100 WSL_RELAY_KILL_CMD="$mock" \
    WSL_RELAY_SIGNAL_LOG="$root/signals" WSL_RELAY_MOCK_STICKY=1 \
    WSL_RELAY_TERM_GRACE_S=1 WSL_RELAY_FINAL_GRACE_S=1 \
    "$PRODUCT" --reap --expect-count 1)
  RUN_EXIT=$?
  set -e
  ended=$(date +%s%3N)
  elapsed_ms=$((ended - started))
  [[ $RUN_EXIT -eq 1 ]] || fail reap_deadline_is_bounded "exit=$RUN_EXIT"
  ((elapsed_ms < 4000)) || fail reap_deadline_is_bounded "elapsed_ms=$elapsed_ms"
  assert_contains reap_deadline_is_bounded "$RUN_OUTPUT" '"verdict":"FAILED"'
  pass reap_deadline_is_bounded
}

make_stat_rewrite_hook() {
  local root=$1 pid=$2 start=$3
  local hook="$root/rewrite-$pid-$start.sh"
  cat >"$hook" <<HOOK
#!/usr/bin/env bash
set -euo pipefail
printf '%s (%s) S 1 0 0 0 0 0 0 0 0 0 0 0 0 0 20 0 1 0 %s 0 0 0\n' \
  '$pid' 'Relay' '$start' >'$root/proc/$pid/stat'
HOOK
  chmod +x "$hook"
  printf '%s\n' "$hook"
}

test_pid_reuse_before_term_is_refused() {
  local root mock hook
  root=$(new_fixture reuse-term)
  write_process "$root" 81 Relay /init 1 1000 ''
  mock=$(make_kill_mock "$root")
  hook=$(make_stat_rewrite_hook "$root" 81 2000)
  : >"$root/signals"
  set +e
  RUN_OUTPUT=$(env WSL_RELAY_TEST_MODE=1 WSL_RELAY_PROC_ROOT="$root/proc" \
    WSL_RELAY_RUN_ROOT="$root/run" WSL_RELAY_UPTIME_FILE="$root/uptime" \
    WSL_RELAY_CLK_TCK=100 WSL_RELAY_KILL_CMD="$mock" \
    WSL_RELAY_SIGNAL_LOG="$root/signals" WSL_RELAY_BEFORE_TERM_HOOK="$hook" \
    "$PRODUCT" --reap --expect-count 1)
  RUN_EXIT=$?
  set -e
  [[ $RUN_EXIT -eq 2 ]] || fail pid_reuse_before_term_is_refused "exit=$RUN_EXIT"
  [[ ! -s "$root/signals" ]] || fail pid_reuse_before_term_is_refused 'signal sent'
  pass pid_reuse_before_term_is_refused
}

test_pid_reuse_before_kill_is_refused() {
  local root mock hook
  root=$(new_fixture reuse-kill)
  write_process "$root" 82 Relay /init 1 1000 ''
  mock=$(make_kill_mock "$root")
  hook=$(make_stat_rewrite_hook "$root" 82 2000)
  : >"$root/signals"
  set +e
  RUN_OUTPUT=$(env WSL_RELAY_TEST_MODE=1 WSL_RELAY_PROC_ROOT="$root/proc" \
    WSL_RELAY_RUN_ROOT="$root/run" WSL_RELAY_UPTIME_FILE="$root/uptime" \
    WSL_RELAY_CLK_TCK=100 WSL_RELAY_KILL_CMD="$mock" \
    WSL_RELAY_SIGNAL_LOG="$root/signals" WSL_RELAY_BEFORE_KILL_HOOK="$hook" \
    WSL_RELAY_TERM_GRACE_S=1 "$PRODUCT" --reap --expect-count 1)
  RUN_EXIT=$?
  set -e
  [[ $RUN_EXIT -eq 2 ]] || fail pid_reuse_before_kill_is_refused "exit=$RUN_EXIT"
  local signals
  signals=$(<"$root/signals")
  assert_contains pid_reuse_before_kill_is_refused "$signals" '-TERM 82'
  [[ "$signals" != *'-KILL'* ]] || fail pid_reuse_before_kill_is_refused 'KILL sent'
  pass pid_reuse_before_kill_is_refused
}

test_candidate_cap_is_enforced() {
  local root pid
  root=$(new_fixture cap)
  for pid in $(seq 1000 1128); do
    write_process "$root" "$pid" Relay /init 1 1000 ''
  done
  run_product "$root" --check
  [[ $RUN_EXIT -eq 2 ]] || fail candidate_cap_is_enforced "exit=$RUN_EXIT"
  assert_contains candidate_cap_is_enforced "$RUN_OUTPUT" '"reason":"candidate_cap_exceeded"'
  pass candidate_cap_is_enforced
}

test_output_is_sanitized_and_deterministic() {
  local root first second
  root=$(new_fixture deterministic)
  write_process "$root" 91 Relay /init 1 1000 ''
  run_product "$root" --check
  first=$RUN_OUTPUT
  run_product "$root" --check
  second=$RUN_OUTPUT
  [[ "$first" = "$second" ]] || fail output_is_sanitized_and_deterministic 'output changed'
  [[ "$first" != *"$root"* ]] || fail output_is_sanitized_and_deterministic 'fixture path leaked'
  [[ "$first" != *'cmdline'* ]] || fail output_is_sanitized_and_deterministic 'command field leaked'
  pass output_is_sanitized_and_deterministic
}

[[ -x "$PRODUCT" ]] || fail production_script_present "$PRODUCT is missing or not executable"

test_clean_fixture_returns_zero
test_exact_orphan_fixture_requires_attention
test_non_candidates_are_not_actionable
test_malformed_proc_state_fails_closed
test_reap_requires_exact_expected_count
test_reap_signals_only_sealed_numeric_pids
test_reap_deadline_is_bounded
test_pid_reuse_before_term_is_refused
test_pid_reuse_before_kill_is_refused
test_candidate_cap_is_enforced
test_output_is_sanitized_and_deterministic

printf 'PASS Test-WslRelayHealth total=%s\n' "$pass_count"
