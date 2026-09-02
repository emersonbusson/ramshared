#!/usr/bin/env bash
# swap-sanitize.sh — diagnose and fail-closed recovery for RamShared-owned swap.
# Foreign nbd, zram, and ublk swaps remain evidence, never cleanup candidates.
set -euo pipefail

for cmd in awk cat grep ls mktemp pgrep python3 rm swapoff; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "swap-sanitize: $cmd is required but not found" >&2
    exit 69
  fi
done

FIX=0
[[ ${1:-} == --fix ]] && FIX=1

test_root=${RAMSHARED_SWAP_SANITIZE_TEST_ROOT:-}
if [[ -n $test_root ]]; then
  [[ $test_root == /* && $test_root != / && -d $test_root && ! -L $test_root ]] || {
    echo 'swap-sanitize test root is invalid' >&2; exit 64;
  }
  proc_swaps="$test_root/proc/swaps"
  run_root="$test_root/run/ramshared"
  PATH="$test_root/bin:$PATH"
else
  proc_swaps=/proc/swaps
  run_root=/run/ramshared
fi

if [[ ! -f "$proc_swaps" ]]; then
  echo "swap-sanitize: $proc_swaps not found" >&2
  exit 69
fi

ownership_manifest="$run_root/ramshared-swap-ownership.json"

echo '=== /proc/swaps ==='
cat "$proc_swaps" || true
echo
echo '=== ramsharedd ==='
pgrep -a -x ramsharedd || echo '(none)'
echo
echo '=== /run/ramshared ==='
ls -la "$run_root" 2>/dev/null || echo '(missing)'

GHOST=0
while read -r line; do
  case "$line" in
    Filename*|*'Type'*) continue ;;
    *ublk*|*nbd*|*zram*)
      if grep -qE '\(deleted\)|\\040\(deleted\)' <<<"$line"; then
        echo "GHOST: $line"
        GHOST=1
      fi
      ;;
  esac
done <"$proc_swaps"

if [[ $GHOST -eq 1 ]]; then
  echo 'ACTION: ghost swap with device deleted; preserve evidence and use attended recovery.'
  exit 74
fi

if [[ $FIX -eq 1 ]]; then
  echo '=== --fix: sealed RamShared-owned live paths only ==='
  [[ -f $ownership_manifest && ! -L $ownership_manifest ]] || {
    echo 'swapoff_refused_missing_ownership_evidence' >&2; exit 78;
  }
  owned_devices_file=$(mktemp "${run_root}/.ramshared-swap-ownership.XXXXXX") || {
    echo 'swapoff_refused_ownership_evidence_staging_failed' >&2; exit 74;
  }
  if ! python3 - "$ownership_manifest" "$run_root" >"$owned_devices_file" <<'PY'
import hashlib
import json
import os
import re
import stat
import sys

manifest_path, run_root = sys.argv[1:]
try:
    with open(manifest_path, encoding="utf-8") as stream:
        manifest = json.load(stream)
except (OSError, UnicodeError, json.JSONDecodeError) as error:
    raise SystemExit(f"swapoff_refused_invalid_ownership_evidence:{error}")
if set(manifest) != {"schema_version", "devices"} or manifest["schema_version"] != 1 or not isinstance(manifest["devices"], list):
    raise SystemExit("swapoff_refused_invalid_ownership_evidence")
seen = set()
for device in manifest["devices"]:
    if set(device) != {"path", "kind", "runtime_file", "runtime_value_sha256"}:
        raise SystemExit("swapoff_refused_invalid_ownership_evidence")
    path, kind, runtime_file, runtime_sha = (device[k] for k in ("path", "kind", "runtime_file", "runtime_value_sha256"))
    if not isinstance(path, str) or not re.fullmatch(r"/dev/(?:nbd|ublk|zram)[0-9]+", path):
        raise SystemExit("swapoff_refused_invalid_ownership_evidence")
    if kind not in {"ramshared_nbd", "ramshared_ublk", "ramshared_zram"}:
        raise SystemExit("swapoff_refused_invalid_ownership_evidence")
    if not isinstance(runtime_file, str) or not re.fullmatch(r"[a-z0-9][a-z0-9._-]{0,63}", runtime_file):
        raise SystemExit("swapoff_refused_invalid_ownership_evidence")
    if not isinstance(runtime_sha, str) or not re.fullmatch(r"[0-9a-f]{64}", runtime_sha):
        raise SystemExit("swapoff_refused_invalid_ownership_evidence")
    runtime_path = os.path.join(run_root, runtime_file)
    try:
        mode = os.lstat(runtime_path).st_mode
        if not stat.S_ISREG(mode):
            raise OSError("runtime file not regular")
        with open(runtime_path, "rb") as stream:
            value = stream.read()
    except OSError:
        raise SystemExit("swapoff_refused_missing_runtime_evidence")
    if hashlib.sha256(value).hexdigest() != runtime_sha or value.decode("utf-8", "strict").strip() != path:
        raise SystemExit("swapoff_refused_runtime_evidence_mismatch")
    if path in seen:
        raise SystemExit("swapoff_refused_duplicate_owned_device")
    seen.add(path)
    print(path)
PY
  then
    rm -f -- "$owned_devices_file"
    echo 'swapoff_refused_invalid_ownership_evidence' >&2
    exit 65
  fi
  mapfile -t owned_devices <"$owned_devices_file"
  rm -f -- "$owned_devices_file"
  ((${#owned_devices[@]} > 0)) || { echo 'swapoff_refused_no_owned_devices' >&2; exit 65; }

  swap_names=$(awk 'NR > 1 { print $1 }' "$proc_swaps")
  swapped=0
  for device in "${owned_devices[@]}"; do
    if [[ ! -b "$device" ]]; then
      echo "swapoff_refused_not_block_device=$device" >&2
      exit 64
    fi

    if ! grep -Fqx -- "$device" <<<"$swap_names"; then
      echo "swapoff_skipped_not_live=$device"
      continue
    fi
    echo "swapoff $device"
    if ! swapoff "$device"; then
      echo "swapoff_failed=$device" >&2
      exit 74
    fi
    swapped=$((swapped + 1))
  done
  echo "CLEANUP=sealed_owned_swapoff_complete count=$swapped"
fi

echo 'OK diagnose complete (exit 0 = no ghost; --fix reports only proven owned cleanup)'
