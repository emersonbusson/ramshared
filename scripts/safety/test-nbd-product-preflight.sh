#!/usr/bin/env bash
# Manufactured contract tests for the read-only NBD product preflight.
set -euo pipefail

REPO_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
PRODUCT="$REPO_ROOT/scripts/safety/nbd-product-preflight.sh"
TEST_ROOT=$(mktemp -d)

cleanup_test_root() {
  if [[ -d ${TEST_ROOT:-} ]]; then
    find "$TEST_ROOT" -depth -type d -exec chmod u+rwx {} + 2>/dev/null || true
    rm -rf -- "$TEST_ROOT"
  fi
}
trap cleanup_test_root EXIT

pass_count=0
fail_count=0
RUN_EXIT=0
RUN_OUTPUT=''
readonly ROLLBACK_POST_WRITE_PHASES=(
  release-roots-prepared
  staging-directory-created
  release-copied
  input-bundle-manifest-copied
  lower-sink-bound
  installed-provenance-recorded
  installed-manifest-regenerated
  installed-manifest-receipt-recorded
  staging-owner-normalized
  staging-directories-sealed
  staging-executables-sealed
  staging-files-sealed
  staging-manifest-verified
  staging-seal-verified
  destination-published
  unit-staged
  unit-linked
  unit-staging-removed
  health-unit-installed
  workloads-slice-installed
  legacy-unit-backed-up
  legacy-unit-staged
  legacy-unit-replaced
  selector-staged
  selector-owner-normalized
  selector-published
  daemon-reloaded
)

fail() {
  fail_count=$((fail_count + 1))
  printf 'FAIL %s\n' "$*" >&2
}

pass() {
  pass_count=$((pass_count + 1))
  printf 'PASS %s\n' "$*"
}

assert_exit() {
  local name=$1 expected=$2
  if [[ $RUN_EXIT -ne $expected ]]; then
    fail "$name exit=$RUN_EXIT expected=$expected output=$RUN_OUTPUT"
    return 1
  fi
}

assert_contains() {
  local name=$1 needle=$2
  if [[ $RUN_OUTPUT != *"$needle"* ]]; then
    fail "$name missing=$needle output=$RUN_OUTPUT"
    return 1
  fi
}

write_manifest() {
  local release=$1
  (
    cd "$release"
    find . -type f ! -name SHA256SUMS ! -name INSTALLED_MANIFEST_SHA256 -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
  )
  chmod 0644 "$release/SHA256SUMS"
}

write_generic_config() {
  local release=$1
  printf '%s\n' \
    'NBD_LOWER_SINK=' \
    'NBD_LOWER_SINK_TYPE=directory' \
    'NBD_LOWER_SINK_IDENTITY_SHA256=' \
    'NBD_LOWER_SINK_FS_BLOCK_BYTES=' \
    'NBD_LOWER_SINK_BINDING=unbound' \
    >"$release/scripts/safety/cascade.conf.example"
  chmod 0644 "$release/scripts/safety/cascade.conf.example"
}

write_bound_config() {
  local release=$1 sink=$2 canonical metadata identity fs_block
  canonical=$(readlink -f -- "$sink")
  metadata=$(stat -c '%d:%i:%u:%g:%a:%F' -- "$canonical")
  identity=$(printf '%s\0%s\0%s' "$canonical" directory "$metadata" | sha256sum | awk '{print $1}')
  fs_block=$(stat -fc '%s' -- "$canonical")
  printf '%s\n' \
    "NBD_LOWER_SINK=$canonical" \
    'NBD_LOWER_SINK_TYPE=directory' \
    "NBD_LOWER_SINK_IDENTITY_SHA256=$identity" \
    "NBD_LOWER_SINK_FS_BLOCK_BYTES=$fs_block" \
    'NBD_LOWER_SINK_BINDING=bound' \
    >"$release/scripts/safety/cascade.conf.example"
  chmod 0644 "$release/scripts/safety/cascade.conf.example"
}

write_installed_provenance() {
  local release=$1 input_digest=$2 sink=$3 canonical metadata identity fs_block available
  canonical=$(readlink -f -- "$sink")
  metadata=$(stat -c '%d:%i:%u:%g:%a:%F' -- "$canonical")
  identity=$(printf '%s\0%s\0%s' "$canonical" directory "$metadata" | sha256sum | awk '{print $1}')
  fs_block=$(stat -fc '%s' -- "$canonical")
  available=$(df -Pk -- "$canonical" | awk 'NR == 2 { print $4 }')
  python3 - "$release/INSTALL_PROVENANCE.json" "$input_digest" \
    "$(tr -d '[:space:]' <"$release/SOURCE_COMMIT")" \
    "$(tr -d '[:space:]' <"$release/SOURCE_BRANCH")" \
    "$(tr -d '[:space:]' <"$release/SOURCE_TREE_STATE")" \
    "$canonical" "$identity" "$fs_block" "$available" <<'PY'
import json
import os
import sys

out, digest, commit, branch, tree_state, sink, identity, block, available = sys.argv[1:]
record = {
    "schema_version": "ramshared-installed-release-provenance/v1",
    "input_bundle_manifest_sha256": digest,
    "source_commit": commit,
    "source_branch": branch,
    "source_tree_state": tree_state,
    "lower_sink": {
        "canonical_path": sink,
        "identity_sha256": identity,
        "filesystem_block_bytes": int(block),
        "available_kib_at_bind": int(available),
    },
}
with open(out, "w", encoding="utf-8") as target:
    json.dump(record, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
PY
  chmod 0644 "$release/INSTALL_PROVENANCE.json"
}

make_generic_bundle_fixture() {
  local release=$1
  rm -f -- "$release/INSTALL_PROVENANCE.json" "$release/INPUT_BUNDLE_SHA256SUMS" "$release/INSTALLED_MANIFEST_SHA256"
  write_generic_config "$release"
  write_manifest "$release"
}

make_installed_release_fixture() {
  local root=$1 release=$2 input_digest
  make_generic_bundle_fixture "$release"
  cp -- "$release/SHA256SUMS" "$release/INPUT_BUNDLE_SHA256SUMS"
  chmod 0644 "$release/INPUT_BUNDLE_SHA256SUMS"
  input_digest=$(sha256sum -- "$release/INPUT_BUNDLE_SHA256SUMS" | awk '{print $1}')
  write_bound_config "$release" "$root/sink"
  write_installed_provenance "$release" "$input_digest" "$root/sink"
  write_manifest "$release"
  sha256sum -- "$release/SHA256SUMS" | awk '{print $1}' >"$release/INSTALLED_MANIFEST_SHA256"
  chmod 0644 "$release/INSTALLED_MANIFEST_SHA256"
}

seal_fixture_release() {
  local release=$1
  find "$release" -type d -exec chmod 0555 {} +
  find "$release" -type f -perm /111 -exec chmod 0555 {} +
  find "$release" -type f ! -perm /111 -exec chmod 0444 {} +
}

unseal_fixture_release() {
  local release=$1
  find "$release" -type d -exec chmod 0755 {} +
  find "$release" -type f -perm /111 -exec chmod 0755 {} +
  find "$release" -type f ! -perm /111 -exec chmod 0644 {} +
}

new_fixture() {
  local name=$1
  local root="$TEST_ROOT/$name"
  local product_root="$root/opt/ramshared"
  local release="$product_root/releases/v1.2.3"

  mkdir -p \
    "$release/bin" \
    "$release/scripts/safety" \
    "$release/systemd" \
    "$root/proc" \
    "$root/run" \
    "$root/sys/block" \
    "$root/sys/module/ublk_drv" \
    "$root/dev" \
    "$root/bin" \
    "$root/sink" \
    "$root/state"
  chmod 0755 "$product_root" "$product_root/releases" "$release" \
    "$release/bin" "$release/scripts" "$release/scripts/safety" "$release/systemd"

  printf '#!/usr/bin/env bash\nexit 0\n' >"$release/bin/ramshared"
  printf '#!/usr/bin/env bash\nexit 0\n' >"$release/bin/ramsharedd"
  chmod 0755 "$release/bin/ramshared" "$release/bin/ramsharedd"
  for script in cascade-up.sh cascade-down.sh cascade-health.sh install-cascade-boot.sh \
    uninstall-cascade-boot.sh nbd-benchmark-cell.sh \
    nbd-benchmark-cgroup-launch.sh cascade_pressure_integrity_worker.py wsl-relay-health.sh; do
    printf '#!/usr/bin/env bash\nexit 0\n' >"$release/scripts/safety/$script"
    chmod 0755 "$release/scripts/safety/$script"
  done
  printf '# benchmark library fixture\n' >"$release/scripts/safety/nbd-benchmark-lib.sh"
  chmod 0644 "$release/scripts/safety/nbd-benchmark-lib.sh"
  if [[ -f "$PRODUCT" ]]; then
    install -m 0755 "$PRODUCT" "$release/scripts/safety/nbd-product-preflight.sh"
  else
    printf '#!/usr/bin/env bash\nexit 0\n' >"$release/scripts/safety/nbd-product-preflight.sh"
    chmod 0755 "$release/scripts/safety/nbd-product-preflight.sh"
  fi
  printf '[Service]\n' >"$release/systemd/ramshared-cascade.service"
  printf '[Service]\n' >"$release/systemd/ramshared-cascade-health.service"
  printf '[Slice]\nMemoryAccounting=yes\n' >"$release/systemd/ramshared-workloads.slice"
  chmod 0644 "$release/systemd/ramshared-cascade.service"
  chmod 0644 "$release/systemd/ramshared-cascade-health.service"
  chmod 0644 "$release/systemd/ramshared-workloads.slice"
  write_generic_config "$release"
  printf '0123456789abcdef0123456789abcdef01234567\n' >"$release/SOURCE_COMMIT"
  printf 'fixture/main\n' >"$release/SOURCE_BRANCH"
  printf 'clean\n' >"$release/SOURCE_TREE_STATE"
  chmod 0644 "$release/SOURCE_COMMIT" "$release/SOURCE_BRANCH" "$release/SOURCE_TREE_STATE"
  make_installed_release_fixture "$root" "$release"
  seal_fixture_release "$release"
  ln -s releases/v1.2.3 "$product_root/current"

  printf 'Filename\tType\tSize\tUsed\tPriority\n' >"$root/proc/swaps"
  printf 'ublk_drv 0 0 - Live 0x0\n' >"$root/proc/modules"

  cat >"$root/bin/relay" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >"${RAMSHARED_RELAY_ARGS:?}"
if [[ -f "${RAMSHARED_RELAY_FAIL:?}" ]]; then
  exit 1
fi
printf '{"status":"ok"}\n'
EOF
  cat >"$root/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == is-active && "${2:-}" == --quiet && "${3:-}" == ramsharedd.service ]]; then
  [[ -f "${RAMSHARED_SYSTEMCTL_STATE:?}/legacy-active" ]] && exit 0
  exit 3
fi
if [[ "${1:-}" == is-enabled && "${2:-}" == --quiet && "${3:-}" == ramsharedd.service ]]; then
  [[ -f "${RAMSHARED_SYSTEMCTL_STATE:?}/legacy-enabled" ]] && exit 0
  exit 1
fi
exit 0
EOF
  cat >"$root/bin/df" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
available=${RAMSHARED_NBD_DF_AVAILABLE_KIB:-3000000}
mount_point=${RAMSHARED_NBD_DF_MOUNT_POINT:?}
printf 'Filesystem 1024-blocks Used Available Capacity Mounted on\n'
printf '/dev/lower 4000000 0 %s 0%% %s\n' "$available" "$mount_point"
if [[ ${RAMSHARED_NBD_DF_EXTRA_RECORD:-0} == 1 ]]; then
  printf '/dev/other 4000000 0 %s 0%% %s\n' "$available" "$mount_point"
fi
EOF
  cat >"$root/bin/stat" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == -fc && "${2:-}" == %s ]]; then
  printf '%s\n' "${RAMSHARED_NBD_STAT_ALIGNMENT_BYTES:-4096}"
  exit 0
fi
if [[ "${1:-}" == -c && "${2:-}" == %u:%g:%a && "${4:-}" == "${RAMSHARED_NBD_SELECTOR_PATH:-}" && -n "${RAMSHARED_NBD_SELECTOR_OWNER_MODE:-}" ]]; then
  printf '%s\n' "$RAMSHARED_NBD_SELECTOR_OWNER_MODE"
  exit 0
fi
exec /usr/bin/stat "$@"
EOF
  cat >"$root/bin/readlink" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ ${RAMSHARED_NBD_ALLOW_MANUFACTURED_PROC_TEST:-} == 1 &&
  ${RAMSHARED_NBD_TEST_EMPTY_EXE_READLINK_PATH:-} == "${!#}" &&
  ${1:-} == -- ]]; then
  if [[ -n ${RAMSHARED_NBD_TEST_EMPTY_EXE_READLINK_HIT:-} ]]; then
    : >"$RAMSHARED_NBD_TEST_EMPTY_EXE_READLINK_HIT"
  fi
  exit 0
fi
exec /usr/bin/readlink "$@"
EOF
  chmod 0755 "$root/bin/relay" "$root/bin/systemctl" "$root/bin/df" "$root/bin/stat" "$root/bin/readlink"
  printf '%s\n' "$root"
}

activate_nbd_fixture() {
  local root=$1 pid=4242
  local release="$root/opt/ramshared/releases/v1.2.3"
  mkdir -p "$root/proc/$pid"
  ln -s "$release/bin/ramsharedd" "$root/proc/$pid/exe"
  printf '%s\n' "$pid" >"$root/run/ramsharedd.pid"
  printf 'Filename\tType\tSize\tUsed\tPriority\n%s\tpartition\t1048576\t0\t100\n' "$root/dev/nbd0" \
    >"$root/proc/swaps"
  : >"$root/dev/nbd0"
}

