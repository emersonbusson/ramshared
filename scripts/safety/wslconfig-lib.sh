# wslconfig-lib.sh — safe encode/validate/write for Windows user .wslconfig
# shellcheck shell=bash
#
# .wslconfig is parsed with escape rules (not a dumb ini). Single "\" begins an
# escape; "\w" is invalid. Platform rule: always emit forward-slash paths.
#
# Source from other scripts:
#   # shellcheck source=wslconfig-lib.sh
#   source "$ROOT/wslconfig-lib.sh"

# shellcheck disable=SC2034
WSLCONFIG_LIB_LOADED=1

# Defaults for this host class (override via env).
WSLCONFIG_MEMORY_BYTES="${WSLCONFIG_MEMORY_BYTES:-17179869184}"   # 16 GiB
WSLCONFIG_SWAP_BYTES="${WSLCONFIG_SWAP_BYTES:-4294967296}"       # 4 GiB
WSLCONFIG_SWAPFILE="${WSLCONFIG_SWAPFILE:-I:/wsl_swap/swap.vhdx}"
WSLCONFIG_KERNEL="${WSLCONFIG_KERNEL:-C:/wsl/kernel-ramshared}"
WSLCONFIG_KERNEL_MODULES="${WSLCONFIG_KERNEL_MODULES:-C:/wsl/modules-ramshared.vhdx}"

# --- path encode (Day-0: one format only) -----------------------------------

