#!/usr/bin/env bash
# wsl-kernel.sh — in-WSL control for RamShared custom WSL2 kernel (P1)
#
# SPEC: docs/specs/no-milestone/wsl2-custom-kernel-p1/SPEC.md
# PRD:  docs/specs/no-milestone/wsl2-custom-kernel-p1/PRD.md
#
# Subcommands:
#   status   read-only state (default)
#   enable   no-op if READY; modprobe if NEED_MODULE; never wsl --shutdown
#   arm      write .wslconfig kernel= (no shutdown); requires bzImage
#   disarm   remove kernel= lines (no shutdown)
#   apply    EXPLICIT reboot of all WSL — requires --i-know-this-stops-all-wsl
#
# Run from product Ubuntu (or any distro). Build happens on lab RamShared-Kernel.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=wsl-kernel-lib.sh
source "$ROOT/wsl-kernel-lib.sh"

usage() {
	cat <<'EOF'
Usage: wsl-kernel.sh <status|enable|arm|disarm|apply> [flags]

  status   Print STATE=… (NEED_BUILD|NEED_ARM|NEED_REBOOT|NEED_MODULE|READY|BROKEN)
  enable   If READY: no-op. If NEED_MODULE: modprobe ublk_drv. Never restarts WSL.
  arm      Point .wslconfig at R:\WSL\kernels\bzImage-ramshared-latest (no restart)
  disarm   Remove kernel= from .wslconfig (no restart)
  apply    Restart all WSL onto armed kernel (requires --i-know-this-stops-all-wsl)

Env: KERNEL_WSL KERNEL_WIN BUILD_DIR_WSL WSL_CONFIG WIN_USER
EOF
}

cmd_status() {
	resolve_state
	print_status_lines
	next_step_msg
	if [[ "$STATE" == "READY" ]]; then
		exit "$E_OK"
	fi
	exit "$E_ACTION"
}

cmd_enable() {
	# Hard rule: never restart the WSL VM from enable (see SPEC ITEM-4).
	local start
	start="$(date +%s)"
	resolve_state
	print_status_lines
	case "$STATE" in
	READY)
		echo "enable: READY (no-op)"
		next_step_msg
		exit "$E_OK"
		;;
	NEED_MODULE)
		echo "enable: loading ublk_drv…"
		if sudo -n modprobe ublk_drv 2>/dev/null || sudo modprobe ublk_drv; then
			resolve_state
			print_status_lines
			if [[ "$STATE" == "READY" ]]; then
				echo "enable: ublk_drv loaded"
				exit "$E_OK"
			fi
			echo "enable: modprobe ran but state=$STATE"
			exit "$E_ACTION"
		fi
		echo "enable: modprobe ublk_drv failed (module missing on this kernel?)"
		exit "$E_ACTION"
		;;
	*)
		echo "enable: cannot activate features in state=$STATE (will not restart WSL)"
		next_step_msg
		exit "$E_ACTION"
		;;
	esac
	if (( $(date +%s) - start > ENABLE_TIMEOUT_SEC )); then
		echo "enable: exceeded ${ENABLE_TIMEOUT_SEC}s (NFR-K8)"
		exit "$E_ACTION"
	fi
}

# Write .wslconfig via pure bash on /mnt/c (no PowerShell required for arm/disarm)
atomic_write_file() {
	local dest="$1"
	local tmp
	tmp="${dest}.tmp.$$"
	cat >"$tmp"
	mv -f "$tmp" "$dest"
}

