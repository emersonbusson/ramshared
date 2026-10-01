#!/usr/bin/env bash
# test-resource-config-helper.sh — manufactured tests for
# scripts/linux/ramshared-resource-config-helper.
#
# These are manufactured FS/command-matrix tests. They never touch real swap,
# never call swapoff on the host, and never load a module. Every external
# command is a stub under $TEST_ROOT/bin.
#
# Test names are contractual (see
# docs/specs/no-milestone/resource-configuration-center/SPEC.md):
#   linux_swap_apply_creates_only_owned_file_and_unit
#   linux_swap_apply_preserves_old_active_swap_on_failure
#   linux_swap_cleanup_refuses_foreign_active_or_used_target
#   linux_swap_cleanup_replay_is_idempotent
#   native_origin_creation_refuses_existing_file_or_capacity_shortfall
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
helper="$repo_root/scripts/linux/ramshared-resource-config-helper"
[[ -f $helper ]] || { echo "helper missing: $helper" >&2; exit 1; }

TEST_ROOT=$(mktemp -d)
trap 'rm -rf -- "$TEST_ROOT"' EXIT

export RAMSHARED_RESOURCE_CONFIG_TEST_ROOT="$TEST_ROOT"
MANAGED="$TEST_ROOT/var/lib/ramshared/swap"
ORIGIN="$TEST_ROOT/var/lib/ramshared/origin"
UNIT_DIR="$TEST_ROOT/etc/systemd/system"
BIN="$TEST_ROOT/bin"
SWAPS="$TEST_ROOT/proc/swaps"
mkdir -p -- "$MANAGED" "$ORIGIN" "$UNIT_DIR" "$BIN" "$TEST_ROOT/proc" "$TEST_ROOT/foreign"

# --- stubs -------------------------------------------------------------------

# mkswap: fails when $TEST_ROOT/control/mkswap-fail exists.
cat >"$BIN/mkswap" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
root=${RAMSHARED_RESOURCE_CONFIG_TEST_ROOT:?}
if [[ -e $root/control/mkswap-fail ]]; then
    echo 'mkswap: manufactured failure' >&2
    exit 1
fi
target=
for arg in "$@"; do
    [[ $arg == -- ]] && continue
    target=$arg
done
[[ -n $target && -f $target ]] || { echo 'mkswap: no target' >&2; exit 1; }
exit 0
STUB

# swapon: fails when $TEST_ROOT/control/swapon-fail exists; otherwise appends
# the target to the manufactured /proc/swaps.
cat >"$BIN/swapon" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
root=${RAMSHARED_RESOURCE_CONFIG_TEST_ROOT:?}
if [[ -e $root/control/swapon-fail ]]; then
    echo 'swapon: manufactured failure' >&2
    exit 1
fi
target=
for arg in "$@"; do
    [[ $arg == -- ]] && continue
    target=$arg
done
[[ -n $target ]] || { echo 'swapon: no target' >&2; exit 1; }
printf '%s\tfile\t%d\t0\t-2\n' "$target" "$(stat -c '%s' -- "$target")" >>"$root/proc/swaps"
exit 0
STUB

# swapoff: a successful run is a test failure — the helper must never call it.
cat >"$BIN/swapoff" <<'STUB'
#!/usr/bin/env bash
root=${RAMSHARED_RESOURCE_CONFIG_TEST_ROOT:?}
echo 'SWAP_OFF_CALLED' >>"$root/control/swapoff-calls"
echo 'swapoff: must never be called by the resource-config helper' >&2
exit 99
STUB

# systemctl: stub that only understands is-enabled/disable.
cat >"$BIN/systemctl" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
root=${RAMSHARED_RESOURCE_CONFIG_TEST_ROOT:?}
cmd=${1:-}
shift || true
case "$cmd" in
    is-enabled)
        unit=${1:-}
        [[ -e $root/control/enabled-$unit ]] && exit 0
        exit 1
        ;;
    disable)
        unit=${2:-${1:-}}
        rm -f -- "$root/control/enabled-$unit"
        exit 0
        ;;
    *)
        echo "systemctl: unsupported stub command '$cmd'" >&2
        exit 1
        ;;
esac
STUB

