#!/usr/bin/env bash
# Seals one immutable qualification bundle after detached build and validation.
# Publication does not make the bundle promotion-eligible: module-to-VHDX
# containment lacks an independently verifiable cryptographic attestation.
# This script never builds, downloads, mounts, boots, arms, or activates.
set -euo pipefail

parse_source_identity() {
	local identity="$1" field1 field2 field3 field4 extra
	IFS=: read -r field1 field2 field3 field4 extra <<<"$identity"
	[[ -z "${extra:-}" && "$field1" =~ ^[0-9]+$ && "$field2" =~ ^[0-9]+$ &&
		"$field3" =~ ^[0-9]+$ && "$field4" =~ ^[0-9]+$ && "$field3" == 1 ]]
}

if [[ -n "${RAMSHARED_IDENTITY_PARSER_FIXTURE:-}" ]]; then
	if parse_source_identity "$RAMSHARED_IDENTITY_PARSER_FIXTURE"; then
		echo 'IDENTITY_PARSER_FIXTURE=ACCEPTED'
		exit 0
	fi
	echo 'IDENTITY_PARSER_FIXTURE=REFUSED'
	exit 2
fi

usage() {
	cat <<'EOF'
Usage: seal-kernel-pair.sh \
  --kernel <bzImage> --modules <modules.vhdx> --module-file <module.ko> \
  --release <kernelrelease> --layout-inventory <file> --qemu-stamp <file> \
  --output-root <directory> [--minimum-wsl-version 2.7.12.0]

The layout inventory must use ramshared.modules-layout.v1 and prove the
legacy_flat_v1 layout with zero release-directory and nested-release layers.
Unified artifacts are intentionally refused until a reviewed released WSL
runtime containing microsoft/WSL#41267 is allowlisted.
EOF
}

kernel=""
modules=""
module_file=""
release=""
layout_inventory=""
qemu_stamp=""
output_root=""
minimum_wsl_version="2.7.12.0"
declare -A seen=()
argument_error() {
	echo "argument refusal: $1" >&2
	usage >&2
	exit 2
}
while (($#)); do
	case "$1" in
	--kernel|--modules|--module-file|--release|--layout-inventory|--qemu-stamp|--output-root|--minimum-wsl-version)
		key="${1#--}"
		[[ -z "${seen[$key]+x}" ]] || argument_error "duplicate --$key"
		[[ $# -ge 2 && -n "${2//[[:space:]]/}" && "$2" != --* ]] || argument_error "missing or blank value for --$key"
		seen[$key]=1
		case "$key" in
		kernel) kernel="$2" ;;
		modules) modules="$2" ;;
		module-file) module_file="$2" ;;
		release) release="$2" ;;
		layout-inventory) layout_inventory="$2" ;;
		qemu-stamp) qemu_stamp="$2" ;;
		output-root) output_root="$2" ;;
		minimum-wsl-version) minimum_wsl_version="$2" ;;
		esac
		shift 2
		;;
	-h|--help) usage; exit 0 ;;
	*) argument_error "unknown argument: $1" ;;
	esac
done

for required in kernel modules module-file release layout-inventory qemu-stamp output-root; do
	[[ -n "${seen[$required]+x}" ]] || argument_error "missing --$required"
done

assert_canonical_path() {
	local value="$1" name="$2" require_leaf="$3" resolved cursor
	[[ "$value" == /* && "$value" != *$'\n'* && "$value" != *$'\r'* &&
		"$value" != *//* && "$value" != */./* && "$value" != */../* &&
		"$value" != */. && "$value" != */.. ]] || {
		echo "$name must be one canonical absolute path" >&2
		return 1
	}
	if [[ "$require_leaf" == 1 ]]; then
		resolved="$(realpath -e -- "$value")" || return 1
	else
		resolved="$(realpath -m -- "$value")" || return 1
	fi
	[[ "$resolved" == "$value" ]] || {
		echo "$name contains traversal or a symlinked ancestor" >&2
		return 1
	}
	cursor="$value"
	while [[ ! -e "$cursor" && "$cursor" != / ]]; do cursor="$(dirname -- "$cursor")"; done
	while [[ "$cursor" != / ]]; do
		[[ ! -L "$cursor" ]] || {
			echo "$name contains a symlinked ancestor" >&2
			return 1
		}
		cursor="$(dirname -- "$cursor")"
	done
}

