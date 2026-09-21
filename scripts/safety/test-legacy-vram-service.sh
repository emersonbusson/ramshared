#!/usr/bin/env bash
# Regression tests for the legacy package service's swapoff-first stop path.
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
service_script="$repo_root/packaging/scripts/ramshared-vram-service.sh"
fixture_dir=$(mktemp -d)
trap 'command rm -f -- "$fixture_dir/pid" "$fixture_dir/output"; rmdir -- "$fixture_dir"' EXIT

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
kill_calls=0
remove_calls=0
daemon_alive=1
observed_exe=$DAEMON_BIN

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

nbd-client() { disconnect_calls=$((disconnect_calls + 1)); }
kill() {
    if [[ ${1:-} == -0 ]]; then
        (( daemon_alive == 1 ))
    else
        kill_calls=$((kill_calls + 1))
        if [[ ${1:-} == -TERM || ${1:-} == -9 ]]; then
            daemon_alive=0
        fi
    fi
}
readlink() { printf '%s\n' "$observed_exe"; }
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
observed_exe=/usr/local/bin/unrelated-daemon

set +e
stop_tier > "$fixture_dir/output" 2>&1
status=$?
set -e

if (( status == 0 || swapoff_calls != 0 || disconnect_calls != 0 || kill_calls != 0 || remove_calls != 0 || swap_active != 1 )); then
    printf 'foreign PID must refuse before mutation: status=%s swapoff=%s disconnect=%s kill=%s remove=%s active=%s\n' \
        "$status" "$swapoff_calls" "$disconnect_calls" "$kill_calls" "$remove_calls" "$swap_active" >&2
    exit 1
fi

echo 'PASS legacy VRAM service refuses foreign daemon identity before teardown'