chmod 0755 "$BIN"/mkswap "$BIN"/swapon "$BIN"/swapoff "$BIN"/systemctl
mkdir -p -- "$TEST_ROOT/control"

# Header row plus one foreign swap that must never be disturbed.
printf 'Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n' >"$SWAPS"
printf '/swapfile\t\t\tfile\t\t1048576\t\t0\t\t-2\n' >>"$SWAPS"

# --- harness -----------------------------------------------------------------

PASS=0
FAIL=0

reset_case() {
    rm -rf -- "$MANAGED" "$ORIGIN" "$UNIT_DIR"
    mkdir -p -- "$MANAGED" "$ORIGIN" "$UNIT_DIR" "$TEST_ROOT/control"
    printf 'Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n' >"$SWAPS"
    printf '/swapfile\t\t\tfile\t\t1048576\t\t0\t\t-2\n' >>"$SWAPS"
    rm -f -- "$TEST_ROOT/control/swapoff-calls"
}

ok() {
    printf 'test %s ... ok\n' "$1"
    PASS=$((PASS + 1))
}

bad() {
    printf 'test %s ... FAILED\n' "$1"
    printf '    %s\n' "$2" >&2
    FAIL=$((FAIL + 1))
}

assert_swapoff_never_called() {
    local test_name=$1
    if [[ -e $TEST_ROOT/control/swapoff-calls ]]; then
        bad "$test_name" 'helper called swapoff; apply/activate must never do so'
        return 1
    fi
    return 0
}

run_helper() {
    bash "$helper" "$@"
}

apply_args=(
    swap-apply
    --name fallback
    --bytes 1048576
    --priority -1
    --fstype ext4
    --transport nvme
    --mount-uuid 11111111-2222-3333-4444-555555555555
    --backing-identity wwn-0x5000c500aabbccdd
)

# --- test 1 ------------------------------------------------------------------

test_apply_creates_only_owned_file_and_unit() {
    local name=linux_swap_apply_creates_only_owned_file_and_unit
    reset_case

    local out
    if ! out=$(run_helper "${apply_args[@]}" 2>&1); then
        bad "$name" "apply failed: $out"
        return
    fi

    local target="$MANAGED/fallback.swap"
    if [[ ! -f $target ]]; then
        bad "$name" 'managed swapfile was not created'
        return
    fi
    if [[ ! -f $target.owner ]]; then
        bad "$name" 'ownership marker was not created'
        return
    fi
    if [[ ! -f $UNIT_DIR/ramshared-swap-fallback.swap ]]; then
        bad "$name" 'systemd .swap unit was not created'
        return
    fi
    if ! grep -Fq -- "$target" "$UNIT_DIR/ramshared-swap-fallback.swap"; then
        bad "$name" 'unit does not reference the exact target path'
        return
    fi
    if ! grep -Fqx -- "$target" <(awk 'NR > 1 { print $1 }' "$SWAPS"); then
        bad "$name" 'swapon did not activate the managed target'
        return
    fi
    if ! grep -Fqx -- '/swapfile' <(awk 'NR > 1 { print $1 }' "$SWAPS"); then
        bad "$name" 'pre-existing foreign swap was disturbed'
        return
    fi

    # A path outside the app-owned root must refuse.
    if run_helper "${apply_args[@]}" --name '../escape' >/dev/null 2>&1; then
        bad "$name" 'apply accepted a name that escapes the managed root'
        return
    fi

    # A second apply of the same name is create-once and must refuse.
    if run_helper "${apply_args[@]}" >/dev/null 2>&1; then
        bad "$name" 'apply accepted a duplicate create-once target'
        return
    fi

    # A network-backed transport must refuse.
    if out=$(run_helper swap-apply --name nbdtest --bytes 1048576 --priority -1 \
        --fstype ext4 --transport nbd \
        --mount-uuid 11111111-2222-3333-4444-555555555555 \
        --backing-identity wwn-0x5000c500aabbccdd 2>&1); then
        bad "$name" 'apply accepted a network-backed transport'
        return
    fi
    if [[ $out != *'network-backed'* ]]; then
        bad "$name" "nbd refusal message missing: $out"
        return
    fi

    if assert_swapoff_never_called "$name"; then
        ok "$name"
    fi
}