for item in kernel modules module_file layout_inventory qemu_stamp; do
	value="${!item}"
	assert_canonical_path "$value" "$item" 1 || exit 2
	[[ -f "$value" && ! -L "$value" && "$(stat -c '%h' -- "$value")" == 1 ]] || {
		echo "$item must be an existing regular non-symlink file: $value" >&2
		exit 2
	}
done
assert_canonical_path "$output_root" output-root 0 || exit 2
[[ "$release" =~ ^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$ ]] || {
	echo "release is not canonical" >&2
	exit 2
}
[[ "$minimum_wsl_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
	echo "minimum-wsl-version must contain four numeric components" >&2
	exit 2
}

kernel_sha="$(sha256sum -- "$kernel" | awk '{print $1}')"
modules_sha="$(sha256sum -- "$modules" | awk '{print $1}')"
module_file_sha="$(sha256sum -- "$module_file" | awk '{print $1}')"
kernel_size="$(stat -c '%s' -- "$kernel")"
modules_size="$(stat -c '%s' -- "$modules")"
((kernel_size > 1048576 && modules_size > 0)) || {
	echo "kernel must exceed 1 MiB and modules must be non-empty" >&2
	exit 2
}

declare -A layout=()
while IFS= read -r line || [[ -n "$line" ]]; do
	[[ -n "$line" && "$line" =~ ^([a-z][a-z0-9_]*)=([ -~]*)$ ]] || {
		echo "layout inventory contains a malformed line" >&2
		exit 2
	}
	key="${BASH_REMATCH[1]}"
	[[ -z "${layout[$key]+x}" ]] || {
		echo "layout inventory contains duplicate key: $key" >&2
		exit 2
	}
	layout[$key]="${BASH_REMATCH[2]}"
done <"$layout_inventory"
expected_layout_keys=(schema layout release release_directory_count nested_release_directory_count modules_sha256 modules_size_bytes)
[[ ${#layout[@]} -eq ${#expected_layout_keys[@]} ]] || {
	echo "layout inventory contains missing or unknown keys" >&2
	exit 2
}
for key in "${expected_layout_keys[@]}"; do
	[[ -n "${layout[$key]+x}" ]] || {
		echo "layout inventory is missing $key" >&2
		exit 2
	}
done
[[ "${layout[schema]}" == "ramshared.modules-layout.v1" &&
	"${layout[layout]}" == "legacy_flat_v1" &&
	"${layout[release]}" == "$release" &&
	"${layout[release_directory_count]}" == "0" &&
	"${layout[nested_release_directory_count]}" == "0" &&
	"${layout[modules_sha256]}" == "$modules_sha" &&
	"${layout[modules_size_bytes]}" == "$modules_size" ]] || {
	echo "layout inventory is not the admitted legacy flat layout" >&2
	exit 2
}

declare -A stamp=()
while IFS= read -r line || [[ -n "$line" ]]; do
	[[ -n "$line" && "$line" =~ ^([A-Z][A-Z0-9_]*)=([ -~]*)$ ]] || {
		echo "QEMU stamp contains a malformed line" >&2
		exit 2
	}
	key="${BASH_REMATCH[1]}"
	[[ -z "${stamp[$key]+x}" ]] || {
		echo "QEMU stamp contains duplicate key: $key" >&2
		exit 2
	}
	stamp[$key]="${BASH_REMATCH[2]}"
done <"$qemu_stamp"
expected_stamp_keys=(REL KERNEL_SHA256 HEAD DATE VALIDATE)
[[ ${#stamp[@]} -eq ${#expected_stamp_keys[@]} ]] || {
	echo "QEMU stamp contains missing or unknown keys" >&2
	exit 2
}
for key in "${expected_stamp_keys[@]}"; do
	[[ -n "${stamp[$key]+x}" ]] || {
		echo "QEMU stamp is missing $key" >&2
		exit 2
	}
done

layout_sha="$(sha256sum -- "$layout_inventory" | awk '{print $1}')"
stamp_sha="$(sha256sum -- "$qemu_stamp" | awk '{print $1}')"
[[ "${stamp[REL]}" == "$release" && "${stamp[KERNEL_SHA256]}" == "$kernel_sha" &&
	"${stamp[VALIDATE]}" == "qemu-validate.sh" ]] || {
	echo "QEMU stamp does not bind the exact kernel and release" >&2
	exit 2
}
[[ "${stamp[HEAD]}" =~ ^[0-9a-f]{7,64}$ && "${stamp[DATE]}" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T ]] || {
	echo "QEMU stamp provenance is malformed" >&2
	exit 2
}

command -v modinfo >/dev/null 2>&1 || {
	echo "modinfo is required to seal module vermagic" >&2
	exit 2
}
module_identity_before="$(stat -c '%d:%i:%h:%s' -- "$module_file")"
module_name="$(timeout --signal=TERM --kill-after=2s 10s modinfo -F name -- "$module_file" | head -n 1)"
module_vermagic="$(timeout --signal=TERM --kill-after=2s 10s modinfo -F vermagic -- "$module_file" | head -n 1)"
[[ "$module_name" =~ ^[A-Za-z0-9_+-]+$ && "${module_vermagic%% *}" == "$release" ]] || {
	echo "module metadata does not bind the exact kernel release" >&2
	exit 2
}
[[ "$(stat -c '%d:%i:%h:%s' -- "$module_file")" == "$module_identity_before" &&
	"$(sha256sum -- "$module_file" | awk '{print $1}')" == "$module_file_sha" ]] || {
	echo 'module file identity changed during metadata validation' >&2
	exit 2
}

pair_id="v1-${kernel_sha:0:16}-${modules_sha:0:16}"
output_parent="$(dirname -- "$output_root")"
[[ -d "$output_parent" && ! -L "$output_parent" ]] || {
	echo 'output-root parent must already be a canonical non-symlink directory' >&2
	exit 2
}
output_root_created=0
if [[ ! -e "$output_root" ]]; then
	if (umask 077; mkdir -- "$output_root"); then
		output_root_created=1
	else
		[[ -d "$output_root" && ! -L "$output_root" ]] || exit 2
	fi
fi
[[ ! -L "$output_root" && -d "$output_root" ]] || {
	echo "output-root must not be a symlink" >&2
	exit 2
}
assert_canonical_path "$output_root" output-root 1 || exit 2
output_mode="$(stat -c '%a' -- "$output_root")"
[[ "$(stat -c '%u' -- "$output_root")" == "$EUID" && $((8#$output_mode & 022)) -eq 0 ]] || {
	echo 'output-root must be owned by the invoking user and not group/world writable' >&2
	exit 2
}
final="$output_root/$pair_id"
staging="$(mktemp -d --tmpdir="$output_root" ".staging-$pair_id.XXXXXXXX")"
output_root_identity="$(stat -c '%d:%i:%h:%s' -- "$output_root")"
staging_identity="$(stat -c '%d:%i:%h:%s' -- "$staging")"
[[ "$output_root_identity" =~ ^[0-9]+:[0-9]+:[0-9]+:[0-9]+$ &&
	"$staging_identity" =~ ^[0-9]+:[0-9]+:[0-9]+:[0-9]+$ ]] || {
	echo 'owned directory identity is malformed' >&2
	exit 2
}
published_here=0
publication_complete=0
published_identity=""
owned_pair_leaves=(
	kernel.bzImage modules.vhdx modules-layout.manifest
	qemu-pass.stamp kernel-pair.manifest
)
cleanup_owned_pair_directory() {
	local path="$1" expected_identity="$2" owned_fd fd_path current leaf child child_identity
	local -a remaining=()
	exec {owned_fd}<"$path" || return 1
	fd_path="/proc/self/fd/$owned_fd"
	current="$(stat -Lc '%d:%i:%h:%s' -- "$fd_path")" || {
		exec {owned_fd}<&-
		return 1
	}
	[[ "$current" == "$expected_identity" ]] || {
		exec {owned_fd}<&-
		return 1
	}
	for leaf in "${owned_pair_leaves[@]}"; do
		child="$fd_path/$leaf"
		if [[ -e "$child" || -L "$child" ]]; then
			[[ -f "$child" && ! -L "$child" ]] || {
				exec {owned_fd}<&-
				return 1
			}
			child_identity="$(stat -Lc '%d:%i:%h:%s' -- "$child")" || {
				exec {owned_fd}<&-
				return 1
			}
			parse_source_identity "$child_identity" || {
				exec {owned_fd}<&-
				return 1
			}
			rm -f -- "$child" || {
				exec {owned_fd}<&-
				return 1
			}
		fi
	done
	shopt -s nullglob dotglob
	remaining=("$fd_path"/*)
	shopt -u nullglob dotglob
	[[ ${#remaining[@]} -eq 0 ]] || {
		exec {owned_fd}<&-
		return 1
	}
	[[ "$(stat -Lc '%d:%i:%h:%s' -- "$fd_path")" == "$expected_identity" &&
		"$(stat -c '%d:%i:%h:%s' -- "$path" 2>/dev/null)" == "$expected_identity" ]] || {
		exec {owned_fd}<&-
		return 1
	}
	exec {owned_fd}<&-
	rmdir -- "$path"
}
cleanup() {
	if [[ $published_here -eq 1 && $publication_complete -eq 0 ]]; then
		cleanup_owned_pair_directory "$final" "$published_identity" ||
			echo 'CLEANUP_LEAK=published-identity-changed-or-unknown-leaf' >&2
	fi
	if [[ -e "$staging" || -L "$staging" ]]; then
		cleanup_owned_pair_directory "$staging" "$staging_identity" ||
			echo 'CLEANUP_LEAK=staging-identity-changed-or-unknown-leaf' >&2
	fi
	if [[ $output_root_created -eq 1 &&
		"$(stat -c '%d:%i:%h:%s' -- "$output_root" 2>/dev/null)" == "$output_root_identity" ]]; then
		rmdir -- "$output_root" 2>/dev/null ||
			echo 'CLEANUP_LEAK=output-root-not-empty' >&2
	fi
}
trap cleanup EXIT

if [[ "${RAMSHARED_CLEANUP_RENAME_FIXTURE:-0}" == 1 ]]; then
	moved_staging="${staging}.owned-moved"
	mv -T -- "$staging" "$moved_staging"
	mkdir -- "$staging"
	printf 'replacement-sentinel\n' >"$staging/replacement.sentinel"
	echo 'CLEANUP_RENAME_FIXTURE=INJECTED' >&2
	exit 2
fi

if [[ -e "$final" || -L "$final" ]]; then
	echo "immutable pair already exists; refusing overwrite: $final" >&2
	exit 2
fi

declare -A source_identity=()
declare -A source_hash=()
for item in kernel modules layout_inventory qemu_stamp; do
	value="${!item}"
	source_identity[$item]="$(stat -c '%d:%i:%h:%s' -- "$value")"
	source_hash[$item]="$(sha256sum -- "$value" | awk '{print $1}')"
	parse_source_identity "${source_identity[$item]}" || {
		echo "$item acquired another filesystem link before sealed copy" >&2
		exit 2
	}
done
[[ "${source_hash[kernel]}" == "$kernel_sha" &&
	"${source_hash[modules]}" == "$modules_sha" &&
	"${source_hash[layout_inventory]}" == "$layout_sha" &&
	"${source_hash[qemu_stamp]}" == "$stamp_sha" ]] || {
	echo 'source artifact changed before sealed copy' >&2
	exit 2
}

exec {kernel_fd}<"$kernel"
exec {modules_fd}<"$modules"
exec {layout_fd}<"$layout_inventory"
exec {qemu_fd}<"$qemu_stamp"
dd status=none bs=1048576 <&"$kernel_fd" >"$staging/kernel.bzImage"
dd status=none bs=1048576 <&"$modules_fd" >"$staging/modules.vhdx"
dd status=none bs=1048576 <&"$layout_fd" >"$staging/modules-layout.manifest"
dd status=none bs=1048576 <&"$qemu_fd" >"$staging/qemu-pass.stamp"
exec {kernel_fd}<&-
exec {modules_fd}<&-
exec {layout_fd}<&-
exec {qemu_fd}<&-
for item in kernel modules layout_inventory qemu_stamp; do
	value="${!item}"
	[[ "$(stat -c '%d:%i:%h:%s' -- "$value")" == "${source_identity[$item]}" &&
		"$(sha256sum -- "$value" | awk '{print $1}')" == "${source_hash[$item]}" ]] || {
		echo "$item identity changed during sealed copy" >&2
		exit 2
	}
done

cat >"$staging/kernel-pair.manifest" <<EOF
schema=ramshared.kernel-pair.v1
pair_id=$pair_id
release=$release
kernel_file=kernel.bzImage
kernel_sha256=$kernel_sha
kernel_size_bytes=$kernel_size
modules_file=modules.vhdx
modules_sha256=$modules_sha
modules_size_bytes=$modules_size
modules_layout=legacy_flat_v1
layout_release_directory_count=0
layout_nested_release_directory_count=0
layout_inventory_sha256=$layout_sha
module_name=$module_name
module_vermagic=$module_vermagic
minimum_wsl_version=$minimum_wsl_version
qemu_stamp_sha256=$stamp_sha
qemu_kernel_sha256=$kernel_sha
qemu_release=$release
EOF

[[ "$(sha256sum -- "$staging/kernel.bzImage" | awk '{print $1}')" == "$kernel_sha" &&
	"$(sha256sum -- "$staging/modules.vhdx" | awk '{print $1}')" == "$modules_sha" &&
	"$(sha256sum -- "$staging/modules-layout.manifest" | awk '{print $1}')" == "$layout_sha" &&
	"$(sha256sum -- "$staging/qemu-pass.stamp" | awk '{print $1}')" == "$stamp_sha" ]] || {
	echo "staged pair hash readback failed" >&2
	exit 2
}
chmod 0444 -- "$staging/kernel.bzImage" "$staging/modules.vhdx" \
	"$staging/modules-layout.manifest" "$staging/qemu-pass.stamp" "$staging/kernel-pair.manifest"
published_identity="$staging_identity"
sync -f "$staging" 2>/dev/null || true
if ! mv -Tn -- "$staging" "$final"; then
	if [[ -d "$staging" && -d "$final" && ! -L "$final" ]]; then
		echo "concurrent immutable publication won; refusing overwrite: $final" >&2
	else
		echo "no-replace immutable publication failed: $final" >&2
	fi
	exit 2
fi
if [[ -d "$staging" ]]; then
	echo "concurrent immutable publication won; refusing overwrite: $final" >&2
	exit 2
fi
published_here=1
[[ -d "$final" && ! -L "$final" &&
	"$(stat -c '%d:%i:%h:%s' -- "$final")" == "$published_identity" &&
	"$(stat -c '%u:%a' -- "$final")" == "$EUID:700" ]] || {
	echo 'published pair directory identity or permissions are not sealed' >&2
	exit 2
}
for published in kernel.bzImage modules.vhdx modules-layout.manifest qemu-pass.stamp kernel-pair.manifest; do
	[[ -f "$final/$published" && ! -L "$final/$published" &&
		"$(stat -c '%u:%a:%h' -- "$final/$published")" == "$EUID:444:1" ]] || {
		echo "published pair file identity or permissions are not sealed: $published" >&2
		exit 2
	}
done
sync -f "$output_root" 2>/dev/null || true
chmod 0555 -- "$final"
[[ "$(stat -c '%d:%i:%h:%s:%u:%a' -- "$final")" == "$published_identity:$EUID:555" ]] || {
	echo 'published pair final directory seal readback failed' >&2
	exit 2
}
publication_complete=1
trap - EXIT
printf 'RAMSHARED_KERNEL_PAIR=%s\n' "$final/kernel-pair.manifest"
printf 'RAMSHARED_KERNEL_PAIR_SHA256=%s\n' "$(sha256sum -- "$final/kernel-pair.manifest" | awk '{print $1}')"
printf 'RAMSHARED_PROMOTION_ELIGIBILITY=REFUSED_MODULE_VHDX_PROVENANCE_UNVERIFIED\n'
