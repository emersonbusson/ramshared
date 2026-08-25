#!/usr/bin/env bash
# In-WSL control for the RamShared custom-kernel qualification contract.
# Live apply remains disruptive and requires the explicit stop-all-WSL token.
set -euo pipefail

COMMAND_STARTED_SECONDS=$SECONDS
COMMAND_DEADLINE_SECONDS=180

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=wsl-kernel-lib.sh
source "$ROOT/wsl-kernel-lib.sh"

usage() {
	cat <<'EOF'
Usage: wsl-kernel.sh <status|enable|arm|disarm|apply> [flags]

  status   Verify receipt, pair, config, runtime release, and module metadata
  enable   Inert NO-GO; never loads a module
  arm      Unsafe lab-only pair arm without restart or auto-recovery
  disarm   Atomically remove both kernel= and kernelModules= without restart
  apply    Run bundled/candidate A/B promotion; stops every WSL distro

apply flags:
  --manifest <absolute-path>         immutable kernel-pair.manifest
  --i-know-this-stops-all-wsl        mandatory disruptive approval

Environment:
  KERNEL_CANARY_DISTRO APPROVED_WSL_FAILED_UNITS WSL_CONFIG WIN_USER
  APPLY_TIMEOUT_SEC PROMOTION_RECEIPT_WSL
EOF
}