# --- test 2 ------------------------------------------------------------------

test_apply_preserves_old_active_swap_on_failure() {
    local name=linux_swap_apply_preserves_old_active_swap_on_failure
    reset_case

    # A pre-existing RamShared-owned swap is already active. It is not the
    # create-once target below; the point is that any later failure must leave
    # every row in the swap table untouched.
    local old="$MANAGED/old.swap"
    dd if=/dev/zero of="$old" bs=1M count=1 status=none
    printf '%s\tfile\t1048576\t0\t-2\n' "$old" >>"$SWAPS"
    local before
    before=$(cat "$SWAPS")

    # Manufacture an mkswap failure and confirm apply fails closed.
    touch "$TEST_ROOT/control/mkswap-fail"
    local out
    if out=$(run_helper "${apply_args[@]}" 2>&1); then
        bad "$name" 'apply succeeded despite mkswap failure'
        return
    fi
    if [[ $out != *'existing swap left untouched'* ]]; then
        bad "$name" "mkswap-failure did not report preservation: $out"
        return
    fi
    if [[ $(cat "$SWAPS") != "$before" ]]; then
        bad "$name" 'swap table changed across a failed apply'
        return
    fi
    if [[ -e $MANAGED/fallback.swap ]]; then
        bad "$name" 'failed apply left the new swapfile behind'
        return
    fi
    if [[ -e $UNIT_DIR/ramshared-swap-fallback.swap ]]; then
        bad "$name" 'failed apply left a systemd unit behind'
        return
    fi
    rm -f -- "$TEST_ROOT/control/mkswap-fail"

    # Manufacture a swapon failure. The file and unit are retained as evidence
    # and the pre-existing swap row must still be untouched.
    touch "$TEST_ROOT/control/swapon-fail"
    if out=$(run_helper "${apply_args[@]}" 2>&1); then
        bad "$name" 'apply succeeded despite swapon failure'
        return
    fi
    if [[ $out != *'retained as evidence'* ]]; then
        bad "$name" "swapon-failure did not retain evidence: $out"
        return
    fi
    if [[ $(cat "$SWAPS") != "$before" ]]; then
        bad "$name" 'swap table changed across a failed swapon'
        return
    fi
    if [[ ! -e $MANAGED/fallback.swap || ! -e $MANAGED/fallback.swap.owner ]]; then
        bad "$name" 'swapon failure discarded the ownership evidence'
        return
    fi
    rm -f -- "$TEST_ROOT/control/swapon-fail"

    if assert_swapoff_never_called "$name"; then
        ok "$name"
    fi
}

# --- test 3 ------------------------------------------------------------------

test_cleanup_refuses_foreign_active_or_used_target() {
    local name=linux_swap_cleanup_refuses_foreign_active_or_used_target
    reset_case

    # Establish one managed swap with exact ownership evidence.
    if ! run_helper "${apply_args[@]}" >/dev/null 2>&1; then
        bad "$name" 'setup apply failed'
        return
    fi
    local target="$MANAGED/fallback.swap"
    local hash
    hash=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["transaction_hash"])' "$target.owner")
    # Keep a second managed file so the target is not the last fallback.
    dd if=/dev/zero of="$MANAGED/other.swap" bs=1M count=1 status=none

    # Foreign path is refused.
    local out
    if out=$(run_helper swap-cleanup --path /etc/fstab \
        --transaction-hash "$hash" 2>&1); then
        bad "$name" 'cleanup accepted a foreign path'
        return
    fi
    if [[ $out != *'foreign path'* ]]; then
        bad "$name" "foreign-path refusal message missing: $out"
        return
    fi

    # Active target is refused (SwapUsed=0 but still active).
    if out=$(run_helper swap-cleanup --path "$target" \
        --transaction-hash "$hash" 2>&1); then
        bad "$name" 'cleanup accepted an active target'
        return
    fi
    if [[ $out != *'still active'* ]]; then
        bad "$name" "active-target refusal message missing: $out"
        return
    fi

    # Used target is refused (SwapUsed>0).
    python3 - "$SWAPS" "$target" <<'PY'
