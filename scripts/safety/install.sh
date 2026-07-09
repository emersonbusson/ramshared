#!/usr/bin/env bash
# install.sh — Instala o Sistema de Seguranca & Black-Box Forense do RamShared
# (WSL2) de forma reproduzivel a partir das copias versionadas no repo. Idempotente.
#
# Instala:
#   - journald drop-in (persistencia do log)
#   - ramshared-kmsg-recorder.service (console do kernel -> C:\wsl-forensics, live)
#   - ramshared-postmortem.service (coletor forense automatico no boot)
#   - ramsharedd.service (com preflight gate falha-seguro no ExecStartPre)
#
# NAO habilita o ramsharedd (rollout supervisionado). Habilita SO os servicos de
# seguranca (recorder + postmortem), que sao read-only/inofensivos.
#
# Uso: sudo bash scripts/safety/install.sh
set -euo pipefail

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
SD="$REPO/scripts/safety/systemd"
BIN_PATH="${RAMSHARED_BIN:-$REPO/target/debug/ramsharedd}"
SCRIPTS_PATH="$REPO/scripts/safety"

[ "$(id -u)" -eq 0 ] || { echo "rode com sudo" >&2; exit 1; }

echo "== instalando black-box forense do RamShared =="

install -m 0644 "$SD/10-ramshared-persistent.conf" \
  /etc/systemd/journald.conf.d/10-ramshared-persistent.conf
echo "  [ok] journald drop-in (persistencia)"

for unit in ramshared-kmsg-recorder.service ramshared-postmortem.service ramsharedd.service; do
  # Substitui os marcadores de caminhos absolutos dinamicamente
  sed -e "s|@REPO_PATH@|$REPO|g" \
      -e "s|@SCRIPTS_PATH@|$SCRIPTS_PATH|g" \
      -e "s|@BINARY_PATH@|$BIN_PATH|g" \
      "$SD/$unit" > "/etc/systemd/system/$unit"
  chmod 0644 "/etc/systemd/system/$unit"
  echo "  [ok] $unit (gerado dinamicamente em /etc/systemd/system/)"
done

# ublk_drv no boot (o daemon precisa; carregar o modulo e' inofensivo).
echo "ublk_drv" > /etc/modules-load.d/ramshared.conf
echo "  [ok] /etc/modules-load.d/ramshared.conf (ublk_drv no boot)"

systemctl daemon-reload
systemctl restart systemd-journald
echo "  [ok] daemon-reload + journald aplicado"

# Habilita SO os servicos de seguranca (nao o ramsharedd — supervisionado).
systemctl enable --now ramshared-kmsg-recorder.service
systemctl enable ramshared-postmortem.service   # oneshot: roda no proximo boot
echo "  [ok] recorder ATIVO agora + postmortem habilitado pro proximo boot"

echo
echo "== INSTALADO =="
echo "  ramsharedd.service segue DISABLED (start manual supervisionado):"
echo "    sudo systemctl start ramsharedd   # o preflight gate roda antes e RECUSA se inseguro"
echo "  Relatorios forenses: /mnt/c/wsl-forensics/"