assert_canonical_path() {
	local value="$1" name="$2" cursor resolved
	[[ -n "$value" && "$value" == /* && "$value" != *$'\n'* && "$value" != *$'\r'* &&
		"$value" != *//* && "$value" != */./* && "$value" != */../* &&
		"$value" != */. && "$value" != */.. ]] || {
		echo "$name is not a canonical absolute path" >&2
		return 1
	}
	resolved="$(realpath -m -- "$value")" || return 1
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

validate_environment_contract() {
	local name value
	for name in MIN_BZIMAGE_BYTES ENABLE_TIMEOUT_SEC INTEROP_FAIL_SEC APPLY_TIMEOUT_SEC; do
		value="${!name}"
		[[ "$value" =~ ^[1-9][0-9]*$ ]] || {
			echo "$name must be a positive decimal integer" >&2
			return 1
		}
	done
	((MIN_BZIMAGE_BYTES >= 1048576 && MIN_BZIMAGE_BYTES <= 1073741824)) || return 1
	((ENABLE_TIMEOUT_SEC >= 1 && ENABLE_TIMEOUT_SEC <= 120)) || return 1
	((INTEROP_FAIL_SEC >= 1 && INTEROP_FAIL_SEC <= 60)) || return 1
	((APPLY_TIMEOUT_SEC >= 10 && APPLY_TIMEOUT_SEC <= 120)) || return 1
	[[ "$KERNEL_CANARY_DISTRO" =~ ^[A-Za-z0-9][A-Za-z0-9._\ -]{0,127}$ ]] || return 1
	[[ -z "$APPROVED_WSL_FAILED_UNITS" ||
		"$APPROVED_WSL_FAILED_UNITS" =~ ^[A-Za-z0-9@_.:-]+(,[A-Za-z0-9@_.:-]+)*$ ]] || return 1
	[[ -z "${WIN_USER:-}" || "$WIN_USER" =~ ^[A-Za-z0-9._-]{1,64}$ ]] || return 1
	if [[ -n "${WSL_CONFIG:-}" ]]; then
		assert_canonical_path "$WSL_CONFIG" WSL_CONFIG || return 1
		[[ "$WSL_CONFIG" =~ ^/mnt/[cC]/Users/[A-Za-z0-9._\ -]+/\.wslconfig$ ]] || {
			echo 'WSL_CONFIG must name one canonical Windows user .wslconfig' >&2
			return 1
		}
	fi
	assert_canonical_path "$PROMOTION_RECEIPT_WSL" PROMOTION_RECEIPT_WSL || return 1
	[[ "$PROMOTION_RECEIPT_WSL" == '/mnt/c/wsl/ramshared-receipts/promotion-current.json' ]] || {
		echo 'PROMOTION_RECEIPT_WSL must use the canonical receipt path' >&2
		return 1
	}
	if [[ -n "${KERNEL_PAIR_MANIFEST:-}" ]]; then
		assert_canonical_path "$KERNEL_PAIR_MANIFEST" KERNEL_PAIR_MANIFEST || return 1
	fi
}

require_no_arguments() {
	[[ $# -eq 0 ]] || {
		echo 'command does not accept arguments' >&2
		return 1
	}
}

check_command_deadline() {
	local elapsed=$((SECONDS - COMMAND_STARTED_SECONDS))
	((elapsed < COMMAND_DEADLINE_SECONDS)) || {
		echo 'command deadline expired before the next operation' >&2
		return 1
	}
}

cmd_status() {
	require_no_arguments "$@" || exit "$E_USAGE"
	check_command_deadline || exit "$E_ACTION"
	resolve_state
	print_status_lines
	next_step_msg
	[[ "$STATE" == "READY" ]] && exit "$E_OK"
	exit "$E_ACTION"
}

cmd_enable() {
	require_no_arguments "$@" || exit "$E_USAGE"
	echo 'ENABLE_LIVE_MUTATION=NO_GO module-loading-is-unconditionally-disabled'
	echo 'enable: inert; qualification state is read-only until a separately reviewed activation design exists'
	return "$E_ACTION"
}

atomic_write_file() {
	local destination="$1"
	local parent base temporary expected actual
	parent="$(dirname -- "$destination")"
	base="$(basename -- "$destination")"
	[[ -d "$parent" && ! -L "$parent" ]] || {
		echo "atomic write parent is unavailable or symlinked: $parent" >&2
		return 1
	}
	[[ ! -L "$destination" ]] || {
		echo "atomic write target must not be a symlink: $destination" >&2
		return 1
	}
	temporary="$(mktemp --tmpdir="$parent" ".${base}.ramshared.XXXXXX")"
	if ! cat >"$temporary"; then
		rm -f -- "$temporary"
		return 1
	fi
	expected="$(sha256sum -- "$temporary" | awk '{print $1}')"
	sync -f "$temporary" 2>/dev/null || true
	if ! mv -fT -- "$temporary" "$destination"; then
		rm -f -- "$temporary"
		return 1
	fi
	sync -f "$parent" 2>/dev/null || true
	actual="$(sha256sum -- "$destination" | awk '{print $1}')"
	[[ "$actual" == "$expected" ]] || {
		echo "atomic write readback hash mismatch for $destination" >&2
		return 1
	}
}

fresh_config_snapshot() {
	local cfg="$1"
	local stamp snapshot expected actual
	stamp="$(date -u +%Y%m%dT%H%M%S%NZ)"
	snapshot="${cfg}.ramshared.snapshot.${stamp}"
	[[ ! -e "$snapshot" && ! -L "$snapshot" && ! -L "$cfg" ]] || {
		echo "fresh snapshot target or config identity is unsafe" >&2
		return 1
	}
	if [[ -f "$cfg" && ! -L "$cfg" ]]; then
		expected="$(sha256sum -- "$cfg" | awk '{print $1}')"
		(umask 077; cp --update=none -- "$cfg" "$snapshot")
	else
		expected="$(printf '' | sha256sum | awk '{print $1}')"
		(umask 077; set -o noclobber; : >"$snapshot")
	fi
	actual="$(sha256sum -- "$snapshot" | awk '{print $1}')"
	[[ "$actual" == "$expected" ]] || {
		echo "fresh snapshot readback hash mismatch" >&2
		return 1
	}
	printf '%s' "$snapshot"
}

arm_config_file() {
	local cfg="$1" kernel_win="$2" modules_win="$3"
	local kernel_line="kernel=${kernel_win//\\//}"
	local modules_line="kernelModules=${modules_win//\\//}"
	local -a lines=() output=()
	local line in_wsl2=0 has_wsl2=0 inserted=0 wsl2_count=0
	[[ ! -L "$cfg" ]] || {
		echo "arm refused a symlinked .wslconfig" >&2
		return 1
	}
	[[ -f "$cfg" ]] && mapfile -t lines <"$cfg"
	for line in "${lines[@]}"; do
		[[ "$line" =~ ^[[:space:]]*\[wsl2\][[:space:]]*$ ]] && ((wsl2_count += 1))
	done
	[[ $wsl2_count -le 1 ]] || {
		echo "arm refused duplicate [wsl2] sections" >&2
		return 1
	}
	for line in "${lines[@]}"; do
		if [[ "$line" =~ ^[[:space:]]*\[wsl2\][[:space:]]*$ ]]; then
			in_wsl2=1
			has_wsl2=1
			output+=("$line")
			continue
		fi
		if [[ "$line" =~ ^[[:space:]]*\[ ]]; then
			if [[ $in_wsl2 -eq 1 && $inserted -eq 0 ]]; then
				output+=("$kernel_line" "$modules_line")
				inserted=1
			fi
			in_wsl2=0
		fi
		if [[ "$line" =~ ^[[:space:]]*(kernel|kernelModules)[[:space:]]*= ]]; then
			continue
		fi
		output+=("$line")
	done
	if [[ $in_wsl2 -eq 1 && $inserted -eq 0 ]]; then
		output+=("$kernel_line" "$modules_line")
		inserted=1
	fi
	if [[ $has_wsl2 -eq 0 ]]; then
		output=("[wsl2]" "$kernel_line" "$modules_line" "${output[@]}")
	fi
	printf '%s\n' "${output[@]}" | atomic_write_file "$cfg"
	local -a values=()
	mapfile -t values < <(config_pair_values "$cfg") || true
	[[ ${#values[@]} -eq 2 &&
		"${values[0]}" == "$(norm_windows_path "$kernel_win")" &&
		"${values[1]}" == "$(norm_windows_path "$modules_win")" ]]
}

disarm_config_file() {
	local cfg="$1"
	[[ -f "$cfg" ]] || return 0
	[[ ! -L "$cfg" ]] || {
		echo "disarm refused a symlinked .wslconfig" >&2
		return 1
	}
	local -a output=()
	local line
	while IFS= read -r line || [[ -n "$line" ]]; do
		[[ "$line" =~ ^[[:space:]]*(kernel|kernelModules)[[:space:]]*= ]] && continue
		output+=("$line")
	done <"$cfg"
	printf '%s\n' "${output[@]}" | atomic_write_file "$cfg"
	! grep -qE '^[[:space:]]*(kernel|kernelModules)[[:space:]]*=' "$cfg"
}

parse_manifest_flag() {
	local allow_approval="$1" manifest_seen=0
	shift
	SELECTED_MANIFEST=""
	APPROVAL_SEEN=0
	if [[ -n "${KERNEL_PAIR_MANIFEST:-}" ]]; then
		SELECTED_MANIFEST="$KERNEL_PAIR_MANIFEST"
		manifest_seen=1
	fi
	while (($#)); do
		case "$1" in
		--manifest)
			[[ $manifest_seen -eq 0 && $# -ge 2 && -n "${2//[[:space:]]/}" && "$2" != --* ]] || return 1
			SELECTED_MANIFEST="$2"
			manifest_seen=1
			shift 2
			;;
		--i-know-this-stops-all-wsl)
			[[ "$allow_approval" == 1 && $APPROVAL_SEEN -eq 0 ]] || return 1
			APPROVAL_SEEN=1
			shift
			;;
		*) return 1 ;;
		esac
	done
	[[ -n "$SELECTED_MANIFEST" ]]
}

cmd_arm() {
	parse_manifest_flag 0 "$@" || {
		echo "arm: provide --manifest <absolute immutable kernel-pair.manifest>" >&2
		exit "$E_USAGE"
	}
	if [[ "${RAMSHARED_UNSAFE_LAB_ARM:-}" != "I_ACCEPT_NO_AUTO_REVERT" ]]; then
		echo "arm: refused; an unattended natural reboot has no bounded auto-recovery"
		echo "production: use apply with the stop-all-WSL token"
		echo "lab only: RAMSHARED_UNSAFE_LAB_ARM=I_ACCEPT_NO_AUTO_REVERT"
		exit "$E_ACTION"
	fi
	load_kernel_pair_manifest "$SELECTED_MANIFEST" || {
		echo "arm: $PAIR_ERROR" >&2
		exit "$E_ACTION"
	}
	echo 'MODULE_VHDX_PROVENANCE=REFUSED cryptographic-containment-not-verifiable'
	echo 'arm: LIVE_PROMOTION=NO_GO; no .wslconfig mutation was attempted'
	exit "$E_ACTION"
	local cfg kernel_win modules_win snapshot
	cfg="$(wslconfig_path)" || {
		echo "arm: cannot resolve the Windows user .wslconfig" >&2
		exit "$E_INTEROP"
	}
	mkdir -p -- "$(dirname -- "$cfg")"
	kernel_win="$(wsl_to_windows_path "${KERNEL_PAIR[kernel_path]}")" || {
		echo "arm: kernel pair must reside on a mounted Windows drive" >&2
		exit "$E_ACTION"
	}
	modules_win="$(wsl_to_windows_path "${KERNEL_PAIR[modules_path]}")" || {
		echo "arm: kernel pair must reside on a mounted Windows drive" >&2
		exit "$E_ACTION"
	}
	snapshot="$(fresh_config_snapshot "$cfg")"
	arm_config_file "$cfg" "$kernel_win" "$modules_win" || {
		echo "arm: atomic pair write or readback failed; snapshot=$snapshot" >&2
		exit "$E_ACTION"
	}
	echo "arm: exact kernel/modules pair written; no WSL restart was performed"
	echo "arm: snapshot=$snapshot"
	echo "arm: LIVE-NO-GO until the bounded apply transaction succeeds"
}

cmd_disarm() {
	require_no_arguments "$@" || exit "$E_USAGE"
	echo 'disarm: LIVE_MUTATION=NO_GO; use the reviewed rollback transaction when promotion is re-enabled'
	exit "$E_ACTION"
	local cfg snapshot
	cfg="$(wslconfig_path)" || {
		echo "disarm: cannot resolve .wslconfig" >&2
		exit "$E_INTEROP"
	}
	if [[ ! -f "$cfg" ]]; then
		echo "disarm: no config; both pair keys are already absent"
		exit "$E_OK"
	fi
	snapshot="$(fresh_config_snapshot "$cfg")"
	disarm_config_file "$cfg" || {
		echo "disarm: atomic pair removal or readback failed; snapshot=$snapshot" >&2
		exit "$E_ACTION"
	}
	echo "disarm: atomically removed kernel= and kernelModules=; no restart"
	echo "disarm: fresh_snapshot=$snapshot"
}

to_windows_path() {
	local source="$1"
	timeout --signal=TERM --kill-after=2s "${INTEROP_FAIL_SEC}s" wslpath -w "$source" 2>/dev/null | tr -d '\r'
}

parse_install_receipt() {
	local text="$1" line key value
	declare -gA INSTALL_RECEIPT=()
	while IFS= read -r line || [[ -n "$line" ]]; do
		[[ "$line" =~ ^(RAMSHARED_INSTALL_SCHEMA|RAMSHARED_BUNDLE_ID|RAMSHARED_INSTALLED_WRAPPER|RAMSHARED_DEPLOYMENT_MANIFEST|RAMSHARED_DEPLOYMENT_SHA256)=(.*)$ ]] || return 1
		key="${BASH_REMATCH[1]}"
		value="${BASH_REMATCH[2]%$'\r'}"
		[[ -z "${INSTALL_RECEIPT[$key]+x}" ]] || return 1
		INSTALL_RECEIPT[$key]="$value"
	done <<<"$text"
	[[ ${#INSTALL_RECEIPT[@]} -eq 5 &&
		"${INSTALL_RECEIPT[RAMSHARED_INSTALL_SCHEMA]}" == "1" &&
		"${INSTALL_RECEIPT[RAMSHARED_BUNDLE_ID]}" =~ ^v1-[0-9a-f]{16}-[0-9a-f]{16}-[0-9a-f]{16}-[0-9a-f]{16}-[0-9a-f]{16}$ &&
		"${INSTALL_RECEIPT[RAMSHARED_INSTALLED_WRAPPER]}" =~ ^[A-Za-z]:\\ &&
		"${INSTALL_RECEIPT[RAMSHARED_DEPLOYMENT_MANIFEST]}" =~ ^[A-Za-z]:\\ ]] &&
		canonical_sha256 "${INSTALL_RECEIPT[RAMSHARED_DEPLOYMENT_SHA256]}"
}

cmd_apply() {
	parse_manifest_flag 1 "$@" || {
		echo "apply: provide --manifest and --i-know-this-stops-all-wsl" >&2
		exit "$E_USAGE"
	}
	if [[ $APPROVAL_SEEN -ne 1 ]]; then
		echo "apply: refused; this stops every WSL distro"
		echo "add --i-know-this-stops-all-wsl in the same attended command"
		exit "$E_USAGE"
	fi
	load_kernel_pair_manifest "$SELECTED_MANIFEST" || {
		echo "apply: $PAIR_ERROR" >&2
		exit "$E_ACTION"
	}
	for artifact in manifest_path kernel_path modules_path layout_inventory_path qemu_stamp_path; do
		assert_canonical_path "${KERNEL_PAIR[$artifact]}" "KERNEL_PAIR[$artifact]" || exit "$E_ACTION"
		[[ "$(stat -c '%h' -- "${KERNEL_PAIR[$artifact]}")" == 1 ]] || {
			echo "apply: sealed artifact has multiple filesystem links: ${KERNEL_PAIR[$artifact]}" >&2
			exit "$E_ACTION"
		}
	done
	echo 'MODULE_VHDX_PROVENANCE=REFUSED cryptographic-containment-not-verifiable'
	echo 'apply: LIVE_PROMOTION=NO_GO before install, log, config, or WSL lifecycle effects'
	exit "$E_APPLY"
	local repo_root wrapper launcher installer
	repo_root="$(cd "$ROOT/../.." && pwd)"
	wrapper="$repo_root/scripts/kernel/boot-kernel-logged.ps1"
	launcher="$repo_root/scripts/kernel/boot-kernel-safe.ps1"
	installer="$repo_root/scripts/kernel/Install-BootKernelLaunchers.ps1"
	for source in "$wrapper" "$launcher" "$installer"; do
		[[ -f "$source" && ! -L "$source" ]] || {
			echo "apply: reviewed source is missing or symlinked: $source" >&2
			exit "$E_ACTION"
		}
	done

	local wrapper_win launcher_win installer_win manifest_win kernel_win modules_win layout_win qemu_win
	wrapper_win="$(to_windows_path "$wrapper")" || exit "$E_INTEROP"
	launcher_win="$(to_windows_path "$launcher")" || exit "$E_INTEROP"
	installer_win="$(to_windows_path "$installer")" || exit "$E_INTEROP"
	manifest_win="$(to_windows_path "${KERNEL_PAIR[manifest_path]}")" || exit "$E_INTEROP"
	kernel_win="$(to_windows_path "${KERNEL_PAIR[kernel_path]}")" || exit "$E_INTEROP"
	modules_win="$(to_windows_path "${KERNEL_PAIR[modules_path]}")" || exit "$E_INTEROP"
	layout_win="$(to_windows_path "${KERNEL_PAIR[layout_inventory_path]}")" || exit "$E_INTEROP"
	qemu_win="$(to_windows_path "${KERNEL_PAIR[qemu_stamp_path]}")" || exit "$E_INTEROP"

	local powershell51='/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe'
	[[ -x "$powershell51" ]] || {
		echo "apply: Windows PowerShell 5.1 is unavailable" >&2
		exit "$E_INTEROP"
	}
	local wrapper_sha launcher_sha install_output
	wrapper_sha="$(sha256sum -- "$wrapper" | awk '{print $1}')"
	launcher_sha="$(sha256sum -- "$launcher" | awk '{print $1}')"
	echo "apply: installing immutable launcher and artifact bundle before shutdown"
	if ! install_output="$(timeout --signal=TERM --kill-after=5s 120s "$powershell51" -NoProfile -NonInteractive -ExecutionPolicy Bypass \
		-File "$installer_win" \
		-SourceWrapper "$wrapper_win" \
		-SourceLauncher "$launcher_win" \
		-SourceKernelManifest "$manifest_win" \
		-SourceKernel "$kernel_win" \
		-SourceModules "$modules_win" \
		-SourceLayoutInventory "$layout_win" \
		-SourceQemuStamp "$qemu_win" \
		-ExpectedWrapperSha256 "$wrapper_sha" \
		-ExpectedLauncherSha256 "$launcher_sha" \
		-ExpectedKernelManifestSha256 "${KERNEL_PAIR[manifest_sha256]}" \
		-ExpectedKernelSha256 "${KERNEL_PAIR[kernel_sha256]}" \
		-ExpectedModulesSha256 "${KERNEL_PAIR[modules_sha256]}" \
		-ExpectedLayoutInventorySha256 "${KERNEL_PAIR[layout_inventory_sha256]}" \
		-ExpectedQemuStampSha256 "${KERNEL_PAIR[qemu_stamp_sha256]}" \
		'-Run' \
		'-ConfirmationToken' 'INSTALL_IMMUTABLE_KERNEL_LAUNCHER_BUNDLE')"; then
		echo "apply: immutable bundle installation failed before shutdown" >&2
		exit "$E_APPLY"
	fi
	parse_install_receipt "$install_output" || {
		echo "apply: installer returned a malformed deployment receipt" >&2
		exit "$E_APPLY"
	}

	echo "apply: bundle=${INSTALL_RECEIPT[RAMSHARED_BUNDLE_ID]}"
	echo "apply: WARNING: the next step stops every WSL distro"
	echo "apply: the installed Windows wrapper owns A/B, rollback, and the final receipt"
	local -a launch_args=(
		-NoProfile -NonInteractive -ExecutionPolicy Bypass
		-File "${INSTALL_RECEIPT[RAMSHARED_INSTALLED_WRAPPER]}"
		-DeploymentManifest "${INSTALL_RECEIPT[RAMSHARED_DEPLOYMENT_MANIFEST]}"
		-ExpectedDeploymentSha256 "${INSTALL_RECEIPT[RAMSHARED_DEPLOYMENT_SHA256]}"
		-Distro "$KERNEL_CANARY_DISTRO"
		-TimeoutSec "$APPLY_TIMEOUT_SEC"
		'-Run'
		'-ConfirmationToken' 'PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL'
	)
	if [[ -n "$APPROVED_WSL_FAILED_UNITS" ]]; then
		launch_args+=(-ApprovedFailedUnits "$APPROVED_WSL_FAILED_UNITS")
	fi
	timeout --signal=TERM --kill-after=5s "$((APPLY_TIMEOUT_SEC + 60))s" "$powershell51" "${launch_args[@]}" || {
		echo "apply: installed launcher returned non-zero; inspect C:\\wsl\\ramshared-launchers\\receipts" >&2
		exit "$E_APPLY"
	}
}

main() {
	validate_environment_contract || exit "$E_USAGE"
	local command="${1:-status}"
	shift || true
	case "$command" in
	status) cmd_status "$@" ;;
	enable) cmd_enable "$@" ;;
	arm) cmd_arm "$@" ;;
	disarm) cmd_disarm "$@" ;;
	apply) cmd_apply "$@" ;;
	-h|--help|help) usage ;;
	*) usage >&2; exit "$E_USAGE" ;;
	esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
	if [[ "${RAMSHARED_COMMAND_SUPERVISED:-0}" == 1 ]]; then
		main "$@"
	else
		exec timeout --signal=TERM --kill-after=2s "${COMMAND_DEADLINE_SECONDS}s" \
			env RAMSHARED_COMMAND_SUPERVISED=1 "$0" "$@"
	fi
fi
