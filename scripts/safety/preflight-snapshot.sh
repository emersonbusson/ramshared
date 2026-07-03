#!/usr/bin/env bash
# preflight-snapshot.sh — Baseline "estado bom conhecido" ANTES de um start arriscado
# do daemon (VRAM/ublk no host vivo). Grava em armazenamento duravel do Windows pra que,
# se a maquina travar, dê pra diff contra o ultimo estado bom — e ARMA o coletor
# postmortem (cria o marcador .armed) pra que o proximo boot saiba que fizemos algo
# arriscado e colete forense mesmo que nao haja kernel BUG capturado (o caso do
# travamento #2, que quase nao deixou rastro).
#
# Uso: preflight-snapshot.sh ["cmdline exata que vai rodar"]
# So LE estado + escreve arquivo. Nao toca GPU/ublk/swap. Seguro.
set -uo pipefail
NVSMI="$(command -v nvidia-smi 2>/dev/null || true)"; [ -x "$NVSMI" ] || NVSMI="/usr/lib/wsl/lib/nvidia-smi"

FORENSICS_DIR="${RAMSHARED_FORENSICS_DIR:-/mnt/c/wsl-forensics}"
REPO="${RAMSHARED_REPO:-/home/emdev/codespace/ramshared}"
CMDLINE_ABOUT_TO_RUN="${1:-<nao informado>}"

mkdir -p "$FORENSICS_DIR" 2>/dev/null || { FORENSICS_DIR="$HOME/wsl-forensics"; mkdir -p "$FORENSICS_DIR"; }

TS="$(date +%Y%m%d-%H%M%S)"
SNAP="$FORENSICS_DIR/snapshot-${TS}.md"

{
  echo "# RamShared preflight snapshot — $(date '+%Y-%m-%d %H:%M:%S %z')"
  echo
  echo "**Cmdline prestes a rodar:** \`${CMDLINE_ABOUT_TO_RUN}\`"
  echo
  echo "## Kernel / plataforma"
  echo '```'
  echo "uname: $(uname -r)"
  echo "cmdline: $(cat /proc/cmdline 2>/dev/null)"
  echo "uptime: $(uptime -p 2>/dev/null) (desde $(uptime -s 2>/dev/null))"
  echo '```'
  echo "## Git (o binario testado deve casar com este commit)"
  echo '```'
  git -C "$REPO" log --oneline -1 2>/dev/null || echo "(sem git)"
  echo "dirty: $(git -C "$REPO" status --porcelain 2>/dev/null | wc -l) arquivo(s) modificado(s)"
  echo '```'
  echo "## GPU (nvidia-smi)"
  echo '```'
  "$NVSMI" --query-gpu=name,memory.used,memory.free,memory.total --format=csv 2>&1 || echo "(nvidia-smi indisponivel)"
  echo '```'
  echo "## Memoria / swap"
  echo '```'
  free -h 2>&1
  echo "---"; cat /proc/swaps 2>&1
  echo "---"; grep -E "^(MemFree|MemAvailable|SwapFree|SwapTotal):" /proc/meminfo 2>&1
  echo '```'
  echo "## ublk / device"
  echo '```'
  ls -la /dev/ublk* 2>&1 || echo "(sem /dev/ublk*)"
  echo '```'
  echo "## .wslconfig (qual kernel/limites o WSL2 vai usar)"
  echo '```'
  cat /mnt/c/Users/*/.wslconfig 2>/dev/null || echo "(nao encontrado)"
  echo '```'
  echo "## dmesg (baseline, ultimas 15)"
  echo '```'
  (sudo -n dmesg 2>/dev/null || dmesg 2>/dev/null || echo "(dmesg precisa de root)") | tail -15
  echo '```'
} > "$SNAP" 2>&1

# ARMA o coletor postmortem: se o boot terminar depois disto sem um teardown limpo,
# o postmortem --auto vai coletar mesmo sem kernel BUG (cobre o caso do travamento #2).
touch "$FORENSICS_DIR/.armed" 2>/dev/null

echo "preflight-snapshot: baseline em $SNAP"
echo "preflight-snapshot: coletor ARMADO (.armed criado em $FORENSICS_DIR)"
