#!/usr/bin/env bash
# cascade-down.sh — ordered teardown (swapoff first). Used as systemd ExecStop.
set -euo pipefail

REPO="${RAMSHARED_REPO:-$(cd "$(dirname "$0")/../.." && pwd)}"
BIN_DIR="${RAMSHARED_BIN_DIR:-$REPO/target/release}"
CLI="${RAMSHARED_CLI:-$BIN_DIR/ramshared}"

exec "$CLI" down