arm_config_file() {
	local cfg="$1"
	# Prefer C:\wsl copies (stable for WSL boot + modules VHDX)
	local kline="kernel=C:\\\\wsl\\\\kernel-ramshared"
	local mline="kernelModules=C:\\\\wsl\\\\modules-ramshared.vhdx"
	local -a lines=()
	local line in_wsl2=0 has_wsl2=0 added_k=0 added_m=0
	if [[ -f "$cfg" ]]; then
		mapfile -t lines <"$cfg" || true
	fi
	local -a out=()
	for line in "${lines[@]+"${lines[@]}"}"; do
		if [[ "$line" =~ ^[[:space:]]*\[wsl2\][[:space:]]*$ ]]; then
			in_wsl2=1
			has_wsl2=1
			out+=("$line")
			continue
		fi
		if [[ "$line" =~ ^[[:space:]]*\[ ]]; then
			if [[ $in_wsl2 -eq 1 ]]; then
				[[ $added_k -eq 0 ]] && { out+=("$kline"); added_k=1; }
				[[ $added_m -eq 0 ]] && { out+=("$mline"); added_m=1; }
			fi
			in_wsl2=0
			out+=("$line")
			continue
		fi
		if [[ $in_wsl2 -eq 1 && "$line" =~ ^[[:space:]]*kernel[[:space:]]*= ]]; then
			continue
		fi
		if [[ $in_wsl2 -eq 1 && "$line" =~ ^[[:space:]]*kernelModules[[:space:]]*= ]]; then
			continue
		fi
		out+=("$line")
	done
	if [[ $in_wsl2 -eq 1 ]]; then
		[[ $added_k -eq 0 ]] && { out+=("$kline"); added_k=1; }
		[[ $added_m -eq 0 ]] && { out+=("$mline"); added_m=1; }
	fi
	if [[ $has_wsl2 -eq 0 ]]; then
		out=("[wsl2]" "$kline" "$mline" "${out[@]+"${out[@]}"}")
	fi
	printf '%s\n' "${out[@]}" | atomic_write_file "$cfg"
}

disarm_config_file() {
	local cfg="$1"
	[[ -f "$cfg" ]] || return 0
	local -a out=()
	local line
	while IFS= read -r line || [[ -n "$line" ]]; do
		[[ "$line" =~ ^[[:space:]]*kernel[[:space:]]*= ]] && continue
		[[ "$line" =~ ^[[:space:]]*kernelModules[[:space:]]*= ]] && continue
		out+=("$line")
	done <"$cfg"
	printf '%s\n' "${out[@]+"${out[@]}"}" | atomic_write_file "$cfg"
}

cmd_arm() {
	if ! probe_bz; then
		echo "arm: refuse — bzImage missing or too small: $KERNEL_WSL"
		echo "NEXT: finish build on lab RamShared-Kernel"
		exit "$E_ACTION"
	fi
	local cfg
	if ! cfg="$(wslconfig_path)"; then
		echo "arm: cannot resolve Windows user .wslconfig (interop?)"
		echo "Set WSL_CONFIG=/mnt/c/Users/<you>/.wslconfig or WIN_USER=<you>"
		exit "$E_INTEROP"
	fi
	mkdir -p "$(dirname "$cfg")" 2>/dev/null || true
	if [[ -f "$cfg" ]]; then
		cp -f "$cfg" "${cfg}.ramshared.bak" 2>/dev/null || true
	fi
	# Ensure Windows-side kernel + modules copies exist (boot uses C:\wsl)
	mkdir -p /mnt/c/wsl 2>/dev/null || true
	if [[ -f /mnt/r/WSL/kernels/bzImage-ramshared-latest ]]; then
		cp -f /mnt/r/WSL/kernels/bzImage-ramshared-latest /mnt/c/wsl/kernel-ramshared
	fi
	if [[ -f /mnt/r/WSL/kernels/modules-ramshared.vhdx ]]; then
		cp -f /mnt/r/WSL/kernels/modules-ramshared.vhdx /mnt/c/wsl/modules-ramshared.vhdx 2>/dev/null || true
	fi
	if [[ ! -f /mnt/c/wsl/kernel-ramshared ]]; then
		echo "arm: missing C:\\wsl\\kernel-ramshared"
		exit "$E_ACTION"
	fi
	if [[ ! -f /mnt/c/wsl/modules-ramshared.vhdx ]]; then
		echo "arm: WARN missing C:\\wsl\\modules-ramshared.vhdx (ublk may fail until modules VHDX is installed)"
	fi
	# seed clean original if absent and current has no kernel=
	local clean="/mnt/c/wsl/wslconfig-original.txt"
	if [[ ! -f "$clean" ]]; then
		if [[ -f "$cfg" ]] && ! grep -qE '^[[:space:]]*kernel[[:space:]]*=' "$cfg" 2>/dev/null; then
			cp -f "$cfg" "$clean" 2>/dev/null || true
		elif [[ ! -f "$cfg" ]]; then
			printf '%s\n' "[wsl2]" >"$clean" 2>/dev/null || true
		fi
	fi
	if [[ ! -f "$cfg" ]]; then
		printf '%s\n' "[wsl2]" >"$cfg"
	fi
	arm_config_file "$cfg"
	echo "arm: wrote kernel= + kernelModules= under [wsl2] in $cfg"
	echo "arm: no WSL restart performed (NEED_REBOOT until next start or apply)"
	resolve_state
	print_status_lines
	next_step_msg
	exit "$E_OK"
}

cmd_disarm() {
	local cfg
	if ! cfg="$(wslconfig_path)"; then
		echo "disarm: cannot resolve .wslconfig"
		exit "$E_INTEROP"
	fi
	if [[ ! -f "$cfg" ]]; then
		echo "disarm: no config (already stock)"
		exit "$E_OK"
	fi
	cp -f "$cfg" "${cfg}.ramshared.bak" 2>/dev/null || true
	disarm_config_file "$cfg"
	echo "disarm: removed kernel= from $cfg (no restart; takes effect next start)"
	exit "$E_OK"
}

cmd_apply() {
	local flag_ok=0
	local a
	for a in "$@"; do
		if [[ "$a" == "--i-know-this-stops-all-wsl" ]]; then
			flag_ok=1
		fi
	done
	if [[ $flag_ok -ne 1 ]]; then
		echo "apply: refused — this stops ALL WSL distros"
		echo "usage: wsl-kernel.sh apply --i-know-this-stops-all-wsl"
		exit "$E_USAGE"
	fi
	if ! probe_bz; then
		echo "apply: no bzImage"
		exit "$E_ACTION"
	fi
	local stamp="$BUILD_DIR_WSL/qemu-pass.stamp"
	if [[ ! -f "$stamp" ]]; then
		echo "apply: missing $stamp — run qemu-validate first (SPEC ITEM-3)"
		exit "$E_ACTION"
	fi
	local stamp_sha file_sha
	stamp_sha="$(grep -E '^KERNEL_SHA256=' "$stamp" | cut -d= -f2- || true)"
	file_sha="$(sha256sum "$KERNEL_WSL" | awk '{print $1}')"
	if [[ -z "$stamp_sha" || "$stamp_sha" != "$file_sha" ]]; then
		echo "apply: qemu stamp sha mismatch — re-run qemu-validate"
		exit "$E_ACTION"
	fi
	local rel
	rel="$(release_rel || true)"
	if [[ -z "$rel" ]]; then
		echo "apply: missing REL in release.txt"
		exit "$E_ACTION"
	fi
	local repo_root ps1
	repo_root="$(cd "$ROOT/../.." && pwd)"
	ps1="$repo_root/scripts/kernel/boot-kernel-logged.ps1"
	if [[ ! -f "$ps1" ]]; then
		echo "apply: missing $ps1"
		exit "$E_ACTION"
	fi
	# Convert to Windows path
	local ps1_win
	if ! ps1_win="$(timeout "$INTEROP_FAIL_SEC" wslpath -w "$ps1" 2>/dev/null)"; then
		ps1_win="$(timeout "$INTEROP_FAIL_SEC" /mnt/c/Windows/System32/wsl.exe -e wslpath -w "$ps1" 2>/dev/null | tr -d '\0\r' || true)"
	fi
	if [[ -z "${ps1_win:-}" ]]; then
		echo "apply: cannot convert path to Windows (interop)"
		echo "Run elevated PowerShell:"
		echo "  powershell -NoProfile -ExecutionPolicy Bypass -File <repo>\\scripts\\kernel\\boot-kernel-logged.ps1 \\"
		echo "    -KernelPath 'R:\\WSL\\kernels\\bzImage-ramshared-latest' -ExpectedVersion '$rel' -TimeoutSec $APPLY_TIMEOUT_SEC"
		exit "$E_INTEROP"
	fi
	echo "apply: WARNING — stopping all WSL distros, then booting custom kernel"
	echo "apply: KernelPath=$KERNEL_WIN ExpectedVersion=$rel TimeoutSec=$APPLY_TIMEOUT_SEC"
	if [[ ! -e /proc/sys/fs/binfmt_misc/WSLInterop ]]; then
		echo ':WSLInterop:M::MZ::/init:PF' | sudo tee /proc/sys/fs/binfmt_misc/register >/dev/null 2>&1 || true
	fi
	# This will kill this process when WSL shuts down — expected
	/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -ExecutionPolicy Bypass \
		-File "$ps1_win" \
		-KernelPath 'R:\WSL\kernels\bzImage-ramshared-latest' \
		-ExpectedVersion "$rel" \
		-TimeoutSec "$APPLY_TIMEOUT_SEC" \
		-CheckModules 'ublk_drv' || {
		echo "apply: launcher returned non-zero (see log); exit $E_APPLY"
		exit "$E_APPLY"
	}
	exit "$E_OK"
}

main() {
	local cmd="${1:-status}"
	shift || true
	case "$cmd" in
	status) cmd_status "$@" ;;
	enable) cmd_enable "$@" ;;
	arm) cmd_arm "$@" ;;
	disarm) cmd_disarm "$@" ;;
	apply) cmd_apply "$@" ;;
	-h | --help | help) usage; exit "$E_OK" ;;
	*)
		usage
		exit "$E_USAGE"
		;;
	esac
}

main "$@"
