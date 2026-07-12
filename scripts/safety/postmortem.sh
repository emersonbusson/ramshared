#!/usr/bin/env bash
# postmortem.sh — Coletor forense "caixa-preta" do RamShared no WSL2.
#
# Gathers into a single dated report in DURABLE Windows storage
# (/mnt/c/wsl-forensics/, survives VM death) all the evidence
# we need to debug a freeze — exactly the manual investigation that was done
# for the 2026-07-03 hangs, now automated:
#   - journalctl do boot alvo (default: boot anterior, -1) + sinais de crash
#     (kernel BUG / Oops / hung_task / OOM / D-state)
#   - deteccao de morte-abrupta (no WSL2 quase todo fim e' abrupto; o sinal
#     confiavel de CRASH e' a assinatura de kernel BUG/Oops)
#   - Windows Event Log via powershell.exe: Kernel-Power 41/6008 (host travou?),
#     nvlddmkm/Display/TDR 4101/4102 (driver GPU crashou?), Hyper-V-VmSwitch
#     (VM recriada = restart?), erros dxgkrnl
#   - dmesg atual, estado de GPU/mem/swap, crash dumps do WSL se houver
#
# Uso:
#   postmortem.sh [BOOT_INDEX]   # BOOT_INDEX default = -1 (boot anterior)
#   postmortem.sh --auto         # so coleta se o boot -1 tiver assinatura de crash
#                                #  (ou marcador "armed" de um start arriscado);
#                                #  idempotente (nao recoleta o mesmo boot)
#
# Nao causa efeito colateral perigoso: so LE logs e escreve o relatorio. Seguro
# rodar a qualquer momento, no host vivo, sem tocar GPU/ublk/swap.
set -uo pipefail
NVSMI="$(command -v nvidia-smi 2>/dev/null || true)"; [ -x "$NVSMI" ] || NVSMI="/usr/lib/wsl/lib/nvidia-smi"

FORENSICS_DIR="${RAMSHARED_FORENSICS_DIR:-/mnt/c/wsl-forensics}"
PS='/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe'

# --- diretorio duravel (fallback pro guest se /mnt/c indisponivel) ---
if ! mkdir -p "$FORENSICS_DIR" 2>/dev/null; then
  FORENSICS_DIR="$HOME/wsl-forensics"
  mkdir -p "$FORENSICS_DIR" || { echo "erro: nao consegui criar diretorio forense" >&2; exit 2; }
  echo "AVISO: /mnt/c indisponivel, usando fallback nao-duravel: $FORENSICS_DIR" >&2
fi

TMPDIR_PM="$(mktemp -d)"; trap 'rm -rf "$TMPDIR_PM"' EXIT

# --- sinais de crash que valem coleta (deterministicos) ---
# \b evita falso-positivo: "kernelOOPSie"/"wh-OOPS-ie" nao casam \bOops:, "deBUG:" nao casa \bBUG.
# Inclui hung_task / blocked (swap ghost ublk) e panics — sem falso positivo solto em "debug".
CRASH_RE='kernel BUG|\bBUG:|\[ cut here \]|\bOops[:# ]|Call Trace:|hung_task|blocked for more than [0-9]|Out of memory|oom-kill|general protection fault|kernel panic|stack segment|kernel NULL pointer|task .* blocked for more than'

# Materializes the boot journal once (prevents calling journalctl N times AND avoids
# the pipefail+grep-q+SIGPIPE gotcha that caused false "no crash" reports).
dump_boot() { # $1 = boot index -> arquivo em $TMPDIR_PM
  local idx="$1" f="$TMPDIR_PM/boot${1}.log"
  [ -f "$f" ] || journalctl -b "$idx" --no-pager >"$f" 2>/dev/null
  echo "$f"
}

boot_has_crash() { # $1 = boot index
  grep -qiE "$CRASH_RE" "$(dump_boot "$1")"
}

