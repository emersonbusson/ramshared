#!/usr/bin/env bash
# RamShared — build the day-1 user binaries (CLI + daemon).
# Usage: ./scripts/quickstart.sh
# Then:  sudo ./target/release/ramshared check && sudo ./target/release/ramshared up ...
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> RamShared quickstart"
echo "    Repo: $ROOT"
echo

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found. Install Rust: https://rustup.rs/" >&2
  exit 1
fi

if ! command -v rustc >/dev/null 2>&1; then
  echo "error: rustc not found. Install Rust: https://rustup.rs/" >&2
  exit 1
fi

echo "==> rustc $(rustc --version)"
echo "==> cargo $(cargo --version)"
echo

echo "==> Building ramshared (CLI) + ramsharedd (daemon) [release]"
cargo build -p ramshared-cli -p ramshared-wsl2d --release

CLI="$ROOT/target/release/ramshared"
DAEMON="$ROOT/target/release/ramsharedd"

if [[ ! -x "$CLI" ]]; then
  echo "error: missing $CLI" >&2
  exit 1
fi
if [[ ! -x "$DAEMON" ]]; then
  echo "error: missing $DAEMON" >&2
  exit 1
fi

echo
echo "==> Build OK"
echo
echo "Next (needs sudo + NVIDIA/CUDA on Linux or WSL2):"
echo
echo "  # Preflight"
echo "  sudo $CLI check"
echo "  # If blocked: sudo $CLI doctor"
echo
echo "  # Start cascade (1 GiB zram + 1 GiB VRAM)"
echo "  sudo $CLI up --vram 1024 --zram 1024"
echo
echo "  # Success = zram + VRAM tier + disk swap visible"
echo "  swapon --show"
echo
echo "  # Tear down"
echo "  sudo $CLI down"
echo
echo "FAQ: docs/FAQ.md"
echo "Diagram: docs/marketing/cascade-diagram.png"
