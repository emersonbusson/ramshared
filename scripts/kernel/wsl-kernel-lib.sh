# wsl-kernel-lib.sh — shared probes for wsl-kernel.sh
# SPEC: docs/specs/no-milestone/wsl2-custom-kernel-p1/SPEC.md
# shellcheck shell=bash

# shellcheck disable=SC2034
WSL_KERNEL_LIB_LOADED=1

# Live defaults match C:\wsl (boot path used after apply). R: copies remain available.
KERNEL_WIN="${KERNEL_WIN:-C:\\wsl\\kernel-ramshared}"
KERNEL_WSL="${KERNEL_WSL:-/mnt/c/wsl/kernel-ramshared}"
# Fallback if C: copy missing (lab artifact)
if [[ ! -f "$KERNEL_WSL" && -f /mnt/r/WSL/kernels/bzImage-ramshared-latest ]]; then
	KERNEL_WSL=/mnt/r/WSL/kernels/bzImage-ramshared-latest
	KERNEL_WIN='R:\WSL\kernels\bzImage-ramshared-latest'
fi
MODULES_WIN="${MODULES_WIN:-C:\\wsl\\modules-ramshared.vhdx}"
BUILD_DIR_WSL="${BUILD_DIR_WSL:-/mnt/r/WSL/RamShared-Kernel-build}"
MIN_BZIMAGE_BYTES="${MIN_BZIMAGE_BYTES:-1048576}"
ENABLE_TIMEOUT_SEC="${ENABLE_TIMEOUT_SEC:-30}"
INTEROP_FAIL_SEC="${INTEROP_FAIL_SEC:-5}"
APPLY_TIMEOUT_SEC="${APPLY_TIMEOUT_SEC:-60}"

# Exit codes (SPEC)
readonly E_OK=0
readonly E_ACTION=2
readonly E_INTEROP=3
readonly E_APPLY=4
readonly E_USAGE=5

