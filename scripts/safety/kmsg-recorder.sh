#!/usr/bin/env bash
# kmsg-recorder.sh — Mirrors the kernel console (/dev/kmsg via `dmesg --follow`) to
# a file in DURABLE Windows storage in real time. It is the closest black-box recorder
# to the crash moment: when a `kernel BUG` triggers, the entire call trace is already
# written on the Windows side (host), even if the guest's journald freezes with a lock
# held before persisting (as happened in #1 — journald only captured the 1st line).
#
# Runs as a systemd service (root). Does not touch GPU/ublk/swap.
set -euo pipefail

if ! command -v dmesg >/dev/null 2>&1; then
  echo "Error: dmesg command not found." >&2
  exit 69 # EX_UNAVAILABLE
fi

if ! command -v stdbuf >/dev/null 2>&1; then
  echo "Error: stdbuf command not found." >&2
  exit 69 # EX_UNAVAILABLE
fi

if [ ! -r /dev/kmsg ]; then
  echo "Error: /dev/kmsg is not readable. Requires root privileges." >&2
  exit 77 # EX_NOPERM
fi

FORENSICS_DIR="${RAMSHARED_FORENSICS_DIR:-/mnt/c/wsl-forensics}"
mkdir -p "$FORENSICS_DIR" 2>/dev/null || FORENSICS_DIR="/var/log"  # fallback guest-local

if [ ! -d "$FORENSICS_DIR" ] || [ ! -w "$FORENSICS_DIR" ]; then
  echo "Error: Directory $FORENSICS_DIR is not writable or is not a directory." >&2
  exit 73 # EX_CANTCREAT
fi

LOG="$FORENSICS_DIR/kernel-console.log"
PREV="$FORENSICS_DIR/kernel-console.prev.log"

# Preserva o log do boot anterior (pode conter o crash) antes de truncar pro boot atual.
if [ -f "$LOG" ]; then
  cp -f "$LOG" "$PREV" 2>/dev/null || true
fi

{
  echo "===== kmsg-recorder boot: $(date '+%Y-%m-%d %H:%M:%S %z') | kernel $(uname -r) ====="
} > "$LOG"

# --follow: imprime o buffer atual e segue em tempo real. -T timestamps legiveis.
# Escreve direto no arquivo host-side (flush por linha via stdbuf pra durabilidade).
exec stdbuf -oL -eL dmesg --follow --ctime >> "$LOG" 2>&1