add_exact_daemon_fixture_process() {
  local root=$1 pid=$2
  local release="$root/opt/ramshared/releases/v1.2.3"
  [[ $pid =~ ^[1-9][0-9]*$ ]] || return 1
  mkdir -p "$root/proc/$pid"
  ln -s "$release/bin/ramsharedd" "$root/proc/$pid/exe"
}

clone_installed_release_fixture() {
  local root=$1 version=$2
  local source="$root/opt/ramshared/releases/v1.2.3"
  local target="$root/opt/ramshared/releases/$version"
  [[ $version =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]] || return 1
  cp -a -- "$source" "$target"
}

add_installed_release_daemon_fixture_process() {
  local root=$1 version=$2 pid=$3
  local release="$root/opt/ramshared/releases/$version"
  [[ $pid =~ ^[1-9][0-9]*$ && -x $release/bin/ramsharedd ]] || return 1
  mkdir -p "$root/proc/$pid"
  ln -s "$release/bin/ramsharedd" "$root/proc/$pid/exe"
}

add_raw_exe_fixture_process() {
  local root=$1 raw_exe=$2 pid=$3
  [[ $pid =~ ^[1-9][0-9]*$ && $raw_exe == /* ]] || return 1
  mkdir -p "$root/proc/$pid"
  ln -s "$raw_exe" "$root/proc/$pid/exe"
}

write_proc_status_metadata() {
  local root=$1 pid=$2 name=$3 kthread=$4
  [[ $pid =~ ^[1-9][0-9]*$ && $kthread =~ ^[01]$ ]] || return 1
  mkdir -p "$root/proc/$pid"
  printf 'Name:\t%s\nState:\tS (sleeping)\nTgid:\t%s\nPid:\t%s\nPPid:\t0\nKthread:\t%s\n' \
    "$name" "$pid" "$pid" "$kthread" >"$root/proc/$pid/status"
}

write_proc_stat_metadata() {
  local root=$1 pid=$2 name=$3 flags=$4
  [[ $pid =~ ^[1-9][0-9]*$ && $flags =~ ^[0-9]+$ ]] || return 1
  mkdir -p "$root/proc/$pid"
  printf '%s (%s) S 0 0 0 0 0 %s\n' "$pid" "$name" "$flags" >"$root/proc/$pid/stat"
}

write_proc_status_state_metadata() {
  local root=$1 pid=$2 name=$3 state=$4 kthread=$5 state_label tgid
  tgid=${6:-$pid}
  [[ $pid =~ ^[1-9][0-9]*$ && $tgid =~ ^[1-9][0-9]*$ &&
    $state =~ ^[RSDTZWI]$ && $kthread =~ ^[01]$ ]] || return 1
  case $state in
    S) state_label=sleeping ;;
    Z) state_label=zombie ;;
    *) state_label=fixture ;;
  esac
  mkdir -p "$root/proc/$pid"
  printf 'Name:\t%s\nState:\t%s (%s)\nTgid:\t%s\nPid:\t%s\nPPid:\t0\nKthread:\t%s\n' \
    "$name" "$state" "$state_label" "$tgid" "$pid" "$kthread" >"$root/proc/$pid/status"
}

write_proc_stat_state_metadata() {
  local root=$1 pid=$2 name=$3 state=$4 flags=$5
  [[ $pid =~ ^[1-9][0-9]*$ && $state =~ ^[RSDTZWI]$ && $flags =~ ^[0-9]+$ ]] || return 1
  mkdir -p "$root/proc/$pid"
  printf '%s (%s) %s 0 0 0 0 0 %s\n' "$pid" "$name" "$state" "$flags" >"$root/proc/$pid/stat"
}

run_product() {
  local root=$1
  shift
  local product_root="$root/opt/ramshared"
  set +e
  RUN_OUTPUT=$(env \
    "PATH=$root/bin:$PATH" \
    "RAMSHARED_PRODUCT_ROOT=$product_root" \
    "RAMSHARED_NBD_PROC_ROOT=$root/proc" \
    "RAMSHARED_NBD_SWAPS_FILE=$root/proc/swaps" \
    "RAMSHARED_NBD_PID_FILE=$root/run/ramsharedd.pid" \
    "RAMSHARED_NBD_MODULES_FILE=$root/proc/modules" \
    "RAMSHARED_NBD_DEV_ROOT=$root/dev" \
    "RAMSHARED_NBD_SYS_BLOCK_ROOT=$root/sys/block" \
    "RAMSHARED_NBD_SYSTEMCTL=$root/bin/systemctl" \
    "RAMSHARED_NBD_RELAY_HEALTH=$root/bin/relay" \
    "RAMSHARED_NBD_DF=$root/bin/df" \
    "RAMSHARED_NBD_STAT=$root/bin/stat" \
    "RAMSHARED_NBD_SELECTOR_PATH=$product_root/current" \
    "RAMSHARED_NBD_DF_MOUNT_POINT=${RAMSHARED_NBD_DF_MOUNT_POINT:-$root/sink}" \
    "RAMSHARED_NBD_EXPECT_UID=${RAMSHARED_NBD_EXPECT_UID:-$(id -u)}" \
    "RAMSHARED_NBD_EXPECT_GID=${RAMSHARED_NBD_EXPECT_GID:-$(id -g)}" \
    "RAMSHARED_RELAY_ARGS=$root/state/relay.args" \
    "RAMSHARED_RELAY_FAIL=$root/state/relay.fail" \
    "RAMSHARED_SYSTEMCTL_STATE=$root/state" \
    "RAMSHARED_NBD_VRAM_MIB=1024" \
    "$PRODUCT" "$@" 2>&1)
  RUN_EXIT=$?
  set -e
}

start_bounded_product_check() {
  local root=$1 output=$2
  local product_root="$root/opt/ramshared"
  shift 2
  [[ -d $root/state && ! -L $root/state && ! -e $output ]] || return 1
  : >"$output"
  timeout --foreground --kill-after=2s 15s env \
    "PATH=$root/bin:$PATH" \
    "RAMSHARED_PRODUCT_ROOT=$product_root" \
    "RAMSHARED_NBD_PROC_ROOT=$root/proc" \
    "RAMSHARED_NBD_SWAPS_FILE=$root/proc/swaps" \
    "RAMSHARED_NBD_PID_FILE=$root/run/ramsharedd.pid" \
    "RAMSHARED_NBD_MODULES_FILE=$root/proc/modules" \
    "RAMSHARED_NBD_DEV_ROOT=$root/dev" \
    "RAMSHARED_NBD_SYS_BLOCK_ROOT=$root/sys/block" \
    "RAMSHARED_NBD_SYSTEMCTL=$root/bin/systemctl" \
    "RAMSHARED_NBD_RELAY_HEALTH=$root/bin/relay" \
    "RAMSHARED_NBD_DF=$root/bin/df" \
    "RAMSHARED_NBD_STAT=$root/bin/stat" \
    "RAMSHARED_NBD_SELECTOR_PATH=$product_root/current" \
    "RAMSHARED_NBD_DF_MOUNT_POINT=${RAMSHARED_NBD_DF_MOUNT_POINT:-$root/sink}" \
    "RAMSHARED_NBD_EXPECT_UID=${RAMSHARED_NBD_EXPECT_UID:-$(id -u)}" \
    "RAMSHARED_NBD_EXPECT_GID=${RAMSHARED_NBD_EXPECT_GID:-$(id -g)}" \
    "RAMSHARED_RELAY_ARGS=$root/state/relay.args" \
    "RAMSHARED_RELAY_FAIL=$root/state/relay.fail" \
    "RAMSHARED_SYSTEMCTL_STATE=$root/state" \
    "RAMSHARED_NBD_VRAM_MIB=1024" \
    "$PRODUCT" "$@" >"$output" 2>&1 &
  BOUNDED_PRODUCT_PID=$!
  [[ $BOUNDED_PRODUCT_PID =~ ^[1-9][0-9]*$ ]]
}

write_proc_status_state_payload() {
  local target=$1 pid=$2 name=$3 state=$4 kthread=$5 state_label tgid
  tgid=${6:-$pid}
  [[ $pid =~ ^[1-9][0-9]*$ && $tgid =~ ^[1-9][0-9]*$ &&
    $state =~ ^[RSDTZWI]$ && $kthread =~ ^[01]$ && ! -e $target ]] || return 1
  case $state in
    S) state_label=sleeping ;;
    Z) state_label=zombie ;;
    *) state_label=fixture ;;
  esac
  printf 'Name:\t%s\nState:\t%s (%s)\nTgid:\t%s\nPid:\t%s\nPPid:\t0\nKthread:\t%s\n' \
    "$name" "$state" "$state_label" "$tgid" "$pid" "$kthread" >"$target"
}

start_status_fifo_writer() {
  local fifo=$1 payload=$2 marker=$3 pid_file=$4 stage=$5
  [[ -p $fifo && -f $payload && ! -e $marker && ! -e $pid_file && $stage =~ ^[1-9][0-9]*$ ]] || return 1
  timeout --foreground --kill-after=1s 5s bash -c '
    set -euo pipefail
    fifo=$1
    payload=$2
    marker=$3
    stage=$4
    exec 3>"$fifo"
    cat -- "$payload" >&3
    exec 3>&-
    printf "stage=%s\\n" "$stage" >"$marker"
  ' _ "$fifo" "$payload" "$marker" "$stage" &
  STATUS_FIFO_WRITER_PID=$!
  [[ $STATUS_FIFO_WRITER_PID =~ ^[1-9][0-9]*$ ]] || return 1
  printf '%s\n' "$STATUS_FIFO_WRITER_PID" >"$pid_file"
}

wait_for_status_fifo_writer() {
  local stage=$1 pid=$2 marker=$3 pid_file=$4 deadline rc
  [[ $pid =~ ^[1-9][0-9]*$ && -f $pid_file && $(<"$pid_file") == "$pid" ]] || return 1
  deadline=$((SECONDS + 6))
  while [[ ! -f $marker ]]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      if wait "$pid"; then rc=0; else rc=$?; fi
      printf 'status FIFO writer exited before marker stage=%s pid=%s rc=%s\n' "$stage" "$pid" "$rc" >&2
      return 1
    fi
    if (( SECONDS >= deadline )); then
      printf 'status FIFO writer deadline stage=%s pid=%s\n' "$stage" "$pid" >&2
      return 1
    fi
    sleep 0.05
  done
  if wait "$pid"; then rc=0; else rc=$?; fi
  [[ $rc == 0 && $(<"$marker") == "stage=$stage" ]] || {
    printf 'status FIFO writer failed stage=%s pid=%s rc=%s\n' "$stage" "$pid" "$rc" >&2
    return 1
  }
}

wait_for_status_fifo_quiescence() {
  local fifo=$1 stage=$2 deadline rc
  [[ -p $fifo && $stage =~ ^[1-9][0-9]*$ ]] || return 1
  deadline=$((SECONDS + 3))
  while :; do
    if fuser -s "$fifo"; then
      rc=0
    else
      rc=$?
    fi
    if (( rc == 1 )); then
      return 0
    fi
    if (( rc != 0 )); then
      printf 'status FIFO probe failed stage=%s rc=%s\n' "$stage" "$rc" >&2
      return 1
    fi
    if (( SECONDS >= deadline )); then
      printf 'status FIFO did not quiesce stage=%s\n' "$stage" >&2
      return 1
    fi
    sleep 0.05
  done
}

stop_owned_bounded_fixture_child() {
  local pid=$1 deadline
  [[ $pid =~ ^[1-9][0-9]*$ ]] || return 0
  if kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid" 2>/dev/null || return 0
    deadline=$((SECONDS + 2))
    while kill -0 "$pid" 2>/dev/null && (( SECONDS < deadline )); do
      sleep 0.05
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -KILL "$pid" 2>/dev/null || return 0
    fi
  fi
  if wait "$pid"; then :; else :; fi
}

exercise_zombie_status_transition_fifo() (
  set -euo pipefail
  local root=$1 fifo="$1/proc/4250/status" product_output="$1/state/zombie-status-race.product.out"
  local stage_one_payload="$1/state/zombie-status-race.stage-1.payload"
  local stage_two_payload="$1/state/zombie-status-race.stage-2.payload"
  local stage_three_payload="$1/state/zombie-status-race.stage-3.payload"
  local stage_one_marker="$1/state/zombie-status-race.stage-1.marker"
  local stage_two_marker="$1/state/zombie-status-race.stage-2.marker"
  local stage_three_marker="$1/state/zombie-status-race.stage-3.marker"
  local stage_one_pid_file="$1/state/zombie-status-race.stage-1.pid"
  local stage_two_pid_file="$1/state/zombie-status-race.stage-2.pid"
  local stage_three_pid_file="$1/state/zombie-status-race.stage-3.pid"
  local product_pid= stage_one_pid= stage_two_pid= stage_three_pid= product_rc
  cleanup_zombie_status_transition_fifo() {
    stop_owned_bounded_fixture_child "$stage_three_pid"
    stop_owned_bounded_fixture_child "$stage_two_pid"
    stop_owned_bounded_fixture_child "$stage_one_pid"
    stop_owned_bounded_fixture_child "$product_pid"
  }
  trap cleanup_zombie_status_transition_fifo EXIT

  mkdir -p "$root/proc/4250"
  mkfifo "$fifo"
  write_proc_stat_state_metadata "$root" 4250 transition-race Z 0
  write_proc_status_state_payload "$stage_one_payload" 4250 transition-race Z 0
  write_proc_status_state_payload "$stage_two_payload" 4250 transition-race S 0
  write_proc_status_state_payload "$stage_three_payload" 4250 transition-race S 0

  start_status_fifo_writer "$fifo" "$stage_one_payload" "$stage_one_marker" "$stage_one_pid_file" 1
  stage_one_pid=$STATUS_FIFO_WRITER_PID
  [[ ! -e $stage_one_marker ]] || {
    printf 'status FIFO stage one did not wait for its reader\n' >&2
    return 1
  }
  kill -0 "$stage_one_pid" 2>/dev/null || {
    printf 'status FIFO stage one exited before product admission\n' >&2
    return 1
  }

  start_bounded_product_check "$root" "$product_output" --check
  product_pid=$BOUNDED_PRODUCT_PID
  wait_for_status_fifo_writer 1 "$stage_one_pid" "$stage_one_marker" "$stage_one_pid_file"
  wait_for_status_fifo_quiescence "$fifo" 1

  start_status_fifo_writer "$fifo" "$stage_two_payload" "$stage_two_marker" "$stage_two_pid_file" 2
  stage_two_pid=$STATUS_FIFO_WRITER_PID
  wait_for_status_fifo_writer 2 "$stage_two_pid" "$stage_two_marker" "$stage_two_pid_file"
  wait_for_status_fifo_quiescence "$fifo" 2

  start_status_fifo_writer "$fifo" "$stage_three_payload" "$stage_three_marker" "$stage_three_pid_file" 3
  stage_three_pid=$STATUS_FIFO_WRITER_PID
  wait_for_status_fifo_writer 3 "$stage_three_pid" "$stage_three_marker" "$stage_three_pid_file"
  wait_for_status_fifo_quiescence "$fifo" 3

  if wait "$product_pid"; then product_rc=0; else product_rc=$?; fi
  [[ $product_rc == 1 ]] || {
    printf 'zombie status transition product exit=%s expected=1\n' "$product_rc" >&2
    return 1
  }
  grep -Fqx 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' "$product_output"
  grep -Fqx 'NBD_PRODUCT_STATE=BLOCKED' "$product_output"
  for marker in "$stage_one_marker" "$stage_two_marker" "$stage_three_marker"; do
    [[ -f $marker ]] || {
      printf 'zombie status transition missing marker=%s\n' "$marker" >&2
      return 1
    }
  done
  for pid_file in "$stage_one_pid_file" "$stage_two_pid_file" "$stage_three_pid_file"; do
    [[ $(<"$pid_file") =~ ^[1-9][0-9]*$ ]] || {
      printf 'zombie status transition invalid writer pid file=%s\n' "$pid_file" >&2
      return 1
    }
  done
  for pid in "$stage_one_pid" "$stage_two_pid" "$stage_three_pid" "$product_pid"; do
    [[ ! -e /proc/$pid ]] || {
      printf 'zombie status transition retained child pid=%s\n' "$pid" >&2
      return 1
    }
  done
)

test_release_manifest_and_modes_are_verified() {
  local root release
  root=$(new_fixture release)
  release="$root/opt/ramshared/releases/v1.2.3"
  run_product "$root" --check
  assert_exit release_manifest_and_modes_are_verified 0 || return
  assert_contains release_manifest_and_modes_are_verified 'NBD_PRODUCT_STATE=PRODUCT_OFF' || return

  chmod 0755 "$release"
  run_product "$root" --check
  assert_exit release_manifest_and_modes_are_verified 1 || return
  assert_contains release_manifest_and_modes_are_verified 'NBD_READINESS_REASON=RELEASE_DIR_MODE_INVALID' || return

  chmod 0555 "$release"
  chmod 0755 "$release/bin/ramshared"
  run_product "$root" --check
  assert_exit release_manifest_and_modes_are_verified 1 || return
  assert_contains release_manifest_and_modes_are_verified 'NBD_READINESS_REASON=RELEASE_FILE_OWNER_MODE_INVALID' || return

  chmod 0755 "$release/bin/ramshared"
  printf 'tamper\n' >>"$release/bin/ramshared"
  chmod 0555 "$release/bin/ramshared"
  run_product "$root" --check
  assert_exit release_manifest_and_modes_are_verified 1 || return
  assert_contains release_manifest_and_modes_are_verified 'NBD_READINESS_REASON=RELEASE_MANIFEST_HASH_MISMATCH' || return
  pass release_manifest_and_modes_are_verified
}

test_binary_match_rejects_stale_or_deleted_daemon() {
  local root release
  root=$(new_fixture binary-match)
  release="$root/opt/ramshared/releases/v1.2.3"
  activate_nbd_fixture "$root"
  run_product "$root" --check
  assert_exit binary_match_rejects_stale_or_deleted_daemon 0 || return
  assert_contains binary_match_rejects_stale_or_deleted_daemon 'NBD_PRODUCT_STATE=READY' || return
  assert_contains binary_match_rejects_stale_or_deleted_daemon 'NBD_BINARY_MATCH=PASS' || return

  printf '#!/usr/bin/env bash\nexit 1\n' >"$root/stale-daemon"
  chmod 0755 "$root/stale-daemon"
  rm "$root/proc/4242/exe"
  ln -s "$root/stale-daemon" "$root/proc/4242/exe"
  run_product "$root" --check
  assert_exit binary_match_rejects_stale_or_deleted_daemon 1 || return
  assert_contains binary_match_rejects_stale_or_deleted_daemon 'NBD_READINESS_REASON=BINARY_MATCH_FAILED' || return
  pass binary_match_rejects_stale_or_deleted_daemon
}

test_product_off_rejects_exact_daemon_without_pidfile() {
  local root
  root=$(new_fixture product-off-exact-daemon)
  add_exact_daemon_fixture_process "$root" 4242

  run_product "$root" --check
  assert_exit product_off_missing_pidfile_for_exact_daemon 1 || return
  assert_contains product_off_missing_pidfile_for_exact_daemon 'NBD_PRODUCT_STATE=BLOCKED' || return
  assert_contains product_off_missing_pidfile_for_exact_daemon 'NBD_READINESS_REASON=DAEMON_PID_MISSING' || return

  add_exact_daemon_fixture_process "$root" 4243
  run_product "$root" --check
  assert_exit product_off_ambiguous_exact_daemons_without_pidfile 1 || return
  assert_contains product_off_ambiguous_exact_daemons_without_pidfile 'NBD_PRODUCT_STATE=BLOCKED' || return
  assert_contains product_off_ambiguous_exact_daemons_without_pidfile 'NBD_READINESS_REASON=DAEMON_PID_AMBIGUOUS' || return
  pass product_off_rejects_exact_daemon_without_pidfile
}

test_product_off_rejects_managed_zram_and_exact_aliases() {
  local root release alias
  root=$(new_fixture product-off-zram)
  printf 'Filename Type Size Used Priority\n%s partition 1048576 0 200\n' "$root/dev/zram0" >"$root/proc/swaps"
  : >"$root/dev/zram0"
  run_product "$root" --check
  assert_exit product_off_rejects_managed_zram_and_exact_aliases 1 || return
  assert_contains product_off_rejects_managed_zram_and_exact_aliases 'NBD_READINESS_REASON=MANAGED_ZRAM_PRESENT' || return

  root=$(new_fixture product-off-hardlink-alias)
  release="$root/opt/ramshared/releases/v1.2.3"
  alias="$root/ramsharedd-alias"
  ln "$release/bin/ramsharedd" "$alias"
  mkdir -p "$root/proc/4242"
  ln -s "$alias" "$root/proc/4242/exe"
  run_product "$root" --check
  assert_exit product_off_rejects_managed_zram_and_exact_aliases 1 || return
  assert_contains product_off_rejects_managed_zram_and_exact_aliases 'NBD_READINESS_REASON=DAEMON_PID_MISSING' || return
  pass product_off_rejects_managed_zram_and_exact_aliases
}

test_product_off_refuses_deleted_and_unreadable_stable_proc_entries() {
  local root release
  root=$(new_fixture product-off-deleted)
  release="$root/opt/ramshared/releases/v1.2.3"
  mkdir -p "$root/proc/4242"
  ln -s "$release/bin/ramsharedd (deleted)" "$root/proc/4242/exe"
  run_product "$root" --check
  assert_exit product_off_refuses_deleted_and_unreadable_stable_proc_entries 1 || return
  assert_contains product_off_refuses_deleted_and_unreadable_stable_proc_entries 'NBD_READINESS_REASON=DAEMON_PID_MISSING' || return

  root=$(new_fixture product-off-unreadable)
  write_proc_status_metadata "$root" 4242 user-process 0
  mkdir -p "$root/proc/4242"
  printf 'not an exe symlink\n' >"$root/proc/4242/exe"
  run_product "$root" --check
  assert_exit product_off_refuses_deleted_and_unreadable_stable_proc_entries 1 || return
  assert_contains product_off_refuses_deleted_and_unreadable_stable_proc_entries 'NBD_READINESS_REASON=PROC_EXE_MALFORMED' || return
  pass product_off_refuses_deleted_and_unreadable_stable_proc_entries
}

test_product_off_tolerates_kernel_thread_and_verified_disappearance() {
  local root
  root=$(new_fixture product-off-kthread)
  write_proc_status_metadata "$root" 2 kthreadd 1
  run_product "$root" --check
  assert_exit product_off_tolerates_kernel_thread_and_verified_disappearance 0 || return
  assert_contains product_off_tolerates_kernel_thread_and_verified_disappearance 'NBD_PRODUCT_STATE=PRODUCT_OFF' || return

  root=$(new_fixture product-off-bracket-user)
  write_proc_status_metadata "$root" 2 kthreadd 0
  printf '[kthreadd]\n' >"$root/proc/2/comm"
  run_product "$root" --check
  assert_exit product_off_tolerates_kernel_thread_and_verified_disappearance 1 || return
  assert_contains product_off_tolerates_kernel_thread_and_verified_disappearance 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' || return

  root=$(new_fixture product-off-disappeared)
  mkdir -p "$root/proc/4242"
  RAMSHARED_NBD_ALLOW_MANUFACTURED_PROC_TEST=1 \
    RAMSHARED_NBD_TEST_PROC_DISAPPEAR_AFTER_EXISTS_PID=4242 \
    run_product "$root" --check
  assert_exit product_off_tolerates_kernel_thread_and_verified_disappearance 0 || return
  [[ ! -e $root/proc/4242 && ! -L $root/proc/4242 ]] || {
    fail 'product_off_tolerates_kernel_thread_and_verified_disappearance did not exercise a real proc-entry disappearance'
    return
  }

  root=$(new_fixture product-off-stable-malformed)
  mkdir -p "$root/proc/4242"
  run_product "$root" --check
  assert_exit product_off_tolerates_kernel_thread_and_verified_disappearance 1 || return
  assert_contains product_off_tolerates_kernel_thread_and_verified_disappearance 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' || return
  pass product_off_tolerates_kernel_thread_and_verified_disappearance
}

test_product_off_tolerates_verified_zombies_only() {
  local root

  root=$(new_fixture product-off-zombie-missing-exe)
  write_proc_status_state_metadata "$root" 4242 unrelated-zombie Z 0
  write_proc_stat_state_metadata "$root" 4242 unrelated-zombie Z 0
  run_product "$root" --check
  assert_exit product_off_zombie_missing_exe 0 || return
  assert_contains product_off_zombie_missing_exe 'NBD_PRODUCT_STATE=PRODUCT_OFF' || return

  root=$(new_fixture product-off-live-empty-exe-readlink)
  write_proc_status_state_metadata "$root" 4243 unrelated-live S 0
  write_proc_stat_state_metadata "$root" 4243 unrelated-live S 0
  ln -s /unreadable-zombie-exe "$root/proc/4243/exe"
  local empty_exe_readlink_hit="$root/state/empty-exe-readlink.hit"
  RAMSHARED_NBD_ALLOW_MANUFACTURED_PROC_TEST=1 \
    RAMSHARED_NBD_TEST_EMPTY_EXE_READLINK_PATH="$root/proc/4243/exe" \
    RAMSHARED_NBD_TEST_EMPTY_EXE_READLINK_HIT="$empty_exe_readlink_hit" \
    run_product "$root" --check
  assert_exit product_off_live_empty_exe_readlink 1 || return
  assert_contains product_off_live_empty_exe_readlink 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' || return
  [[ -f $empty_exe_readlink_hit ]] || {
    fail 'product_off_live_empty_exe_readlink did not execute the empty-exe readlink hook'
    return 1
  }

  root=$(new_fixture product-off-live-missing-exe)
  write_proc_status_state_metadata "$root" 4244 unrelated-live S 0
  write_proc_stat_state_metadata "$root" 4244 unrelated-live S 0
  run_product "$root" --check
  assert_exit product_off_live_missing_exe 1 || return
  assert_contains product_off_live_missing_exe 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' || return

  root=$(new_fixture product-off-zombie-race-to-live)
  write_proc_status_state_metadata "$root" 4245 transition-race Z 0
  write_proc_stat_state_metadata "$root" 4245 transition-race S 0
  run_product "$root" --check
  assert_exit product_off_zombie_race_to_live 1 || return
  assert_contains product_off_zombie_race_to_live 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' || return

  root=$(new_fixture product-off-zombie-malformed-status)
  mkdir -p "$root/proc/4246"
  printf 'Name:\tmalformed\nState:\tZ (zombie)\nState:\tZ (zombie)\nKthread:\t0\n' >"$root/proc/4246/status"
  write_proc_stat_state_metadata "$root" 4246 malformed Z 0
  run_product "$root" --check
  assert_exit product_off_zombie_malformed_status 1 || return
  assert_contains product_off_zombie_malformed_status 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' || return

  root=$(new_fixture product-off-zombie-malformed-stat)
  write_proc_status_state_metadata "$root" 4247 malformed Z 0
  printf 'malformed stat\n' >"$root/proc/4247/stat"
  run_product "$root" --check
  assert_exit product_off_zombie_malformed_stat 1 || return
  assert_contains product_off_zombie_malformed_stat 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' || return

  root=$(new_fixture product-off-zombie-stale-pidfile)
  write_proc_status_state_metadata "$root" 4248 stale-daemon Z 0
  write_proc_stat_state_metadata "$root" 4248 stale-daemon Z 0
  printf '4248\n' >"$root/run/ramsharedd.pid"
  run_product "$root" --check
  assert_exit product_off_zombie_stale_pidfile 1 || return
  assert_contains product_off_zombie_stale_pidfile 'NBD_READINESS_REASON=BINARY_MATCH_FAILED' || return

  root=$(new_fixture product-off-zombie-tgid-mismatch)
  write_proc_status_state_metadata "$root" 4249 mismatched-zombie Z 0 4248
  write_proc_stat_state_metadata "$root" 4249 mismatched-zombie Z 0
  [[ $(awk -F: '$1 == "Tgid" { gsub(/[[:space:]]/, "", $2); print $2 }' \
    "$root/proc/4249/status") == 4248 ]] || {
    fail 'product_off_zombie_tgid_mismatch did not create the mismatched Tgid fixture'
    return 1
  }
  run_product "$root" --check
  assert_exit product_off_zombie_tgid_mismatch 1 || return
  assert_contains product_off_zombie_tgid_mismatch 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' || return

  root=$(new_fixture product-off-zombie-status-race)
  exercise_zombie_status_transition_fifo "$root" || {
    fail 'product_off_zombie_status_race did not refuse the final Z-to-S status transition through all FIFO stages'
    return 1
  }

  pass product_off_tolerates_verified_zombies_only
}

test_product_off_uses_kernel_thread_metadata_not_comm_names() {
  local root
  root=$(new_fixture product-off-kthread-metadata)
  write_proc_status_metadata "$root" 2 kthreadd 1
  write_proc_status_metadata "$root" 3 'kworker/0:0' 1
  write_proc_status_metadata "$root" 4 rcu_preempt 1
  write_proc_status_metadata "$root" 5 migration/0 1
  run_product "$root" --check
  assert_exit product_off_uses_kernel_thread_metadata_not_comm_names 0 || return
  assert_contains product_off_uses_kernel_thread_metadata_not_comm_names 'NBD_PRODUCT_STATE=PRODUCT_OFF' || return

  root=$(new_fixture product-off-forged-kthread-name)
  write_proc_status_metadata "$root" 4242 kthreadd 0
  printf 'kthreadd\n' >"$root/proc/4242/comm"
  run_product "$root" --check
  assert_exit product_off_uses_kernel_thread_metadata_not_comm_names 1 || return
  assert_contains product_off_uses_kernel_thread_metadata_not_comm_names 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' || return

  root=$(new_fixture product-off-kthread-stat-fallback)
  mkdir -p "$root/proc/7"
  write_proc_stat_metadata "$root" 7 kworker/1:0 2097152
  run_product "$root" --check
  assert_exit product_off_uses_kernel_thread_metadata_not_comm_names 0 || return
  assert_contains product_off_uses_kernel_thread_metadata_not_comm_names 'NBD_PRODUCT_STATE=PRODUCT_OFF' || return

  root=$(new_fixture product-off-userspace-stat)
  write_proc_stat_metadata "$root" 8 'kworker/2:0' 0
  printf 'kworker/2:0\n' >"$root/proc/8/comm"
  run_product "$root" --check
  assert_exit product_off_uses_kernel_thread_metadata_not_comm_names 1 || return
  assert_contains product_off_uses_kernel_thread_metadata_not_comm_names 'NBD_READINESS_REASON=PROC_EXE_UNREADABLE' || return

  root=$(new_fixture product-off-forged-kthread-metadata)
  write_proc_status_metadata "$root" 9 kworker/3:0 1
  printf 'fake executable\n' >"$root/proc/9/exe"
  run_product "$root" --check
  assert_exit product_off_uses_kernel_thread_metadata_not_comm_names 1 || return
  assert_contains product_off_uses_kernel_thread_metadata_not_comm_names 'NBD_READINESS_REASON=PROC_EXE_MALFORMED' || return
  pass product_off_uses_kernel_thread_metadata_not_comm_names
}

test_product_off_and_ready_reject_other_installed_release_daemons() {
  local root extra alias foreign

  root=$(new_fixture product-off-extra-installed-release)
  clone_installed_release_fixture "$root" v1.2.4
  add_installed_release_daemon_fixture_process "$root" v1.2.4 4343
  run_product "$root" --check
  assert_exit product_off_and_ready_reject_other_installed_release_daemons 1 || return
  assert_contains product_off_and_ready_reject_other_installed_release_daemons 'NBD_READINESS_REASON=DAEMON_PID_MISSING' || return

  root=$(new_fixture ready-extra-installed-release)
  clone_installed_release_fixture "$root" v1.2.4
  activate_nbd_fixture "$root"
  add_installed_release_daemon_fixture_process "$root" v1.2.4 4343
  run_product "$root" --check
  assert_exit product_off_and_ready_reject_other_installed_release_daemons 1 || return
  assert_contains product_off_and_ready_reject_other_installed_release_daemons 'NBD_READINESS_REASON=DAEMON_PID_AMBIGUOUS' || return

  root=$(new_fixture product-off-extra-hardlink)
  clone_installed_release_fixture "$root" v1.2.4
  extra="$root/opt/ramshared/releases/v1.2.4"
  alias="$root/ramsharedd-installed-alias"
  ln "$extra/bin/ramsharedd" "$alias"
  mkdir -p "$root/proc/4344"
  ln -s "$alias" "$root/proc/4344/exe"
  run_product "$root" --check
  assert_exit product_off_and_ready_reject_other_installed_release_daemons 1 || return
  assert_contains product_off_and_ready_reject_other_installed_release_daemons 'NBD_READINESS_REASON=DAEMON_PID_MISSING' || return

  root=$(new_fixture product-off-extra-deleted)
  clone_installed_release_fixture "$root" v1.2.4
  extra="$root/opt/ramshared/releases/v1.2.4"
  mkdir -p "$root/proc/4345"
  ln -s "$extra/bin/ramsharedd (deleted)" "$root/proc/4345/exe"
  run_product "$root" --check
  assert_exit product_off_and_ready_reject_other_installed_release_daemons 1 || return
  assert_contains product_off_and_ready_reject_other_installed_release_daemons 'NBD_READINESS_REASON=DAEMON_PID_MISSING' || return

  root=$(new_fixture product-off-extra-deleted-binary)
  clone_installed_release_fixture "$root" v1.2.4
  extra="$root/opt/ramshared/releases/v1.2.4"
  chmod 0755 "$extra/bin"
  rm -- "$extra/bin/ramsharedd"
  chmod 0555 "$extra/bin"
  mkdir -p "$root/proc/4347"
  ln -s "$extra/bin/ramsharedd (deleted)" "$root/proc/4347/exe"
  run_product "$root" --check
  assert_exit product_off_and_ready_reject_other_installed_release_daemons 1 || return
  assert_contains product_off_and_ready_reject_other_installed_release_daemons 'NBD_READINESS_REASON=DAEMON_PID_MISSING' || return

  root=$(new_fixture product-off-foreign-path)
  foreign="$root/opt/ramshared/releases-foreign/v1.2.4"
  mkdir -p "$foreign/bin" "$root/proc/4346"
  cp -- "$root/opt/ramshared/releases/v1.2.3/bin/ramsharedd" "$foreign/bin/ramsharedd"
  ln -s "$foreign/bin/ramsharedd" "$root/proc/4346/exe"
  run_product "$root" --check
  assert_exit product_off_and_ready_reject_other_installed_release_daemons 0 || return
  assert_contains product_off_and_ready_reject_other_installed_release_daemons 'NBD_PRODUCT_STATE=PRODUCT_OFF' || return
  pass product_off_and_ready_reject_other_installed_release_daemons
}

test_product_off_recognizes_canonical_release_paths_without_fs_identity() {
  local root invalid_mode missing deleted foreign traversal malformed wrong_case bad_version

  root=$(new_fixture product-off-invalid-old-release-mode)
  clone_installed_release_fixture "$root" v1.2.4
  invalid_mode="$root/opt/ramshared/releases/v1.2.4"
  chmod 0755 "$invalid_mode"
  add_raw_exe_fixture_process "$root" "$invalid_mode/bin/ramsharedd" 4348
  run_product "$root" --check
  assert_exit product_off_recognizes_canonical_release_paths_without_fs_identity 1 || return
  assert_contains product_off_recognizes_canonical_release_paths_without_fs_identity 'NBD_READINESS_REASON=DAEMON_PID_MISSING' || return

  root=$(new_fixture product-off-missing-release-deleted-exe)
  missing="$root/opt/ramshared/releases/v1.2.4"
  deleted="$missing/bin/ramsharedd (deleted)"
  add_raw_exe_fixture_process "$root" "$deleted" 4349
  run_product "$root" --check
  assert_exit product_off_recognizes_canonical_release_paths_without_fs_identity 1 || return
  assert_contains product_off_recognizes_canonical_release_paths_without_fs_identity 'NBD_READINESS_REASON=DAEMON_PID_MISSING' || return

  root=$(new_fixture product-off-release-path-grammar)
  foreign="$root/opt/ramshared/releases-foreign/v1.2.4/bin/ramsharedd"
  traversal="$root/opt/ramshared/releases/v1.2.4/../v1.2.3/bin/ramsharedd"
  malformed="$root/opt/ramshared/releases/v1.2.3/../../bad/bin/ramsharedd"
  wrong_case="$root/opt/ramshared/releases/v1.2.3/bin/Ramsharedd"
  bad_version="$root/opt/ramshared/releases/v1.2.3?/bin/ramsharedd"
  add_raw_exe_fixture_process "$root" "$foreign" 4350
  add_raw_exe_fixture_process "$root" "$traversal" 4351
  add_raw_exe_fixture_process "$root" "$malformed" 4352
  add_raw_exe_fixture_process "$root" "$wrong_case" 4353
  add_raw_exe_fixture_process "$root" "$bad_version" 4354
  run_product "$root" --check
  assert_exit product_off_recognizes_canonical_release_paths_without_fs_identity 0 || return
  assert_contains product_off_recognizes_canonical_release_paths_without_fs_identity 'NBD_PRODUCT_STATE=PRODUCT_OFF' || return
  pass product_off_recognizes_canonical_release_paths_without_fs_identity
}

test_relay_check_failure_blocks_readiness() {
  local root
  root=$(new_fixture relay)
  activate_nbd_fixture "$root"
  : >"$root/state/relay.fail"
  run_product "$root" --check
  assert_exit relay_check_failure_blocks_readiness 1 || return
  assert_contains relay_check_failure_blocks_readiness 'NBD_READINESS_REASON=RELAY_CHECK_FAILED' || return
  if [[ $(<"$root/state/relay.args") != --check ]]; then
    fail "relay_check_failure_blocks_readiness relay arguments were not exactly --check"
    return
  fi
  pass relay_check_failure_blocks_readiness
}

test_reboot_and_shutdown_requests_are_refused() {
  if [[ ! -f "$PRODUCT" ]]; then
    fail 'reboot_and_shutdown_requests_are_refused missing product script'
    return
  fi
  if grep -Eq '(^|[;&|[:space:]])(rmmod|reboot|shutdown|kill|pkill)([;&|[:space:]]|$)|modprobe[[:space:]]+-r' "$PRODUCT"; then
    fail 'reboot_and_shutdown_requests_are_refused found a forbidden host-action command'
    return
  fi
  if "$PRODUCT" --reap >/dev/null 2>&1; then
    fail 'reboot_and_shutdown_requests_are_refused accepted --reap'
    return
  fi
  pass reboot_and_shutdown_requests_are_refused
}

test_legacy_ublk_retirement_never_unloads_module() {
  local root
  root=$(new_fixture legacy-ublk)
  run_product "$root" --check
  assert_exit legacy_ublk_retirement_never_unloads_module 0 || return
  assert_contains legacy_ublk_retirement_never_unloads_module 'NBD_PRODUCT_STATE=PRODUCT_OFF' || return

  mkdir "$root/sys/block/ublkb0"
  run_product "$root" --check
  assert_exit legacy_ublk_retirement_never_unloads_module 1 || return
  assert_contains legacy_ublk_retirement_never_unloads_module 'NBD_READINESS_REASON=ACTIVE_UBLK_DEVICE' || return
  pass legacy_ublk_retirement_never_unloads_module
}

test_product_off_ready_blocked_state_matrix() {
  local root
  root=$(new_fixture state-matrix)
  run_product "$root" --check
  assert_exit product_off_ready_blocked_state_matrix 0 || return
  assert_contains product_off_ready_blocked_state_matrix 'NBD_PRODUCT_STATE=PRODUCT_OFF' || return

  activate_nbd_fixture "$root"
  run_product "$root" --check
  assert_exit product_off_ready_blocked_state_matrix 0 || return
  assert_contains product_off_ready_blocked_state_matrix 'NBD_PRODUCT_STATE=READY' || return

  : >"$root/state/legacy-active"
  run_product "$root" --check
  assert_exit product_off_ready_blocked_state_matrix 1 || return
  assert_contains product_off_ready_blocked_state_matrix 'NBD_PRODUCT_STATE=BLOCKED' || return
  assert_contains product_off_ready_blocked_state_matrix 'NBD_READINESS_REASON=LEGACY_UBLK_SERVICE_ACTIVE' || return
  pass product_off_ready_blocked_state_matrix
}

test_capacity_sink_identity_and_alignment_refusals() {
  local root release
  root=$(new_fixture capacity)
  release="$root/opt/ramshared/releases/v1.2.3"
  activate_nbd_fixture "$root"
  run_product "$root" --check
  assert_exit capacity_sink_identity_and_alignment_refusals 0 || return

  unseal_fixture_release "$release"
  make_generic_bundle_fixture "$release"
  seal_fixture_release "$release"
  run_product "$root" --check
  assert_exit capacity_sink_identity_and_alignment_refusals 1 || return
  assert_contains capacity_sink_identity_and_alignment_refusals 'NBD_READINESS_REASON=INSTALL_PROVENANCE_UNMANIFESTED' || return

  unseal_fixture_release "$release"
  make_installed_release_fixture "$root" "$release"
  seal_fixture_release "$release"
  RAMSHARED_NBD_STAT_ALIGNMENT_BYTES=0 run_product "$root" --check
  assert_exit capacity_sink_identity_and_alignment_refusals 1 || return
  assert_contains capacity_sink_identity_and_alignment_refusals 'NBD_READINESS_REASON=LOWER_TIER_ALIGNMENT_INVALID' || return
  pass capacity_sink_identity_and_alignment_refusals
}

test_environment_lower_sink_cannot_override_derived_release() {
  local root
  root=$(new_fixture environment-override)
  RAMSHARED_NBD_LOWER_SINK="$root/sink" run_product "$root" --check
  assert_exit environment_lower_sink_cannot_override_derived_release 1 || return
  assert_contains environment_lower_sink_cannot_override_derived_release 'NBD_READINESS_REASON=LOWER_TIER_ENV_OVERRIDE_FORBIDDEN' || return
  pass environment_lower_sink_cannot_override_derived_release
}

test_n3_or_ublk_capability_does_not_promote_nbd_product() {
  local root
  root=$(new_fixture capability)
  : >"$root/dev/ublk-control"
  run_product "$root" --check
  assert_exit n3_or_ublk_capability_does_not_promote_nbd_product 0 || return
  assert_contains n3_or_ublk_capability_does_not_promote_nbd_product 'NBD_PRODUCT_STATE=PRODUCT_OFF' || return
  if [[ -f "$PRODUCT" ]] && grep -Eq -- '--reap|rmmod|modprobe[[:space:]]+-r' "$PRODUCT"; then
    fail 'n3_or_ublk_capability_does_not_promote_nbd_product found an unload or reap path'
    return
  fi
  pass n3_or_ublk_capability_does_not_promote_nbd_product
}

test_space_delimited_swap_rows_are_not_missed() {
  local root failures=0
  root=$(new_fixture space-delimited-swaps)

  printf 'Filename Type Size Used Priority\n%s partition 1048576 0 100\n' "$root/dev/ublkb0" >"$root/proc/swaps"
  run_product "$root" --check
  if ! assert_exit space_delimited_ublk_row 1 || ! assert_contains space_delimited_ublk_row 'NBD_READINESS_REASON=ACTIVE_UBLK_SWAP'; then
    failures=1
  fi

  printf 'Filename Type Size Used Priority\n%s file 1048576 0 100\n' "$root/managed-swap (deleted)" >"$root/proc/swaps"
  run_product "$root" --check
  if ! assert_exit space_delimited_ghost_row 1 || ! assert_contains space_delimited_ghost_row 'NBD_READINESS_REASON=GHOST_MANAGED_SWAP'; then
    failures=1
  fi

  activate_nbd_fixture "$root"
  printf 'Filename Type Size Used Priority\n%s partition 1048576 0 100\n' "$root/dev/nbd0" >"$root/proc/swaps"
  run_product "$root" --check
  if ! assert_exit space_delimited_nbd_row 0 || ! assert_contains space_delimited_nbd_row 'NBD_PRODUCT_STATE=READY'; then
    failures=1
  fi

  (( failures == 0 )) || return
  pass space_delimited_swap_rows_are_not_missed
}

test_capacity_sink_identity_requires_one_bound_df_record() {
  local root failures=0
  root=$(new_fixture bound-df)
  activate_nbd_fixture "$root"

  RAMSHARED_NBD_DF_MOUNT_POINT="$root" run_product "$root" --check
  if ! assert_exit capacity_parent_df_mount 0 || ! assert_contains capacity_parent_df_mount 'NBD_PRODUCT_STATE=READY'; then
    failures=1
  fi

  RAMSHARED_NBD_DF_MOUNT_POINT="$root/foreign" run_product "$root" --check
  if ! assert_exit capacity_foreign_df_mount 1 || ! assert_contains capacity_foreign_df_mount 'NBD_READINESS_REASON=LOWER_TIER_SINK_IDENTITY_INVALID'; then
    failures=1
  fi

  RAMSHARED_NBD_DF_EXTRA_RECORD=1 run_product "$root" --check
  if ! assert_exit capacity_multiple_df_records 1 || ! assert_contains capacity_multiple_df_records 'NBD_READINESS_REASON=LOWER_TIER_CAPACITY_AMBIGUOUS'; then
    failures=1
  fi

  (( failures == 0 )) || return
  pass capacity_sink_identity_requires_one_bound_df_record
}

test_selector_lstat_owner_is_verified() {
  local root
  root=$(new_fixture selector-owner)
  RAMSHARED_NBD_SELECTOR_OWNER_MODE="999:$(id -g):777" run_product "$root" --check
  assert_exit selector_lstat_owner_is_verified 1 || return
  assert_contains selector_lstat_owner_is_verified 'NBD_READINESS_REASON=RELEASE_SELECTOR_OWNER_MODE_INVALID' || return
  pass selector_lstat_owner_is_verified
}

test_symlinked_sysfs_ublk_is_detected() {
  local root
  root=$(new_fixture sysfs-symlink)
  mkdir -p "$root/sys/ublk-target"
  ln -s ../ublk-target "$root/sys/block/ublkb0"
  run_product "$root" --check
  assert_exit symlinked_sysfs_ublk_is_detected 1 || return
  assert_contains symlinked_sysfs_ublk_is_detected 'NBD_READINESS_REASON=ACTIVE_UBLK_DEVICE' || return
  pass symlinked_sysfs_ublk_is_detected
}

test_manifest_special_objects_refuse_without_reading_them() {
  local root release
  root=$(new_fixture manifest-special-object)
  release="$root/opt/ramshared/releases/v1.2.3"
  chmod 0755 "$release"
  mkfifo "$release/rogue-fifo"
  chmod 0555 "$release"
  run_product "$root" --check
  assert_exit manifest_special_objects_refuse_without_reading_them 1 || return
  assert_contains manifest_special_objects_refuse_without_reading_them 'NBD_READINESS_REASON=RELEASE_NON_REGULAR_OBJECT' || return
  pass manifest_special_objects_refuse_without_reading_them
}

test_sealed_nbd_bundle_and_lifecycle_wiring() {
  local up down benchmark launcher benchmark_lib worker install uninstall bundle service config source failures=0
  up="$REPO_ROOT/scripts/safety/cascade-up.sh"
  down="$REPO_ROOT/scripts/safety/cascade-down.sh"
  benchmark="$REPO_ROOT/scripts/safety/nbd-benchmark-cell.sh"
  launcher="$REPO_ROOT/scripts/safety/nbd-benchmark-cgroup-launch.sh"
  benchmark_lib="$REPO_ROOT/scripts/safety/nbd-benchmark-lib.sh"
  worker="$REPO_ROOT/scripts/safety/cascade_pressure_integrity_worker.py"
  install="$REPO_ROOT/scripts/safety/install-cascade-boot.sh"
  uninstall="$REPO_ROOT/scripts/safety/uninstall-cascade-boot.sh"
  bundle="$REPO_ROOT/scripts/package/build-linux-bundle.sh"
  service="$REPO_ROOT/scripts/safety/systemd/ramshared-cascade.service"
  config="$REPO_ROOT/scripts/safety/cascade.conf.example"

  for source in "$up" "$down" "$benchmark" "$launcher" "$benchmark_lib" "$worker" "$install" "$uninstall" "$bundle" "$service" "$config"; do
    if [[ ! -f $source ]]; then
      fail "sealed_nbd_bundle_and_lifecycle_wiring missing=$source"
      failures=1
    fi
  done
  for source in "$up" "$down" "$benchmark" "$launcher" "$worker" "$install" "$uninstall" "$bundle" "$PRODUCT" "$0"; do
    if [[ ! -x $source ]]; then
      fail "sealed_nbd_bundle_and_lifecycle_wiring entrypoint_not_executable=$source"
      failures=1
    fi
  done

  if ! grep -Fq 'PRODUCT_ROOT=/opt/ramshared' "$up" ||
    ! grep -Fq -- '--transport nbd' "$up" ||
    ! grep -Fq 'activate:$RELEASE_VERSION' "$up" ||
    ! grep -Fq 'activate:$RELEASE_VERSION:vram=$VRAM_MIB:zram=$ZRAM_MIB' "$up" ||
    ! grep -Fq '^(1024|2048|4096)$' "$up" ||
    ! grep -Fq 'RAMSHARED_NBD_VRAM_MIB=$VRAM_MIB "$PREFLIGHT" --check' "$up" ||
    ! grep -Fq 'NBD_LIFECYCLE_STATE=PLAN' "$up"; then
    fail 'sealed_nbd_bundle_and_lifecycle_wiring activation is not sealed NBD plan/refuse'
    failures=1
  fi
  if ! grep -Fq 'PRODUCT_ROOT=/opt/ramshared' "$down" ||
    ! grep -Fq 'deactivate:$RELEASE_VERSION' "$down" ||
    ! grep -Fq 'NBD_LIFECYCLE_STATE=PLAN' "$down" ||
    ! grep -Fq 'exec "$CLI" down' "$down"; then
    fail 'sealed_nbd_bundle_and_lifecycle_wiring deactivation does not preserve sealed swapoff owner'
    failures=1
  fi
  if ! grep -Fq -- '--approve-nbd-product-install' "$install" ||
    ! grep -Fq 'RELEASE_ROOT="$PRODUCT_ROOT/releases"' "$install" ||
    ! grep -Fq 'mv -Tf' "$install" ||
    ! grep -Fq 'sha256sum -c --status SHA256SUMS' "$install" ||
    ! grep -Fq 'RELEASE_MANIFEST_INCOMPLETE' "$install" ||
    ! grep -Fq 'RELEASE_MANIFEST_PATH_INVALID' "$install" ||
    ! grep -Fq 'PRODUCT_UNIT_CONFLICT' "$install" ||
    ! grep -Fq 'check_unit_inert ramshared-cascade.service' "$install" ||
    ! grep -Fq 'check_unit_inert ramsharedd.service' "$install" ||
    ! grep -Fq 'PUBLISHED_DESTINATION=1' "$install" ||
    ! grep -Fq 'rollback_after_failure' "$install" ||
    ! grep -Fq 'chmod 0555' "$install" ||
    ! grep -Fq 'chmod 0444' "$install" ||
    grep -Fq 'cargo build' "$install" ||
    grep -Fq 'systemctl enable' "$install"; then
    fail 'sealed_nbd_bundle_and_lifecycle_wiring installer is not immutable selector-only'
    failures=1
  fi
  if ! grep -Fq 'STAGE_RELEASE="$STAGE/release"' "$bundle" ||
    ! grep -Fq 'nbd-product-preflight.sh' "$bundle" ||
    ! grep -Fq 'nbd-benchmark-cell.sh' "$bundle" ||
    ! grep -Fq 'nbd-benchmark-cgroup-launch.sh' "$bundle" ||
    ! grep -Fq 'nbd-benchmark-lib.sh' "$bundle" ||
    ! grep -Fq 'cascade_pressure_integrity_worker.py' "$bundle" ||
    ! grep -Fq 'cascade-health.sh' "$bundle" ||
    ! grep -Fq 'uninstall-cascade-boot.sh' "$bundle" ||
    ! grep -Fq 'ramshared-cascade-health.service' "$bundle" ||
    ! grep -Fq 'ramshared-workloads.slice' "$bundle" ||
    ! grep -Fq 'SOURCE_COMMIT' "$bundle" ||
    ! grep -Fq 'SOURCE_TREE_STATE' "$bundle" ||
    ! grep -Fq 'wsl-relay-health.sh' "$bundle" ||
    ! grep -Fq 'find . -type f ! -name SHA256SUMS -print0' "$bundle" ||
    ! grep -Fq 'chmod 0644 "$STAGE_RELEASE/SHA256SUMS"' "$bundle" ||
    grep -Fq 'ramsharedd.service' "$bundle"; then
    fail 'sealed_nbd_bundle_and_lifecycle_wiring bundle is not sealed NBD-only'
    failures=1
  fi
  if ! grep -Fq '/opt/ramshared/current/scripts/safety/nbd-product-preflight.sh --check' "$service" ||
    ! grep -Fq '/opt/ramshared/current/scripts/safety/cascade-up.sh --execute' "$service" ||
    grep -Fq '/etc/ramshared/cascade.conf' "$service" ||
    grep -Fq 'ramsharedd.service' "$service"; then
    fail 'sealed_nbd_bundle_and_lifecycle_wiring service is not current-selector NBD-only'
    failures=1
  fi
  if ! grep -Fq 'NBD_LOWER_SINK=' "$config" || grep -Fq '/etc/ramshared/cascade.conf' "$config"; then
    fail 'sealed_nbd_bundle_and_lifecycle_wiring config is not sealed release configuration'
    failures=1
  fi

  (( failures == 0 )) || return
  pass sealed_nbd_bundle_and_lifecycle_wiring
}

test_sealed_bundle_contains_benchmark_runner_and_worker() {
  local root bundle_root generic_version generic_release sink
  root=$(new_fixture produced-bundle)
  bundle_root="$root/packages"
  generic_version="generic-fixture-$(date +%s)-$$"
  sink="$root/sink"

  set +e
  RUN_OUTPUT=$(RAMSHARED_PACKAGE_OUT="$bundle_root" \
    RAMSHARED_PACKAGE_VERSION="$generic_version" \
    "$REPO_ROOT/scripts/package/build-linux-bundle.sh" --skip-build 2>&1)
  RUN_EXIT=$?
  set -e
  assert_exit sealed_bundle_contains_benchmark_runner_and_worker 0 || return
  generic_release="$bundle_root/ramshared-linux-$generic_version/release"
  grep -qx 'NBD_LOWER_SINK_BINDING=unbound' "$generic_release/scripts/safety/cascade.conf.example" || {
    fail 'sealed_bundle_contains_benchmark_runner_and_worker generic bundle is not explicitly unbound'
    return
  }
  grep -qx 'NBD_LOWER_SINK=' "$generic_release/scripts/safety/cascade.conf.example" || {
    fail 'sealed_bundle_contains_benchmark_runner_and_worker generic bundle contains a lower sink path'
    return
  }
  [[ ! -e $generic_release/INSTALL_PROVENANCE.json && ! -e $generic_release/INPUT_BUNDLE_SHA256SUMS \
    && ! -e $generic_release/INSTALLED_MANIFEST_SHA256 ]] || {
    fail 'sealed_bundle_contains_benchmark_runner_and_worker generic bundle contains a machine binding receipt'
    return
  }
  (cd "$generic_release" && sha256sum -c --status SHA256SUMS) || {
    fail 'sealed_bundle_contains_benchmark_runner_and_worker generic bundle manifest does not verify'
    return
  }
  for required in \
    scripts/safety/nbd-benchmark-cell.sh \
    scripts/safety/nbd-benchmark-cgroup-launch.sh \
    scripts/safety/nbd-benchmark-lib.sh \
    scripts/safety/cascade_pressure_integrity_worker.py \
    scripts/safety/nbd-product-preflight.sh \
    scripts/safety/uninstall-cascade-boot.sh \
    SOURCE_COMMIT SOURCE_BRANCH SOURCE_TREE_STATE; do
    [[ -f $generic_release/$required && ! -L $generic_release/$required ]] || {
      fail "sealed_bundle_contains_benchmark_runner_and_worker missing=$required"
      return
    }
  done
  set +e
  RUN_OUTPUT=$(RAMSHARED_PACKAGE_OUT="$bundle_root" \
    RAMSHARED_PACKAGE_VERSION="obsolete-lower-sink-$(date +%s)-$$" \
    "$REPO_ROOT/scripts/package/build-linux-bundle.sh" --skip-build --lower-sink "$sink" 2>&1)
  RUN_EXIT=$?
  set -e
  if [[ $RUN_EXIT -eq 0 || $RUN_OUTPUT != *'unsupported argument: --lower-sink'* ]]; then
    fail 'sealed_bundle_contains_benchmark_runner_and_worker builder still accepts lower-sink binding'
    return
  fi
  pass sealed_bundle_contains_benchmark_runner_and_worker
}

new_installer_fixture() {
  local name=$1 root release
  root=$(new_fixture "installer-$name")
  release="$root/opt/ramshared/releases/v1.2.3"
  unseal_fixture_release "$release"
  make_generic_bundle_fixture "$release"
  install -m 0755 "$REPO_ROOT/scripts/safety/install-cascade-boot.sh" "$release/scripts/safety/"
  install -m 0755 "$REPO_ROOT/scripts/safety/wsl-relay-health.sh" "$release/scripts/safety/"
  printf 'v1.2.3\n' >"$release/RELEASE_VERSION"
  chmod 0644 "$release/RELEASE_VERSION"
  write_manifest "$release"

  cat >"$root/bin/id" <<'EOF'
#!/usr/bin/env bash
[[ ${1:-} == -u ]] && { printf '0\n'; exit 0; }
exec /usr/bin/id "$@"
EOF
  cat >"$root/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
state=${RAMSHARED_INSTALL_SYSTEMCTL_STATE:?}
unit=${3:-}
if [[ ${1:-} == is-active && ${2:-} == --quiet ]]; then
  case "$state:$unit" in
    active:ramshared-cascade.service|legacy-active:ramsharedd.service) exit 0 ;;
    unknown:*) exit 9 ;;
    *) exit 3 ;;
  esac
fi
if [[ ${1:-} == is-enabled && ${2:-} == --quiet ]]; then
  case "$state:$unit" in
    enabled:ramshared-cascade.service|legacy-enabled:ramsharedd.service) exit 0 ;;
    unknown-enabled:*) exit 9 ;;
    *) exit 1 ;;
  esac
fi
exit 9
EOF
  for command in install cp mv chown ln; do
    cat >"$root/bin/$command" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "${0##*/}" >>"${RAMSHARED_INSTALL_WRITE_MARKER:?}"
exit 99
EOF
    chmod 0755 "$root/bin/$command"
  done
  chmod 0755 "$root/bin/id" "$root/bin/systemctl"
  printf '%s\n' "$root"
}

inject_rollback_failure_after_phase() {
  local phase=$1 install=$2 injection=${3:-"false # manufactured rollback failure after $phase"} temporary="$2.manufactured"
  if ! awk -v phase="$phase" -v injection="$injection" '
    {
      print
      if ($0 ~ "^[[:space:]]*# NBD_INSTALL_POST_WRITE_PHASE=" phase "$" ) {
        matches += 1
        print injection
      }
    }
    END { exit(matches == 1 ? 0 : 91) }
  ' "$install" >"$temporary"; then
    rm -f -- "$temporary"
    return 1
  fi
  mv -f -- "$temporary" "$install"
}

new_rollback_installer_fixture() {
  local name=$1 phase=$2 unit_mode=${3:-existing} injection=${4:-} root source target unit
  root=$(new_fixture "rollback-$name")
  source="$root/opt/ramshared/releases/v1.2.3"
  target="$root/product"
  unit="$root/systemd/ramshared-cascade.service"
  unseal_fixture_release "$source"
  make_generic_bundle_fixture "$source"
  install -m 0755 "$REPO_ROOT/scripts/safety/wsl-relay-health.sh" "$source/scripts/safety/"
  sed \
    -e "s|^PRODUCT_ROOT=/opt/ramshared$|PRODUCT_ROOT=$target|" \
    -e "s|^UNIT_PATH=/etc/systemd/system/ramshared-cascade.service$|UNIT_PATH=$unit|" \
    -e "s|^HEALTH_UNIT_PATH=/etc/systemd/system/ramshared-cascade-health.service$|HEALTH_UNIT_PATH=$root/systemd/ramshared-cascade-health.service|" \
    -e "s|^WORKLOADS_SLICE_PATH=/etc/systemd/system/ramshared-workloads.slice$|WORKLOADS_SLICE_PATH=$root/systemd/ramshared-workloads.slice|" \
    "$REPO_ROOT/scripts/safety/install-cascade-boot.sh" >"$source/scripts/safety/install-cascade-boot.sh"
  inject_rollback_failure_after_phase "$phase" "$source/scripts/safety/install-cascade-boot.sh" "$injection" || return 1
  chmod 0755 "$source/scripts/safety/install-cascade-boot.sh"
  printf 'v1.2.3\n' >"$source/RELEASE_VERSION"
  chmod 0644 "$source/RELEASE_VERSION"
  write_manifest "$source"

  mkdir -p "$target/releases/v0.0.1" "$root/systemd"
  chmod 0755 "$target" "$target/releases"
  chmod 0555 "$target/releases/v0.0.1"
  ln -s releases/v0.0.1 "$target/current"
  if [[ $unit_mode == existing ]]; then
    install -m 0644 "$source/systemd/ramshared-cascade.service" "$unit"
  elif [[ $unit_mode == legacy ]]; then
    printf '[Unit]\nDescription=legacy fixture for exact migration\n' >"$unit"
    chmod 0644 "$unit"
  fi

  cat >"$root/bin/id" <<'EOF'
#!/usr/bin/env bash
[[ ${1:-} == -u ]] && { printf '0\n'; exit 0; }
exec /usr/bin/id "$@"
EOF
cat >"$root/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${RAMSHARED_SYSTEMCTL_LOG:-/dev/null}"
case "${1:-}" in
  is-active) exit 3 ;;
  is-enabled) exit 1 ;;
  daemon-reload) exit 0 ;;
