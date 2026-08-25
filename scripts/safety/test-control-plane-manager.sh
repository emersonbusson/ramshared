#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
manager="$root/scripts/safety/manage-control-plane.sh"
[[ -f $manager && ! -L $manager ]] || exit 1
output=$(bash "$manager")
grep -Fqx 'RAMSHARED_CONTROL_PLANE=PLAN' <<<"$output"
grep -Fqx 'ACTIVATION=disabled' <<<"$output"
grep -Fqx 'DOCKER_RESTART=not_performed' <<<"$output"
grep -Fqx 'AMBIENT_SERVICE_BEHAVIOR=unchanged' <<<"$output"
grep -Fq 'systemctl daemon-reload' "$manager"
if grep -Eq 'systemctl (enable|start|restart)|docker restart' "$manager"; then
  echo 'manager must install source disabled without restarting workloads' >&2
  exit 1
fi
if grep -Fq 'docker-daemon.json' "$manager"; then
  echo 'disabled staging must not rewrite Docker daemon configuration' >&2
  exit 1
fi
lease_revoke_line=$(grep -nF 'rm -f -- "$lease"' "$root/scripts/safety/ramshared-host-gate.sh" | head -n1 | cut -d: -f1)
origin_revoke_line=$(grep -nF 'rm -f -- "$origin_config"' "$root/scripts/safety/ramshared-host-gate.sh" | head -n1 | cut -d: -f1)
origin_parse_line=$(grep -nF 'if [[ -f $origin_manifest' "$root/scripts/safety/ramshared-host-gate.sh" | head -n1 | cut -d: -f1)
[[ $lease_revoke_line =~ ^[0-9]+$ && $origin_revoke_line =~ ^[0-9]+$ && $origin_parse_line =~ ^[0-9]+$ && $lease_revoke_line -lt $origin_parse_line && $origin_revoke_line -lt $origin_parse_line ]] || {
  echo 'host gate must revoke prior lease and origin authority before parsing origin proof' >&2
  exit 1
}
grep -Fq 'PTUUID' "$root/scripts/safety/ramshared-host-gate.sh" || {
  echo 'host gate must verify actual GPT disk GUID before importing origin' >&2
  exit 1
}

# R4-HOST-01: a legacy direct `cascade-app.sh start` is never an activation
# path. The production command must return a disabled-staging refusal before
# it can reach preflight, modprobe, or `ramshared up`.
cascade_app="$root/scripts/safety/cascade-app.sh"
grep -Fq 'CASCADE_APP=STAGING_REFUSED' "$cascade_app" || {
  echo 'cascade app start must report disabled staging refusal' >&2
  exit 1
}

fixture=$(mktemp -d)
cleanup_fixture() {
  rm -rf -- "$fixture"
}
trap cleanup_fixture EXIT

fixture_manager="$fixture/manage-control-plane.sh"
fixture_unit_dir="$fixture/etc/systemd/system"
fixture_state_dir="$fixture/var/lib/ramshared/control-plane-install"
fixture_docker_config="$fixture/etc/docker/daemon.json"
fixture_bin="$fixture/bin"
fixture_meminfo="$fixture/proc/meminfo"
mkdir -p "$fixture_unit_dir" "$(dirname -- "$fixture_docker_config")" "$(dirname -- "$fixture_meminfo")" "$fixture_bin"

sed \
  -e "s|^source_dir=.*$|source_dir=$root/scripts/safety/systemd|" \
  -e "s|^unit_dir=/etc/systemd/system$|unit_dir=$fixture_unit_dir|" \
  -e "s|^state_dir=/var/lib/ramshared/control-plane-install$|state_dir=$fixture_state_dir|" \
  -e "s|^docker_config=/etc/docker/daemon.json$|docker_config=$fixture_docker_config|" \
  -e "s|^meminfo=/proc/meminfo$|meminfo=$fixture_meminfo|" \
  "$manager" >"$fixture_manager"
