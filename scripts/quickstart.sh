#!/usr/bin/env bash
# Build RamShared for first-time users (CLI + background service).
# Usage: ./scripts/quickstart.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo ""
echo "  RamShared — quick build"
echo "  ------------------------"
echo "  What this does: compiles the two programs you need"
echo "  (ramshared = commands, ramsharedd = GPU service)."
echo ""

if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
  echo "  Rust is not installed (need cargo + rustc)."
  echo "  Install from: https://rustup.rs/"
  echo ""
  exit 1
fi

echo "  Using: $(rustc --version)"
echo ""
echo "  Building (release)… this can take a few minutes the first time."
echo ""

cargo build -p ramshared-cli -p ramshared-wsl2d --release

CLI="$ROOT/target/release/ramshared"
DAEMON="$ROOT/target/release/ramsharedd"

if [[ ! -x "$CLI" || ! -x "$DAEMON" ]]; then
  echo "  Build finished but binaries are missing. Please open an issue."
  exit 1
fi

echo ""
echo "  Build OK."
echo ""
echo "  Next steps (needs Linux/WSL2 + NVIDIA GPU + sudo):"
echo ""
echo "    1) Is this machine ready?"
echo "         sudo $CLI check"
echo ""
echo "       If it says blocked:"
echo "         sudo $CLI doctor"
echo ""
echo "    2) Turn the memory cushion on (1 GB + 1 GB):"
echo "         sudo $CLI up --vram 1024 --zram 1024"
echo ""
echo "    3) Confirm it worked (you want ~3 swap lines):"
echo "         swapon --show"
echo ""
echo "    4) Turn it off later:"
echo "         sudo $CLI down"
echo ""
echo "  Simple FAQ:  docs/FAQ.md"
echo "  Picture:     docs/marketing/cascade-diagram.png"
echo ""
