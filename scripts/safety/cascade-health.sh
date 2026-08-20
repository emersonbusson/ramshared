#!/usr/bin/env bash
# Compatibility entry point for the typed RamShared monitor.
#
# This wrapper only translates the historical sampler flags. All observation,
# timeout, schema, rotation, and atomic-heartbeat logic lives in the Rust CLI.
set -euo pipefail

MODE=once
INTERVAL_S=${INTERVAL_S:-2}
OUT=""
HEARTBEAT=""

usage() {
  printf '%s\n' \
    'usage: cascade-health.sh [--once|--loop] [--interval SECONDS]' \
    '                         [--out PATH] [--heartbeat PATH]'
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --once) MODE=once; shift ;;
    --loop) MODE=loop; shift ;;
    --interval) INTERVAL_S=${2:-}; shift 2 ;;
    --out) OUT=${2:-}; shift 2 ;;
    --heartbeat) HEARTBEAT=${2:-}; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
done

case "$INTERVAL_S" in
  ''|*[!0-9]*) printf 'interval must be an integer number of seconds\n' >&2; exit 2 ;;
esac
if [ "$INTERVAL_S" -lt 1 ] || [ "$INTERVAL_S" -gt 60 ]; then
  printf 'interval must be between 1 and 60 seconds\n' >&2
  exit 2
fi

if [ -n "${RAMSHARED_BIN:-}" ]; then
  BIN=$RAMSHARED_BIN
elif [ -x ./target/release/ramshared ]; then
  BIN=./target/release/ramshared
elif command -v ramshared >/dev/null 2>&1; then
  BIN=$(command -v ramshared)
else
  printf 'ramshared binary not found\n' >&2
  exit 1
fi
if [ ! -x "$BIN" ]; then
  printf 'ramshared binary is not executable: %s\n' "$BIN" >&2
  exit 1
fi

ARGS=(monitor --jsonl)
if [ "$MODE" = once ]; then
  ARGS+=(--once)
fi
ARGS+=(--interval-ms "$((INTERVAL_S * 1000))")
if [ -n "$OUT" ]; then
  ARGS+=(--output "$OUT")
fi
if [ -n "$HEARTBEAT" ]; then
  ARGS+=(--heartbeat "$HEARTBEAT")
fi

exec "$BIN" "${ARGS[@]}"
