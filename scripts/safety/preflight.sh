#!/usr/bin/env bash
# preflight.sh — Security gate (fail-safe) before bringing up the daemon
# VRAM/ublk on live WSL2 host. REFUSES (exit != 0) instead of allowing a dangerous start
# to freeze the machine. Runs the baseline snapshot on success.
#
# Motivated by the 2026-07-03 incident: a `--backend vram` run with a binary missing the
# mlockall fix froze the host (kernel BUG). This gate guarantees that only a binary WITH the
# fix, with a healthy GPU, and without orphaned devices, gets to run.
#
# Usage: preflight.sh [binary_path]
#   exit 0 = safe to proceed (snapshot written, collector armed)
#   exit != 0 = REFUSED (reason in stderr) — DO NOT start daemon
# Only READS state; does not touch GPU/ublk/swap. Only effect is writing the snapshot.
set -euo pipefail

REPO="${RAMSHARED_REPO:-$(cd "$(dirname "$0")/../.." && pwd)}"
BIN="${1:-$REPO/target/debug/ramsharedd}"
FIX_MARKER='MCL_CURRENT-only no caminho ublk+vram'   # string do fix anti-dxgkrnl-BUG (#1)
MIN_VRAM_FREE_MIB="${RAMSHARED_MIN_VRAM_FREE_MIB:-256}"

# nvidia-smi in WSL2 is located in /usr/lib/wsl/lib, which is NOT in systemd's minimal PATH.
# Resolves the full path so the gate works both in the shell and via ExecStartPre.
NVSMI="$(command -v nvidia-smi 2>/dev/null || true)"
[ -x "$NVSMI" ] || NVSMI="/usr/lib/wsl/lib/nvidia-smi"

fail() { echo "PREFLIGHT: REFUSED — $1" >&2; exit "${2:-1}"; }

echo "== RamShared preflight (fail-safe) =="

# 1. Binary exists and HAS the mlockall fix (otherwise = guaranteed crash in #1).
# Materializes `strings` in a var and uses here-string in grep -q: avoids the
# pipefail+grep-q+SIGPIPE gotcha (the pipe `strings | grep -q` returned SIGPIPE from strings,
# not grep's success, and refused a good binary).
[ -x "$BIN" ] || fail "[FAIL] Binary not found/executable: $BIN (run 'cargo build -p ramshared-wsl2d --bin ramsharedd')" 69
BIN_STRINGS="$(strings "$BIN" 2>/dev/null)"
if ! grep -qF "$FIX_MARKER" <<<"$BIN_STRINGS"; then
  fail "[FAIL] Binary WITHOUT mlockall fix ($BIN). Recompile with fix (arm_future_lock) before running VRAM+ublk. Running as is will FREEZE the host." 78
fi
echo "  [PASS] Binary has mlockall fix"

# 2. Healthy GPU: nvidia-smi responds and there is enough free VRAM.
SMI_OUT="$("$NVSMI" --query-gpu=memory.free --format=csv,noheader,nounits 2>/dev/null || true)"
[ -n "$SMI_OUT" ] || fail "[FAIL] nvidia-smi unresponsive — GPU/driver in bad state; DO NOT start VRAM now" 69
VRAM_FREE="$(echo "$SMI_OUT" | head -1 | tr -dc '0-9')"
[ -n "$VRAM_FREE" ] || fail "[FAIL] Could not read free VRAM from nvidia-smi" 69
if [ "$VRAM_FREE" -lt "$MIN_VRAM_FREE_MIB" ]; then
  fail "[FAIL] Free VRAM ${VRAM_FREE} MiB < minimum ${MIN_VRAM_FREE_MIB} MiB — unsafe margin" 74
fi
echo "  [PASS] GPU responsive, free VRAM=${VRAM_FREE} MiB (>= ${MIN_VRAM_FREE_MIB})"

# 3. No orphaned /dev/ublkb* (leftover from a previous crash -> collision/dirty state).
if ls /dev/ublkb* >/dev/null 2>&1; then
  fail "[FAIL] Orphaned /dev/ublkb* device exists (from previous run): $(ls /dev/ublkb* 2>/dev/null | tr '\n' ' '). Clean up first." 74
fi
echo "  [PASS] No orphaned ublk device"

# 4. ublk module loaded (/dev/ublk-control present).
[ -e /dev/ublk-control ] || fail "[FAIL] /dev/ublk-control missing — run 'sudo modprobe ublk_drv' first" 69
echo "  [PASS] ublk_drv loaded (/dev/ublk-control present)"

# 5. All ok -> baseline snapshot + arm the collector.
"$REPO/scripts/safety/preflight-snapshot.sh" "${*:-ramsharedd (via preflight)}" >/dev/null 2>&1 \
  && echo "  [PASS] Baseline snapshot written + collector armed" \
  || echo "  [SKIP] Snapshot failed (non-blocking), but security checks passed"

echo "PREFLIGHT: OK — safe to proceed."
exit 0