win_user() {
	local u=""
	if [[ -n "${WIN_USER:-}" ]]; then
		printf '%s' "$WIN_USER"
		return 0
	fi
	if [[ ! -e /proc/sys/fs/binfmt_misc/WSLInterop ]] && [[ -w /proc/sys/fs/binfmt_misc/register ]]; then
		echo ':WSLInterop:M::MZ::/init:PF' > /proc/sys/fs/binfmt_misc/register 2>/dev/null || true
	fi
	u="$(timeout "${INTEROP_FAIL_SEC}" /mnt/c/Windows/System32/cmd.exe /c "echo %USERNAME%" 2>/dev/null | tr -d '\0\r\n' || true)"
	if [[ -z "$u" || "$u" == *'%USERNAME%'* ]]; then
		# fallback: first profile under /mnt/c/Users that has .wslconfig or Desktop
		if [[ -d /mnt/c/Users ]]; then
			local d
			for d in /mnt/c/Users/*; do
				[[ -d "$d" ]] || continue
				base="$(basename "$d")"
				[[ "$base" == "Public" || "$base" == "Default" || "$base" == "Default User" || "$base" == "All Users" ]] && continue
				if [[ -f "$d/.wslconfig" || -d "$d/Desktop" ]]; then
					u="$base"
					break
				fi
			done
		fi
	fi
	printf '%s' "$u"
}

wslconfig_path() {
	if [[ -n "${WSL_CONFIG:-}" ]]; then
		printf '%s' "$WSL_CONFIG"
		return 0
	fi
	local u
	u="$(win_user)"
	if [[ -z "$u" ]]; then
		return 1
	fi
	printf '/mnt/c/Users/%s/.wslconfig' "$u"
}

# Normalize kernel= value for comparison (lowercase drive, forward slashes, collapse \\)
norm_kernel_path() {
	local p="$1"
	p="${p//$'\r'/}"
	p="${p#"${p%%[![:space:]]*}"}"
	p="${p%"${p##*[![:space:]]}"}"
	p="${p//\\//}"
	# collapse multiple slashes
	while [[ "$p" == *//* ]]; do p="${p//\/\//\/}"; done
	# R:/WSL/... form
	if [[ "$p" =~ ^[Rr]: ]]; then
		p="R:${p:2}"
	fi
	printf '%s' "$p"
}

is_our_kernel_path() {
	local val
	val="$(norm_kernel_path "$1")"
	# Accept C:/wsl/kernel-ramshared and R:/WSL/kernels/bzImage*
	[[ "$val" =~ [Cc]:/wsl/kernel-ramshared$ ]] && return 0
	[[ "$val" =~ [Rr]:/WSL/kernels/bzImage ]] && return 0
	[[ "$val" == "$(norm_kernel_path "$KERNEL_WIN")" ]] && return 0
	return 1
}

probe_bz() {
	[[ -f "$KERNEL_WSL" ]] || return 1
	local sz
	sz="$(stat -c%s "$KERNEL_WSL" 2>/dev/null || echo 0)"
	[[ "$sz" -gt "$MIN_BZIMAGE_BYTES" ]]
}

release_rel() {
	local f="$BUILD_DIR_WSL/release.txt"
	[[ -f "$f" ]] || return 1
	# First REL= only (file may have trailing MODULES_* lines)
	grep -E '^REL=' "$f" | head -1 | cut -d= -f2- | tr -d '\r'
}

probe_cfg_armed() {
	local cfg
	cfg="$(wslconfig_path 2>/dev/null)" || return 2
	[[ -f "$cfg" ]] || return 1
	local line val
	while IFS= read -r line || [[ -n "$line" ]]; do
		[[ "$line" =~ ^[[:space:]]*kernel[[:space:]]*=[[:space:]]*(.*)$ ]] || continue
		val="${BASH_REMATCH[1]}"
		if is_our_kernel_path "$val"; then
			return 0
		fi
	done <"$cfg"
	return 1
}

probe_cfg_modules() {
	local cfg
	cfg="$(wslconfig_path 2>/dev/null)" || return 2
	[[ -f "$cfg" ]] || return 1
	local line val
	while IFS= read -r line || [[ -n "$line" ]]; do
		[[ "$line" =~ ^[[:space:]]*kernelModules[[:space:]]*=[[:space:]]*(.*)$ ]] || continue
		val="$(norm_kernel_path "${BASH_REMATCH[1]}")"
		[[ "$val" =~ modules-ramshared\.vhdx$ ]] && return 0
	done <"$cfg"
	return 1
}

probe_custom_running() {
	local rel uname_r
	rel="$(release_rel 2>/dev/null || true)"
	uname_r="$(uname -r)"
	if [[ -n "$rel" && "$uname_r" == "$rel" ]]; then
		return 0
	fi
	return 1
}

probe_ublk() {
	if [[ -d /sys/module/ublk_drv ]] || lsmod 2>/dev/null | grep -q '^ublk_drv'; then
		return 0
	fi
	# loadable without loading?
	if modprobe -n ublk_drv >/dev/null 2>&1; then
		return 1 # not loaded but loadable → NEED_MODULE
	fi
	# module unknown
	return 2
}

# Sets global STATE
resolve_state() {
	STATE="NEED_BUILD"
	local cfg_rc=0

	if ! probe_bz; then
		if probe_cfg_armed 2>/dev/null; then
			STATE="BROKEN"
		else
			STATE="NEED_BUILD"
		fi
		return 0
	fi

	probe_cfg_armed
	cfg_rc=$?
	if [[ $cfg_rc -eq 2 ]]; then
		# unknown config → still may detect custom via uname
		if probe_custom_running; then
			local ub
			probe_ublk
			ub=$?
			if [[ $ub -eq 0 ]]; then
				STATE="READY"
			elif [[ $ub -eq 1 ]]; then
				STATE="NEED_MODULE"
			else
				STATE="NEED_MODULE"
			fi
		else
			STATE="NEED_ARM"
		fi
		return 0
	fi

	if [[ $cfg_rc -ne 0 ]]; then
		STATE="NEED_ARM"
		return 0
	fi

	# armed
	if ! probe_custom_running; then
		STATE="NEED_REBOOT"
		return 0
	fi

	local ub
	probe_ublk
	ub=$?
	if [[ $ub -eq 0 ]]; then
		STATE="READY"
	else
		STATE="NEED_MODULE"
	fi
}

print_status_lines() {
	local cfg rel
	cfg="$(wslconfig_path 2>/dev/null || echo '(unknown)')"
	rel="$(release_rel 2>/dev/null || echo '(none)')"
	echo "STATE=$STATE"
	echo "uname=$(uname -r)"
	echo "release_txt=$rel"
	echo "bzImage=$KERNEL_WSL exists=$(probe_bz && echo yes || echo no)"
	echo "wslconfig=$cfg"
	echo "cfg_armed=$(probe_cfg_armed 2>/dev/null && echo yes || echo no)"
	echo "cfg_modules=$(probe_cfg_modules 2>/dev/null && echo yes || echo no)"
	if [[ -d /sys/module/ublk_drv ]] || lsmod 2>/dev/null | grep -q '^ublk_drv'; then
		echo "ublk=loaded"
	elif modprobe -n ublk_drv >/dev/null 2>&1; then
		echo "ublk=loadable"
	else
		echo "ublk=unavailable"
	fi
}

next_step_msg() {
	case "$STATE" in
	NEED_BUILD)
		echo "NEXT: build custom kernel (lab RamShared-Kernel / scripts/kernel/build-wsl-kernel.sh)"
		;;
	NEED_ARM)
		echo "NEXT: bash scripts/kernel/wsl-kernel.sh arm"
		;;
	NEED_REBOOT)
		echo "NEXT: restart WSL (close sessions) or: bash scripts/kernel/wsl-kernel.sh apply --i-know-this-stops-all-wsl"
		;;
	NEED_MODULE)
		echo "NEXT: enable will try modprobe ublk_drv"
		;;
	BROKEN)
		echo "NEXT: bash scripts/kernel/wsl-kernel.sh disarm  # then repair bzImage / arm again"
		;;
	READY)
		echo "NEXT: nothing (READY)"
		;;
	*)
		echo "NEXT: unknown state"
		;;
	esac
}
