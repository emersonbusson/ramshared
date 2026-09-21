#!/usr/bin/env bash
# Regression tests for the legacy package service's swapoff-first stop path.
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
service_script="$repo_root/packaging/scripts/ramshared-vram-service.sh"
fixture_dir=$(mktemp -d)
trap 'command rm -f -- "$fixture_dir/pid" "$fixture_dir/output" "$fixture_dir/log" "$fixture_dir/socket" "$fixture_dir/swap-dev" "$fixture_dir/capacity-guaranteed"; rmdir -- "$fixture_dir"' EXIT

# Source only the function definition: the production script has top-level
# host setup and dispatch that must never run inside a regression test.
stop_definition=$(sed -n '/^stop_tier() {/,/^}/p' "$service_script")
[[ $stop_definition == 'stop_tier() {'* ]] || {
    echo 'stop_tier definition missing' >&2
    exit 1
}
source <(printf '%s\n' "$stop_definition")

NBD_DEV=/dev/nbd-fixture
DAEMON_BIN=/usr/local/bin/ramsharedd
PID_FILE="$fixture_dir/pid"
SOCK_PATH="$fixture_dir/socket"
SWAP_DEV_FILE="$fixture_dir/swap-dev"
ZRAM_DEV_FILE="$fixture_dir/zram-dev"
CAPACITY_STATUS_FILE="$fixture_dir/capacity-guaranteed"
printf '4242\n' > "$PID_FILE"

swap_active=1
swapoff_result=1
swapoff_calls=0
disconnect_calls=0
disconnect_result=0
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
rm() { remove_calls=$((remove_calls + 1)); }
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