chmod 0755 "$fixture_manager"
printf '%s\n' '#!/usr/bin/env bash' 'set -euo pipefail' \
  'printf "%s\\n" "$*" >>"${RAMSHARED_TEST_SYSTEMCTL_LOG:?}"' \
  'if [[ ${RAMSHARED_TEST_FAIL_DAEMON_RELOAD_ONCE:-} == 1 && $1 == daemon-reload ]] && (( $(grep -Fxc daemon-reload "$RAMSHARED_TEST_SYSTEMCTL_LOG") == 1 )); then exit 71; fi' >"$fixture_bin/systemctl"
chmod 0755 "$fixture_bin/systemctl"
cat >"$fixture_bin/mv" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ ${RAMSHARED_TEST_FAIL_TARGET:-} == "${!#}" ]]; then
  if [[ -n ${RAMSHARED_TEST_MUTATE_TARGET:-} ]]; then
    printf '%s\n' 'operator-owned-content' >"$RAMSHARED_TEST_MUTATE_TARGET"
  fi
  exit 70
fi
exec /usr/bin/mv "$@"
EOF
chmod 0755 "$fixture_bin/mv"

# Execute the production legacy app command in an isolated fixture. Fake root,
# preflight, and CLI scripts merely log calls; disabled staging must reach none.
cascade_fixture_repo="$fixture/cascade-repo"
mkdir -p "$cascade_fixture_repo/scripts/safety" "$cascade_fixture_repo/bin"
cp -- "$cascade_app" "$cascade_fixture_repo/scripts/safety/cascade-app.sh"
cat >"$cascade_fixture_repo/scripts/safety/cascade-preflight.sh" <<'EOF'
#!/usr/bin/env bash
printf 'preflight\n' >>"${RAMSHARED_CASCADE_TEST_LOG:?}"
printf 'modprobe\n' >>"${RAMSHARED_CASCADE_TEST_LOG:?}"
EOF
cat >"$cascade_fixture_repo/bin/ramshared" <<'EOF'
#!/usr/bin/env bash
printf 'ramshared %s\n' "$*" >>"${RAMSHARED_CASCADE_TEST_LOG:?}"
EOF
cat >"$fixture_bin/id" <<'EOF'
#!/usr/bin/env bash
if [[ ${1:-} == -u ]]; then printf '0\n'; else exec /usr/bin/id "$@"; fi
EOF
chmod 0755 "$cascade_fixture_repo/scripts/safety/cascade-preflight.sh" "$cascade_fixture_repo/bin/ramshared" "$fixture_bin/id"
cascade_log="$fixture/cascade-start.log"
: >"$cascade_log"
set +e
cascade_start_output=$(PATH="$fixture_bin:$PATH" RAMSHARED_REPO="$cascade_fixture_repo" \
  RAMSHARED_CLI="$cascade_fixture_repo/bin/ramshared" RAMSHARED_CASCADE_TEST_LOG="$cascade_log" \
  bash "$cascade_fixture_repo/scripts/safety/cascade-app.sh" --cli start 2>&1)
cascade_start_status=$?
set -e
(( cascade_start_status != 0 )) || { echo 'direct cascade start unexpectedly succeeded' >&2; exit 1; }
grep -Fqx 'CASCADE_APP=STAGING_REFUSED' <<<"$cascade_start_output" || {
  echo 'direct cascade start did not expose plan-only staging refusal' >&2; exit 1;
}
[[ ! -s $cascade_log ]] || { echo 'direct cascade start reached preflight, modprobe, or CLI up' >&2; exit 1; }

# Final legacy-path gate: the old installer and direct preflight are source
# fixtures only.  Both must refuse before an installer can write a system path
# or preflight can reach a module load.  The fixture forces a non-root caller
# for install, while the preflight's root/modprobe dependencies are harmless
# loggers in the temporary fixture.
legacy_install="$root/scripts/safety/install.sh"
legacy_preflight="$root/scripts/safety/cascade-preflight.sh"
legacy_refusal_bin="$fixture/legacy-refusal-bin"
mkdir -p "$legacy_refusal_bin"
cat >"$legacy_refusal_bin/id" <<'EOF'
#!/usr/bin/env bash
if [[ ${1:-} == -u ]]; then printf '65534\n'; else exec /usr/bin/id "$@"; fi
EOF
chmod 0755 "$legacy_refusal_bin/id"
legacy_install_log="$fixture/legacy-install.log"
: >"$legacy_install_log"
set +e
legacy_install_output=$(PATH="$legacy_refusal_bin:$PATH" RAMSHARED_LEGACY_TEST_LOG="$legacy_install_log" \
  bash "$legacy_install" 2>&1)