esac
exit 9
EOF
  cat >"$root/bin/chown" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
  cat >"$root/bin/stat" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ ${1:-} == -c && ${2:-} == %u:%g:%a ]]; then
  probe_path=${4:-}
  systemd_dir=$(dirname -- "${RAMSHARED_ROLLBACK_UNIT_PATH:?}")
  if [[ $probe_path == "${RAMSHARED_ROLLBACK_PRODUCT_ROOT:?}"/* || $probe_path == "$systemd_dir"/* ]]; then
    mode=$(/usr/bin/stat -c '%a' -- "$probe_path")
    printf '0:0:%s\n' "$mode"
    exit 0
  fi
fi
exec /usr/bin/stat "$@"
EOF
cat >"$root/bin/ln" <<'EOF'
#!/usr/bin/env bash
exec /usr/bin/ln "$@"
EOF
  cat >"$root/bin/rm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
product_root=${RAMSHARED_ROLLBACK_PRODUCT_ROOT:?}
for argument in "$@"; do
  case "$argument" in
    "$product_root"/releases/*)
      if [[ -d $argument ]]; then
        /usr/bin/find "$argument" -depth -type d -exec /bin/chmod u+rwx {} +
      fi
      ;;
  esac
done
exec /bin/rm "$@"
EOF
  chmod 0755 "$root/bin/id" "$root/bin/systemctl" "$root/bin/chown" "$root/bin/stat" "$root/bin/ln" "$root/bin/rm"
  printf '%s\n' "$root"
}

run_rollback_installer() {
  local root=$1 phase=$2 migration_hash=${3:-} source="$1/opt/ramshared/releases/v1.2.3"
  local -a arguments=(--approve-nbd-product-install v1.2.3 --lower-sink "$root/sink")
  if [[ -n $migration_hash ]]; then
    arguments+=(--approve-legacy-unit-replacement "$migration_hash")
  fi
  set +e
  RUN_OUTPUT=$(env \
    "PATH=$root/bin:$PATH" \
    "RAMSHARED_ROLLBACK_PHASE=$phase" \
    "RAMSHARED_ROLLBACK_PRODUCT_ROOT=$root/product" \
    "RAMSHARED_ROLLBACK_UNIT_PATH=$root/systemd/ramshared-cascade.service" \
    "RAMSHARED_SYSTEMCTL_LOG=$root/state/systemctl.log" \
    "RAMSHARED_NBD_DF_MOUNT_POINT=$root/sink" \
    "$source/scripts/safety/install-cascade-boot.sh" "${arguments[@]}" 2>&1)
  RUN_EXIT=$?
  set -e
}

assert_rollback_preserves_prior_selector_and_unit() {
  local name=$1 root=$2 selector_before=$3 unit_before=$4
  if [[ $(readlink -- "$root/product/current") != "$selector_before" ]]; then
    fail "$name selector_changed=$(readlink -- "$root/product/current")"
    return 1
  fi
  if [[ $unit_before == absent ]]; then
    if [[ -e $root/systemd/ramshared-cascade.service || -L $root/systemd/ramshared-cascade.service ]]; then
      fail "$name unit_created_from_absent_prior_state"
      return 1
    fi
  elif [[ $(sha256sum -- "$root/systemd/ramshared-cascade.service" | awk '{print $1}') != "$unit_before" ]]; then
    fail "$name unit_changed"
    return 1
  fi
  if [[ -e $root/product/releases/v1.2.3 || -L $root/product/releases/v1.2.3 ]]; then
    fail "$name published_destination_remained"
    return 1
  fi
  for auxiliary in ramshared-cascade-health.service ramshared-workloads.slice; do
    if [[ -e $root/systemd/$auxiliary || -L $root/systemd/$auxiliary ]]; then
      fail "$name auxiliary_unit_remained=$auxiliary"
      return 1
    fi
  done
}

test_installer_every_post_write_phase_rolls_back() {
  local root selector_before unit_before unit_mode migration_hash failures=0 phase
  for phase in "${ROLLBACK_POST_WRITE_PHASES[@]}"; do
    case $phase in
      unit-staged|unit-linked|unit-staging-removed) unit_mode=absent ;;
      legacy-unit-backed-up|legacy-unit-staged|legacy-unit-replaced) unit_mode=legacy ;;
      *) unit_mode=existing ;;
    esac
    if ! root=$(new_rollback_installer_fixture "$phase" "$phase" "$unit_mode"); then
      fail "installer_rollback_$phase missing_or_duplicate_phase_marker"
      failures=1
      continue
    fi
    selector_before=$(readlink -- "$root/product/current")
    if [[ -e $root/systemd/ramshared-cascade.service || -L $root/systemd/ramshared-cascade.service ]]; then
      unit_before=$(sha256sum -- "$root/systemd/ramshared-cascade.service" | awk '{print $1}')
    else
      unit_before=absent
    fi
    migration_hash=
    if [[ $unit_mode == legacy ]]; then
      migration_hash=$(sha256sum -- "$root/systemd/ramshared-cascade.service" | awk '{print $1}')
    fi
    run_rollback_installer "$root" "$phase" "$migration_hash"
    if ! assert_exit "installer_rollback_$phase" 1 || ! assert_rollback_preserves_prior_selector_and_unit "installer_rollback_$phase" "$root" "$selector_before" "$unit_before"; then
      failures=1
    fi
  done
  (( failures == 0 )) || return
  pass installer_every_post_write_phase_rolls_back
}

test_attended_derived_install_is_bound_and_sealed() {
  local root source installed input_digest installed_digest
  root=$(new_rollback_installer_fixture attended-derived-install daemon-reloaded existing)
  source="$root/opt/ramshared/releases/v1.2.3"
  sed \
    -e "s|^PRODUCT_ROOT=/opt/ramshared$|PRODUCT_ROOT=$root/product|" \
    -e "s|^UNIT_PATH=/etc/systemd/system/ramshared-cascade.service$|UNIT_PATH=$root/systemd/ramshared-cascade.service|" \
    -e "s|^HEALTH_UNIT_PATH=/etc/systemd/system/ramshared-cascade-health.service$|HEALTH_UNIT_PATH=$root/systemd/ramshared-cascade-health.service|" \
    -e "s|^WORKLOADS_SLICE_PATH=/etc/systemd/system/ramshared-workloads.slice$|WORKLOADS_SLICE_PATH=$root/systemd/ramshared-workloads.slice|" \
    "$REPO_ROOT/scripts/safety/install-cascade-boot.sh" >"$source/scripts/safety/install-cascade-boot.sh"
  chmod 0755 "$source/scripts/safety/install-cascade-boot.sh"
  write_manifest "$source"
  run_rollback_installer "$root" no-injection
  assert_exit attended_derived_install_is_bound_and_sealed 0 || return
  installed="$root/product/releases/v1.2.3"
  [[ -d $installed && ! -L $installed ]] || { fail 'attended_derived_install_is_bound_and_sealed destination missing'; return; }
  [[ $(readlink -- "$root/product/current") == releases/v1.2.3 ]] || { fail 'attended_derived_install_is_bound_and_sealed selector mismatch'; return; }
  grep -qx "NBD_LOWER_SINK=$root/sink" "$installed/scripts/safety/cascade.conf.example" || { fail 'attended_derived_install_is_bound_and_sealed binding missing'; return; }
  grep -qx 'NBD_LOWER_SINK_BINDING=bound' "$installed/scripts/safety/cascade.conf.example" || { fail 'attended_derived_install_is_bound_and_sealed binding state invalid'; return; }
  input_digest=$(sha256sum -- "$installed/INPUT_BUNDLE_SHA256SUMS" | awk '{print $1}')
  installed_digest=$(sha256sum -- "$installed/SHA256SUMS" | awk '{print $1}')
  [[ $(tr -d '[:space:]' <"$installed/INSTALLED_MANIFEST_SHA256") == "$installed_digest" ]] || { fail 'attended_derived_install_is_bound_and_sealed installed receipt mismatch'; return; }
  python3 - "$installed/INSTALL_PROVENANCE.json" "$input_digest" <<'PY' || { fail 'attended_derived_install_is_bound_and_sealed provenance invalid'; return; }
import json
import sys
with open(sys.argv[1], encoding="utf-8") as source:
    record = json.load(source)
assert record["schema_version"] == "ramshared-installed-release-provenance/v1"
assert record["input_bundle_manifest_sha256"] == sys.argv[2]
assert record["lower_sink"]["canonical_path"].startswith("/")
PY
  [[ $(find "$installed" -type d -perm /0222 -print -quit) == '' ]] || { fail 'attended_derived_install_is_bound_and_sealed directory not sealed'; return; }
  [[ $(find "$installed" -type f -perm /0222 -print -quit) == '' ]] || { fail 'attended_derived_install_is_bound_and_sealed file not sealed'; return; }
  for auxiliary in ramshared-cascade-health.service ramshared-workloads.slice; do
    [[ -f $root/systemd/$auxiliary && ! -L $root/systemd/$auxiliary ]] || {
      fail "attended_derived_install_is_bound_and_sealed auxiliary unit missing=$auxiliary"
      return
    }
    cmp -s "$installed/systemd/$auxiliary" "$root/systemd/$auxiliary" || {
      fail "attended_derived_install_is_bound_and_sealed auxiliary unit differs=$auxiliary"
      return
    }
  done
  printf 'PASS installed_release_and_input_bundle_manifests_are_distinct\n'
  pass attended_derived_install_is_bound_and_sealed
}

test_auxiliary_unit_conflict_refuses_and_rolls_back() {
  local root source selector_before health_unit health_before
  root=$(new_rollback_installer_fixture auxiliary-unit-conflict daemon-reloaded existing)
  source="$root/opt/ramshared/releases/v1.2.3"
  sed \
    -e "s|^PRODUCT_ROOT=/opt/ramshared$|PRODUCT_ROOT=$root/product|" \
    -e "s|^UNIT_PATH=/etc/systemd/system/ramshared-cascade.service$|UNIT_PATH=$root/systemd/ramshared-cascade.service|" \
    -e "s|^HEALTH_UNIT_PATH=/etc/systemd/system/ramshared-cascade-health.service$|HEALTH_UNIT_PATH=$root/systemd/ramshared-cascade-health.service|" \
    -e "s|^WORKLOADS_SLICE_PATH=/etc/systemd/system/ramshared-workloads.slice$|WORKLOADS_SLICE_PATH=$root/systemd/ramshared-workloads.slice|" \
    "$REPO_ROOT/scripts/safety/install-cascade-boot.sh" >"$source/scripts/safety/install-cascade-boot.sh"
  chmod 0755 "$source/scripts/safety/install-cascade-boot.sh"
  write_manifest "$source"
  selector_before=$(readlink -- "$root/product/current")
  health_unit="$root/systemd/ramshared-cascade-health.service"
  printf '[Unit]\nDescription=foreign health service\n' >"$health_unit"
  chmod 0644 "$health_unit"
  health_before=$(sha256sum -- "$health_unit" | awk '{print $1}')
  run_rollback_installer "$root" no-injection
  if ! assert_exit auxiliary_unit_conflict_refuses_and_rolls_back 1 ||
    ! assert_contains auxiliary_unit_conflict_refuses_and_rolls_back 'NBD_INSTALL_REASON=AUXILIARY_UNIT_CONFLICT' ||
    [[ $(sha256sum -- "$health_unit" | awk '{print $1}') != "$health_before" ]] ||
    [[ $(readlink -- "$root/product/current") != "$selector_before" ]] ||
    [[ -e $root/systemd/ramshared-workloads.slice ]] ||
    [[ -e $root/product/releases/v1.2.3 ]]; then
    fail 'auxiliary_unit_conflict_refuses_and_rolls_back mutated an existing unit or published a release'
    return
  fi
  pass auxiliary_unit_conflict_refuses_and_rolls_back
}

test_uninstaller_removes_auxiliary_units_without_stopping_workloads() {
  local uninstall="$REPO_ROOT/scripts/safety/uninstall-cascade-boot.sh"
  if ! grep -Fq 'ramshared-cascade-health.service' "$uninstall" ||
    ! grep -Fq 'ramshared-workloads.slice' "$uninstall" ||
    ! grep -Fq 'remove_sealed_unit_if_owned' "$uninstall" ||
    ! grep -Fq 'DEFAULT_BIN_DIR="$REPO/bin"' "$uninstall" ||
    grep -Fq 'systemctl stop ramshared-workloads.slice' "$uninstall"; then
    fail 'uninstaller_removes_auxiliary_units_without_stopping_workloads lifecycle contract missing'
    return
  fi
  pass uninstaller_removes_auxiliary_units_without_stopping_workloads
}

test_uninstaller_preserves_foreign_unit_definitions() {
  local root release uninstall output health_before
  root=$(new_fixture uninstaller-foreign-unit)
  release="$root/opt/ramshared/releases/v1.2.3"
  unseal_fixture_release "$release"
  for unit in ramshared-cascade.service ramshared-cascade-health.service ramshared-workloads.slice; do
    install -m 0644 "$REPO_ROOT/scripts/safety/systemd/$unit" "$release/systemd/$unit"
  done
  mkdir -p "$root/systemd"
  install -m 0644 "$release/systemd/ramshared-cascade.service" "$root/systemd/ramshared-cascade.service"
  install -m 0644 "$release/systemd/ramshared-workloads.slice" "$root/systemd/ramshared-workloads.slice"
  printf '[Unit]\nDescription=foreign health service\n' >"$root/systemd/ramshared-cascade-health.service"
  chmod 0644 "$root/systemd/ramshared-cascade-health.service"
  health_before=$(sha256sum -- "$root/systemd/ramshared-cascade-health.service" | awk '{print $1}')
  sed "s|/etc/systemd/system|$root/systemd|g" "$REPO_ROOT/scripts/safety/uninstall-cascade-boot.sh" >"$release/scripts/safety/uninstall-cascade-boot.sh"
  chmod 0755 "$release/scripts/safety/uninstall-cascade-boot.sh"
  cat >"$root/bin/id" <<'EOF'
#!/usr/bin/env bash
[[ ${1:-} == -u ]] && { printf '0\n'; exit 0; }
exit 1
EOF
  cat >"$root/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"${RAMSHARED_UNINSTALL_SYSTEMCTL_LOG:?}"
exit 0
EOF
  chmod 0755 "$root/bin/id" "$root/bin/systemctl"
  set +e
  output=$(env \
    "PATH=$root/bin:$PATH" \
    "RAMSHARED_UNINSTALL_SYSTEMCTL_LOG=$root/state/systemctl.log" \
    "$release/scripts/safety/uninstall-cascade-boot.sh" 2>&1)
  RUN_EXIT=$?
  set -e
  if ! assert_exit uninstaller_preserves_foreign_unit_definitions 0 ||
    [[ -e $root/systemd/ramshared-cascade.service ]] ||
    [[ -e $root/systemd/ramshared-workloads.slice ]] ||
    [[ ! -f $root/systemd/ramshared-cascade-health.service ]] ||
    [[ $(sha256sum -- "$root/systemd/ramshared-cascade-health.service" | awk '{print $1}') != "$health_before" ]] ||
    grep -Fq 'ramshared-cascade-health.service' "$root/state/systemctl.log"; then
    fail "uninstaller_preserves_foreign_unit_definitions output=$output"
    return
  fi
  pass uninstaller_preserves_foreign_unit_definitions
}

test_packaged_uninstaller_uses_sealed_binary_and_removes_units() {
  local root release output
  root=$(new_fixture packaged-uninstaller)
  release="$root/opt/ramshared/releases/v1.2.3"
  unseal_fixture_release "$release"
  install -m 0755 "$REPO_ROOT/scripts/safety/uninstall-cascade-boot.sh" \
    "$release/scripts/safety/uninstall-cascade-boot.sh"
  for unit in ramshared-cascade.service ramshared-cascade-health.service ramshared-workloads.slice; do
    install -m 0644 "$REPO_ROOT/scripts/safety/systemd/$unit" "$release/systemd/$unit"
  done
  mkdir -p "$root/systemd"
  for unit in ramshared-cascade.service ramshared-cascade-health.service ramshared-workloads.slice; do
    install -m 0644 "$release/systemd/$unit" "$root/systemd/$unit"
  done
  sed "s|^SYSTEMD_UNIT_DIR=/etc/systemd/system$|SYSTEMD_UNIT_DIR=$root/systemd|" \
    "$release/scripts/safety/uninstall-cascade-boot.sh" >"$release/scripts/safety/uninstall-cascade-boot.sh.patched"
  mv -f "$release/scripts/safety/uninstall-cascade-boot.sh.patched" \
    "$release/scripts/safety/uninstall-cascade-boot.sh"
  chmod 0755 "$release/scripts/safety/uninstall-cascade-boot.sh"
  cat >"$release/bin/ramshared" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"${RAMSHARED_UNINSTALL_BINARY_LOG:?}"
EOF
  cat >"$root/bin/id" <<'EOF'
#!/usr/bin/env bash
[[ ${1:-} == -u ]] && { printf '0\n'; exit 0; }
exit 1
EOF
  cat >"$root/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"${RAMSHARED_UNINSTALL_SYSTEMCTL_LOG:?}"
exit 0
EOF
  chmod 0755 "$release/bin/ramshared" "$root/bin/id" "$root/bin/systemctl"
  set +e
  output=$(env \
    "PATH=$root/bin:$PATH" \
    "RAMSHARED_UNINSTALL_BINARY_LOG=$root/state/binary.log" \
    "RAMSHARED_UNINSTALL_SYSTEMCTL_LOG=$root/state/systemctl.log" \
    "$release/scripts/safety/uninstall-cascade-boot.sh" 2>&1)
  RUN_EXIT=$?
  set -e
  if ! assert_exit packaged_uninstaller_uses_sealed_binary_and_removes_units 0 ||
    ! grep -Fxq down "$root/state/binary.log" ||
    ! grep -Fq 'ramshared-cascade-health.service' "$root/state/systemctl.log" ||
    grep -Fq 'ramshared-workloads.slice' "$root/state/systemctl.log" ||
    [[ -e $root/systemd/ramshared-cascade.service ]] ||
    [[ -e $root/systemd/ramshared-cascade-health.service ]] ||
    [[ -e $root/systemd/ramshared-workloads.slice ]]; then
    fail "packaged_uninstaller_uses_sealed_binary_and_removes_units output=$output"
    return
  fi
  pass packaged_uninstaller_uses_sealed_binary_and_removes_units
}

run_installer() {
  local root=$1 state=$2
  shift 2
  local release="$root/opt/ramshared/releases/v1.2.3"
  : >"$root/state/installer-writes"
  set +e
  RUN_OUTPUT=$(env \
    "PATH=$root/bin:$PATH" \
    "RAMSHARED_INSTALL_SYSTEMCTL_STATE=$state" \
    "RAMSHARED_INSTALL_WRITE_MARKER=$root/state/installer-writes" \
    "RAMSHARED_NBD_DF_MOUNT_POINT=$root/sink" \
    "$release/scripts/safety/install-cascade-boot.sh" "$@" 2>&1)
  RUN_EXIT=$?
  set -e
}

assert_installer_was_read_only() {
  local name=$1 marker=$2
  if [[ -s $marker ]]; then
    fail "$name write_commands=$(<"$marker")"
    return 1
  fi
}

test_installer_manifest_and_unit_refusals_are_prewrite() {
  local root release failures=0
  root=$(new_installer_fixture manifest)
  release="$root/opt/ramshared/releases/v1.2.3"

  printf '%064d  ./safe/../outside\n' 0 >"$release/SHA256SUMS"
  run_installer "$root" inactive --plan
  if ! assert_exit installer_manifest_path_refusal 1 || ! assert_contains installer_manifest_path_refusal 'NBD_INSTALL_REASON=RELEASE_MANIFEST_PATH_INVALID' || ! assert_installer_was_read_only installer_manifest_path_refusal "$root/state/installer-writes"; then
    failures=1
  fi

  write_manifest "$release"
  printf 'unlisted\n' >"$release/unlisted-file"
  run_installer "$root" inactive --plan
  if ! assert_exit installer_unlisted_file_refusal 1 || ! assert_contains installer_unlisted_file_refusal 'NBD_INSTALL_REASON=RELEASE_MANIFEST_INCOMPLETE' || ! assert_installer_was_read_only installer_unlisted_file_refusal "$root/state/installer-writes"; then
    failures=1
  fi

  (( failures == 0 )) || return
  pass installer_manifest_and_unit_refusals_are_prewrite
}

test_installer_active_enabled_or_unknown_units_refuse_without_writes() {
  local root failures=0 state expected
  root=$(new_installer_fixture units)
  for state in active enabled unknown unknown-enabled legacy-active legacy-enabled; do
    case $state in
      active|legacy-active) expected=PRODUCT_UNIT_ACTIVE ;;
      enabled|legacy-enabled) expected=PRODUCT_UNIT_ENABLED ;;
      unknown) expected=PRODUCT_UNIT_ACTIVITY_UNKNOWN ;;
      unknown-enabled) expected=PRODUCT_UNIT_ENABLEMENT_UNKNOWN ;;
    esac
    run_installer "$root" "$state" --approve-nbd-product-install v1.2.3 --lower-sink "$root/sink"
    if ! assert_exit "installer_$state" 1 || ! assert_contains "installer_$state" "NBD_INSTALL_REASON=$expected" || ! assert_installer_was_read_only "installer_$state" "$root/state/installer-writes"; then
      failures=1
    fi
  done
  (( failures == 0 )) || return
  pass installer_active_enabled_or_unknown_units_refuse_without_writes
}

test_legacy_unit_migration_requires_exact_hash_and_restores_on_failure() {
  local root source unit old_hash backup_hash
  root=$(new_rollback_installer_fixture legacy-unit-migration daemon-reloaded existing)
  source="$root/opt/ramshared/releases/v1.2.3"
  unit="$root/systemd/ramshared-cascade.service"
  printf '[Unit]\nDescription=legacy exact migration fixture\n' >"$unit"
  chmod 0644 "$unit"
  old_hash=$(sha256sum -- "$unit" | awk '{print $1}')

  run_rollback_installer "$root" daemon-reloaded
  if ! assert_exit legacy_unit_migration_requires_exact_hash_and_restores_on_failure 1 ||
    ! assert_contains legacy_unit_migration_requires_exact_hash_and_restores_on_failure 'NBD_INSTALL_REASON=LEGACY_UNIT_APPROVAL_MISSING'; then
    return
  fi

  set +e
  RUN_OUTPUT=$(env \
    "PATH=$root/bin:$PATH" \
    "RAMSHARED_ROLLBACK_PRODUCT_ROOT=$root/product" \
    "RAMSHARED_ROLLBACK_UNIT_PATH=$unit" \
    "RAMSHARED_NBD_DF_MOUNT_POINT=$root/sink" \
    "$source/scripts/safety/install-cascade-boot.sh" \
    --approve-nbd-product-install v1.2.3 \
    --lower-sink "$root/sink" \
    --approve-legacy-unit-replacement "${old_hash/a/b}" 2>&1)
  RUN_EXIT=$?
  set -e
  if ! assert_exit legacy_unit_migration_requires_exact_hash_and_restores_on_failure 1 ||
    ! assert_contains legacy_unit_migration_requires_exact_hash_and_restores_on_failure 'NBD_INSTALL_REASON=LEGACY_UNIT_HASH_MISMATCH' ||
    [[ $(sha256sum -- "$unit" | awk '{print $1}') != "$old_hash" ]]; then
    fail 'legacy_unit_migration_requires_exact_hash_and_restores_on_failure stale approval mutated the legacy unit'
    return
  fi

  set +e
  RUN_OUTPUT=$(env \
    "PATH=$root/bin:$PATH" \
    "RAMSHARED_ROLLBACK_PRODUCT_ROOT=$root/product" \
    "RAMSHARED_ROLLBACK_UNIT_PATH=$unit" \
    "RAMSHARED_NBD_DF_MOUNT_POINT=$root/sink" \
    "$source/scripts/safety/install-cascade-boot.sh" \
    --approve-nbd-product-install v1.2.3 \
    --lower-sink "$root/sink" \
    --approve-legacy-unit-replacement "$old_hash" 2>&1)
  RUN_EXIT=$?
  set -e
  if ! assert_exit legacy_unit_migration_requires_exact_hash_and_restores_on_failure 1 ||
    [[ $(sha256sum -- "$unit" | awk '{print $1}') != "$old_hash" ]]; then
    fail 'legacy_unit_migration_requires_exact_hash_and_restores_on_failure old unit was not restored'
    return
  fi
  backup_hash=$(sha256sum -- "$root/product/legacy-units/ramshared-cascade.service.$old_hash.bak" | awk '{print $1}')
  if [[ $backup_hash != "$old_hash" ]]; then
    fail 'legacy_unit_migration_requires_exact_hash_and_restores_on_failure immutable backup mismatch'
    return
  fi

  pass legacy_unit_migration_requires_exact_hash_and_restores_on_failure
}

test_corrupt_legacy_backup_is_never_restored() {
  local root source unit old_hash injection
  injection='chmod u+w "$LEGACY_BACKUP"; printf corrupt >"$LEGACY_BACKUP"; chmod 0444 "$LEGACY_BACKUP"; false # manufactured corrupt-backup rollback'
  root=$(new_rollback_installer_fixture corrupt-legacy-backup legacy-unit-replaced legacy "$injection")
  source="$root/opt/ramshared/releases/v1.2.3"
  unit="$root/systemd/ramshared-cascade.service"
  old_hash=$(sha256sum -- "$unit" | awk '{print $1}')
  run_rollback_installer "$root" legacy-unit-replaced "$old_hash"
  if ! assert_exit corrupt_legacy_backup_is_never_restored 1 ||
    ! assert_contains corrupt_legacy_backup_is_never_restored 'NBD_INSTALL_ROLLBACK=LEGACY_UNIT_RESTORE_FAILED'; then
    return
  fi
  if [[ $(sha256sum -- "$unit" | awk '{print $1}') == "$old_hash" ]]; then
    fail 'corrupt_legacy_backup_is_never_restored silently restored a corrupted backup'
    return
  fi
  pass corrupt_legacy_backup_is_never_restored
}

test_corrupt_published_legacy_backup_refuses_before_replacement() {
  local root unit old_hash injection
  injection='chmod u+w "$LEGACY_BACKUP"; printf corrupt >"$LEGACY_BACKUP"; chmod 0444 "$LEGACY_BACKUP" # manufactured pre-replacement backup corruption'
  root=$(new_rollback_installer_fixture corrupt-published-legacy-backup legacy-unit-backed-up legacy "$injection")
  unit="$root/systemd/ramshared-cascade.service"
  old_hash=$(sha256sum -- "$unit" | awk '{print $1}')
  run_rollback_installer "$root" legacy-unit-backed-up "$old_hash"
  if ! assert_exit corrupt_published_legacy_backup_refuses_before_replacement 1 ||
    ! assert_contains corrupt_published_legacy_backup_refuses_before_replacement 'NBD_INSTALL_REASON=LEGACY_UNIT_BACKUP_HASH_MISMATCH' ||
    [[ $(sha256sum -- "$unit" | awk '{print $1}') != "$old_hash" ]]; then
    fail 'corrupt_published_legacy_backup_refuses_before_replacement replaced the legacy unit after backup corruption'
    return
  fi
  pass corrupt_published_legacy_backup_refuses_before_replacement
}

test_legacy_backup_root_symlink_is_refused() {
  local root unit old_hash redirect
  root=$(new_rollback_installer_fixture legacy-backup-root-symlink legacy-unit-backed-up legacy)
  unit="$root/systemd/ramshared-cascade.service"
  old_hash=$(sha256sum -- "$unit" | awk '{print $1}')
  redirect="$root/redirected-legacy-backups"
  mkdir -p "$redirect"
  ln -s "$redirect" "$root/product/legacy-units"
  run_rollback_installer "$root" legacy-unit-backed-up "$old_hash"
  if ! assert_exit legacy_backup_root_symlink_is_refused 1 ||
    ! assert_contains legacy_backup_root_symlink_is_refused 'NBD_INSTALL_REASON=LEGACY_BACKUP_ROOT_INVALID' ||
    [[ -n $(find "$redirect" -mindepth 1 -print -quit) ]]; then
    fail 'legacy_backup_root_symlink_is_refused followed a redirected backup root'
    return
  fi
  pass legacy_backup_root_symlink_is_refused
}

test_legacy_restore_reloads_systemd_after_daemon_reload() {
  local root unit old_hash reloads
  root=$(new_rollback_installer_fixture legacy-restore-reload daemon-reloaded legacy)
  unit="$root/systemd/ramshared-cascade.service"
  old_hash=$(sha256sum -- "$unit" | awk '{print $1}')
  run_rollback_installer "$root" daemon-reloaded "$old_hash"
  reloads=$(grep -cx 'daemon-reload' "$root/state/systemctl.log" || true)
  if ! assert_exit legacy_restore_reloads_systemd_after_daemon_reload 1 ||
    [[ $reloads != 2 ]] ||
    [[ $(sha256sum -- "$unit" | awk '{print $1}') != "$old_hash" ]]; then
    fail "legacy_restore_reloads_systemd_after_daemon_reload reloads=$reloads"
    return
  fi
  pass legacy_restore_reloads_systemd_after_daemon_reload
}

test_release_manifest_and_modes_are_verified
test_binary_match_rejects_stale_or_deleted_daemon
test_product_off_rejects_exact_daemon_without_pidfile
test_product_off_rejects_managed_zram_and_exact_aliases
test_product_off_refuses_deleted_and_unreadable_stable_proc_entries
test_product_off_tolerates_kernel_thread_and_verified_disappearance
test_product_off_tolerates_verified_zombies_only
test_product_off_uses_kernel_thread_metadata_not_comm_names
test_product_off_and_ready_reject_other_installed_release_daemons
test_product_off_recognizes_canonical_release_paths_without_fs_identity
test_relay_check_failure_blocks_readiness
test_reboot_and_shutdown_requests_are_refused
test_legacy_ublk_retirement_never_unloads_module
test_product_off_ready_blocked_state_matrix
test_capacity_sink_identity_and_alignment_refusals
test_environment_lower_sink_cannot_override_derived_release
test_n3_or_ublk_capability_does_not_promote_nbd_product
test_space_delimited_swap_rows_are_not_missed
test_capacity_sink_identity_requires_one_bound_df_record
test_selector_lstat_owner_is_verified
test_symlinked_sysfs_ublk_is_detected
test_manifest_special_objects_refuse_without_reading_them
test_sealed_nbd_bundle_and_lifecycle_wiring
test_sealed_bundle_contains_benchmark_runner_and_worker
test_installer_manifest_and_unit_refusals_are_prewrite
test_installer_active_enabled_or_unknown_units_refuse_without_writes
test_installer_every_post_write_phase_rolls_back
test_attended_derived_install_is_bound_and_sealed
test_auxiliary_unit_conflict_refuses_and_rolls_back
test_uninstaller_removes_auxiliary_units_without_stopping_workloads
test_uninstaller_preserves_foreign_unit_definitions
test_packaged_uninstaller_uses_sealed_binary_and_removes_units
test_legacy_unit_migration_requires_exact_hash_and_restores_on_failure
test_corrupt_legacy_backup_is_never_restored
test_corrupt_published_legacy_backup_refuses_before_replacement
test_legacy_backup_root_symlink_is_refused
test_legacy_restore_reloads_systemd_after_daemon_reload

if [[ $fail_count -ne 0 ]]; then
  printf 'FAIL Test-NbdProductPreflight passed=%s failed=%s\n' "$pass_count" "$fail_count" >&2
  exit 1
fi
printf 'PASS Test-NbdProductPreflight total=%s\n' "$pass_count"
