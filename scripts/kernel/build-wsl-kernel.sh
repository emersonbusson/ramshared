#!/usr/bin/env bash
# build-wsl-kernel.sh — build reutilizável de kernel WSL2 custom a partir da base
# OFICIAL da Microsoft + configs extras. Embute as lições do toolkit:
#   - parte do Microsoft/config-wsl (todos os configs de boot do WSL2 garantidos);
#   - aplica configs extras e VERIFICA que pegaram (pega o gotcha bool vs --module);
#   - roda modules_install (senão os .ko não carregam no boot).
#
# uso: build-wsl-kernel.sh [CONFIG=y|m|n ...]
#   default: CONFIG_BLK_DEV_UBLK=m CONFIG_ZRAM_WRITEBACK=y CONFIG_IO_URING=y (Fase B)
# env: KTAG (branch/tag, default linux-msft-wsl-6.6.y), KSRC (dir fonte), JOBS.
#
# Saída: imprime o bzImage + a kernelrelease (passe ao qemu-validate.sh / boot-kernel-safe.ps1).
set -euo pipefail

# SPEC wsl2-custom-kernel-p1: default 6.18.y; override with KTAG=
KTAG="${KTAG:-linux-msft-wsl-6.18.y}"
KSRC="${KSRC:-$HOME/src/WSL2-Linux-Kernel}"
JOBS="${JOBS:-2}"; [ "$JOBS" -lt 1 ] && JOBS=1
CONFIGS=("$@"); [ ${#CONFIGS[@]} -eq 0 ] && CONFIGS=(CONFIG_BLK_DEV_UBLK=m CONFIG_ZRAM_WRITEBACK=y CONFIG_IO_URING=y)

echo "[build] deps..."
sudo apt-get install -y -q build-essential flex bison libelf-dev libssl-dev bc dwarves cpio python3 >/dev/null

if [ ! -d "$KSRC/.git" ]; then
  echo "[build] clonando $KTAG -> $KSRC"
  git clone --depth 1 --branch "$KTAG" https://github.com/microsoft/WSL2-Linux-Kernel.git "$KSRC"
fi
cd "$KSRC"

echo "[build] base = Microsoft/config-wsl + configs extras"
cp Microsoft/config-wsl .config
for kv in "${CONFIGS[@]}"; do
  name="${kv%%=*}"; val="${kv##*=}"
  case "$val" in
    y) ./scripts/config --file .config --enable  "$name" ;;
    m) ./scripts/config --file .config --module  "$name" ;;
    n) ./scripts/config --file .config --disable "$name" ;;
    *) echo "[build] valor inválido em $kv (use y|m|n)"; exit 2 ;;
  esac
done
make olddefconfig >/dev/null

# VERIFICA que cada config pegou (olddefconfig reverte os inválidos — ex.: bool pedido como --module).
fail=0
for kv in "${CONFIGS[@]}"; do
  name="${kv%%=*}"; val="${kv##*=}"
  got="$(grep -E "^${name}=" .config | cut -d= -f2 || true)"; [ -z "$got" ] && got="(unset)"
  want="$val"; [ "$val" = "y" ] && want="y"; [ "$val" = "m" ] && want="m"
  if { [ "$val" = "y" ] && [ "$got" = "y" ]; } || { [ "$val" = "m" ] && [ "$got" = "m" ]; } || { [ "$val" = "n" ] && [ "$got" = "(unset)" ]; }; then
    echo "[build]  OK  $name=$got"
  else
    echo "[build] !!!! $name pedido=$val mas ficou=$got — provável dependência faltando (ex.: bool exige --enable, não --module; ou depende de outro CONFIG)."
    fail=1
  fi
done
[ "$fail" = 1 ] && { echo "[build] configs não aplicaram; abortando."; exit 1; }

echo "[build] make -j$JOBS (pesado; -j limitado evita travar o WSL2)..."
make -j"$JOBS"
echo "[build] modules_install..."
sudo make modules_install >/dev/null

REL="$(make -s kernelrelease)"
echo "=============================================="
echo "[build] OK"
echo "  bzImage : $KSRC/arch/x86/boot/bzImage"
echo "  release : $REL"
echo "  validar : sudo bash scripts/kernel/qemu-validate.sh $KSRC/arch/x86/boot/bzImage \"$REL\" \\"
echo "              $KSRC/drivers/block/ublk_drv.ko $KSRC/mm/zsmalloc.ko $KSRC/drivers/block/zram/zram.ko"
echo "=============================================="
