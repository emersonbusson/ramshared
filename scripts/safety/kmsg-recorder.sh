#!/usr/bin/env bash
# kmsg-recorder.sh — Espelha o console do kernel (/dev/kmsg via `dmesg --follow`) pra
# um arquivo em armazenamento DURAVEL do Windows, em tempo real. E' a caixa-preta mais
# proxima do instante do travamento: quando um `kernel BUG` dispara, o call trace inteiro
# ja esta escrito no lado Windows (host), mesmo que o journald do guest congele com lock
# preso antes de persistir (foi o que aconteceu no #1 — journald so pegou a 1a linha).
#
# Roda como servico systemd (root). Nao toca GPU/ublk/swap.
set -uo pipefail

FORENSICS_DIR="${RAMSHARED_FORENSICS_DIR:-/mnt/c/wsl-forensics}"
mkdir -p "$FORENSICS_DIR" 2>/dev/null || FORENSICS_DIR="/var/log"  # fallback guest-local
LOG="$FORENSICS_DIR/kernel-console.log"
PREV="$FORENSICS_DIR/kernel-console.prev.log"

# Preserva o log do boot anterior (pode conter o crash) antes de truncar pro boot atual.
[ -f "$LOG" ] && cp -f "$LOG" "$PREV" 2>/dev/null

{
  echo "===== kmsg-recorder boot: $(date '+%Y-%m-%d %H:%M:%S %z') | kernel $(uname -r) ====="
} > "$LOG" 2>/dev/null

# --follow: imprime o buffer atual e segue em tempo real. -T timestamps legiveis.
# Escreve direto no arquivo host-side (flush por linha via stdbuf pra durabilidade).
exec stdbuf -oL -eL dmesg --follow --ctime >> "$LOG" 2>&1
