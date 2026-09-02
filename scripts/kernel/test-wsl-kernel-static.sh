#!/usr/bin/env bash
# Hermetic parser tests for the WSL-side kernel-pair and READY contracts.
# This script never invokes wsl.exe, changes .wslconfig, or touches a device.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=wsl-kernel.sh
source "$ROOT/wsl-kernel.sh"

spec_test=""
if [[ "${1:-}" == '--spec-test' ]]; then
	[[ $# -eq 2 && "${2:-}" == 'apply_is_the_only_gate_forwarder' ]] || {
		echo 'unknown or malformed --spec-test selection' >&2
		exit 2
	}
	spec_test="$2"
	shift 2
fi

if [[ "${1:-}" == '--r6-static-only' ]]; then
	if grep -Eq 'sudo[[:space:]]+-n[[:space:]]+--[[:space:]]+modprobe|(^|[[:space:]])modprobe([[:space:]]|$)' "$ROOT/wsl-kernel.sh"; then
		echo 'R6_RED enable still reaches a module loader' >&2; exit 1
	fi
	grep -q 'ENABLE_LIVE_MUTATION=NO_GO' "$ROOT/wsl-kernel.sh" || { echo 'R6_RED enable lacks exact NO-GO' >&2; exit 1; }
	if grep -Eq 'rm[[:space:]]+-rf' "$ROOT/seal-kernel-pair.sh" "$ROOT/test-wsl-kernel-static.sh"; then
		echo 'R6_RED recursive shell cleanup remains' >&2; exit 1
	fi
	grep -q 'parse_source_identity' "$ROOT/seal-kernel-pair.sh" || { echo 'R6_RED four-field identity parser is missing' >&2; exit 1; }
	grep -q 'field3.*==.*1' "$ROOT/seal-kernel-pair.sh" || { echo 'R6_RED link-count field check is missing' >&2; exit 1; }
	grep -q -- '--spec-test' "$ROOT/test-wsl-kernel-static.sh" || { echo 'R6_RED selectable shell SPEC mode is missing' >&2; exit 1; }
	echo 'R6_STATIC_SHELL=PASS'
	exit 0
fi

fixture="$(mktemp -d "${TMPDIR:-/tmp}/ramshared-kernel-static.XXXXXX")"
fixture_identity="$(stat -c '%d:%i' -- "$fixture")"
cleanup() {
	local cleanup_fd cleanup_root entry identity
	local -a directories=() entries=()
	[[ "$fixture" == "${TMPDIR:-/tmp}"/ramshared-kernel-static.* ]] || return 1
	exec {cleanup_fd}<"$fixture" || return 1
	cleanup_root="/proc/self/fd/$cleanup_fd/."
	[[ "$(stat -Lc '%d:%i' -- "$cleanup_root")" == "$fixture_identity" ]] || {
		exec {cleanup_fd}<&-
		return 1
	}
	mapfile -d '' -t directories < <(find "$cleanup_root" -xdev -type d -print0)
	for entry in "${directories[@]}"; do
		[[ ! -L "$entry" ]] || { exec {cleanup_fd}<&-; return 1; }
		chmod u+rwx -- "$entry" || { exec {cleanup_fd}<&-; return 1; }
	done
	mapfile -d '' -t entries < <(find "$cleanup_root" -xdev -depth -mindepth 1 -print0)
	for entry in "${entries[@]}"; do
		if [[ -L "$entry" ]]; then
			[[ "$(stat -c '%h' -- "$entry")" == 1 ]] || { exec {cleanup_fd}<&-; return 1; }
			rm -f -- "$entry" || { exec {cleanup_fd}<&-; return 1; }
		elif [[ -f "$entry" ]]; then
			identity="$(stat -Lc '%d:%i:%h:%s' -- "$entry")"
			[[ "$identity" =~ ^[0-9]+:[0-9]+:1:[0-9]+$ ]] || { exec {cleanup_fd}<&-; return 1; }
			rm -f -- "$entry" || { exec {cleanup_fd}<&-; return 1; }
		elif [[ -d "$entry" ]]; then
			rmdir -- "$entry" || { exec {cleanup_fd}<&-; return 1; }
		else
			exec {cleanup_fd}<&-
			return 1
		fi
	done
	[[ "$(stat -Lc '%d:%i' -- "$cleanup_root")" == "$fixture_identity" &&
		"$(stat -c '%d:%i' -- "$fixture" 2>/dev/null)" == "$fixture_identity" ]] || {
		exec {cleanup_fd}<&-
		return 1
	}
	exec {cleanup_fd}<&-
	rmdir -- "$fixture"
}
trap cleanup EXIT

pair="$fixture/pair"
mkdir -- "$pair"
truncate -s 1048577 "$pair/kernel.bzImage"
printf 'modules-fixture\n' >"$pair/modules.vhdx"
	printf 'initramfs-fixture\n' >"$pair/initramfs.cpio.gz"
release='6.18.test-microsoft-standard-WSL2+'
kernel_sha="$(sha256sum -- "$pair/kernel.bzImage" | awk '{print $1}')"
	initramfs_sha="$(sha256sum -- "$pair/initramfs.cpio.gz" | awk '{print $1}')"
modules_sha="$(sha256sum -- "$pair/modules.vhdx" | awk '{print $1}')"
cat >"$pair/modules-layout.manifest" <<EOF
schema=ramshared.modules-layout.v1
layout=legacy_flat_v1
release=$release
release_directory_count=0
nested_release_directory_count=0
modules_sha256=$modules_sha
modules_size_bytes=16
EOF
cat >"$pair/qemu-pass.stamp" <<EOF
REL=$release
KERNEL_SHA256=$kernel_sha
HEAD=abcdef1
DATE=2026-08-23T00:00:00Z
VALIDATE=qemu-validate.sh
EOF
layout_sha="$(sha256sum -- "$pair/modules-layout.manifest" | awk '{print $1}')"
qemu_sha="$(sha256sum -- "$pair/qemu-pass.stamp" | awk '{print $1}')"
cat >"$pair/kernel-pair.manifest" <<EOF
schema=ramshared.kernel-pair.v1
pair_id=v1-${kernel_sha:0:16}-${modules_sha:0:16}
release=$release
kernel_file=kernel.bzImage
kernel_sha256=$kernel_sha
kernel_size_bytes=1048577
initramfs_file=initramfs.cpio.gz
initramfs_sha256=$initramfs_sha
initramfs_size_bytes=18
modules_file=modules.vhdx
modules_sha256=$modules_sha
modules_size_bytes=16
modules_layout=legacy_flat_v1
layout_release_directory_count=0
layout_nested_release_directory_count=0
layout_inventory_sha256=$layout_sha
module_name=ublk_drv
module_vermagic=$release SMP preempt mod_unload
minimum_wsl_version=2.7.12.0
qemu_stamp_sha256=$qemu_sha
qemu_kernel_sha256=$kernel_sha
qemu_release=$release
EOF

load_kernel_pair_manifest "$pair/kernel-pair.manifest" || {
	echo "valid sealed pair was rejected: $PAIR_ERROR" >&2
	exit 1
}

fake_bin="$fixture/fake-bin"
sealed_root="$fixture/sealed"
module_fixture="$fixture/ublk_drv.ko"
mkdir -- "$fake_bin"
printf 'module-fixture\n' >"$module_fixture"
cat >"$fake_bin/modinfo" <<EOF
#!/usr/bin/env bash
case "\${2:-}" in
name) printf 'ublk_drv\\n' ;;
vermagic) printf '$release SMP preempt mod_unload\\n' ;;
*) exit 2 ;;
esac
EOF
chmod 0700 "$fake_bin/modinfo"
seal_output="$(PATH="$fake_bin:$PATH" timeout --signal=TERM --kill-after=2s 20s bash "$ROOT/seal-kernel-pair.sh" \
	--kernel "$pair/kernel.bzImage" \
		--initramfs "$pair/initramfs.cpio.gz" \
	--modules "$pair/modules.vhdx" \
	--module-file "$module_fixture" \
	--release "$release" \
	--layout-inventory "$pair/modules-layout.manifest" \
	--qemu-stamp "$pair/qemu-pass.stamp" \
	--output-root "$sealed_root")"
