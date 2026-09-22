#!/usr/bin/env bash
# Regression tests for the legacy package service's swapoff-first stop path.
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
service_script="$repo_root/packaging/scripts/ramshared-vram-service.sh"
fixture_dir=$(mktemp -d)
trap 'command rm -f -- "$fixture_dir/pid" "$fixture_dir/pid-target" "$fixture_dir/output" "$fixture_dir/log" "$fixture_dir/socket" "$fixture_dir/swap-dev" "$fixture_dir/swaps" "$fixture_dir/zram-dev" "$fixture_dir/capacity-guaranteed" "$fixture_dir/nbd-sysfs/size" "$fixture_dir/nbd-sysfs/pid"; if [[ -d "$fixture_dir/nbd-sysfs" ]]; then rmdir -- "$fixture_dir/nbd-sysfs"; fi; rmdir -- "$fixture_dir"' EXIT

# Source only the function definition: the production script has top-level
# host setup and dispatch that must never run inside a regression test.
stop_definition=$(sed -n '/^stop_tier() {/,/^}/p' "$service_script")
[[ $stop_definition == 'stop_tier() {'* ]] || {
    echo 'stop_tier definition missing' >&2
    exit 1
}
source <(printf '%s\n' "$stop_definition")
stop_managed_zram() { :; }

NBD_DEV=/dev/nbd-fixture
DAEMON_BIN=/usr/local/bin/ramsharedd
PID_FILE="$fixture_dir/pid"
SOCK_PATH="$fixture_dir/socket"
SWAP_DEV_FILE="$fixture_dir/swap-dev"
ZRAM_DEV_FILE="$fixture_dir/zram-dev"
CAPACITY_STATUS_FILE="$fixture_dir/capacity-guaranteed"
printf '4242\n' > "$PID_FILE"

swap_active=1
nbd_swap_active() { (( swap_active == 1 )); }
nbd_swap_absent() { (( swap_active == 0 )); }
swapoff_result=1
swapoff_calls=0
disconnect_calls=0
disconnect_result=0
nbd_connected=1
nbd_connection_absent() { (( nbd_connected == 0 )); }
kill_calls=0
kill_signals=()
remove_calls=0
daemon_alive=1
mock_exe=$DAEMON_BIN

grep() {
    if [[ ${1:-} == -q && ${2:-} == "$NBD_DEV" && ${3:-} == /proc/swaps ]]; then
        (( swap_active == 1 ))
    else
        return 1
    fi
}

swapoff() {
    swapoff_calls=$((swapoff_calls + 1))
    if (( swapoff_result == 0 )); then
        swap_active=0
    fi
    return "$swapoff_result"
}

nbd-client() {
    disconnect_calls=$((disconnect_calls + 1))
    if (( nbd_connected == 0 )); then
        return 1
    fi
    if (( disconnect_result == 0 )); then
        nbd_connected=0
    fi
    return "$disconnect_result"
}
kill() {
    if [[ ${1:-} == -0 ]]; then
        (( daemon_alive == 1 ))
    else
        kill_calls=$((kill_calls + 1))
        kill_signals+=("${1:-}")
        if [[ ${1:-} == -TERM || ${1:-} == -9 ]]; then
            daemon_alive=0
        fi
    fi
}
readlink() { printf '%s\n' "$mock_exe"; }
rm() {
    remove_calls=$((remove_calls + 1))
    local path
    for path in "$@"; do
        [[ $path == -f ]] && continue
        [[ $path == "$fixture_dir/"* ]] || return 90
    done
    command rm "$@"
}
sleep() { :; }
zramctl() { :; }

set +e
stop_tier > "$fixture_dir/output" 2>&1
status=$?
set -e

if (( status == 0 || swapoff_calls != 1 || disconnect_calls != 0 || kill_calls != 0 || remove_calls != 0 )); then
    printf 'failed swapoff must refuse teardown: status=%s swapoff=%s disconnect=%s kill=%s remove=%s\n' \
        "$status" "$swapoff_calls" "$disconnect_calls" "$kill_calls" "$remove_calls" >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi

echo 'PASS legacy VRAM service refuses disconnect and daemon stop after failed swapoff'