legacy_install_status=$?
set -e
(( legacy_install_status != 0 )) || { echo 'legacy installer unexpectedly succeeded' >&2; exit 1; }
grep -Fqx 'RAMSHARED_LEGACY_INSTALL=STAGING_REFUSED' <<<"$legacy_install_output" || {
  echo 'legacy installer must expose an inert staging refusal' >&2; exit 1;
}
[[ ! -s $legacy_install_log ]] || { echo 'legacy installer reached a mutable fixture command' >&2; exit 1; }

preflight_fixture_repo="$fixture/preflight-repo"
preflight_fixture_bin="$fixture/preflight-bin"
mkdir -p "$preflight_fixture_repo/scripts/safety" "$preflight_fixture_repo/bin" "$preflight_fixture_bin"
cp -- "$legacy_preflight" "$preflight_fixture_repo/scripts/safety/cascade-preflight.sh"
for fixture_binary in ramshared ramsharedd; do
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$preflight_fixture_repo/bin/$fixture_binary"
  chmod 0755 "$preflight_fixture_repo/bin/$fixture_binary"
done
cat >"$preflight_fixture_bin/id" <<'EOF'
#!/usr/bin/env bash
if [[ ${1:-} == -u ]]; then printf '0\n'; else exec /usr/bin/id "$@"; fi
EOF
cat >"$preflight_fixture_bin/modprobe" <<'EOF'
#!/usr/bin/env bash
printf 'modprobe %s\n' "$*" >>"${RAMSHARED_PREFLIGHT_TEST_LOG:?}"
EOF
cat >"$preflight_fixture_bin/nbd-client" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$preflight_fixture_bin/nvidia-smi" <<'EOF'
#!/usr/bin/env bash
printf '65536\n'
EOF
chmod 0755 "$preflight_fixture_bin/id" "$preflight_fixture_bin/modprobe" \
  "$preflight_fixture_bin/nbd-client" "$preflight_fixture_bin/nvidia-smi"
preflight_log="$fixture/preflight.log"
: >"$preflight_log"
set +e
preflight_output=$(PATH="$preflight_fixture_bin:$PATH" RAMSHARED_REPO="$preflight_fixture_repo" \
  RAMSHARED_CLI="$preflight_fixture_repo/bin/ramshared" \
  RAMSHARED_DAEMON="$preflight_fixture_repo/bin/ramsharedd" \
  RAMSHARED_PREFLIGHT_TEST_LOG="$preflight_log" \
  bash "$preflight_fixture_repo/scripts/safety/cascade-preflight.sh" 2>&1)
preflight_status=$?
set -e
(( preflight_status != 0 )) || { echo 'legacy preflight unexpectedly succeeded' >&2; exit 1; }
grep -Fqx 'CASCADE_PREFLIGHT=STAGING_REFUSED' <<<"$preflight_output" || {
  echo 'legacy preflight must expose an inert staging refusal' >&2; exit 1;
}
[[ ! -s $preflight_log ]] || { echo 'legacy preflight reached modprobe before refusal' >&2; exit 1; }
printf '%s\n' 'PASS legacy_installer_and_preflight_are_inert_before_mutation'

original_docker='{"proxies":{"default":{"httpProxy":"http://secret.invalid"}}}'
changed_docker='{"cgroup-parent":"operator-changed.slice"}'
printf '%s\n' "$original_docker" >"$fixture_docker_config"
chmod 0600 "$fixture_docker_config"
export RAMSHARED_TEST_SYSTEMCTL_LOG="$fixture/systemctl.log"

write_memtotal() {
  printf 'MemTotal:       %s kB\n' "$1" >"$fixture_meminfo"
}

write_memtotal 16777216

