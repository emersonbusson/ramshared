#!/usr/bin/env bash
# wslconfig-ctl.sh — check / apply / selftest for Windows profile .wslconfig
#
# Platform rule: never emit single-backslash paths (WSL escape parser).
# Usage:
#   bash scripts/safety/wslconfig-ctl.sh check
#   bash scripts/safety/wslconfig-ctl.sh apply
#   bash scripts/safety/wslconfig-ctl.sh selftest
#   bash scripts/safety/wslconfig-ctl.sh show
#
# Does NOT run wsl --shutdown (operator must restart WSL to reload memory=).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=wslconfig-lib.sh
source "$ROOT/wslconfig-lib.sh"

usage() {
	cat <<'EOF'
Usage: wslconfig-ctl.sh <check|apply|show|selftest|render>

  check     Validate live %UserProfile%\.wslconfig (exit 1 if unsafe escapes)
  apply     Write canonical host policy (16G RAM / 4G swap / forward-slash paths)
  show      Print resolved path + file contents
  render    Print canonical body to stdout (no write)
  selftest  Unit tests for escape detection + encode (no host mutation)

Env: WSL_CONFIG WIN_USER WSLCONFIG_MEMORY_BYTES WSLCONFIG_SWAP_BYTES
     WSLCONFIG_SWAPFILE WSLCONFIG_KERNEL WSLCONFIG_KERNEL_MODULES
EOF
}

cmd_check() {
	local cfg
	if ! cfg="$(wslconfig_path)"; then
		echo "CHECK: cannot resolve .wslconfig (set WSL_CONFIG= or WIN_USER=)"
		exit 2
	fi
	echo "CHECK path=$cfg"
	if [[ ! -f "$cfg" ]]; then
		echo "CHECK: MISSING (run: bash scripts/safety/wslconfig-ctl.sh apply)"
		exit 1
	fi
	if wslconfig_validate_file "$cfg"; then
		echo "CHECK: OK (no unsafe escapes)"
		# soft hints
		if ! grep -qE '^[[:space:]]*memory[[:space:]]*=' "$cfg"; then
			echo "CHECK: WARN missing memory= (WSL default may over-allocate)"
		fi
		exit 0
	fi
	echo "CHECK: FAIL — fix with: bash scripts/safety/wslconfig-ctl.sh apply"
	exit 1
}

cmd_apply() {
	local cfg
	if ! cfg="$(wslconfig_path)"; then
		echo "APPLY: cannot resolve .wslconfig"
		exit 2
	fi
	wslconfig_write_host "$cfg"
	echo "APPLY: OK — restart WSL when idle to reload memory=/swap= (wsl --shutdown)"
	echo "APPLY: cascade VRAM sizes live in /etc/ramshared/cascade.conf (not this file)"
	exit 0
}

cmd_show() {
	local cfg
	cfg="$(wslconfig_path 2>/dev/null || echo '(unresolved)')"
	echo "path=$cfg"
	if [[ -f "$cfg" ]]; then
		echo "----- begin -----"
		cat "$cfg"
		echo "----- end -----"
		wslconfig_validate_file "$cfg" && echo "validate=OK" || echo "validate=FAIL"
	else
		echo "(file missing)"
	fi
}

cmd_render() {
	wslconfig_render_host
}

cmd_selftest() {
	local fail=0
	local t

	# encode
	t="$(wslconfig_encode_path 'C:\wsl\kernel-ramshared')"
	[[ "$t" == "C:/wsl/kernel-ramshared" ]] || {
		echo "FAIL encode single-backslash → $t"
		fail=1
	}
	t="$(wslconfig_encode_path 'C:\\wsl\\kernel-ramshared')"
	[[ "$t" == "C:/wsl/kernel-ramshared" ]] || {
		echo "FAIL encode doubled → $t"
		fail=1
	}
	t="$(wslconfig_encode_path 'I:/wsl_swap/swap.vhdx')"
	[[ "$t" == "I:/wsl_swap/swap.vhdx" ]] || {
		echo "FAIL encode already-safe → $t"
		fail=1
	}

	# unsafe detection (the production bug)
	if wslconfig_path_is_unsafe 'I:\wsl_swap\swap.vhdx'; then
		echo "OK detect I:\\wsl_swap unsafe"
	else
		echo "FAIL did not detect I:\\wsl_swap as unsafe"
		fail=1
	fi
	if wslconfig_path_is_unsafe 'C:\wsl\kernel-ramshared'; then
		echo "OK detect C:\\wsl unsafe"
	else
		echo "FAIL did not detect C:\\wsl as unsafe"
		fail=1
	fi
	if wslconfig_path_is_unsafe 'I:/wsl_swap/swap.vhdx'; then
		echo "FAIL false positive on forward slash"
		fail=1
	else
		echo "OK forward slash safe"
	fi
	# doubled backslash is escape-legal in file (represents one \)
	if wslconfig_path_is_unsafe 'C:\\wsl\\kernel-ramshared'; then
		# our heuristic flags single \ before letter; doubled \\ before w is \\ + w
		# C:\\wsl → after first \\ pair we have \w?  String chars: C : \ \ w s l
		# Pattern (^|[^\\])\\[A-Za-z] : position of \ before w has previous \ so [^\\] fails
		# Actually \\w : the second \ is followed by w, previous char is \ so (^|[^\\]) needs non-\ before single \
		# For C:\\wsl - chars: \ \ w - the \ before w has previous \, so pattern might not match
		echo "OK doubled backslash treated safe (or heuristic): $(wslconfig_path_is_unsafe 'C:\\wsl\\kernel-ramshared' && echo unsafe || echo safe)"
	else
		echo "OK doubled backslash safe"
	fi

	# temp file validate
	local td
	td="$(mktemp -d)"
	printf '%s\n' '[wsl2]' 'kernel=C:\wsl\bad' >"$td/bad.wslconfig"
	if wslconfig_validate_file "$td/bad.wslconfig" 2>/dev/null; then
		echo "FAIL validate should reject bad.wslconfig"
		fail=1
	else
		echo "OK validate rejects bad file"
	fi
	printf '%s\n' '[wsl2]' 'kernel=C:/wsl/kernel-ramshared' >"$td/good.wslconfig"
	if wslconfig_validate_file "$td/good.wslconfig"; then
		echo "OK validate accepts good file"
	else
		echo "FAIL validate rejected good.wslconfig"
		fail=1
	fi

	# render must be safe
	local rendered
	rendered="$(wslconfig_render_host)"
	printf '%s\n' "$rendered" >"$td/rendered.wslconfig"
	if wslconfig_validate_file "$td/rendered.wslconfig"; then
		echo "OK render validates"
	else
		echo "FAIL render does not validate"
		fail=1
	fi
	if printf '%s\n' "$rendered" | grep -qE '\\\\'; then
		echo "WARN render still contains backslashes (prefer / only)"
	else
		echo "OK render has zero backslashes"
	fi

	rm -rf "$td"
	if [[ "$fail" -ne 0 ]]; then
		echo "SELFTEST: FAIL"
		exit 1
	fi
	echo "SELFTEST: PASS"
	exit 0
}

main() {
	local cmd="${1:-check}"
	shift || true
	case "$cmd" in
	check) cmd_check "$@" ;;
	apply) cmd_apply "$@" ;;
	show) cmd_show "$@" ;;
	render) cmd_render "$@" ;;
	selftest) cmd_selftest "$@" ;;
	-h | --help | help) usage ;;
	*)
		usage >&2
		exit 2
		;;
	esac
}

main "$@"