# A stale/reused PID must not be treated as ownership of the live NBD tier.
swapoff_result=0
swapoff_calls=0
disconnect_calls=0
kill_calls=0
remove_calls=0
daemon_alive=1
mock_exe=/usr/local/bin/unrelated-daemon

set +e
stop_tier > "$fixture_dir/output" 2>&1
status=$?
set -e

if (( status == 0 || swapoff_calls != 0 || disconnect_calls != 0 || kill_calls != 0 || remove_calls != 0 || swap_active != 1 )); then
    printf 'foreign PID must refuse before mutation: status=%s swapoff=%s disconnect=%s kill=%s remove=%s active=%s\n' \
        "$status" "$swapoff_calls" "$disconnect_calls" "$kill_calls" "$remove_calls" "$swap_active" >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi

echo 'PASS legacy VRAM service refuses foreign daemon identity before teardown'

# A failed detach must not permit daemon termination or state deletion.
swap_active=1
swapoff_result=0
swapoff_calls=0
disconnect_calls=0
disconnect_result=1
nbd_connected=1
kill_calls=0
kill_signals=()
remove_calls=0
daemon_alive=1
mock_exe=$DAEMON_BIN

set +e
stop_tier > "$fixture_dir/output" 2>&1
status=$?
set -e

if (( status == 0 || swapoff_calls != 1 || disconnect_calls != 1 || kill_calls != 0 || remove_calls != 0 )); then
    printf 'failed NBD detach must retain daemon and state: status=%s swapoff=%s disconnect=%s kill=%s remove=%s\n' \
        "$status" "$swapoff_calls" "$disconnect_calls" "$kill_calls" "$remove_calls" >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi

echo 'PASS legacy VRAM service retains daemon and state after failed NBD detach'

# A successful stop uses one graceful signal, never SIGKILL.
swap_active=1
swapoff_calls=0
disconnect_calls=0
disconnect_result=0
nbd_connected=1
kill_calls=0
kill_signals=()
remove_calls=0
daemon_alive=1

set +e
stop_tier > "$fixture_dir/output" 2>&1
status=$?
set -e

if (( status != 0 || swapoff_calls != 1 || disconnect_calls != 1 || kill_calls != 1 || remove_calls != 1 )) \
    || [[ ${kill_signals[*]} != '-TERM' ]]; then
    printf 'successful stop must use TERM only: status=%s swapoff=%s disconnect=%s kill=%s signals=%s remove=%s\n' \
        "$status" "$swapoff_calls" "$disconnect_calls" "$kill_calls" "${kill_signals[*]}" "$remove_calls" >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi

echo 'PASS legacy VRAM service stops its daemon gracefully after confirmed detach'

# stop is replayable: the second invocation may not detach or signal again.
swapoff_calls=0
disconnect_calls=0
kill_calls=0
remove_calls=0
set +e
stop_tier > "$fixture_dir/output" 2>&1
status=$?
set -e
if (( status != 0 || swapoff_calls != 0 || disconnect_calls != 0 || kill_calls != 0 || remove_calls != 0 )); then
    printf 'second stop must be an owned no-op: status=%s swapoff=%s disconnect=%s kill=%s remove=%s\n' \
        "$status" "$swapoff_calls" "$disconnect_calls" "$kill_calls" "$remove_calls" >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi

# A connected NBD with no daemon record is foreign/unknown, not an idle tier.
nbd_connected=1
disconnect_calls=0
set +e
stop_tier > "$fixture_dir/output" 2>&1
status=$?
set -e
if (( status == 0 || disconnect_calls != 0 || nbd_connected != 1 )); then
    printf 'unowned connected NBD must refuse: status=%s disconnect=%s connected=%s\n' \
        "$status" "$disconnect_calls" "$nbd_connected" >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi

# A leftover ownership marker is not an idempotent clean state.
nbd_connected=0
printf 'stale\n' > "$SWAP_DEV_FILE"
disconnect_calls=0
set +e
stop_tier > "$fixture_dir/output" 2>&1
status=$?
set -e
if (( status == 0 || disconnect_calls != 0 )) || [[ ! -f $SWAP_DEV_FILE ]]; then
    echo 'stale service marker must block no-op stop without deleting evidence' >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi
command rm -f -- "$SWAP_DEV_FILE"

echo 'PASS legacy VRAM service makes clean stop replayable without detaching an unowned NBD'