run_fixture_as_root() {
  if unshare -Ur true >/dev/null 2>&1; then
    unshare -Ur env PATH="$fixture_bin:$PATH" \
      RAMSHARED_TEST_FAIL_TARGET="${RAMSHARED_TEST_FAIL_TARGET:-}" \
      RAMSHARED_TEST_MUTATE_TARGET="${RAMSHARED_TEST_MUTATE_TARGET:-}" \
      RAMSHARED_TEST_FAIL_DAEMON_RELOAD_ONCE="${RAMSHARED_TEST_FAIL_DAEMON_RELOAD_ONCE:-}" \
      "$fixture_manager" "$@"
    return
  fi
  if command -v fakeroot >/dev/null 2>&1; then
    fakeroot env PATH="$fixture_bin:$PATH" \
      RAMSHARED_TEST_FAIL_TARGET="${RAMSHARED_TEST_FAIL_TARGET:-}" \
      RAMSHARED_TEST_MUTATE_TARGET="${RAMSHARED_TEST_MUTATE_TARGET:-}" \
      RAMSHARED_TEST_FAIL_DAEMON_RELOAD_ONCE="${RAMSHARED_TEST_FAIL_DAEMON_RELOAD_ONCE:-}" \
      "$fixture_manager" "$@"
    return
  fi
  echo 'control-plane manager test requires user namespace or fakeroot' >&2
  return 77
}

small_plan=$(write_memtotal 2097152; bash "$fixture_manager" plan)
grep -Fqx 'WORKLOAD_LIMITS=UNAVAILABLE' <<<"$small_plan"
if grep -Eq '^WORKLOAD_MEMORY_(HIGH|MAX)_BYTES=[1-9][0-9]*$' <<<"$small_plan"; then
  echo 'small guests must not receive an unsafe workload limit' >&2
  exit 1
fi

sixteen_gib_plan=$(write_memtotal 16777216; bash "$fixture_manager" plan)
grep -Fqx 'CONTROL_RESERVE_BYTES=4294967296' <<<"$sixteen_gib_plan"
grep -Fqx 'WORKLOAD_MEMORY_HIGH_BYTES=11166914560' <<<"$sixteen_gib_plan"
grep -Fqx 'WORKLOAD_MEMORY_MAX_BYTES=12884901888' <<<"$sixteen_gib_plan"

large_plan=$(write_memtotal 67108864; bash "$fixture_manager" plan)
grep -Fqx 'CONTROL_RESERVE_BYTES=17179869184' <<<"$large_plan"
grep -Fqx 'WORKLOAD_MEMORY_HIGH_BYTES=44667659264' <<<"$large_plan"
grep -Fqx 'WORKLOAD_MEMORY_MAX_BYTES=51539607552' <<<"$large_plan"

write_memtotal 16777216

managed_unit_relatives=(
  ramshared-control.slice
  ramshared-workloads.slice
  ramshared-workloads-docker.slice
  ramshared-workloads-cron.slice
  ramshared-host-gate.service
  ramshared-supervisor.service
  ramshared-cron-workload.service.in
  docker.service.d/10-ramshared-control.conf
  containerd.service.d/10-ramshared-control.conf
  cron.service.d/10-ramshared-control.conf
  ramshared-workloads.slice.d/10-ramshared-guest-memory.conf
)
originals_dir="$fixture/originals"
mkdir -p "$originals_dir"
for relative in "${managed_unit_relatives[@]}"; do
  target="$fixture_unit_dir/$relative"
  expected="$originals_dir/${relative//\//__}"
  mkdir -p "$(dirname -- "$target")"
  printf 'operator-owned-before-install:%s\n' "$relative" >"$expected"
  cp -- "$expected" "$target"
  chmod 0640 "$target"
done
original_control=$(<"$fixture_unit_dir/ramshared-control.slice")

