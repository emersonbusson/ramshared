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
    find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
  )
  chmod 0644 "$release/SHA256SUMS"
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
  for script in cascade-up.sh cascade-down.sh; do
    printf '#!/usr/bin/env bash\nexit 0\n' >"$release/scripts/safety/$script"
    chmod 0755 "$release/scripts/safety/$script"
  done
  if [[ -f "$PRODUCT" ]]; then
    install -m 0755 "$PRODUCT" "$release/scripts/safety/nbd-product-preflight.sh"
  else
    printf '#!/usr/bin/env bash\nexit 0\n' >"$release/scripts/safety/nbd-product-preflight.sh"
    chmod 0755 "$release/scripts/safety/nbd-product-preflight.sh"
  fi
  printf '[Service]\n' >"$release/systemd/ramshared-cascade.service"
  chmod 0644 "$release/systemd/ramshared-cascade.service"
  printf 'NBD_LOWER_SINK=\n' >"$release/scripts/safety/cascade.conf.example"
  chmod 0644 "$release/scripts/safety/cascade.conf.example"
  write_manifest "$release"
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
  chmod 0755 "$root/bin/relay" "$root/bin/systemctl" "$root/bin/df" "$root/bin/stat"
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

run_product() {
  local root=$1
  shift
  local product_root="$root/opt/ramshared"
  set +e
  RUN_OUTPUT=$(env \
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
    "RAMSHARED_NBD_LOWER_SINK=$root/sink" \
    "RAMSHARED_NBD_VRAM_MIB=1024" \
    "$PRODUCT" "$@" 2>&1)
  RUN_EXIT=$?
  set -e
}

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
  local root
  root=$(new_fixture capacity)
  activate_nbd_fixture "$root"
  run_product "$root" --check
  assert_exit capacity_sink_identity_and_alignment_refusals 0 || return

  set +e
  RUN_OUTPUT=$(env \
    "RAMSHARED_PRODUCT_ROOT=$root/opt/ramshared" \
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
    "RAMSHARED_NBD_EXPECT_UID=$(id -u)" \
    "RAMSHARED_NBD_EXPECT_GID=$(id -g)" \
    "RAMSHARED_RELAY_ARGS=$root/state/relay.args" \
    "RAMSHARED_RELAY_FAIL=$root/state/relay.fail" \
    "RAMSHARED_SYSTEMCTL_STATE=$root/state" \
    "RAMSHARED_NBD_VRAM_MIB=1024" \
    "$PRODUCT" --check 2>&1)
  RUN_EXIT=$?
  set -e
  assert_exit capacity_sink_identity_and_alignment_refusals 1 || return
  assert_contains capacity_sink_identity_and_alignment_refusals 'NBD_READINESS_REASON=LOWER_TIER_SINK_UNKNOWN' || return

  RAMSHARED_NBD_STAT_ALIGNMENT_BYTES=0 run_product "$root" --check
  assert_exit capacity_sink_identity_and_alignment_refusals 1 || return
  assert_contains capacity_sink_identity_and_alignment_refusals 'NBD_READINESS_REASON=LOWER_TIER_ALIGNMENT_INVALID' || return
  pass capacity_sink_identity_and_alignment_refusals
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
  local up down install bundle service config source failures=0
  up="$REPO_ROOT/scripts/safety/cascade-up.sh"
  down="$REPO_ROOT/scripts/safety/cascade-down.sh"
  install="$REPO_ROOT/scripts/safety/install-cascade-boot.sh"
  bundle="$REPO_ROOT/scripts/package/build-linux-bundle.sh"
  service="$REPO_ROOT/scripts/safety/systemd/ramshared-cascade.service"
  config="$REPO_ROOT/scripts/safety/cascade.conf.example"

  for source in "$up" "$down" "$install" "$bundle" "$service" "$config"; do
    if [[ ! -f $source ]]; then
      fail "sealed_nbd_bundle_and_lifecycle_wiring missing=$source"
      failures=1
    fi
  done
  for source in "$up" "$down" "$install" "$bundle" "$PRODUCT" "$0"; do
    if [[ ! -x $source ]]; then
      fail "sealed_nbd_bundle_and_lifecycle_wiring entrypoint_not_executable=$source"
      failures=1
    fi
  done

  if ! grep -Fq 'PRODUCT_ROOT=/opt/ramshared' "$up" ||
    ! grep -Fq -- '--transport nbd' "$up" ||
    ! grep -Fq 'activate:$RELEASE_VERSION' "$up" ||
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

new_installer_fixture() {
  local name=$1 root release
  root=$(new_fixture "installer-$name")
  release="$root/opt/ramshared/releases/v1.2.3"
  unseal_fixture_release "$release"
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
  install -m 0755 "$REPO_ROOT/scripts/safety/wsl-relay-health.sh" "$source/scripts/safety/"
  sed \
    -e "s|^PRODUCT_ROOT=/opt/ramshared$|PRODUCT_ROOT=$target|" \
    -e "s|^UNIT_PATH=/etc/systemd/system/ramshared-cascade.service$|UNIT_PATH=$unit|" \
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
  path=${4:-}
  if [[ $path == "${RAMSHARED_ROLLBACK_PRODUCT_ROOT:?}"/* || $path == "${RAMSHARED_ROLLBACK_UNIT_PATH:?}" ]]; then
    mode=$(/usr/bin/stat -c '%a' -- "$path")
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
  local -a arguments=(--approve-nbd-product-install v1.2.3)
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
    run_installer "$root" "$state" --approve-nbd-product-install v1.2.3
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
    "$source/scripts/safety/install-cascade-boot.sh" \
    --approve-nbd-product-install v1.2.3 \
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
    "$source/scripts/safety/install-cascade-boot.sh" \
    --approve-nbd-product-install v1.2.3 \
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

test_release_manifest_and_modes_are_verified || true
test_binary_match_rejects_stale_or_deleted_daemon || true
test_relay_check_failure_blocks_readiness || true
test_reboot_and_shutdown_requests_are_refused || true
test_legacy_ublk_retirement_never_unloads_module || true
test_product_off_ready_blocked_state_matrix || true
test_capacity_sink_identity_and_alignment_refusals || true
test_n3_or_ublk_capability_does_not_promote_nbd_product || true
test_space_delimited_swap_rows_are_not_missed || true
test_capacity_sink_identity_requires_one_bound_df_record || true
test_selector_lstat_owner_is_verified || true
test_symlinked_sysfs_ublk_is_detected || true
test_manifest_special_objects_refuse_without_reading_them || true
test_sealed_nbd_bundle_and_lifecycle_wiring || true
test_installer_manifest_and_unit_refusals_are_prewrite || true
test_installer_active_enabled_or_unknown_units_refuse_without_writes || true
test_installer_every_post_write_phase_rolls_back || true
test_legacy_unit_migration_requires_exact_hash_and_restores_on_failure || true
test_corrupt_legacy_backup_is_never_restored || true
test_corrupt_published_legacy_backup_refuses_before_replacement || true
test_legacy_backup_root_symlink_is_refused || true
test_legacy_restore_reloads_systemd_after_daemon_reload || true

if [[ $fail_count -ne 0 ]]; then
  printf 'FAIL Test-NbdProductPreflight passed=%s failed=%s\n' "$pass_count" "$fail_count" >&2
  exit 1
fi
printf 'PASS Test-NbdProductPreflight total=%s\n' "$pass_count"