# A symlinked PID record can redirect ownership checks to attacker-chosen
# content and must not authorize swapoff or daemon termination.
printf '4242\n' > "$fixture_dir/pid-target"
ln -s "$fixture_dir/pid-target" "$PID_FILE"
swap_active=1
nbd_connected=1
daemon_alive=1
mock_exe=$DAEMON_BIN
swapoff_result=0
disconnect_result=0
swapoff_calls=0
disconnect_calls=0
kill_calls=0
remove_calls=0
set +e
stop_tier > "$fixture_dir/output" 2>&1
status=$?
set -e
if (( status == 0 || swapoff_calls != 0 || disconnect_calls != 0 || kill_calls != 0 || remove_calls != 0 )) \
    || [[ ! -L $PID_FILE ]]; then
    printf 'symlinked PID must refuse before mutation: status=%s swapoff=%s disconnect=%s kill=%s remove=%s\n' \
        "$status" "$swapoff_calls" "$disconnect_calls" "$kill_calls" "$remove_calls" >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi
command rm -f -- "$PID_FILE" "$fixture_dir/pid-target"

echo 'PASS legacy VRAM service refuses symlinked daemon ownership record'

# A boot-time legacy start may not adopt an NBD swap created by another path.
start_definition=$(sed -n '/^start_tier() {/,/^}/p' "$service_script")
[[ $start_definition == 'start_tier() {'* ]] || {
    echo 'start_tier definition missing' >&2
    exit 1
}
source <(printf '%s\n' "$start_definition")
setup_protected_cgroup() { :; }
modprobe() { :; }
detect_vram_capacity() { printf '1024\n'; }
pgrep() { printf '4242\n'; }
chmod() { :; }
ZRAM_MIB=0
swap_active=1
daemon_alive=1
printf 'preserve-pid\n' > "$PID_FILE"
printf 'preserve-swap\n' > "$SWAP_DEV_FILE"
printf 'preserve-capacity\n' > "$CAPACITY_STATUS_FILE"

set +e
start_tier > "$fixture_dir/output" 2>&1
status=$?
set -e

if (( status == 0 )) || [[ $(<"$PID_FILE") != preserve-pid ]] \
    || [[ $(<"$SWAP_DEV_FILE") != preserve-swap ]] \
    || [[ $(<"$CAPACITY_STATUS_FILE") != preserve-capacity ]]; then
    printf 'start must refuse existing NBD swap without adopting records: status=%s pid=%s swap=%s capacity=%s\n' \
        "$status" "$(<"$PID_FILE")" "$(<"$SWAP_DEV_FILE")" "$(<"$CAPACITY_STATUS_FILE")" >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi

echo 'PASS legacy VRAM service refuses to adopt active NBD swap'

# The activation seam must not publish a capacity guarantee until the NBD
# device is genuinely active in /proc/swaps. These commands are all mocked;
# the fixture never connects, formats, or enables a real block device.
activation_definition=$(sed -n '/^activate_nbd_tier() {/,/^}/p' "$service_script")
[[ $activation_definition == 'activate_nbd_tier() {'* ]] || {
    echo 'activate_nbd_tier definition missing' >&2
    exit 1
}
source <(printf '%s\n' "$activation_definition")
nbd_device_ready() { return 0; }
mkswap() { return "$mkswap_result"; }
swapon() {
    if (( swapon_result == 0 && publish_swap == 1 )); then
        swap_active=1
    fi
    return "$swapon_result"
}
nbd_result=0
mkswap_result=0
swapon_result=0
publish_swap=1
nbd-client() {
    disconnect_calls=$((disconnect_calls + 1))
    return "$nbd_result"
}

for failure in nbd mkswap swapon missing_swap; do
    swap_active=0
    nbd_result=0
    mkswap_result=0
    swapon_result=0
    publish_swap=1
    disconnect_calls=0
    command rm -f -- "$SWAP_DEV_FILE" "$CAPACITY_STATUS_FILE"
    case "$failure" in
        nbd) nbd_result=1 ;;
        mkswap) mkswap_result=1 ;;
        swapon) swapon_result=1 ;;
        missing_swap) publish_swap=0 ;;
    esac

    set +e
    activate_nbd_tier 'fixture backend' 1024 > "$fixture_dir/output" 2>&1
    status=$?
    set -e

    if (( status == 0 )) || [[ -e $SWAP_DEV_FILE || -e $CAPACITY_STATUS_FILE ]]; then
        printf '%s activation failure must not publish capacity: status=%s swap=%s capacity=%s\n' \
            "$failure" "$status" "$SWAP_DEV_FILE" "$CAPACITY_STATUS_FILE" >&2
        sed -n '1,20p' "$fixture_dir/output" >&2
        exit 1
    fi