import sys
path, target = sys.argv[1], sys.argv[2]
lines = open(path, encoding="utf-8").read().splitlines()
out = []
for line in lines:
    parts = line.split()
    if parts and parts[0] == target and len(parts) >= 4:
        parts[3] = "64"
        line = "\t".join(parts)
    out.append(line)
open(path, "w", encoding="utf-8").write("\n".join(out) + "\n")
PY
    if out=$(run_helper swap-cleanup --path "$target" \
        --transaction-hash "$hash" 2>&1); then
        bad "$name" 'cleanup accepted a used target'
        return
    fi
    if [[ $out != *'still has'* && $out != *'in use'* ]]; then
        bad "$name" "used-target refusal message missing: $out"
        return
    fi

    # Ownership mismatch is refused.
    if out=$(run_helper swap-cleanup --path "$target" \
        --transaction-hash "$(printf 'a%.0s' {1..64})" 2>&1); then
        bad "$name" 'cleanup accepted a mismatched transaction hash'
        return
    fi
    if [[ $out != *'transaction hash changed'* ]]; then
        bad "$name" "hash-mismatch refusal message missing: $out"
        return
    fi

    # Last persistent fallback is refused even with every other proof exact.
    rm -f -- "$MANAGED/other.swap"
    awk -v t="$target" 'NR == 1 || $1 != t' "$SWAPS" >"$SWAPS.clean"
    mv -f -- "$SWAPS.clean" "$SWAPS"
    if out=$(run_helper swap-cleanup --path "$target" \
        --transaction-hash "$hash" 2>&1); then
        bad "$name" 'cleanup accepted the last persistent fallback'
        return
    fi
    if [[ $out != *'last persistent fallback'* ]]; then
        bad "$name" "last-fallback refusal message missing: $out"
        return
    fi
    if [[ ! -e $target ]]; then
        bad "$name" 'refused cleanup still deleted the target'
        return
    fi

    if assert_swapoff_never_called "$name"; then
        ok "$name"
    fi
}

# --- test 4 ------------------------------------------------------------------

test_cleanup_replay_is_idempotent() {
    local name=linux_swap_cleanup_replay_is_idempotent
    reset_case

    if ! run_helper "${apply_args[@]}" >/dev/null 2>&1; then
        bad "$name" 'setup apply failed'
        return
    fi
    local target="$MANAGED/fallback.swap"
    local hash
    hash=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["transaction_hash"])' "$target.owner")
    dd if=/dev/zero of="$MANAGED/other.swap" bs=1M count=1 status=none

    # DT-9 precondition: the swap must already be inactive and unused. This
    # helper never calls swapoff, so the owner deactivates first.
    awk -v t="$target" 'NR == 1 || $1 != t' "$SWAPS" >"$SWAPS.clean"
    mv -f -- "$SWAPS.clean" "$SWAPS"

    # First cleanup succeeds and removes the exact owned target only.
    local out
    if ! out=$(run_helper swap-cleanup --path "$target" --transaction-hash "$hash" 2>&1); then
        bad "$name" "first cleanup failed: $out"
        return
    fi
    if [[ $out != *'cleaned'* ]]; then
        bad "$name" "first cleanup did not report cleaned: $out"
        return
    fi
    if [[ -e $target || -e $target.owner ]]; then
        bad "$name" 'first cleanup left the target or its marker behind'
        return
    fi
    if [[ ! -e $MANAGED/other.swap ]]; then
        bad "$name" 'first cleanup removed an unrelated managed file'
        return
    fi

    # Replay of the same cleanup is a stable no-op success (Kahneman #17).
    if ! out=$(run_helper swap-cleanup --path "$target" --transaction-hash "$hash" 2>&1); then
        bad "$name" "replay cleanup failed: $out"
        return
    fi
    if [[ $out != *'already-clean'* ]]; then
        bad "$name" "replay did not report already-clean: $out"
        return
    fi

    # A third replay is the same no-op; the swap table never changes.
    local before
    before=$(cat "$SWAPS")
    if ! out=$(run_helper swap-cleanup --path "$target" --transaction-hash "$hash" 2>&1); then
        bad "$name" "third cleanup failed: $out"
        return
    fi
    if [[ $out != *'already-clean'* ]]; then
        bad "$name" "third cleanup did not report already-clean: $out"
        return
    fi
    if [[ $(cat "$SWAPS") != "$before" ]]; then
        bad "$name" 'replay mutated the swap table'
        return
    fi
    if [[ -e $target || -e $target.owner ]]; then
        bad "$name" 'replay recreated a deleted target'
        return
    fi

    if assert_swapoff_never_called "$name"; then
        ok "$name"
    fi
}