# Convert any Windows path to .wslconfig-safe form (forward slashes, no escapes).
# Examples:  C:\wsl\k  →  C:/wsl/k   |   C:\\wsl\\k  →  C:/wsl/k
wslconfig_encode_path() {
	local p="${1//$'\r'/}"
	p="${p#"${p%%[![:space:]]*}"}"
	p="${p%"${p##*[![:space:]]}"}"
	# strip surrounding quotes
	if [[ "$p" == \"*\" && "$p" == *\" ]]; then
		p="${p:1:${#p}-2}"
	fi
	p="${p//\\//}"
	while [[ "$p" == *//* ]]; do p="${p//\/\//\/}"; done
	printf '%s' "$p"
}

# True if a path *value* (right-hand side of key=) is unsafe for .wslconfig.
# Unsafe: any single-backslash that is not part of a doubled \\ pair.
# Heuristic used by WSL: backslash starts escape; letter after single \ fails.
wslconfig_path_is_unsafe() {
	local v="$1"
	# Has a backslash followed by a non-backslash non-empty char that is not
	# a known TOML/simple escape we allow as doubled only — fail on single \.
	# Match: odd backslash run before a path-ish char (letter, digit, .)
	[[ "$v" =~ (^|[^\\])\\[A-Za-z0-9._] ]] && return 0
	# trailing lone backslash
	[[ "$v" =~ [^\\]\\$ ]] && return 0
	[[ "$v" == '\' ]] && return 0
	return 1
}

# --- locate profile .wslconfig ----------------------------------------------

wslconfig_win_user() {
	local u=""
	if [[ -n "${WIN_USER:-}" ]]; then
		printf '%s' "$WIN_USER"
		return 0
	fi
	if [[ -e /proc/sys/fs/binfmt_misc/WSLInterop ]] || [[ -w /proc/sys/fs/binfmt_misc/register ]]; then
		u="$(timeout 5 /mnt/c/Windows/System32/cmd.exe /c "echo %USERNAME%" 2>/dev/null | tr -d '\0\r\n' || true)"
	fi
	if [[ -z "$u" || "$u" == *'%USERNAME%'* ]]; then
		local d base
		for d in /mnt/c/Users/*; do
			[[ -d "$d" ]] || continue
			base="$(basename "$d")"
			case "$base" in Public|Default|"Default User"|"All Users") continue ;; esac
			if [[ -f "$d/.wslconfig" || -d "$d/Desktop" ]]; then
				u="$base"
				break
			fi
		done
	fi
	[[ -n "$u" ]] || return 1
	printf '%s' "$u"
}

wslconfig_path() {
	if [[ -n "${WSL_CONFIG:-}" ]]; then
		printf '%s' "$WSL_CONFIG"
		return 0
	fi
	local u
	u="$(wslconfig_win_user)" || return 1
	printf '/mnt/c/Users/%s/.wslconfig' "$u"
}

# --- validate file ----------------------------------------------------------

# Prints issues to stdout; returns 0 if clean, 1 if errors.
wslconfig_validate_file() {
	local cfg="$1"
	local err=0
	local line n=0 key val
	if [[ ! -f "$cfg" ]]; then
		echo "MISSING $cfg"
		return 1
	fi
	while IFS= read -r line || [[ -n "$line" ]]; do
		n=$((n + 1))
		line="${line//$'\r'/}"
		# skip comments and blanks
		[[ -z "${line//[[:space:]]/}" ]] && continue
		[[ "$line" =~ ^[[:space:]]*# ]] && continue
		[[ "$line" =~ ^[[:space:]]*\[ ]] && continue
		if [[ "$line" =~ ^[[:space:]]*([A-Za-z][A-Za-z0-9_]*)[[:space:]]*=[[:space:]]*(.*)$ ]]; then
			key="${BASH_REMATCH[1]}"
			val="${BASH_REMATCH[2]}"
			val="${val%"${val##*[![:space:]]}"}"
			case "$key" in
			swapFile|kernel|kernelModules|guiApplications|localhostForwarding)
				if wslconfig_path_is_unsafe "$val"; then
					echo "L${n}: UNSAFE_ESCAPE key=${key} value=${val}"
					echo "     fix: use forward slashes e.g. C:/wsl/kernel-ramshared"
					err=1
				fi
				;;
			esac
		fi
	done <"$cfg"
	return "$err"
}

# --- render canonical body --------------------------------------------------

wslconfig_render_host() {
	local mem swap sf kern mods
	mem="$(wslconfig_encode_path "${WSLCONFIG_MEMORY_BYTES}")"
	# memory/swap are integers — encode_path is no-op for digits
	mem="${WSLCONFIG_MEMORY_BYTES}"
	swap="${WSLCONFIG_SWAP_BYTES}"
	sf="$(wslconfig_encode_path "${WSLCONFIG_SWAPFILE}")"
	kern="$(wslconfig_encode_path "${WSLCONFIG_KERNEL}")"
	mods="$(wslconfig_encode_path "${WSLCONFIG_KERNEL_MODULES}")"

	# Refuse to emit unsafe paths (defense in depth)
	local p
	for p in "$sf" "$kern" "$mods"; do
		if wslconfig_path_is_unsafe "$p"; then
			echo "wslconfig_render_host: internal error unsafe path: $p" >&2
			return 1
		fi
	done

	cat <<EOF
# Managed by scripts/safety/wslconfig-ctl.sh — do not hand-edit path backslashes.
# Paths use forward slashes only (WSL escape-safe). See scripts/safety/wslconfig.host.example.

[wsl2]
# 16 GiB WSL hard cap; host residual for Windows + Hyper-V (civm, win11-drill).
memory=${mem}
# 4 GiB pagefile on I: (product cascade adds zram/nbd separately).
swap=${swap}
swapFile=${sf}
kernel=${kern}
kernelModules=${mods}

[experimental]
autoMemoryReclaim=Gradual
sparseVhd=true
EOF
}

# Atomic write + post-validate. Backs up existing file once per call.
wslconfig_write_host() {
	local cfg="${1:-}"
	if [[ -z "$cfg" ]]; then
		cfg="$(wslconfig_path)" || {
			echo "wslconfig_write_host: cannot resolve profile .wslconfig" >&2
			return 1
		}
	fi
	local dir body tmp bak
	dir="$(dirname "$cfg")"
	mkdir -p "$dir" 2>/dev/null || true
	body="$(wslconfig_render_host)" || return 1
	tmp="${cfg}.tmp.$$"
	bak="${cfg}.ramshared.bak.$(date +%Y%m%d%H%M%S)"
	if [[ -f "$cfg" ]]; then
		cp -f "$cfg" "$bak" 2>/dev/null || true
	fi
	printf '%s\n' "$body" >"$tmp"
	# Validate temp before replace
	if ! wslconfig_validate_file "$tmp"; then
		echo "wslconfig_write_host: rendered body failed validation (not installing)" >&2
		rm -f "$tmp"
		return 1
	fi
	mv -f "$tmp" "$cfg"
	# Final validate live file
	wslconfig_validate_file "$cfg" || {
		echo "wslconfig_write_host: post-write validation failed" >&2
		return 1
	}
	echo "WROTE $cfg"
	echo "BACKUP ${bak:-none}"
	return 0
}