done

swap_active=0
nbd_result=0
mkswap_result=0
swapon_result=0
publish_swap=1
command rm -f -- "$SWAP_DEV_FILE" "$CAPACITY_STATUS_FILE"
activate_nbd_tier 'fixture backend' 1024 > "$fixture_dir/output" 2>&1
if (( swap_active != 1 )) || [[ $(<"$SWAP_DEV_FILE") != "$NBD_DEV" ]] \
    || [[ $(<"$CAPACITY_STATUS_FILE") != 1 ]]; then
    echo 'successful activation must publish confirmed NBD capacity' >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi

echo 'PASS legacy VRAM service publishes capacity only after confirmed NBD swap'

# Existing daemon state must be rejected before ZRAM/cgroup setup. The mocked
# bash launcher guarantees a regression cannot start a host daemon.
setup_calls=0
setup_protected_cgroup() { setup_calls=$((setup_calls + 1)); }
bash() { return 1; }
LOG_FILE="$fixture_dir/log"
ZRAM_MIB=0
pgrep_running=0
pgrep() {
    if (( pgrep_running == 1 )); then
        printf '4242\n'
    else
        return 1
    fi
}

for existing_state in pid socket daemon; do
    swap_active=0
    setup_calls=0
    remove_calls=0
    pgrep_running=0
    command rm -f -- "$PID_FILE" "$SOCK_PATH"
    case "$existing_state" in
        pid) printf '4242\n' > "$PID_FILE" ;;
        socket) touch "$SOCK_PATH" ;;
        daemon) pgrep_running=1 ;;
    esac

    set +e
    start_tier > "$fixture_dir/output" 2>&1
    status=$?
    set -e

    if (( status == 0 || setup_calls != 0 || remove_calls != 0 )); then
        printf 'existing %s must refuse before any startup mutation: status=%s setup=%s remove=%s\n' \
            "$existing_state" "$status" "$setup_calls" "$remove_calls" >&2
        sed -n '1,20p' "$fixture_dir/output" >&2
        exit 1
    fi
done

echo 'PASS legacy VRAM service refuses startup against existing PID, socket, or daemon'

# Boot-time self-deployment cannot qualify binary identity or perform the
# attended swap handoff. Assert the deprecated entry point has no installer or
# restart side effects without executing the historical host-mutating script.
auto_deploy_script="$repo_root/packaging/scripts/ramshared-auto-deploy.sh"
if command grep -Eq '(^|[[:space:]])(cp|rsync|install|systemctl)[[:space:]]|ramshared-vram-service[.]sh restart' "$auto_deploy_script"; then
    echo 'legacy auto-deploy must not copy binaries or restart the tier' >&2
    exit 1
fi

echo 'PASS legacy auto-deploy has no boot-time install or restart side effects'

# Only the device recorded by this service may be reset. In particular a
# failed swapoff must never be followed by zramctl --reset.
zram_stop_definition=$(sed -n '/^stop_managed_zram() {/,/^}/p' "$service_script")
[[ $zram_stop_definition == 'stop_managed_zram() {'* ]] || {
    echo 'stop_managed_zram definition missing' >&2
    exit 1
}
source <(printf '%s\n' "$zram_stop_definition")
zram_swap_active() { (( zram_active == 1 )); }
managed_zram=/dev/zram7
zram_active=1
zram_swapoff_result=1
zram_swapoff_calls=0
zram_reset_result=0
zram_reset_calls=0
grep() {
    if [[ ${1:-} == -q && ${2:-} == "$managed_zram" && ${3:-} == /proc/swaps ]]; then
        (( zram_active == 1 ))
    else
        return 1
    fi
}
swapoff() {
    zram_swapoff_calls=$((zram_swapoff_calls + 1))
    if (( zram_swapoff_result == 0 )); then
        zram_active=0
    fi
    return "$zram_swapoff_result"
}
zramctl() {
    zram_reset_calls=$((zram_reset_calls + 1))
    return "$zram_reset_result"
}

