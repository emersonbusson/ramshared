#!/usr/bin/env bash
set -euo pipefail
# preflight.sh — Security gate (fail-safe) before bringing up the daemon
# VRAM/ublk on a live WSL2 host. REJECTS (exit != 0) instead of letting a dangerous start
# freeze the machine. Runs the baseline snapshot on success.
#
# Motivated by the 2026-07-03 incident: a `--backend vram` run with a binary missing the
# mlockall fix froze the host (kernel BUG). This gate guarantees that only a binary WITH the
# fix, with a healthy GPU, and without orphaned devices, gets to run.
#
# Usage: preflight.sh [binary_path]
#   exit 0 = safe to proceed (snapshot written, collector armed)
#   exit != 0 = REJECTED (reason on stderr) — DO NOT start the daemon
# Only READS state; does not touch GPU/ublk/swap. The only effect is writing the snapshot.

readonly EX_USAGE=64
readonly EX_UNAVAILABLE=69
readonly EX_IOERR=74
readonly EX_CONFIG=78

REPO="${RAMSHARED_REPO:-$(cd "$(dirname "$0")/../.." && pwd)}"
BIN="${1:-$REPO/target/debug/ramsharedd}"
FIX_MARKER='MCL_CURRENT-only no caminho ublk+vram'   # string of the anti-dxgkrnl-BUG fix (#1)
MIN_VRAM_FREE_MIB="${RAMSHARED_MIN_VRAM_FREE_MIB:-256}"

# nvidia-smi in WSL2 is located in /usr/lib/wsl/lib, which is NOT in systemd's minimal PATH.
# Resolves the full path so the gate works both in the shell and via ExecStartPre.
NVSMI="$(command -v nvidia-smi 2>/dev/null || true)"
[ -x "$NVSMI" ] || NVSMI="/usr/lib/wsl/lib/nvidia-smi"

fail() {
  local code="${2:-1}"
  echo "PREFLIGHT: [FAIL] — $1" >&2
  exit "$code"
}

pass() {
  echo "PREFLIGHT: [PASS] — $1"
}

skip() {
  echo "PREFLIGHT: [SKIP] — $1"
}

echo "== RamShared preflight (fail-safe) =="

# 1. Binary exists and HAS the mlockall fix (otherwise = guaranteed freeze in #1).
# Materializes `strings` into a var and uses here-string in grep -q: avoids the gotcha
# pipefail+grep-q+SIGPIPE (the pipe `strings | grep -q` returned the SIGPIPE of strings,
# not the success of grep, and rejected the good binary).
[ -x "$BIN" ] || fail "binary not found/executable: $BIN (run 'cargo build -p ramshared-wsl2d --bin ramsharedd')" "$EX_UNAVAILABLE"
BIN_STRINGS="$(strings "$BIN" 2>/dev/null || true)"
if ! grep -qF "$FIX_MARKER" <<<"$BIN_STRINGS"; then
  fail "binary WITHOUT the mlockall fix ($BIN). Recompile with the fix (arm_future_lock) before running VRAM+ublk. Running like this FREEZES the host." "$EX_CONFIG"
fi
pass "binary has the mlockall fix"

# 2. Healthy GPU: nvidia-smi responds and there is enough free VRAM.
SMI_OUT="$("$NVSMI" --query-gpu=memory.free --format=csv,noheader,nounits 2>/dev/null || true)"
[ -n "$SMI_OUT" ] || fail "nvidia-smi did not respond — GPU/driver in bad state; DO NOT start VRAM now" "$EX_UNAVAILABLE"
VRAM_FREE="$(echo "$SMI_OUT" | head -1 | tr -dc '0-9' || true)"
[ -n "$VRAM_FREE" ] || fail "could not read free VRAM from nvidia-smi" "$EX_UNAVAILABLE"
if [ "$VRAM_FREE" -lt "$MIN_VRAM_FREE_MIB" ]; then
  fail "free VRAM ${VRAM_FREE} MiB < minimum ${MIN_VRAM_FREE_MIB} MiB — no safe margin" "$EX_CONFIG"
fi
pass "GPU responds, free VRAM=${VRAM_FREE} MiB (>= ${MIN_VRAM_FREE_MIB})"

# 3. No orphaned /dev/ublkb* (leftover from a previous crash -> collision/dirty state).
shopt -s nullglob
UBLK_DEVS=(/dev/ublkb*)
shopt -u nullglob
if [ "${#UBLK_DEVS[@]}" -gt 0 ]; then
  fail "orphaned /dev/ublkb* exists (leftover from previous execution): ${UBLK_DEVS[*]}. Clean up first (the postmortem collector should have already run)." "$EX_IOERR"
fi
pass "no orphaned ublk device"

# 4. ublk module loaded (/dev/ublk-control present).
[ -e /dev/ublk-control ] || fail "/dev/ublk-control missing — 'sudo modprobe ublk_drv' first" "$EX_UNAVAILABLE"
pass "ublk_drv loaded (/dev/ublk-control present)"

# 5. Everything ok -> baseline snapshot + arm collector.
"$REPO/scripts/safety/preflight-snapshot.sh" "${*:-ramsharedd (via preflight)}" >/dev/null 2>&1 \
  && pass "baseline snapshot written + collector armed" \
  || skip "snapshot failed (non-blocking), but security checks passed"

echo "PREFLIGHT: [PASS] — safe to proceed."
exit 0