# --- modo --auto: decide se coleta (crash em -1 OU marcador armed) e nao duplica ---
AUTO=0
BOOT_INDEX="-1"
case "${1:-}" in
  --auto) AUTO=1 ;;
  "" ) BOOT_INDEX="-1" ;;
  * ) BOOT_INDEX="$1" ;;
esac

BOOT_ID="$(journalctl -b "$BOOT_INDEX" --no-pager -o json -n 1 2>/dev/null \
            | grep -o '"_BOOT_ID":"[a-f0-9]*"' | head -1 | cut -d'"' -f4)"
[ -z "$BOOT_ID" ] && BOOT_ID="unknown-$(date +%s)"

if [ "$AUTO" = "1" ]; then
  ARMED_MARKER="$FORENSICS_DIR/.armed"
  DONE_MARKER="$FORENSICS_DIR/.collected-$BOOT_ID"
  if [ -f "$DONE_MARKER" ]; then
    echo "postmortem: boot $BOOT_ID ja coletado, pulando."; exit 0
  fi
  if boot_has_crash "-1"; then
    echo "postmortem: assinatura de crash no boot anterior -> coletando."
  elif [ -f "$ARMED_MARKER" ]; then
    echo "postmortem: marcador 'armed' presente (start arriscado antes do fim do boot) -> coletando."
  else
    echo "postmortem: boot anterior sem crash e sem 'armed' -> nada a coletar."
    exit 0
  fi
fi

TS="$(date +%Y%m%d-%H%M%S)"
REPORT="$FORENSICS_DIR/postmortem-${TS}-boot${BOOT_INDEX}.md"