command rm -f -- "$ZRAM_DEV_FILE"
set +e
stop_managed_zram > "$fixture_dir/output" 2>&1
status=$?
set -e
if (( status != 0 || zram_swapoff_calls != 0 || zram_reset_calls != 0 )); then
    echo 'absent ownership record must leave other ZRAM devices untouched' >&2
    exit 1
fi

printf '%s\n' "$managed_zram" > "$ZRAM_DEV_FILE"
set +e
stop_managed_zram > "$fixture_dir/output" 2>&1
status=$?
set -e
if (( status == 0 || zram_swapoff_calls != 1 || zram_reset_calls != 0 )) || [[ ! -f $ZRAM_DEV_FILE ]]; then
    echo 'failed managed ZRAM swapoff must retain device and ownership record' >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi

zram_swapoff_result=0
zram_swapoff_calls=0
zram_reset_calls=0
remove_calls=0
stop_managed_zram > "$fixture_dir/output" 2>&1
if (( zram_swapoff_calls != 1 || zram_reset_calls != 1 || remove_calls != 1 )); then
    echo 'confirmed managed ZRAM swapoff must precede reset and marker removal' >&2
    sed -n '1,20p' "$fixture_dir/output" >&2
    exit 1
fi

echo 'PASS legacy VRAM service resets only recorded ZRAM after confirmed swapoff'

# ZRAM setup must not adopt an unrelated active device or report success when
# its own mkswap/swapon fails. No real ZRAM command is executed in this fixture.
zram_start_definition=$(sed -n '/^start_managed_zram() {/,/^}/p' "$service_script")
[[ $zram_start_definition == 'start_managed_zram() {'* ]] || {
    echo 'start_managed_zram definition missing' >&2
    exit 1
}
source <(printf '%s\n' "$zram_start_definition")
unmanaged_zram_active=0
any_zram_swap_active() { (( unmanaged_zram_active == 1 )); }
zram_device_ready() { return 0; }
zram_allocations=0
zramctl() {
    if [[ ${1:-} == --find ]]; then
        zram_allocations=$((zram_allocations + 1))
        printf '%s\n' "$managed_zram"
    else
        zram_reset_calls=$((zram_reset_calls + 1))
    fi
}
mkswap() { return "$zram_mkswap_result"; }
swapon() {
    if (( zram_swapon_result == 0 )); then
        zram_active=1
    fi
    return "$zram_swapon_result"
}
ZRAM_MIB=1024
command rm -f -- "$ZRAM_DEV_FILE"
zram_allocations=0
unmanaged_zram_active=1
start_managed_zram > "$fixture_dir/output" 2>&1
if (( zram_allocations != 0 )) || [[ -e $ZRAM_DEV_FILE ]]; then
    echo 'existing unmanaged ZRAM must not be allocated or adopted' >&2
    exit 1
fi

unmanaged_zram_active=0
zram_mkswap_result=1
zram_swapon_result=0
set +e
start_managed_zram > "$fixture_dir/output" 2>&1
status=$?
set -e
if (( status == 0 )); then
    echo 'failed ZRAM mkswap must make startup fail' >&2
    exit 1
fi

command rm -f -- "$ZRAM_DEV_FILE"
zram_mkswap_result=0
zram_swapon_result=1
zram_active=0
set +e
start_managed_zram > "$fixture_dir/output" 2>&1
status=$?
set -e
if (( status == 0 || zram_active != 0 )); then
    echo 'failed ZRAM swapon must make startup fail without active claim' >&2
    exit 1
fi

echo 'PASS legacy VRAM service does not adopt unmanaged or failed ZRAM setup'

# A substring probe for /dev/nbd0 must not match /dev/nbd01 in /proc/swaps.
swap_check_definition=$(sed -n '/^swap_device_active() {/,/^}/p' "$service_script")
[[ $swap_check_definition == 'swap_device_active() {'* ]] || {
    echo 'swap_device_active definition missing' >&2
    exit 1
}
source <(printf '%s\n' "$swap_check_definition")
swap_absent_definition=$(sed -n '/^swap_device_absent() {/,/^}/p' "$service_script")
[[ $swap_absent_definition == 'swap_device_absent() {'* ]] || {
    echo 'swap_device_absent definition missing' >&2
    exit 1
}
source <(printf '%s\n' "$swap_absent_definition")
NBD_DEV=/dev/nbd0
printf 'Filename\tType\tSize\tUsed\tPriority\n/dev/nbd01\tpartition\t1024\t0\t50\n' > "$fixture_dir/swaps"
if swap_device_active "$NBD_DEV" "$fixture_dir/swaps"; then
    echo 'exact swap probe must not accept a longer device name' >&2
    exit 1