# --- test 5 ------------------------------------------------------------------

test_native_origin_creation_refuses_existing_file_or_capacity_shortfall() {
    local name=native_origin_creation_refuses_existing_file_or_capacity_shortfall
    reset_case

    local out
    if ! out=$(run_helper native-origin-apply --name data --bytes 1048576 \
        --fstype ext4 --transport nvme \
        --mount-uuid 11111111-2222-3333-4444-555555555555 \
        --backing-identity wwn-0x5000c500aabbccdd \
        --free-bytes 8388608 2>&1); then
        bad "$name" "legitimate origin creation failed: $out"
        return
    fi
    if [[ $out != *'origin-created'* ]]; then
        bad "$name" "creation did not report origin-created: $out"
        return
    fi
    if [[ ! -f $ORIGIN/data.origin || ! -f $ORIGIN/data.origin.owner ]]; then
        bad "$name" 'origin file or ownership marker missing after creation'
        return
    fi

    # Create-once: an existing file must refuse without replacing it.
    local inode_before
    inode_before=$(stat -c '%i' -- "$ORIGIN/data.origin")
    if out=$(run_helper native-origin-apply --name data --bytes 2097152 \
        --fstype ext4 --transport nvme \
        --mount-uuid 11111111-2222-3333-4444-555555555555 \
        --backing-identity wwn-0x5000c500aabbccdd \
        --free-bytes 8388608 2>&1); then
        bad "$name" 'origin creation replaced an existing sealed origin'
        return
    fi
    if [[ $out != *'already exists'* ]]; then
        bad "$name" "existing-file refusal message missing: $out"
        return
    fi
    if [[ $(stat -c '%i' -- "$ORIGIN/data.origin") != "$inode_before" ]]; then
        bad "$name" 'existing origin inode changed after a refused create'
        return
    fi

    # Capacity shortfall must refuse before any mutation.
    if out=$(run_helper native-origin-apply --name tiny --bytes 4194304 \
        --fstype ext4 --transport nvme \
        --mount-uuid 11111111-2222-3333-4444-555555555555 \
        --backing-identity wwn-0x5000c500aabbccdd \
        --free-bytes 1048576 2>&1); then
        bad "$name" 'origin creation accepted a capacity shortfall'
        return
    fi
    if [[ $out != *'capacity shortfall'* ]]; then
        bad "$name" "capacity-refusal message missing: $out"
        return
    fi
    if [[ -e $ORIGIN/tiny.origin ]]; then
        bad "$name" 'capacity shortfall still created the origin file'
        return
    fi

    # A network-backed transport refuses before any mutation.
    if out=$(run_helper native-origin-apply --name nbdorigin --bytes 1048576 \
        --fstype ext4 --transport nbd \
        --mount-uuid 11111111-2222-3333-4444-555555555555 \
        --backing-identity wwn-0x5000c500aabbccdd \
        --free-bytes 8388608 2>&1); then
        bad "$name" 'origin creation accepted a network-backed transport'
        return
    fi
    if [[ $out != *'network-backed'* ]]; then
        bad "$name" "nbd refusal message missing: $out"
        return
    fi

    if assert_swapoff_never_called "$name"; then
        ok "$name"
    fi
}

# --- run ---------------------------------------------------------------------

test_apply_creates_only_owned_file_and_unit
test_apply_preserves_old_active_swap_on_failure
test_cleanup_refuses_foreign_active_or_used_target
test_cleanup_replay_is_idempotent
test_native_origin_creation_refuses_existing_file_or_capacity_shortfall

printf '\nresource-config-helper manufactured tests: %d passed, %d failed\n' \
    "$PASS" "$FAIL"
(( FAIL == 0 )) || exit 1
exit 0
