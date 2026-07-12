#!/usr/bin/env bash
# preflight.sh — PORTAO de seguranca (falha-seguro) antes de subir o daemon
# VRAM/ublk no host WSL2 vivo. RECUSA (exit != 0) em vez de deixar um start perigoso
# travar a maquina. Roda o snapshot baseline no sucesso.
#
# Motivated by the 2026-07-03 incident: a `--backend vram` run with a binary missing the
# mlockall fix froze the host (kernel BUG). This gate guarantees that only a binary WITH the
# fix, with a healthy GPU, and without orphaned devices, gets to run.
#
# Uso: preflight.sh [caminho_do_binario]
#   exit 0 = seguro prosseguir (snapshot escrito, coletor armado)
#   exit 1 = RECUSADO (motivo no stderr) — NAO suba o daemon
# So LE estado; nao toca GPU/ublk/swap. O unico efeito e escrever o snapshot.
set -uo pipefail

REPO="${RAMSHARED_REPO:-$(cd "$(dirname "$0")/../.." && pwd)}"
BIN="${1:-$REPO/target/debug/ramsharedd}"
FIX_MARKER='MCL_CURRENT-only no caminho ublk+vram'   # string do fix anti-dxgkrnl-BUG (#1)
MIN_VRAM_FREE_MIB="${RAMSHARED_MIN_VRAM_FREE_MIB:-256}"

# nvidia-smi in WSL2 is located in /usr/lib/wsl/lib, which is NOT in systemd's minimal PATH.
# Resolves the full path so the gate works both in the shell and via ExecStartPre.
NVSMI="$(command -v nvidia-smi 2>/dev/null || true)"
[ -x "$NVSMI" ] || NVSMI="/usr/lib/wsl/lib/nvidia-smi"

fail() { echo "PREFLIGHT: RECUSADO — $1" >&2; exit 1; }

echo "== RamShared preflight (falha-seguro) =="

# 1. Binario existe e TEM o fix do mlockall (senao = travamento garantido no #1).
# Materializa `strings` numa var e usa here-string no grep -q: evita o gotcha
# pipefail+grep-q+SIGPIPE (o pipe `strings | grep -q` retornava o SIGPIPE do strings,
# nao o sucesso do grep, e recusava o binario bom).
[ -x "$BIN" ] || fail "binario nao encontrado/executavel: $BIN (rode 'cargo build -p ramshared-wsl2d --bin ramsharedd')"
BIN_STRINGS="$(strings "$BIN" 2>/dev/null)"
if ! grep -qF "$FIX_MARKER" <<<"$BIN_STRINGS"; then
  fail "binario SEM o fix do mlockall ($BIN). Recompile com o fix (arm_future_lock) antes de rodar VRAM+ublk. Rodar assim TRAVA o host."
fi
echo "  [ok] binario tem o fix do mlockall"

# 2. GPU saudavel: nvidia-smi responde e ha VRAM livre suficiente.
SMI_OUT="$("$NVSMI" --query-gpu=memory.free --format=csv,noheader,nounits 2>/dev/null)"
[ -n "$SMI_OUT" ] || fail "nvidia-smi nao respondeu — GPU/driver em estado ruim; NAO suba VRAM agora"
VRAM_FREE="$(echo "$SMI_OUT" | head -1 | tr -dc '0-9')"
[ -n "$VRAM_FREE" ] || fail "nao consegui ler VRAM livre de nvidia-smi"
if [ "$VRAM_FREE" -lt "$MIN_VRAM_FREE_MIB" ]; then
  fail "VRAM livre ${VRAM_FREE} MiB < minimo ${MIN_VRAM_FREE_MIB} MiB — sem folga segura"
fi
echo "  [ok] GPU responde, VRAM livre=${VRAM_FREE} MiB (>= ${MIN_VRAM_FREE_MIB})"

# 3. Sem /dev/ublkb* orfao (sobra de um crash anterior -> colisao/estado sujo).
if ls /dev/ublkb* >/dev/null 2>&1; then
  fail "existe /dev/ublkb* orfao (sobra de execucao anterior): $(ls /dev/ublkb* 2>/dev/null | tr '\n' ' '). Limpe antes (o coletor postmortem ja deve ter rodado)."
fi
echo "  [ok] sem device ublk orfao"

# 4. Modulo ublk carregado (/dev/ublk-control presente).
[ -e /dev/ublk-control ] || fail "/dev/ublk-control ausente — 'sudo modprobe ublk_drv' primeiro"
echo "  [ok] ublk_drv carregado (/dev/ublk-control presente)"

# 5. Tudo ok -> snapshot baseline + arma o coletor.
"$REPO/scripts/safety/preflight-snapshot.sh" "${*:-ramsharedd (via preflight)}" >/dev/null 2>&1 \
  && echo "  [ok] snapshot baseline escrito + coletor armado" \
  || echo "  [aviso] snapshot falhou (nao-bloqueante), mas checks de seguranca passaram"

echo "PREFLIGHT: OK — seguro prosseguir."
exit 0