assert_preinstall_targets_restored() {
  local relative target expected
  for relative in "${managed_unit_relatives[@]}"; do
    target="$fixture_unit_dir/$relative"
    expected="$originals_dir/${relative//\//__}"
    cmp -s -- "$expected" "$target" || {
      echo "transaction rollback must restore the exact prior managed file: $relative" >&2
      exit 1
    }
    [[ $(stat -c '%a' -- "$target") == 640 ]] || {
      echo "transaction rollback must restore managed file mode: $relative" >&2
      exit 1
    }
  done
  [[ $(<"$fixture_docker_config") == "$original_docker" ]] || {
    echo 'transaction rollback must restore the exact prior Docker configuration' >&2
    exit 1
  }
  [[ $(stat -c '%a' -- "$fixture_docker_config") == 600 ]] || {
    echo 'transaction rollback must restore Docker configuration mode' >&2
    exit 1
  }
  [[ ! -e "$fixture_state_dir/manifest.tsv" && ! -e "$fixture_state_dir/transaction.tsv" ]] || {
    echo 'transaction rollback must remove install state only after recovery completes' >&2
    exit 1
  }
}

set +e
failed_install_output=$(RAMSHARED_TEST_FAIL_TARGET="$fixture_unit_dir/ramshared-workloads-docker.slice" \
  run_fixture_as_root install --apply 2>&1)
failed_install_status=$?
set -e
(( failed_install_status != 0 )) || {
  echo 'injected mid-stage write failure must fail installation' >&2
  exit 1
}
[[ $(<"$fixture_unit_dir/ramshared-control.slice") == "$original_control" ]] || {
  printf '%s\n' "$failed_install_output" >&2
  echo 'transaction rollback must restore the exact prior unit content' >&2
  exit 1
}
assert_preinstall_targets_restored

: >"$fixture/systemctl.log"
set +e
failed_reload_output=$(RAMSHARED_TEST_FAIL_DAEMON_RELOAD_ONCE=1 \
  run_fixture_as_root install --apply 2>&1)
failed_reload_status=$?
set -e
(( failed_reload_status != 0 )) || {
  echo 'injected daemon-reload failure must fail installation' >&2
  exit 1
}
assert_preinstall_targets_restored
[[ $(grep -Fxc 'daemon-reload' "$fixture/systemctl.log") == 2 ]] || {
  printf '%s\n' "$failed_reload_output" >&2
  echo 'daemon-reload failure must reload the recovered control-plane state' >&2
  exit 1
}

install_output=$(run_fixture_as_root install --apply)
grep -Fqx 'RAMSHARED_CONTROL_PLANE=INSTALLED_DISABLED' <<<"$install_output"
[[ $(stat -c '%a' -- "$fixture_docker_config") == 600 ]] || {
  echo 'manager must preserve restrictive Docker configuration permissions' >&2
  exit 1
}
[[ $(<"$fixture_docker_config") == "$original_docker" ]] || {
  echo 'disabled staging must leave Docker configuration untouched' >&2
  exit 1
}
installed_docker="$fixture/installed-daemon.json"
cp -- "$fixture_docker_config" "$installed_docker"
installed_control="$fixture/installed-control.slice"
cp -- "$fixture_unit_dir/ramshared-control.slice" "$installed_control"
installed_targets_dir="$fixture/installed-targets"
mkdir -p "$installed_targets_dir"
for relative in "${managed_unit_relatives[@]}"; do
  cp -- "$fixture_unit_dir/$relative" "$installed_targets_dir/${relative//\//__}"
done
grep -Fqx 'MemoryHigh=11166914560' "$fixture_unit_dir/ramshared-workloads.slice.d/10-ramshared-guest-memory.conf"
grep -Fqx 'MemoryMax=12884901888' "$fixture_unit_dir/ramshared-workloads.slice.d/10-ramshared-guest-memory.conf"

: >"$fixture/systemctl.log"
set +e
failed_uninstall_output=$(RAMSHARED_TEST_FAIL_DAEMON_RELOAD_ONCE=1 \
  run_fixture_as_root uninstall --apply 2>&1)
failed_uninstall_status=$?
set -e
(( failed_uninstall_status != 0 )) || {
  echo 'injected daemon-reload failure must fail uninstall' >&2
  exit 1
}
for relative in "${managed_unit_relatives[@]}"; do
  cmp -s -- "$installed_targets_dir/${relative//\//__}" "$fixture_unit_dir/$relative" || {
    printf '%s\n' "$failed_uninstall_output" >&2
    echo "uninstall reload failure must restore the exact installed target: $relative" >&2
    exit 1
  }