[[ "$seal_output" == *'RAMSHARED_PROMOTION_ELIGIBILITY=REFUSED_MODULE_VHDX_PROVENANCE_UNVERIFIED'* ]]
sealed_manifest="$(printf '%s\n' "$seal_output" | awk -F= '/^RAMSHARED_KERNEL_PAIR=/{print $2}')"
[[ -n "$sealed_manifest" && -f "$sealed_manifest" ]]
load_kernel_pair_manifest "$sealed_manifest" || {
	echo "newly sealed pair was rejected: $PAIR_ERROR" >&2
	exit 1
}
cleanup_rename_root="$fixture/sealed-cleanup-rename"
set +e
cleanup_rename_output="$(RAMSHARED_CLEANUP_RENAME_FIXTURE=1 PATH="$fake_bin:$PATH" \
	timeout --signal=TERM --kill-after=2s 20s bash "$ROOT/seal-kernel-pair.sh" \
	--kernel "$pair/kernel.bzImage" \
		--initramfs "$pair/initramfs.cpio.gz" \
	--modules "$pair/modules.vhdx" \
	--module-file "$module_fixture" \
	--release "$release" \
	--layout-inventory "$pair/modules-layout.manifest" \
	--qemu-stamp "$pair/qemu-pass.stamp" \
	--output-root "$cleanup_rename_root" 2>&1)"