{
  echo "# RamShared postmortem — boot ${BOOT_INDEX} (boot_id=${BOOT_ID})"
  echo
  echo "Gerado: $(date '+%Y-%m-%d %H:%M:%S %z') | kernel atual: $(uname -r)"
  echo

  echo "## 1. Veredito rapido"
  if boot_has_crash "$BOOT_INDEX"; then
    echo "- **CRASH detectado** no boot ${BOOT_INDEX} (assinatura de kernel BUG/Oops/hung_task/OOM)."
  else
    echo "- Sem assinatura de crash de kernel no boot ${BOOT_INDEX}."
    echo "  (No WSL2 um fim abrupto sem esta assinatura geralmente = \`wsl --shutdown\` ou VM"
    echo "   morta pelo host, NAO um kernel BUG do guest. Ver Event Log do Windows abaixo.)"
  fi
  echo

  BOOT_LOG="$(dump_boot "$BOOT_INDEX")"

  echo "## 2. Assinaturas de crash (grep no journal do boot ${BOOT_INDEX})"
  echo '```'
  grep -iE "$CRASH_RE" "$BOOT_LOG" | tail -40 || echo "(nenhuma)"
  echo '```'
  echo

  echo "## 3. Fim do boot ${BOOT_INDEX} (ultimas 40 linhas antes do termino)"
  echo '```'
  tail -40 "$BOOT_LOG"
  echo '```'
  echo

  echo "## 4. Contexto RamShared no boot ${BOOT_INDEX} (daemon/ublk/mlockall/swap)"
  echo '```'
  grep -iE "ramshared|ublk|blk_queue_max_hw|mlockall|memoria travada|swapon|dxgkrnl|dxgk:" \
    "$BOOT_LOG" | tail -40 || echo "(nada relacionado ao RamShared/ublk/dxg)"
  echo '```'
  echo

  echo "## 5. Windows Event Log (host travou? GPU crashou? VM reiniciou?)"
  if [ -x "$PS" ]; then
    echo '```'
    # `iconv//TRANSLIT` + `tr -d \r` limpa o output do powershell (CP1252/NEL) pra UTF-8
    # greppavel; a saida ja pede UTF-8 via [Console]::OutputEncoding.
    "$PS" -NoProfile -NonInteractive -Command "
      [Console]::OutputEncoding=[System.Text.Encoding]::UTF8
      \$ErrorActionPreference='SilentlyContinue'
      Write-Output '--- Kernel-Power 41 / 6008 (shutdowns inesperados / host travou), ultimos 5 ---'
      Get-WinEvent -FilterHashtable @{LogName='System'; Id=41,6008} -MaxEvents 5 |
        Select-Object TimeCreated, Id, @{N='Msg';E={(\$_.Message -split \"\`n\")[0]}} |
        Format-Table -AutoSize -Wrap | Out-String -Width 200
      Write-Output '--- Display/GPU TDR (nvlddmkm, Event 4101/4102), ultimos 5 ---'
      Get-WinEvent -FilterHashtable @{LogName='System'; Id=4101,4102} -MaxEvents 5 |
        Select-Object TimeCreated, Id, ProviderName, @{N='Msg';E={(\$_.Message -split \"\`n\")[0]}} |
        Format-Table -AutoSize -Wrap | Out-String -Width 200
      Write-Output '--- Hyper-V-VmSwitch (teardown/recriacao de VM = restart do WSL), ultimos 10 ---'
      Get-WinEvent -FilterHashtable @{LogName='System'; ProviderName='Microsoft-Windows-Hyper-V-VmSwitch'} -MaxEvents 10 |
        Select-Object TimeCreated, Id, @{N='Msg';E={(\$_.Message -split \"\`n\")[0]}} |
        Format-Table -AutoSize -Wrap | Out-String -Width 200
    " 2>&1 | tr -d '\r' | iconv -f UTF-8 -t UTF-8//TRANSLIT 2>/dev/null || true
    echo '```'
  else
    echo "(powershell.exe indisponivel em $PS — pulando Event Log do Windows)"
  fi
  echo

  echo "## 6. Estado atual (pos-restart): GPU / memoria / swap / dmesg"
  echo '```'
  echo "# nvidia-smi:"; "$NVSMI" --query-gpu=memory.used,memory.free,memory.total --format=csv 2>&1 || echo "(nvidia-smi indisponivel)"
  echo; echo "# free -h:"; free -h 2>&1
  echo; echo "# /proc/swaps:"; cat /proc/swaps 2>&1
  echo; echo "# dmesg (ultimas 20):"; (sudo -n dmesg 2>/dev/null || dmesg 2>/dev/null || echo "(dmesg precisa de root)") | tail -20
  echo '```'
  echo

  echo "## 7. Console do kernel DURAVEL do boot que travou (host-side)"
  echo "(kernel-console.prev.log = console do boot anterior, preservado pelo recorder no"
  echo " lado Windows; pode conter o call trace COMPLETO do BUG que o journald perdeu ao"
  echo " congelar. Ultimas 60 linhas:)"
  echo '```'
  PREV_CONSOLE="$FORENSICS_DIR/kernel-console.prev.log"
  if [ -f "$PREV_CONSOLE" ]; then
    tail -60 "$PREV_CONSOLE"
  else
    echo "(sem kernel-console.prev.log — recorder pode nao ter rodado no boot anterior)"
  fi
  echo '```'
  echo

  echo "## 8. Crash dumps do WSL (se houver)"
  echo '```'
  ls -la /var/crash/ 2>/dev/null | tail -10 || echo "(sem /var/crash)"
  ls -la /mnt/c/Users/*/AppData/Local/Temp/*.dmp 2>/dev/null | tail -5 || echo "(sem .dmp em Temp)"
  echo '```'
} > "$REPORT" 2>&1

echo "postmortem: relatorio escrito em:"
echo "  $REPORT"
if [ "$AUTO" = "1" ]; then
  touch "$FORENSICS_DIR/.collected-$BOOT_ID" 2>/dev/null
  rm -f "$FORENSICS_DIR/.armed" 2>/dev/null  # consumido; proximo start rearma
fi