done
cmp -s -- "$installed_docker" "$fixture_docker_config" || {
  echo 'uninstall reload failure must restore the exact installed Docker configuration' >&2
  exit 1
}
[[ -f "$fixture_state_dir/manifest.tsv" && ! -L "$fixture_state_dir/manifest.tsv" ]] || {
  echo 'uninstall reload failure must retain rollback authority' >&2
  exit 1
}
run_fixture_as_root status >/dev/null || {
  echo 'uninstall reload failure must retain a valid installed manifest' >&2
  exit 1
}
[[ $(grep -Fxc 'daemon-reload' "$fixture/systemctl.log") == 2 ]] || {
  printf '%s\n' "$failed_uninstall_output" >&2
  echo 'uninstall daemon-reload failure must reload the restored installed state' >&2
  exit 1
}

printf '%s\n' 'operator-owned-content' >"$fixture_unit_dir/ramshared-control.slice"
if bash "$fixture_manager" status >/dev/null 2>&1; then
  echo 'stale manifest must not count as installed' >&2
  exit 1
fi
if run_fixture_as_root uninstall --apply; then
  echo 'manager must refuse rollback after an operator changes a managed unit' >&2
  exit 1
fi
[[ $(<"$fixture_unit_dir/ramshared-control.slice") == 'operator-owned-content' ]] || {
  echo 'manager must preserve an operator-modified managed unit' >&2
  exit 1
}
if run_fixture_as_root install --apply; then
  echo 'retry must refuse a stale owned receipt rather than overwrite it' >&2
  exit 1
fi
[[ $(<"$fixture_unit_dir/ramshared-control.slice") == 'operator-owned-content' ]] || {
  echo 'retry overwrote an operator-modified managed unit' >&2
  exit 1
}
cp -- "$installed_control" "$fixture_unit_dir/ramshared-control.slice"

printf '%s\n' "$changed_docker" >"$fixture_docker_config"
chmod 0600 "$fixture_docker_config"

rollback_output=$(run_fixture_as_root uninstall --apply)
grep -Fqx 'RAMSHARED_CONTROL_PLANE=ROLLED_BACK' <<<"$rollback_output"
[[ $(<"$fixture_docker_config") == "$changed_docker" ]] || {
  echo 'manager rollback must not modify an ambient Docker configuration' >&2
  exit 1
}
[[ $(stat -c '%a' -- "$fixture_docker_config") == 600 ]] || {
  echo 'manager must preserve ambient Docker configuration permissions' >&2
  exit 1
}
[[ -f "$fixture_unit_dir/ramshared-control.slice" && ! -L "$fixture_unit_dir/ramshared-control.slice" ]] || {
  echo 'manager rollback must restore prior control-plane units' >&2
  exit 1
}
[[ $(<"$fixture_unit_dir/ramshared-control.slice") == "$original_control" ]] || {
  echo 'manager rollback must restore the original control-plane unit' >&2
  exit 1
}

printf 'not-a-control-plane-manifest\n' >"$fixture_state_dir/manifest.tsv"
if bash "$fixture_manager" status >/dev/null 2>&1; then
  echo 'malformed manifest must not count as installed' >&2
  exit 1
fi
printf 'RAMSHARED_CONTROL_PLANE_MANIFEST_V1\t0000000000000000000000000000000000000000000000000000000000000000\nentry\tremove\t/etc/passwd\t-\t-\t-\t0000000000000000000000000000000000000000000000000000000000000000\t0:0:644\n' >"$fixture_state_dir/manifest.tsv"
if bash "$fixture_manager" status >/dev/null 2>&1; then
  echo 'foreign manifest must not count as installed' >&2
  exit 1
fi
rm -f -- "$fixture_state_dir/manifest.tsv"
echo 'PASS control_plane_install_is_plan_first_disabled_and_reversible'
echo 'PASS control_plane_rollback_preserves_operator_configuration'
echo 'PASS control_plane_staging_is_transactional_identity_bound_and_guest_aware'
echo 'PASS legacy_cascade_app_start_is_inert_and_never_reaches_preflight_or_up'