cleanup_rename_status=$?
set -e
[[ $cleanup_rename_status -eq 2 && "$cleanup_rename_output" == *'CLEANUP_RENAME_FIXTURE=INJECTED'* ]]
mapfile -t replacement_sentinels < <(find "$cleanup_rename_root" -name replacement.sentinel -type f -print)
[[ ${#replacement_sentinels[@]} -eq 1 ]]
[[ "$(sha256sum -- "${replacement_sentinels[0]}" | awk '{print $1}')" == \
	"$(printf 'replacement-sentinel\n' | sha256sum | awk '{print $1}')" ]]
mapfile -t moved_staging < <(find "$cleanup_rename_root" -maxdepth 1 -type d -name '*.owned-moved' -print)
[[ ${#moved_staging[@]} -eq 1 ]]
if PATH="$fake_bin:$PATH" timeout --signal=TERM --kill-after=2s 20s bash "$ROOT/seal-kernel-pair.sh" \
	--kernel "$pair/kernel.bzImage" \
		--initramfs "$pair/initramfs.cpio.gz" \
	--modules "$pair/modules.vhdx" \
	--module-file "$module_fixture" \
	--release "$release" \
	--layout-inventory "$pair/modules-layout.manifest" \
	--qemu-stamp "$pair/qemu-pass.stamp" \
	--output-root "$sealed_root" >/dev/null 2>&1; then
	echo 'immutable pair sealer overwrote an existing pair' >&2
	exit 1
fi

seal_args=(
	--kernel "$pair/kernel.bzImage"
		--initramfs "$pair/initramfs.cpio.gz"
	--modules "$pair/modules.vhdx"
	--module-file "$module_fixture"
	--release "$release"
	--layout-inventory "$pair/modules-layout.manifest"
	--qemu-stamp "$pair/qemu-pass.stamp"
)
parser_probe_root="$fixture/parser-probe-output"
if PATH="$fake_bin:$PATH" timeout --signal=TERM --kill-after=2s 10s bash "$ROOT/seal-kernel-pair.sh" \
	--kernel "$pair/kernel.bzImage" --kernel "$pair/kernel.bzImage" \
		--initramfs "$pair/initramfs.cpio.gz" \
		--initramfs "$pair/initramfs.cpio.gz" \
	--modules "$pair/modules.vhdx" --module-file "$module_fixture" --release "$release" \
	--layout-inventory "$pair/modules-layout.manifest" --qemu-stamp "$pair/qemu-pass.stamp" \
	--output-root "$parser_probe_root" >/dev/null 2>&1; then
	echo 'sealer accepted duplicate --kernel' >&2
	exit 1
fi
if PATH="$fake_bin:$PATH" timeout --signal=TERM --kill-after=2s 10s bash "$ROOT/seal-kernel-pair.sh" \
	"${seal_args[@]}" --output-root '   ' >/dev/null 2>&1; then
	echo 'sealer accepted a blank output root' >&2
	exit 1
fi
if PATH="$fake_bin:$PATH" timeout --signal=TERM --kill-after=2s 10s bash "$ROOT/seal-kernel-pair.sh" \
	"${seal_args[@]}" --unknown value --output-root "$parser_probe_root" >/dev/null 2>&1; then
	echo 'sealer accepted an unknown argument' >&2
	exit 1
fi
if timeout --signal=TERM --kill-after=2s 10s bash "$ROOT/seal-kernel-pair.sh" --initramfs >/dev/null 2>&1; then
	echo 'sealer accepted a missing argument value' >&2
	exit 1
fi
[[ ! -e "$parser_probe_root" ]]

race_root="$fixture/sealed-race"
race_out_one="$fixture/sealed-race-one.out"
race_out_two="$fixture/sealed-race-two.out"
set +e
PATH="$fake_bin:$PATH" timeout --signal=TERM --kill-after=2s 20s bash "$ROOT/seal-kernel-pair.sh" \
	"${seal_args[@]}" --output-root "$race_root" >"$race_out_one" 2>&1 &
race_pid_one=$!
PATH="$fake_bin:$PATH" timeout --signal=TERM --kill-after=2s 20s bash "$ROOT/seal-kernel-pair.sh" \
	"${seal_args[@]}" --output-root "$race_root" >"$race_out_two" 2>&1 &
race_pid_two=$!
wait "$race_pid_one"; race_status_one=$?
wait "$race_pid_two"; race_status_two=$?
set -e
race_successes=0
[[ $race_status_one -eq 0 ]] && ((race_successes += 1))
[[ $race_status_two -eq 0 ]] && ((race_successes += 1))
[[ $race_successes -eq 1 ]] || {
	echo "sealer publication race expected one winner, got statuses $race_status_one/$race_status_two" >&2
	exit 1
}
shopt -s nullglob
race_pairs=("$race_root"/v1-*)
race_staging=("$race_root"/.staging-*)
shopt -u nullglob
[[ ${#race_pairs[@]} -eq 1 && -d "${race_pairs[0]}" && ! -L "${race_pairs[0]}" ]]
[[ ${#race_staging[@]} -eq 0 ]] || {
	echo 'sealer publication race left an invocation-owned staging directory' >&2
	exit 1
}
[[ "$(stat -c '%a' -- "${race_pairs[0]}")" == 555 ]]
for published in kernel.bzImage modules.vhdx modules-layout.manifest qemu-pass.stamp kernel-pair.manifest; do
	[[ "$(stat -c '%a:%h' -- "${race_pairs[0]}/$published")" == '444:1' ]]
done
if [[ $race_status_one -eq 0 ]]; then
	grep -q '^RAMSHARED_PROMOTION_ELIGIBILITY=REFUSED_MODULE_VHDX_PROVENANCE_UNVERIFIED$' "$race_out_one"
else
	grep -Eq 'concurrent immutable publication won|immutable pair already exists' "$race_out_one" || {
		echo 'first race loser returned an unexpected refusal:' >&2
		sed 's/^/  /' "$race_out_one" >&2
		exit 1
	}
fi
if [[ $race_status_two -eq 0 ]]; then
	grep -q '^RAMSHARED_PROMOTION_ELIGIBILITY=REFUSED_MODULE_VHDX_PROVENANCE_UNVERIFIED$' "$race_out_two"
else
	grep -Eq 'concurrent immutable publication won|immutable pair already exists' "$race_out_two" || {
		echo 'second race loser returned an unexpected refusal:' >&2
		sed 's/^/  /' "$race_out_two" >&2
		exit 1
	}
fi

mismatch="$fixture/layout-mismatch"
cp -a -- "$pair" "$mismatch"
sed -i 's/^release=.*/release=6.18.other-microsoft-standard-WSL2+/' "$mismatch/modules-layout.manifest"
mismatch_layout_sha="$(sha256sum -- "$mismatch/modules-layout.manifest" | awk '{print $1}')"
sed -i "s/^layout_inventory_sha256=.*/layout_inventory_sha256=$mismatch_layout_sha/" "$mismatch/kernel-pair.manifest"
if load_kernel_pair_manifest "$mismatch/kernel-pair.manifest"; then
	echo 'layout inventory mismatch was accepted' >&2
	exit 1
fi

receipt="$fixture/promotion-current.json"
python3 - "$receipt" "$release" "$kernel_sha" "$modules_sha" "$(sha256sum -- "$pair/kernel-pair.manifest" | awk '{print $1}')" <<'PY'
import json
import sys

path, release, kernel_sha, modules_sha, manifest_sha = sys.argv[1:]

def canary(phase, boot_id, uname, modules, vermagic, tree, errors):
    return {
        "CANARY_SCHEMA": "1",
        "CANARY_PHASE": phase,
        "CANARY_WSL_EXIT": "0",
        "CANARY_BOOT_ID": boot_id,
        "CANARY_UNAME": uname,
        "CANARY_SYSTEMD": "running",
        "CANARY_FAILED_UNITS": "none",
        "CANARY_DXG_NODE": "char",
        "CANARY_DXG_DEV_T": "e7:0",
        "CANARY_DXG_COUNT": "1",
        "CANARY_XWAYLAND_COUNT_BEFORE": "0",
        "CANARY_XWAYLAND_COUNT_AFTER": "1",
        "CANARY_WSLG_TRANSACTION": "ok",
        "CANARY_GPU_DRIVER": "560.94",
        "CANARY_DXG_PROBE": "ok",
        "CANARY_MODULES": modules,
        "CANARY_MODULE_VERMAGIC": vermagic,
        "CANARY_MODULE_TREE": tree,
        "CANARY_DISTRO_ID": "ubuntu",
        "CANARY_DISTRO_VERSION_ID": "24.04",
        "CANARY_DMESG_READABLE": "1",
        "CANARY_DMESG_SHA256": "d" * 64,
        "CANARY_DXG_FORTIFY_WARNINGS": "0",
        "CANARY_WAIT_FOR_BOOT_FAILURES": "0",
        "CANARY_JOURNAL_UNCLEAN": "0",
        "CANARY_P9_CANCELLED": "0",
        "CANARY_KERNEL_FATALS": "0",
        "CANARY_DXG_QUERY_ERRORS": str(errors),
    }

vermagic = f"{release} SMP preempt mod_unload"
data = {
    "schema": "ramshared.kernel-promotion-receipt.v1",
    "transaction_id": "20260823T123456789Z-0123456789abcdef0123456789abcdef",
    "status": "READY",
    "started_at_utc": "2026-08-23T12:34:56.0000000Z",
    "completed_at_utc": "2026-08-23T12:35:56.0000000Z",
    "distro": "Ubuntu-24.04",
    "pair_id": f"v1-{kernel_sha[:16]}-{modules_sha[:16]}",
    "manifest_sha256": manifest_sha,
    "kernel_path": r"C:\wsl\fixture\kernel.bzImage",
    "kernel_sha256": kernel_sha,
    "modules_path": r"C:\wsl\fixture\modules.vhdx",
    "modules_sha256": modules_sha,
    "modules_layout": "legacy_flat_v1",
    "release": release,
    "module_name": "ublk_drv",
    "module_vermagic": vermagic,
    "original_wslconfig_existed": True,
    "original_wslconfig_sha256": "a" * 64,
    "original_wslconfig_snapshot": r"C:\wsl\fixture\snapshot",
    "bundled_wslconfig_sha256": "b" * 64,
    "candidate_wslconfig_sha256": "c" * 64,
    "approved_failed_units": [],
    "host": {
        "windows_product_name": "Windows 11 Pro",
        "windows_display_version": "24H2",
        "windows_current_build": "26100",
        "windows_ubr": "4946",
        "wsl_version": "2.7.12.0",
        "wsl_kernel_version": "6.6.87.2-1",
        "wslg_version": "1.0.71",
        "wsl_reported_windows_version": "10.0.26100.4946",
        "wsl_version_output_sha256": "e" * 64,
    },
    "baseline": canary(
        "bundled", "11111111-2222-4333-8444-555555555555",
        "6.6.87.2-microsoft-standard-WSL2", "missing", "unavailable", "failed", 3,
    ),
    "candidate": canary(
        "candidate", "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
        release, "ok", vermagic, "ok", 2,
    ),
    "rollback": None,
    "failure": None,
}
with open(path, "w", encoding="utf-8") as handle:
    json.dump(data, handle, sort_keys=True, separators=(",", ":"))
    handle.write("\n")
PY

load_ready_receipt "$receipt" || {
	echo "valid strict READY receipt was rejected: $RECEIPT_ERROR" >&2
	exit 1
}
python3 - "$receipt" "$fixture/receipt-unknown.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
data["unknown"] = "must-fail"
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(data, handle, separators=(",", ":"))
PY
if load_ready_receipt "$fixture/receipt-unknown.json" 2>/dev/null; then
	echo 'READY parser accepted an unknown top-level field' >&2
	exit 1
fi

config="$fixture/.wslconfig"
printf '[wsl2]\nmemory=8GB\nkernel=C:/stale/kernel\nkernelModules=C:/stale/modules.vhdx\n[experimental]\nautoMemoryReclaim=gradual\n' >"$config"
snapshot="$(fresh_config_snapshot "$config")"
[[ -f "$snapshot" && "$(sha256sum -- "$snapshot" | awk '{print $1}')" == "$(printf '[wsl2]\nmemory=8GB\nkernel=C:/stale/kernel\nkernelModules=C:/stale/modules.vhdx\n[experimental]\nautoMemoryReclaim=gradual\n' | sha256sum | awk '{print $1}')" ]]
arm_config_file "$config" 'C:\\wsl\\pair\\kernel.bzImage' 'C:\\wsl\\pair\\modules.vhdx'
mapfile -t configured_pair < <(config_pair_values "$config")
[[ ${#configured_pair[@]} -eq 2 && "${configured_pair[0]}" == 'c:/wsl/pair/kernel.bzImage' && "${configured_pair[1]}" == 'c:/wsl/pair/modules.vhdx' ]]
disarm_config_file "$config"
if grep -qE '^[[:space:]]*(kernel|kernelModules)[[:space:]]*=' "$config"; then
	echo 'atomic disarm retained one side of the kernel/modules pair' >&2
	exit 1
fi
grep -q '^memory=8GB$' "$config"
grep -q '^autoMemoryReclaim=gradual$' "$config"

duplicate="$fixture/.wslconfig-duplicate"
printf '[wsl2]\nmemory=8GB\n[wsl2]\nprocessors=4\n' >"$duplicate"
if arm_config_file "$duplicate" 'C:\\wsl\\pair\\kernel.bzImage' 'C:\\wsl\\pair\\modules.vhdx' 2>/dev/null; then
	echo 'shell pair arm accepted duplicate [wsl2] sections' >&2
	exit 1
fi

parse_install_receipt $'RAMSHARED_INSTALL_SCHEMA=1\nRAMSHARED_BUNDLE_ID=v1-1111111111111111-2222222222222222-3333333333333333-4444444444444444-5555555555555555\nRAMSHARED_INSTALLED_WRAPPER=C:\\wsl\\bundle\\boot-kernel-logged.ps1\nRAMSHARED_DEPLOYMENT_MANIFEST=C:\\wsl\\bundle\\deployment.json\nRAMSHARED_DEPLOYMENT_SHA256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
[[ "${INSTALL_RECEIPT[RAMSHARED_BUNDLE_ID]}" == 'v1-1111111111111111-2222222222222222-3333333333333333-4444444444444444-5555555555555555' ]]

if parse_manifest_flag 1 --manifest "$pair/kernel-pair.manifest" --manifest "$pair/kernel-pair.manifest"; then
	echo 'manifest parser accepted duplicate --manifest' >&2
	exit 1
fi
if parse_manifest_flag 1 --manifest "$pair/kernel-pair.manifest" \
	--i-know-this-stops-all-wsl --i-know-this-stops-all-wsl; then
	echo 'manifest parser accepted duplicate disruptive approval' >&2
	exit 1
fi
if parse_manifest_flag 1 --manifest '   '; then
	echo 'manifest parser accepted blank --manifest' >&2
	exit 1
fi
if parse_manifest_flag 1 --manifest; then
	echo 'manifest parser accepted missing --manifest value' >&2
	exit 1
fi
if parse_manifest_flag 1 --unknown value; then
	echo 'manifest parser accepted an unknown argument' >&2
	exit 1
fi
if parse_manifest_flag 0 --manifest "$pair/kernel-pair.manifest" --i-know-this-stops-all-wsl; then
	echo 'arm parser accepted the apply-only disruptive approval' >&2
	exit 1
fi
KERNEL_PAIR_MANIFEST="$pair/kernel-pair.manifest"
if parse_manifest_flag 1 --manifest "$pair/kernel-pair.manifest" --i-know-this-stops-all-wsl; then
	echo 'manifest parser accepted environment plus explicit duplicate manifest' >&2
	exit 1
fi
KERNEL_PAIR_MANIFEST=""
parse_manifest_flag 1 --manifest "$pair/kernel-pair.manifest" --i-know-this-stops-all-wsl
[[ "$SELECTED_MANIFEST" == "$pair/kernel-pair.manifest" && $APPROVAL_SEEN -eq 1 ]]

apply_sentinel="$fixture/apply-no-go.sentinel"
printf 'apply-no-go\n' >"$apply_sentinel"
apply_sentinel_sha="$(sha256sum -- "$apply_sentinel" | awk '{print $1}')"
set +e
apply_output="$(cmd_apply --manifest "$pair/kernel-pair.manifest" --i-know-this-stops-all-wsl 2>&1)"
apply_status=$?
set -e
[[ $apply_status -eq $E_APPLY ]]
[[ "$apply_output" == *'MODULE_VHDX_PROVENANCE=REFUSED cryptographic-containment-not-verifiable'* ]]
[[ "$apply_output" == *'LIVE_PROMOTION=NO_GO before install, log, config, or WSL lifecycle effects'* ]]
[[ "$(sha256sum -- "$apply_sentinel" | awk '{print $1}')" == "$apply_sentinel_sha" ]]

arm_config="$fixture/arm-no-go.wslconfig"
printf '[wsl2]\nmemory=8GB\n' >"$arm_config"
arm_config_sha="$(sha256sum -- "$arm_config" | awk '{print $1}')"
set +e
arm_output="$(RAMSHARED_UNSAFE_LAB_ARM=I_ACCEPT_NO_AUTO_REVERT cmd_arm --manifest "$pair/kernel-pair.manifest" 2>&1)"
arm_status=$?
set -e
[[ $arm_status -eq $E_ACTION && "$arm_output" == *'LIVE_PROMOTION=NO_GO'* ]]
[[ "$(sha256sum -- "$arm_config" | awk '{print $1}')" == "$arm_config_sha" ]]

symlink_target="$fixture/canonical-target"
symlink_path="$fixture/canonical-link"
mkdir -- "$symlink_target"
ln -s -- "$symlink_target" "$symlink_path"
if assert_canonical_path "$fixture/../escape" traversal-probe; then
	echo 'canonical path check accepted traversal' >&2
	exit 1
fi
set +e
module_output="$(cmd_enable 2>&1)"
module_status=$?
set -e
[[ $module_status -eq $E_ACTION ]]
[[ "$module_output" == *'ENABLE_LIVE_MUTATION=NO_GO module-loading-is-unconditionally-disabled'* ]]

set +e
direct_enable_output="$(timeout --signal=TERM --kill-after=2s 8s "$ROOT/wsl-kernel.sh" enable 2>&1)"
direct_enable_status=$?
set -e
[[ $direct_enable_status -eq $E_ACTION ]]
[[ "$direct_enable_output" == *'ENABLE_LIVE_MUTATION=NO_GO module-loading-is-unconditionally-disabled'* ]]
if grep -Eq 'sudo[[:space:]]+-n[[:space:]]+--[[:space:]]+modprobe|(^|[[:space:]])modprobe([[:space:]]|$)' "$ROOT/wsl-kernel.sh"; then
	echo 'inert enable retains a module-loader path' >&2
	exit 1
fi

[[ "$(RAMSHARED_IDENTITY_PARSER_FIXTURE='10:20:1:30' bash "$ROOT/seal-kernel-pair.sh")" == 'IDENTITY_PARSER_FIXTURE=ACCEPTED' ]]
set +e
identity_reject="$(RAMSHARED_IDENTITY_PARSER_FIXTURE='10:1:2:30' bash "$ROOT/seal-kernel-pair.sh" 2>&1)"
identity_status=$?
set -e
[[ $identity_status -eq 2 && "$identity_reject" == 'IDENTITY_PARSER_FIXTURE=REFUSED' ]]

grep -q 'kernelModules=' "$ROOT/wsl-kernel.sh"
if grep -q 'bzImage-ramshared-latest' "$ROOT/wsl-kernel.sh"; then
	echo 'WSL kernel control references a mutable latest artifact' >&2
	exit 1
fi
apply_source="$(sed -n '/^cmd_apply() {/,/^}/p' "$ROOT/wsl-kernel.sh")"
[[ "$apply_source" == *"--i-know-this-stops-all-wsl"* ]]
[[ "$apply_source" == *"-Run"* ]]
[[ "$apply_source" == *"'-ConfirmationToken' 'INSTALL_IMMUTABLE_KERNEL_LAUNCHER_BUNDLE'"* ]]
[[ "$apply_source" == *"'-ConfirmationToken' 'PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL'"* ]]
[[ "$apply_source" == *'timeout --signal=TERM --kill-after=5s'* ]]
[[ "$apply_source" == *'MODULE_VHDX_PROVENANCE=REFUSED'* ]]
non_apply_source="$(sed '/^cmd_apply() {/,/^}/d' "$ROOT/wsl-kernel.sh")"
if [[ "$non_apply_source" == *"INSTALL_IMMUTABLE_KERNEL_LAUNCHER_BUNDLE"* ||
	"$non_apply_source" == *"PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL"* ]]; then
	echo 'a non-apply shell command can forward a PowerShell live gate' >&2
	exit 1
fi

echo 'SPEC_TEST=apply_is_the_only_gate_forwarder PASS'
echo 'test-wsl-kernel-static: PASS'