fi
printf '/dev/nbd0\tpartition\t1024\t0\t50\n' >> "$fixture_dir/swaps"
if ! swap_device_active "$NBD_DEV" "$fixture_dir/swaps"; then
    echo 'exact swap probe must detect its own device' >&2
    exit 1
fi
printf 'Filename\tType\tSize\tUsed\tPriority\n/nbd01\tpartition\t1024\t0\t50\n' > "$fixture_dir/swaps"
if swap_device_active "$NBD_DEV" "$fixture_dir/swaps"; then
    echo 'kernel-style alias must still reject longer device names' >&2
    exit 1
fi
printf '/nbd0\tpartition\t1024\t0\t50\n' >> "$fixture_dir/swaps"
if ! swap_device_active "$NBD_DEV" "$fixture_dir/swaps"; then
    echo 'kernel-style /nbd alias must match its /dev/nbd device' >&2
    exit 1
fi
any_zram_definition=$(sed -n '/^any_zram_swap_active() {/,/^}/p' "$service_script")
source <(printf '%s\n' "$any_zram_definition")
printf 'Filename\tType\tSize\tUsed\tPriority\n/zram7\tpartition\t1024\t0\t100\n' > "$fixture_dir/swaps"
if ! any_zram_swap_active "$fixture_dir/swaps"; then
    echo 'kernel-style /zram alias must count as an existing ZRAM swap' >&2
    exit 1
fi
if swap_device_absent "$NBD_DEV" "$fixture_dir"; then
    echo 'unreadable or non-file swap table must not count as confirmed absence' >&2
    exit 1
fi
printf 'unexpected header\n' > "$fixture_dir/swaps"
if swap_device_absent "$NBD_DEV" "$fixture_dir/swaps"; then
    echo 'malformed swap table must not count as confirmed absence' >&2
    exit 1
fi
if command grep -Eq 'grep -q "\$NBD_DEV" /proc/swaps' "$service_script"; then
    echo 'NBD paths must use the exact swap-device probe' >&2
    exit 1
fi

echo 'PASS legacy VRAM service matches exact block devices and kernel-style aliases'

# A no-op stop requires independent kernel evidence that NBD is disconnected.
connection_definition=$(sed -n '/^nbd_connection_absent() {/,/^}/p' "$service_script")
[[ $connection_definition == 'nbd_connection_absent() {'* ]] || {
    echo 'nbd_connection_absent definition missing' >&2
    exit 1
}
source <(printf '%s\n' "$connection_definition")
mkdir -p "$fixture_dir/nbd-sysfs"
printf '0\n' > "$fixture_dir/nbd-sysfs/size"
if ! nbd_connection_absent "$fixture_dir/nbd-sysfs"; then
    echo 'zero-size NBD without kernel PID must count as disconnected' >&2
    exit 1
fi
printf '8\n' > "$fixture_dir/nbd-sysfs/size"
if nbd_connection_absent "$fixture_dir/nbd-sysfs"; then
    echo 'positive-size NBD without kernel PID must not count as disconnected' >&2
    exit 1
fi
printf '0\n' > "$fixture_dir/nbd-sysfs/size"
printf '654\n' > "$fixture_dir/nbd-sysfs/pid"
if nbd_connection_absent "$fixture_dir/nbd-sysfs"; then
    echo 'kernel PID must block disconnected classification even at zero size' >&2
    exit 1
fi
command rm -f -- "$fixture_dir/nbd-sysfs/pid"
printf 'unknown\n' > "$fixture_dir/nbd-sysfs/size"
if nbd_connection_absent "$fixture_dir/nbd-sysfs"; then
    echo 'malformed kernel size must not count as disconnected' >&2
    exit 1
fi

echo 'PASS legacy VRAM service verifies kernel NBD disconnection before no-op stop'
